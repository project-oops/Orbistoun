//! A submitted command buffer, end to end.
//!
//! The fixture is a command stream in the shape a guest submits: packets that write the
//! shader-address registers, pointing at a shader in guest memory at the address those registers
//! name. Nothing calls the translator directly, so a shader is found only because the stream asked
//! for it, as in a real frame.

use orbistoun_gpu::pipeline::{GuestMemory, MAX_SHADER_BYTES, Pipeline, Queue, RegisteredShader};
use orbistoun_gpu::{RenderCommand, ShaderStage};
use orbistoun_shader::EncodingTable;
use orbistoun_translate::{Fidelity, Strategy, Width};

/// Guest memory as a single mapping at a known base: the smallest thing that answers the one
/// question the pipeline asks.
struct Mapping {
    base: u64,
    bytes: Vec<u8>,
}

impl GuestMemory for Mapping {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        let offset = usize::try_from(address.checked_sub(self.base)?).ok()?;
        self.bytes.get(offset..offset.checked_add(length)?)
    }
}

/// The address the fixture puts its shader at. Arbitrary, and deliberately not zero.
const SHADER_ADDRESS: u64 = 0x1_0000;

/// The first word of a SPIR-V module, written out so this test needs no dependency on the emitter.
const SPIRV_MAGIC: u32 = 0x0723_0203;

/// A shader that writes a constant and stops.
fn shader() -> Vec<u8> {
    // v_mov_b32 v0, 9 ; s_endpgm
    let words: [u32; 2] = [0x7E00_0000 | (1 << 9) | (128 + 9), 0xBF81_0000];
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Guest memory holding the shader, and rubbish after it.
///
/// A shader in memory has no length and the pipeline reads a window, so decoding must stop at the
/// end of the program rather than decode the rubbish and desynchronise.
fn memory() -> Mapping {
    let mut bytes = shader();
    bytes.extend(std::iter::repeat_n(0xAB, 4096));
    Mapping {
        base: SHADER_ADDRESS,
        bytes,
    }
}

/// A command stream that sets the compute shader address, as a guest would, with register numbers
/// from the crate's own vocabulary.
fn command_stream(address: u64) -> Vec<u8> {
    let vocabulary = orbistoun_gpu::registers::Vocabulary::builtin().expect("vocabulary");
    let (low, high) = shader_registers(&vocabulary);
    // The registers decide which opcode reaches them: each register-writing opcode has its own
    // base.
    let (opcode, base) = vocabulary
        .opcode_for_register(low)
        .expect("an opcode reaching the shader address registers");
    // Two writes, each a header, a register offset and a value, as separate packets so the offsets
    // are obvious.
    let mut words: Vec<u32> = Vec::new();
    for (register, value) in [
        // The registers take the address in 256-byte units (see `shader_candidates`).
        (
            low,
            u32::try_from((address >> 8) & 0xFFFF_FFFF).expect("low half"),
        ),
        (high, u32::try_from(address >> 40).expect("high half")),
    ] {
        words.push((3 << 30) | ((2 - 1) << 16) | (u32::from(opcode) << 8));
        words.push(register - base);
        words.push(value);
    }
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// The pair of registers holding the compute shader's address.
fn shader_registers(vocabulary: &orbistoun_gpu::registers::Vocabulary) -> (u32, u32) {
    let mut low = None;
    let mut high = None;
    for (register, (stage, is_high)) in vocabulary.shader_registers() {
        if stage != "compute" {
            continue;
        }
        if *is_high {
            high = Some(*register);
        } else {
            low = Some(*register);
        }
    }
    (
        low.expect("a compute shader address register"),
        high.expect("a compute shader address register"),
    )
}

fn pipeline() -> Pipeline {
    Pipeline::new(Strategy::Predicated {
        fidelity: Fidelity::Lane,
        width: Width::default(),
    })
    .expect("pipeline")
}

#[test]
fn a_submitted_command_buffer_yields_a_translated_shader() {
    // The whole path: packets -> register writes -> a shader address -> guest memory ->
    // decode -> translate -> a command a backend can act on.
    let mut pipeline = pipeline();
    let submission = pipeline.submit(
        &command_stream(SHADER_ADDRESS),
        Queue::Compute,
        &[],
        &memory(),
    );

    assert_eq!(
        submission.report.failures,
        Vec::new(),
        "nothing should have failed: {:?}",
        submission.report
    );
    assert_eq!(
        submission.report.shaders_found, 1,
        "the registers name one shader: {:?}",
        submission.report
    );
    assert_eq!(submission.report.shaders_translated, 1);
    assert_eq!(submission.modules.len(), 1, "one module for the backend");

    let module = submission.modules.values().next().expect("a module");
    assert_eq!(
        module.first().copied(),
        Some(SPIRV_MAGIC),
        "what came out should be a SPIR-V module"
    );

    assert!(
        submission.commands.iter().any(|command| matches!(
            command,
            RenderCommand::BindShader {
                stage: ShaderStage::Compute,
                ..
            }
        )),
        "the backend should be told to bind it: {:?}",
        submission.commands
    );
}

/// An indexed draw reaches the backend as a `DrawIndexed`, with the index count from its own body
/// and the running instance count.
#[test]
fn an_indexed_draw_reaches_the_backend_as_a_draw_indexed() {
    // NUM_INSTANCES(2), then DRAW_INDEX_2 whose body carries thirty-six indices and an address.
    let address: u64 = 0x2_0000;
    let words: [u32; 8] = [
        (3 << 30) | (0x2F << 8), // NUM_INSTANCES, one body word
        2,
        (3 << 30) | ((5 - 1) << 16) | (0x27 << 8), // DRAW_INDEX_2, five body words
        0,                                         // max_size
        u32::try_from(address & 0xFFFF_FFFF).expect("low half"),
        u32::try_from(address >> 32).expect("high half"),
        36, // index count
        0,  // initiator
    ];
    let stream: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

    let mut pipeline = pipeline();
    let submission = pipeline.submit(&stream, Queue::Draw, &[], &memory());

    assert!(
        submission.commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIndexed {
                indices: 36,
                instances: 2,
                ..
            }
        )),
        "the indexed draw should reach the backend with its measured count: {:?}",
        submission.commands
    );
    assert_eq!(submission.report.draws, 1, "and it counts as one draw");
}

