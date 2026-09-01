//! A submitted command buffer, end to end.
//!
//! # What this is checking that nothing else was
//!
//! Every part of this path had tests and none of them met. The packet walker was tested
//! on packets, the register decoder on register writes, and the shader translator on
//! instruction streams its own tests handed it. Not one of them asked whether a shader
//! could be found *because a guest asked for it* - which is the only way a shader is
//! ever found in a real frame.
//!
//! So the fixture here is a command stream in the shape a guest submits: packets that
//! write the shader-address registers, pointing at a shader that lives in guest memory
//! at an address those registers name. Nothing calls the translator directly.

use orbistoun_gpu::pipeline::{GuestMemory, MAX_SHADER_BYTES, Pipeline, Queue, RegisteredShader};
use orbistoun_gpu::{RenderCommand, ShaderStage};
use orbistoun_shader::EncodingTable;
use orbistoun_translate::{Fidelity, Strategy, Width};

/// Guest memory as a single mapping at a known base.
///
/// A real address space is pages, permissions and holes. This is the smallest thing
/// that can answer the one question the pipeline asks, which is the point of the trait
/// being one method.
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

/// The first word of a SPIR-V module.
///
/// Written out rather than imported, so this test needs no dependency on the emitter to
/// say what it is asserting about the emitter's output.
const SPIRV_MAGIC: u32 = 0x0723_0203;