/// A stream that sizes its colour target reaches the backend as a `SetRenderTargets`.
///
/// A `CB_COLOR0_ATTRIB2` write is decoded into a target the submission carries and a
/// `SetRenderTargets` that selects it, so a backend sizes its attachment to the guest's frame. The
/// value is the measured one for a 64x64 target.
#[test]
fn a_target_size_write_reaches_the_backend_as_set_render_targets() {
    use orbistoun_gpu::ColourTargetExtent;

    // SET_CONTEXT_REG (0x69, count 1) writing CB_COLOR0_ATTRIB2 (offset 0x3b0) = 0x000fc03f, the
    // measured 64x64 value; then the compute shader address, so the submission also finds a shader.
    let header = (3u32 << 30) | ((2 - 1) << 16) | (0x69 << 8);
    let mut stream: Vec<u8> = [header, 0x3b0, 0x000f_c03f]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    stream.extend(command_stream(SHADER_ADDRESS));

    let mut pipeline = pipeline();
    let submission = pipeline.submit(&stream, Queue::Compute, &[], &memory());

    assert_eq!(
        submission.targets.len(),
        1,
        "one colour target was sized: {:?}",
        submission.targets
    );
    let (id, extent) = submission.targets.iter().next().expect("the sized target");
    assert_eq!(
        *extent,
        ColourTargetExtent {
            width: 64,
            height: 64,
        },
        "the decoded 64x64 target"
    );
    assert!(
        submission.commands.iter().any(|command| matches!(
            command,
            RenderCommand::SetRenderTargets { colour, depth: None }
                if colour == &vec![*id]
        )),
        "a SetRenderTargets selecting the target should reach the backend: {:?}",
        submission.commands
    );
}

/// A stream that sets the generic scissor reaches the backend as a `SetViewport`: the
/// `GENERIC_SCISSOR` write pair is decoded into the rectangle a draw is restricted to.
#[test]
fn a_scissor_write_reaches_the_backend_as_set_viewport() {
    use orbistoun_gpu::Rect;

    // SET_CONTEXT_REG (0x69, count 1) writing GENERIC_SCISSOR_TL (offset 0x090) then _BR (0x091)
    // for a sub-rect: top-left (16, 8), bottom-right (48, 56).
    let header = (3u32 << 30) | ((2 - 1) << 16) | (0x69 << 8);
    // Top-left (x 16, y 8) is 0x0008_0010; bottom-right (x 48, y 56) is 0x0038_0030.
    let top_left = 0x0008_0010;
    let bottom_right = 0x0038_0030;
    let stream: Vec<u8> = [header, 0x090, top_left, header, 0x091, bottom_right]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();

    let mut pipeline = pipeline();
    let submission = pipeline.submit(&stream, Queue::Draw, &[], &memory());
    assert!(
        submission.commands.iter().any(|command| matches!(
            command,
            RenderCommand::SetViewport(Rect {
                x: 16,
                y: 8,
                width: 32,
                height: 48,
            })
        )),
        "a SetViewport with the decoded scissor should reach the backend: {:?}",
        submission.commands
    );
}

/// A stream's pipeline state - colour-target base and tiling, and depth, stencil and blend -
/// reaches the submission, carrying the same decoded values `registers.rs`'s own tests pin.
#[test]
fn a_stream_sets_the_pipeline_state_the_submission_carries() {
    use orbistoun_gpu::{BlendFactor, ColourTarget, CompareFunc, StencilOp, SwizzleMode};

    // SET_CONTEXT_REG (0x69, count 1) writes, one per register, at their context-dword offsets:
    // CB_COLOR0_BASE 0x318, ATTRIB2 0x3b0, ATTRIB3 0x3b8, DB_DEPTH_CONTROL 0x200,
    // DB_STENCIL_CONTROL 0x10b, CB_BLEND0_CONTROL 0x1e0.
    let header = (3u32 << 30) | ((2 - 1) << 16) | (0x69 << 8);
    let words = [
        header,
        0x318,
        0x0200_0e00, // base -> 0x2000e0000 (value << 8)
        header,
        0x3b0,
        0x000f_c03f, // extent 64x64
        header,
        0x3b8,
        0x08c6_c000, // tiling 64KB_R_X
        header,
        0x200,
        0x0050_07b7, // depth: Z on+write, ZFUNC LEQUAL
        header,
        0x10b,
        0x00f5_1730, // stencil ops
        header,
        0x1e0,
        0x6081_0564, // blend: enable, colour SRC_ALPHA/ONE_MINUS_SRC_ALPHA
    ];
    let stream: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

    let mut pipeline = pipeline();
    let submission = pipeline.submit(&stream, Queue::Draw, &[], &memory());

    assert_eq!(
        submission.colour_target,
        Some(ColourTarget {
            base: 0x2_000e_0000,
            width: 64,
            height: 64,
        }),
        "colour target zero's base and extent reach the submission"
    );
    assert_eq!(
        submission.colour_target_tiling,
        Some(SwizzleMode::Tiled64KbRX),
        "and its tiling mode"
    );

    let depth = submission.depth_control.expect("depth control was set");
    assert!(depth.depth_test_enable && depth.depth_write_enable);
    assert_eq!(depth.depth_func, CompareFunc::LessEqual);

    let stencil = submission.stencil_control.expect("stencil control was set");
    assert_eq!(stencil.fail_op, StencilOp::Keep);
    assert_eq!(stencil.depth_pass_op, StencilOp::ReplaceTest);

    let blend = submission.blend_control.expect("blend control was set");
    assert!(blend.enable);
    assert_eq!(blend.color_src, BlendFactor::SrcAlpha);
    assert_eq!(blend.color_dst, BlendFactor::OneMinusSrcAlpha);
}

/// The guest-memory window is read out of guest memory and carried on the submission.
///
/// The frontend reads exactly the window's span - the length the module masks against - so a
/// backend can bind it (D703). A window over unmapped memory carries nothing.
#[test]
fn the_guest_memory_window_is_read_from_guest_memory() {
    use orbistoun_translate::wavefront::Window;

    let base = 0x2_0000u32;
    let words = [
        0x1111_1111u32,
        0x2222_2222,
        0x3333_3333,
        0x4444_4444,
        0x5555_5555,
    ];
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mapped = Mapping {
        base: u64::from(base),
        bytes,
    };

    // A window spanning four of the five words reads exactly those four.
    let mut placed = pipeline().with_window(Window::spanning(base, 4).expect("a power of two"));
    let submission = placed.submit(&[0u8; 64], Queue::Draw, &[], &mapped);
    assert_eq!(
        submission.guest_memory,
        &words[..4],
        "the window's four words are read from guest memory"
    );

    // A window somewhere unmapped carries nothing rather than guessing.
    let mut elsewhere =
        pipeline().with_window(Window::spanning(0x9_0000, 4).expect("a power of two"));
    let submission = elsewhere.submit(&[0u8; 64], Queue::Draw, &[], &mapped);
    assert!(
        submission.guest_memory.is_empty(),
        "an unmapped window carries nothing: {:?}",
        submission.guest_memory
    );
}