/// A shader that writes a constant and stops.
fn shader() -> Vec<u8> {
    // v_mov_b32 v0, 9 ; s_endpgm
    let words: [u32; 2] = [0x7E00_0000 | (1 << 9) | (128 + 9), 0xBF81_0000];
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Guest memory holding the shader, and rubbish after it.
///
/// The rubbish matters: a shader in memory has no length, and the pipeline reads a
/// window rather than a shader. If decoding did not stop at the end of the program it
/// would decode this too, desynchronise, and report a perfectly good shader as
/// untrustworthy.
fn memory() -> Mapping {
    let mut bytes = shader();
    bytes.extend(std::iter::repeat_n(0xAB, 4096));
    Mapping {
        base: SHADER_ADDRESS,
        bytes,
    }
}

/// A command stream that sets the compute shader address, as a guest would.
///
/// The register numbers come from the crate's own vocabulary, so this is not asserting
/// on numbers invented here - the same table the emulator would use decides which
/// registers name a shader.
fn command_stream(address: u64) -> Vec<u8> {
    let vocabulary = orbistoun_gpu::registers::Vocabulary::builtin().expect("vocabulary");
    let (low, high) = shader_registers(&vocabulary);
    // Which opcode reaches these registers is decided by the registers. Several opcodes
    // write registers, each to a class with its own base.
    let (opcode, base) = vocabulary
        .opcode_for_register(low)
        .expect("an opcode reaching the shader address registers");
    // Two writes, each a header, a register offset and a value. Written as separate
    // packets so the offsets are obvious rather than as one run.
    let mut words: Vec<u32> = Vec::new();
    for (register, value) in [
        (low, u32::try_from(address & 0xFFFF_FFFF).expect("low half")),
        (high, u32::try_from(address >> 32).expect("high half")),
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

#[test]
fn the_same_shader_is_translated_once() {
    // A guest rebinds the same shader every draw. Translating it each time would cost
    // more than everything else this does, so the second submission must hit the cache
    // and must not re-send the module.
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

    // And the command is still emitted, or caching would have silently dropped the draw.
    assert_eq!(second.commands.len(), first.commands.len());
}

#[test]
fn two_addresses_holding_the_same_shader_share_one_translation() {
    // The cache is keyed on the shader's bytes, not its address. A guest may hold the
    // same shader at two addresses, and translating it twice would be waste; more to the
    // point, keying on the address would mean a shader *moving* looked like a new one
    // and a shader *replaced in place* looked like the old one - and only the second of
    // those is visible as a wrong frame.
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
    // The failure a wrong register decode produces, and the one that must never be
    // silent. A frame missing a draw is visible; a frame where the draw quietly drew
    // nothing is a week of somebody's life.
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
    // The open half of D101 is whether a GPU virtual address is a guest address. Nothing
    // here can answer that in the abstract - but every address a submission names is a
    // test of it, and the answers add up.
    //
    // Counted apart from whether the shader translated, because those are different
    // questions with different answers: an address can resolve perfectly and its shader
    // still be refused, and folding the two together loses the only measurement available
    // for the assumption.
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

    // An address it does not: the submission names somewhere unmapped, so the assumption
    // failed *there* - and the report says so without blaming the shader.
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
    // The shape a real call site has: a guest builds a command buffer somewhere in its own
    // memory and passes a pointer and a length. Nothing calls this yet - no guest has
    // reached a submission - so this test is what says the entry point works, and it is
    // the difference between wiring a shim to a function later and designing an interface
    // under time pressure later.
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
    // The pointer here came from the guest's own CPU-side code, so if it does not resolve
    // the fault is in the shim's arguments rather than in any assumption about GPU
    // addresses - which is a different thing to suspect, and why this answers `None`
    // rather than producing an empty submission that reads as "nothing to do".
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
    // D112 calls the 64 KiB window "a guess with no real shader to check it against".
    // There are real shaders now and the largest is 320 bytes, so the number is generous
    // by two orders of magnitude - but that is the less important half.
    //
    // The half that matters is what happens when a shader *does* exceed it. Truncating
    // would hand the translator a fragment of a real shader, which decodes cleanly right
    // up to the cut and produces a module that is a genuine prefix of the right one -
    // plausible, wrong, and with nothing to indicate it. Refusing says the window is too
    // small, which is a fact somebody can act on.
    //
    // Untested until now, which is the usual way a guess stays a guess.
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
    // The address itself was fine - this is a window problem, and the report should not
    // count it against the assumption that GPU and guest addresses coincide.
    assert_eq!(report.addresses_resolved, 1);
    assert_eq!(report.addresses_unresolved, 0);
}

#[test]
fn a_shader_that_does_not_translate_is_reported_with_its_reason() {
    // Not every instruction is translated yet, and most real shaders will contain one
    // that is not. That has to arrive as a named failure a worklist can rank, which is
    // the same shape the import survey uses one layer up.
    let mut pipeline = pipeline();

    // An export. Chosen because it is *blocked on a subsystem* rather than merely
    // unimplemented - it needs a render target and there is no concept of one - so it
    // will not quietly become supported and turn this test green for the wrong reason.
    // The instruction that was here before did exactly that: it named a vector subtract,
    // which was translated an hour later, and the test then passed while asserting
    // nothing.
    //
    // Its bytes are **asked for by family name**, not written down. A written-down
    // export word is a word for one architecture generation, and on the next one it
    // matches no family at all - so the shader fails to *decode* rather than failing to
    // *translate*, and this test starts asserting the wrong thing while still failing
    // for a reason that looks like a decoder bug. Family names survive a retarget;
    // encodings do not (D139).
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
    // The honest empty answer. A stream this understands nothing of produces no
    // commands and a report saying how little it recognised - which is what a worklist
    // needs, and is different from an error.
    let mut pipeline = pipeline();
    let submission = pipeline.submit(&[0u8; 64], Queue::Compute, &[], &memory());

    assert_eq!(submission.report.shaders_found, 0);
    assert!(submission.commands.is_empty());
    assert!(submission.report.failures.is_empty());
}

#[test]
fn a_registered_shader_is_believed_over_the_register_writes() {
    // Two routes to a shader address: the guest registering it by name, and a register
    // write in the submitted packets pointing at it. Registration is *stated* and the
    // register path is *inferred* from a table this crate's own data file calls the
    // least certain thing in it - so where they overlap, registration wins.
    let mut pipeline = pipeline();
    let registered = [RegisteredShader {
        address: SHADER_ADDRESS,
        stage: ShaderStage::Compute,
    }];

    // The packets point somewhere unmapped. If the inferred address were used, this
    // would fail to read; if registration is believed, it translates.
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
    // The most useful line in the report, and the reason both routes keep running even
    // once one has answered. The registered address is what the guest said; a mismatch
    // means the register vocabulary found the wrong bits, and nothing else in this crate
    // can tell you that.
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
    // Agreement is the only evidence available that the register vocabulary is right.
    // It is worth counting for the same reason the disagreement is worth listing.
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
    // A vertex shader named by a compute submission is a decode that went wrong. Dropping
    // it quietly would hide exactly the signal that says so.
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
    // The pipeline's input is a buffer a guest submitted, and a guest is entitled to
    // submit anything - including a buffer this understands none of, one truncated by a
    // page boundary, or one whose register writes name addresses that are not shaders.
    //
    // Every one of those must produce a report rather than a panic. A crash here is the
    // emulator dying on a frame it could have skipped, which is a far worse failure than
    // a frame that draws nothing.
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

        // Whatever it decided, it must be self-consistent: nothing may be bound that was
        // not translated, and nothing may be both translated and reported as failed.
        assert!(
            submission.commands.len() <= submission.report.shaders_translated,
            "round {round}: {} commands from {} translated shaders",
            submission.commands.len(),
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
    // A submission clipped at a page boundary: the last packet claims more words than
    // remain. The walk has to stop rather than read past the end.
    let memory = memory();
    let full = command_stream(SHADER_ADDRESS);

    for keep in 0..full.len() {
        let mut pipeline = pipeline();
        let submission = pipeline.submit(&full[..keep], Queue::Compute, &[], &memory);
        // No assertion about what it finds - a clipped stream may legitimately contain a
        // complete register write or none. The property is that it returns at all.
        let _ = submission.report.packets;
    }
}

#[test]
fn a_registered_shader_pointing_at_rubbish_is_reported_not_translated() {
    // Registration is believed over the register writes, so a wrong registration is the
    // one input that can send this at an address nothing checked. It has to come back as
    // a named failure.
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