#[test]
fn the_same_shader_is_translated_once() {
    // A guest rebinds the same shader every draw, so the second submission hits the cache and does
    // not re-send the module.
    let mut pipeline = pipeline();
    let stream = command_stream(SHADER_ADDRESS);

    let first = pipeline.submit(&stream, Queue::Compute, &[], &memory());
    assert_eq!(first.report.cache_hits, 0);
    assert_eq!(first.modules.len(), 1, "the backend has not seen it yet");

    let second = pipeline.submit(&stream, Queue::Compute, &[], &memory());
    assert_eq!(second.report.cache_hits, 1, "{:?}", second.report);
    assert!(
        second.modules.is_empty(),
        "a module the backend already has must not travel again"
    );
    assert_eq!(pipeline.cached_shaders(), 1);

    // The command is still emitted, or caching would silently drop the draw.
    assert_eq!(second.commands.len(), first.commands.len());
}

/// The window a pipeline is given reaches the modules it translates, and the cache knows.
///
/// A window's base and length are compiled into a module - the base is subtracted before an index
/// is masked, and the length is the mask - so one shader against two windows is two modules. The
/// cache is keyed on the shader's bytes, so it must not serve the first window's module to the
/// second. Asserted in order: the second window is not a cache hit, its module differs, and going
/// back to the first is not a hit either, because the setter clears the cache.
#[test]
fn a_shader_translated_against_two_windows_is_two_modules() {
    use orbistoun_translate::wavefront::Window;

    let stream = command_stream(SHADER_ADDRESS);

    let mut near = pipeline().with_window(Window::default());
    let first = near.submit(&stream, Queue::Compute, &[], &memory());
    assert_eq!(first.report.cache_hits, 0);
    let first_module = first
        .modules
        .values()
        .next()
        .expect("a module for the first window")
        .clone();

    // Somewhere a guest's buffers might be, rather than address zero.
    let elsewhere = Window::spanning(0x0090_0000, 1 << 16).expect("a power-of-two window");
    let mut far = pipeline().with_window(elsewhere);
    let second = far.submit(&stream, Queue::Compute, &[], &memory());
    assert_eq!(
        second.report.cache_hits, 0,
        "a fresh pipeline has nothing cached: {:?}",
        second.report
    );
    let second_module = second
        .modules
        .values()
        .next()
        .expect("a module for the second window")
        .clone();

    assert_ne!(
        first_module, second_module,
        concat!(
            "two windows produced the same module - the base and the length are compiled in, ",
            "so identical words mean the window never reached the translation"
        )
    );
    assert_eq!(
        far.window(),
        elsewhere,
        "the pipeline kept the window it was given"
    );

    // Within one pipeline, changing the window does not serve the old module.
    let mut moved = near.with_window(elsewhere);
    let third = moved.submit(&stream, Queue::Compute, &[], &memory());
    assert_eq!(
        third.report.cache_hits, 0,
        "the cache served a module built against the window this pipeline no longer has: {:?}",
        third.report
    );
    assert_eq!(
        third
            .modules
            .values()
            .next()
            .expect("a module after the window moved"),
        &second_module,
        "the same shader and the same window must translate to the same module"
    );
}

#[test]
fn two_addresses_holding_the_same_shader_share_one_translation() {
    // The cache is keyed on the shader's bytes, not its address: keying on the address would make a
    // moved shader look new and a shader replaced in place look old.
    let mut pipeline = pipeline();

    let first = pipeline.submit(
        &command_stream(SHADER_ADDRESS),
        Queue::Compute,
        &[],
        &memory(),
    );
    assert_eq!(first.report.shaders_translated, 1);

    let elsewhere = 0x2_0000;
    let mut moved = memory();
    moved.base = elsewhere;
    let second = pipeline.submit(&command_stream(elsewhere), Queue::Compute, &[], &moved);

    assert_eq!(
        second.report.cache_hits, 1,
        "the same bytes at another address are the same shader: {:?}",
        second.report
    );
    assert_eq!(pipeline.cached_shaders(), 1);
}

#[test]
fn a_shader_address_that_is_not_mapped_is_reported_not_ignored() {
    // A wrong register decode must not produce a draw that silently draws nothing.
    let mut pipeline = pipeline();
    let submission = pipeline.submit(&command_stream(0xDEAD_0000), Queue::Compute, &[], &memory());

    assert_eq!(submission.report.shaders_found, 1);
    assert_eq!(submission.report.shaders_translated, 0);
    assert_eq!(submission.report.failures.len(), 1);
    assert_eq!(submission.report.failures[0].address, 0xDEAD_0000);
    assert!(
        submission.report.failures[0]
            .reason
            .contains("no mapped memory"),
        "the reason should say what went wrong: {:?}",
        submission.report.failures[0]
    );
    assert!(
        submission.commands.is_empty(),
        "nothing should be bound when nothing was translated"
    );
}

#[test]
fn address_resolution_is_counted_apart_from_the_shader_outcome() {
    // Whether a GPU virtual address is a guest address is an assumption (D130); every address a
    // submission names tests it. Counted apart from whether the shader translated: an address can
    // resolve and its shader still be refused.
    let mut pipeline = pipeline();

    // An address guest memory knows: it resolves, and the shader behind it translates.
    let good = Mapping {
        base: SHADER_ADDRESS,
        bytes: shader(),
    };
    let report = pipeline
        .submit(&command_stream(SHADER_ADDRESS), Queue::Compute, &[], &good)
        .report;
    assert_eq!(report.addresses_resolved, 1, "{report:?}");
    assert_eq!(report.addresses_unresolved, 0);

    // An address it does not know: the submission names somewhere unmapped, and the report says so
    // without blaming the shader.
    let elsewhere = SHADER_ADDRESS + 0x10_0000;
    let report = pipeline
        .submit(&command_stream(elsewhere), Queue::Compute, &[], &good)
        .report;
    assert_eq!(report.addresses_unresolved, 1, "{report:?}");
    assert_eq!(report.addresses_resolved, 0);
    assert!(
        report.failures[0].reason.contains("GPU virtual address"),
        "the failure should say which assumption to suspect: {:?}",
        report.failures[0]
    );
}

#[test]
fn a_command_buffer_can_be_submitted_from_guest_memory() {
    // The shape a real call site has: a guest builds a command buffer in its own memory and passes
    // a pointer and a length.
    let mut pipeline = pipeline();

    // The command buffer and the shader both live in guest memory, at different addresses.
    let stream = command_stream(SHADER_ADDRESS);
    let buffer_address = SHADER_ADDRESS + 0x1000;
    let mut bytes = shader();
    bytes.resize(0x1000, 0);
    bytes.extend_from_slice(&stream);

    let memory = Mapping {
        base: SHADER_ADDRESS,
        bytes,
    };

    let submission = pipeline
        .submit_at(buffer_address, stream.len(), Queue::Compute, &[], &memory)
        .expect("the command buffer is readable");

    assert_eq!(
        submission.report.shaders_translated, 1,
        "{:?}",
        submission.report
    );
    assert_eq!(submission.report.addresses_resolved, 1);
}

#[test]
fn an_unreadable_command_buffer_is_reported_rather_than_guessed_at() {
    // The pointer comes from the guest's CPU-side code, so if it does not resolve the fault is in
    // the shim's arguments, not in any GPU address assumption. `None`, rather than an empty
    // submission that reads as "nothing to do".
    let mut pipeline = pipeline();
    let memory = Mapping {
        base: SHADER_ADDRESS,
        bytes: shader(),
    };

    assert!(
        pipeline
            .submit_at(SHADER_ADDRESS + 0x10_0000, 64, Queue::Compute, &[], &memory)
            .is_none(),
        "an unreadable command buffer must not read as an empty one"
    );
}

#[test]
fn a_shader_with_no_terminator_in_the_window_is_refused_rather_than_truncated() {
    // A shader that exceeds the read window is refused rather than truncated: a truncated shader
    // decodes cleanly up to the cut and produces a plausible prefix of the right module.
    let mut pipeline = pipeline();

    // Instructions all the way to the horizon and no end-of-program anywhere.
    let filler = shader()[..4].to_vec();
    let mut bytes = Vec::new();
    while bytes.len() < MAX_SHADER_BYTES + 4096 {
        bytes.extend_from_slice(&filler);
    }
    let memory = Mapping {
        base: SHADER_ADDRESS,
        bytes,
    };

    let report = pipeline
        .submit(
            &command_stream(SHADER_ADDRESS),
            Queue::Compute,
            &[],
            &memory,
        )
        .report;

    assert_eq!(report.shaders_translated, 0, "{report:?}");
    let reason = &report.failures.first().expect("a failure").reason;
    assert!(
        reason.contains("end-of-program"),
        "the refusal should say the window ran out, not blame the shader: {reason}"
    );
    // The address itself was fine: this is a window problem, not a failed address assumption.
    assert_eq!(report.addresses_resolved, 1);
    assert_eq!(report.addresses_unresolved, 0);
}

#[test]
fn a_shader_that_does_not_translate_is_reported_with_its_reason() {
    // An untranslated instruction arrives as a named failure a worklist can rank.
    let mut pipeline = pipeline();

    // An export, blocked on a subsystem rather than merely unimplemented, so it will not quietly
    // become supported and turn the test green for the wrong reason. Its bytes are asked for by
    // family name: an encoded word belongs to one architecture generation, while family names
    // survive a retarget (D139).
    let encodings = EncodingTable::builtin().expect("encodings");
    let export = encodings
        .encodings()
        .iter()
        .find(|encoding| encoding.name == "EXP")
        .expect("the table declares an export family");
    let terminator = encodings
        .find_by_name("s_endpgm")
        .map(|(family, opcode)| {
            let found = encodings
                .encodings()
                .iter()
                .find(|e| e.name == family)
                .expect("the family the name was found in");
            found.value | (opcode << found.opcode.shift)
        })
        .expect("the table names the terminator");
    let words: [u32; 3] = [export.value, 0, terminator];
    let mut bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    bytes.extend(std::iter::repeat_n(0u8, 256));
    let memory = Mapping {
        base: SHADER_ADDRESS,
        bytes,
    };

    let submission = pipeline.submit(
        &command_stream(SHADER_ADDRESS),
        Queue::Compute,
        &[],
        &memory,
    );
    assert_eq!(
        submission.report.failures.len(),
        1,
        "{:?}",
        submission.report
    );
    assert!(
        submission.report.failures[0]
            .reason
            .contains("could not be translated"),
        "got: {:?}",
        submission.report.failures[0]
    );
    assert!(submission.commands.is_empty());
}

#[test]
fn a_command_buffer_with_no_shader_registers_reports_nothing_found() {
    // The empty answer: a stream this understands nothing of produces no commands and a report
    // saying how little it recognised, which is different from an error.
    let mut pipeline = pipeline();
    let submission = pipeline.submit(&[0u8; 64], Queue::Compute, &[], &memory());

    assert_eq!(submission.report.shaders_found, 0);
    assert!(submission.commands.is_empty());
    assert!(submission.report.failures.is_empty());
}

#[test]
fn a_registered_shader_is_believed_over_the_register_writes() {
    // Two routes to a shader address: the guest registering it by name, and a register write in the
    // packets. Registration is stated and the register path inferred from a transcribed table, so
    // registration wins where they overlap.
    let mut pipeline = pipeline();
    let registered = [RegisteredShader {
        address: SHADER_ADDRESS,
        stage: ShaderStage::Compute,
    }];

    // The packets point somewhere unmapped: the inferred address would fail to read, and the
    // registered one translates.
    let submission = pipeline.submit(
        &command_stream(0xDEAD_0000),
        Queue::Compute,
        &registered,
        &memory(),
    );

    assert_eq!(
        submission.report.failures,
        Vec::new(),
        "registration should have supplied a good address: {:?}",
        submission.report
    );
    assert_eq!(submission.report.shaders_translated, 1);
}

#[test]
fn the_two_routes_disagreeing_is_reported_as_evidence() {
    // Both routes keep running once one has answered: a mismatch means the register vocabulary
    // found the wrong bits, which nothing else can reveal.
    let mut pipeline = pipeline();
    let registered = [RegisteredShader {
        address: SHADER_ADDRESS,
        stage: ShaderStage::Compute,
    }];

    let submission = pipeline.submit(
        &command_stream(0xDEAD_0000),
        Queue::Compute,
        &registered,
        &memory(),
    );

    assert_eq!(
        submission.report.disagreed.len(),
        1,
        "{:?}",
        submission.report
    );
    assert_eq!(submission.report.disagreed[0].registered, SHADER_ADDRESS);
    assert_eq!(submission.report.disagreed[0].inferred, 0xDEAD_0000);
    assert_eq!(submission.report.agreed, 0);
}

#[test]
fn the_two_routes_agreeing_is_reported_too() {
    // Agreement is the evidence that the register vocabulary is right, so it is counted.
    let mut pipeline = pipeline();
    let registered = [RegisteredShader {
        address: SHADER_ADDRESS,
        stage: ShaderStage::Compute,
    }];

    let submission = pipeline.submit(
        &command_stream(SHADER_ADDRESS),
        Queue::Compute,
        &registered,
        &memory(),
    );

    assert_eq!(submission.report.agreed, 1, "{:?}", submission.report);
    assert!(submission.report.disagreed.is_empty());
    assert_eq!(
        submission.report.shaders_translated, 1,
        "one shader, not two - the routes named the same one"
    );
}

#[test]
fn a_stage_the_queue_cannot_run_is_reported_not_filtered() {
    // A vertex shader named by a compute submission is a wrong decode, and it is reported, not
    // dropped.
    let mut pipeline = pipeline();
    let registered = [RegisteredShader {
        address: SHADER_ADDRESS,
        stage: ShaderStage::Vertex,
    }];

    let submission = pipeline.submit(&[0u8; 64], Queue::Compute, &registered, &memory());

    assert_eq!(
        submission.report.impossible_stages.len(),
        1,
        "{:?}",
        submission.report
    );
    assert!(submission.commands.is_empty(), "and it must not be bound");
}

/// A seeded generator, so a failure is reproducible.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

#[test]
fn an_arbitrary_command_stream_is_survived() {
    // A guest may submit anything: a buffer this understands none of, one truncated by a page
    // boundary, or register writes naming addresses that are not shaders. Each produces a report,
    // never a panic.
    let mut rng = Rng(0x00C0_FFEE);
    let memory = memory();

    for round in 0..256 {
        let length = (rng.next() % 128) as usize;
        let stream: Vec<u8> = (0..length * 4).map(|_| (rng.next() >> 24) as u8).collect();

        let mut pipeline = pipeline();
        let queue = if round % 2 == 0 {
            Queue::Draw
        } else {
            Queue::Compute
        };
        let submission = pipeline.submit(&stream, queue, &[], &memory);

        // Self-consistency: nothing is bound that was not translated, and nothing is both
        // translated and reported failed. Counted over bind commands only, since a stream can emit
        // render state it recognises (`SetRenderTargets`, `Draw`, `Dispatch`) without translating a
        // shader.
        let bound = submission
            .commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::BindShader { .. }))
            .count();
        assert!(
            bound <= submission.report.shaders_translated,
            "round {round}: {bound} bound shaders from {} translated",
            submission.report.shaders_translated
        );
        assert!(
            submission.report.shaders_translated + submission.report.failures.len()
                <= submission.report.shaders_found,
            "round {round}: more outcomes than shaders found - {:?}",
            submission.report
        );
    }
}

#[test]
fn a_truncated_command_stream_is_survived() {
    // A submission clipped at a page boundary: the last packet claims more words than remain, and
    // the walk stops rather than reading past the end.
    let memory = memory();
    let full = command_stream(SHADER_ADDRESS);

    for keep in 0..full.len() {
        let mut pipeline = pipeline();
        let submission = pipeline.submit(&full[..keep], Queue::Compute, &[], &memory);
        // No assertion about what it finds; the property is that it returns.
        let _ = submission.report.packets;
    }
}

#[test]
fn a_registered_shader_pointing_at_rubbish_is_reported_not_translated() {
    // Registration is believed over the register writes, so a wrong registration can point at an
    // unchecked address. It must come back as a named failure.
    let mut pipeline = pipeline();
    let rubbish = Mapping {
        base: SHADER_ADDRESS,
        // Never a terminator, so `decode_program` cannot find the end of a program.
        bytes: vec![0xAB; 4096],
    };
    let registered = [RegisteredShader {
        address: SHADER_ADDRESS,
        stage: ShaderStage::Compute,
    }];

    let submission = pipeline.submit(&[0u8; 64], Queue::Compute, &registered, &rubbish);
    assert_eq!(submission.report.shaders_translated, 0);
    assert_eq!(
        submission.report.failures.len(),
        1,
        "{:?}",
        submission.report
    );
    assert!(
        submission.commands.is_empty(),
        "nothing should be bound when nothing translated"
    );
}
