//! The console's own pixel shader, on a device.
//!
//! `orbistoun-gpu`'s oracle test translates it and `spirv-val` accepts the module. Valid and
//! *runnable* are different claims - a module can satisfy every structural rule and still ask
//! for a feature the device was not created with, or for a descriptor nothing bound - and this
//! crate is where the difference is measured, the same way it was for the hand-written modules
//! (D549, D550).
//!
//! The shader is the GL cube's untextured pixel shader, read from the payload image captured
//! beside the command stream in oracle record A: `crates/orbistoun-gpu/tests/captures`,
//! payload offset `0x300`. Nothing here re-translates anything the other test does not; it
//! takes the same bytes to the same translator and then keeps going.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_with;
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_spirv::interpolated_vertex_module;
use orbistoun_translate::wavefront::Stage;
use orbistoun_translate::{Fidelity, Strategy, Width, translate_staged};

fn device_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

/// Where the untextured pixel shader sits inside the captured payload image.
const FRAGMENT_OFFSET: usize = 0x300;

/// The payload image from oracle record A, as words.
///
/// Read out of the other crate's capture directory rather than copied here. A second copy of
/// a capture is a second thing to keep true, and the whole point of a capture is that there is
/// one of it.
fn payload() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("orbistoun-gpu")
        .join("tests")
        .join("captures")
        .join("agc-gl-cube-fw1240-a.payload.hex");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut bytes = Vec::new();
    for line in text.lines() {
        for word in line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace()
        {
            let value = u32::from_str_radix(word, 16)
                .unwrap_or_else(|e| panic!("{}: {word:?}: {e}", path.display()));
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

/// The console's pixel shader, translated for the fragment stage.
fn console_fragment_module(window: orbistoun_translate::wavefront::Window) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let payload = payload();
    // `decode_program`, not `decode`: a shader carries no length and the payload image holds
    // more than this shader, so the terminator is what says where it ends - which is exactly
    // what the submission pipeline does when it reads one out of guest memory.
    let decoded = decode_program(&payload[FRAGMENT_OFFSET..], &encodings, &operands);
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    };
    // **The window is a parameter now, and it has to be.** A pixel shader stores to guest
    // memory - the console's writes a canary - and where its window sits decides whether that
    // store lands or is refused. Translating it at the default while the vertex program was
    // translated somewhere else meant the two disagreed about what guest memory is, which is a
    // difference no picture would show.
    orbistoun_translate::translate_windowed(&decoded, &encodings, strategy, Stage::Fragment, window)
        .expect("the console's pixel shader translates")
        .module
}

/// **A shader a console ran, translated here, draws on a device.**
///
/// # What it asserts
///
/// The shader interpolates attribute zero and exports it, so with all three corners the same
/// colour every pixel is that colour exactly - no sample position and no rounding enters into
/// it. The clear is a colour the shader never writes, so a draw that produced nothing reads as
/// a different answer rather than an absent one.
///
/// # What it cannot assert
///
/// **That these are the console's pixels.** The geometry here is the oracle's hand-written
/// triangle, deliberately: this test isolates the pixel shader, so what it draws depends on
/// nothing else that could be wrong. The console's own vertex program does translate now, and
/// `the_consoles_two_shaders_draw_together` below is the pair running together.
///
/// The frame hash in the oracle record is still a different claim, and needs a vertex buffer
/// the record did not capture.
///
/// # Validation
///
/// Run it under the Khronos validation layer with `tools/validate-device.sh`. That is not a
/// detail: before the descriptor set and the device features arrived, this drew the right
/// colour *and* produced five validation errors, so the picture was the driver being tolerant
/// rather than a module that was correct (worklog 554).
#[test]
fn the_consoles_pixel_shader_draws_on_a_device() {
    if !device_or_skip("the_consoles_pixel_shader_draws_on_a_device") {
        return;
    }
    // The default window, because this test is about the picture and touches no guest memory.
    // Naming it rather than defaulting it keeps the choice visible: the other test below wants
    // a window somewhere else entirely, and the two are asking different questions.
    let module = console_fragment_module(orbistoun_translate::wavefront::Window::default());
    println!("module: {} words", module.len());

    let green = [0.0, 1.0, 0.0, 1.0];
    let red = [1.0, 0.0, 0.0, 1.0];
    let vertex = interpolated_vertex_module([green; 3]);
    let pixels = draw_with(&vertex, &module, red, 8, 5).expect("the console's pixel shader drew");

    for y in 0..5 {
        for x in 0..8 {
            assert_eq!(
                pixels.at(x, y),
                Some([0, 255, 0, 255]),
                "pixel ({x}, {y}) - red here is the clear, which means the shader wrote nothing"
            );
        }
    }
}

/// **The console's vertex program, translated for the mesh stage.**
///
/// Reported rather than asserted while the stage is new: what a driver and a validator make of
/// a translated primitive shader is the measurement this test exists to take.
#[test]
fn the_consoles_vertex_program_translates_for_the_mesh_stage() {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let payload = payload();
    let decoded = decode_program(&payload, &encodings, &operands);
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    };
    match translate_staged(&decoded, &encodings, strategy, Stage::Mesh) {
        Ok(translated) => {
            println!("translated: {} words", translated.module.len());
            let bytes: Vec<u8> = translated
                .module
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect();
            let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("target")
                .join("spirv");
            if std::fs::create_dir_all(&dir).is_ok() {
                let _ = std::fs::write(dir.join("console-vertex-mesh.spv"), bytes);
            }
        }
        Err(e) => println!("refused: {e}"),
    }
}

/// The low half of the guest address the console's vertex buffer sat at, `0x200900000`.
const VERTEX_BUFFER_BASE: u32 = 0x0090_0000;

/// Where the console's vertex program looks for each vertex, in the memory window.
///
/// Its address arithmetic is `vbo + lane * 48`, and the window is addressed by the module's own
/// mask - so a guest address lands at `(address >> 2) & 63`. The vertex buffer sat at
/// `0x200900000`, which masks to word zero, and each vertex is twelve words: four of position,
/// four of colour, four of texture coordinates.
const VERTEX_WORDS: [usize; 3] = [0, 12, 24];

/// The low half of the guest address the console's pixel shader stores its canary at,
/// `0x200920000`.
const CANARY_BASE: u32 = 0x0092_0000;

/// Which word of a window anchored at the vertex buffer the canary is.
///
/// A hundred and twenty-eight kilobytes further on, which is thirty-two thousand words. The
/// number is what makes the point: a window sized to hold a vertex buffer cannot reach it, and
/// that is why one span covering both had to be possible before this could be asked at all.
const CANARY_WORD: usize = ((CANARY_BASE - VERTEX_BUFFER_BASE) / 4) as usize;

/// Words in a window that covers the vertex buffer **and** the canary.
///
/// The next power of two past the canary's own index, because
/// [`Window::spanning`](orbistoun_translate::wavefront::Window::spanning) refuses anything else
/// - the index is kept legal by masking, and masking is only a bound on a power of two.
const SPANNING_WORDS: u32 = 1 << 16;

/// A value nothing else here writes, so a canary word that changed can only have been stored.
const SENTINEL: u32 = 0x5EED_1234;

/// What the console's pixel shader stores at its canary address.
///
/// **Measured, not assumed.** The word was seeded with [`SENTINEL`] in a host-visible buffer
/// this test wrote, the draw ran, and this is what came back - so it is the shader's own
/// constant, arriving through a translated store at a guest address. Pinned rather than printed
/// for the usual reason: a change that stopped the store landing would otherwise be a line of
/// output nobody was watching.
const CANARY_VALUE: u32 = 0xBEEF_0001;

/// The window both of the console's shaders are translated against.
///
/// One span, covering the vertex buffer the primitive shader reads and the canary the pixel
/// shader writes. A function rather than a constant because
/// [`Window::spanning`](orbistoun_translate::wavefront::Window::spanning) is fallible, and the
/// one way it fails - a length that is not a power of two - is a mistake in this file rather
/// than anything a run could produce.
fn window() -> orbistoun_translate::wavefront::Window {
    orbistoun_translate::wavefront::Window::spanning(VERTEX_BUFFER_BASE, SPANNING_WORDS)
        .expect("a power-of-two window length")
}

/// The console's whole frame path: its primitive shader and its pixel shader, together.
///
/// # What this is
///
/// Two shaders a console ran, translated here, drawn in one pipeline over vertices placed where
/// the primitive shader fetches them. The mesh module reads the positions and colours out of
/// the guest-memory window, declares three vertices and one primitive, exports them; the pixel
/// shader interpolates the colour and writes it. Nothing in either was written for this test.
///
/// # What it is not
///
/// **Not the console's frame.** The vertices here are this test's, because the oracle record
/// captured the command stream and the shader payload and not the vertex buffer.
///
/// # It draws, and it stores
///
/// Both halves are asserted. Every pixel is the colour the vertex carried, so the primitive
/// shader fetched real vertices and the pixel shader interpolated a real colour. And the word
/// the pixel shader's own canary store names comes back holding the console's constant rather
/// than the sentinel this test seeded - so the shader reached its store, computed a guest
/// address, and wrote there.
///
/// The seeding and the read-back are what make those two separable at all: the attachment says
/// what was drawn and the window says what was stored, and a guest's shaders do both. For a
/// long time this test could only report that neither happened (worklog 559).
///
/// # What the window had to become
///
/// The vertex buffer and the canary are a hundred and twenty-eight kilobytes apart, and a
/// window is one span. Anchored at zero, every fetch was refused (worklog 561). Sized to the
/// vertex buffer alone, the store was past the end and refused - correct behaviour, recorded as
/// unfinished (worklog 565). One span covering both, with **both shaders translated against
/// it**, is what this test now does. Translating the two against different windows would have
/// meant the pair disagreeing about what guest memory is, which no picture would show.
#[test]
fn the_consoles_two_shaders_draw_together() {
    use orbistoun_gpu_vulkan::framebuffer::draw_mesh_over;

    if !device_or_skip("the_consoles_two_shaders_draw_together") {
        return;
    }
    let mesh = {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let decoded = decode_program(&payload(), &encodings, &operands);
        let strategy = Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        };
        // **The window sits where the console's vertex buffer sat, and reaches as far as its
        // canary.** Its low half, because a flat access names its address in a register pair
        // and the translation reads the low one. Anchored at zero the shader's every fetch was
        // refused and it drew three identical vertices (worklog 561); sized to the vertex
        // buffer alone, the pixel shader's store to a word a hundred and twenty-eight kilobytes
        // further on was refused too, and both shaders have to agree about what memory is.
        orbistoun_translate::translate_windowed(
            &decoded,
            &encodings,
            strategy,
            Stage::Mesh,
            window(),
        )
        .expect("the vertex program translates")
        .module
    };
    let fragment = console_fragment_module(window());

    // A triangle covering the viewport, every corner the same colour, so every pixel is that
    // colour exactly and no sample position or rounding enters into it.
    let corners = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [3.0, -1.0, 0.0, 1.0],
        [-1.0, 3.0, 0.0, 1.0],
    ];
    let green = [0.0f32, 1.0, 0.0, 1.0];
    let mut memory = vec![0u32; SPANNING_WORDS as usize];
    for (vertex, at) in VERTEX_WORDS.into_iter().enumerate() {
        for component in 0..4 {
            memory[at + component] = corners[vertex][component].to_bits();
            memory[at + 4 + component] = green[component].to_bits();
        }
    }
    // The canary word, seeded with something the shader cannot have written, so "it stored
    // here" and "it stored nothing" are different answers rather than the same zero.
    memory[CANARY_WORD] = SENTINEL;

    let red = [1.0, 0.0, 0.0, 1.0];
    // Isolation: a fragment shader that writes one colour whatever it is given, so the picture
    // depends only on the positions the mesh module produced.
    let constant = orbistoun_spirv::constant_colour_fragment_module([0.0, 1.0, 0.0, 1.0]);
    let (by_constant, left) =
        draw_mesh_over(&mesh, &constant, red, 8, 5, &memory).expect("the constant draw ran");
    println!("constant-shaded first pixel: {:?}", by_constant.at(0, 0));
    println!("seeded words 0..4: {:08x?}", &left[0..4]);
    println!("canary words 0..8: {:08x?}", &left[0..8]);

    let (pixels, after) =
        draw_mesh_over(&mesh, &fragment, red, 8, 5, &memory).expect("the draw ran");
    for y in 0..5 {
        for x in 0..8 {
            assert_eq!(
                pixels.at(x, y),
                Some([0, 255, 0, 255]),
                "pixel ({x}, {y}) - red is the clear, so the pair drew nothing"
            );
        }
    }

    // **The canary lands.**
    //
    // A word a hundred and twenty-eight kilobytes past the vertex buffer, seeded with a value
    // this test invented, comes back holding the console's own constant. That is the shader
    // reaching its store, computing the guest address, and writing there - and it is the first
    // time anything in this project has observed a guest's shader change guest memory.
    //
    // It needed both halves of the window change. Anchored at zero, the address was refused
    // (worklog 561). Sized to the vertex buffer alone, the address was past the end and refused
    // again, which worklog 565 recorded as correct-but-unfinished. One span covering both, and
    // both shaders translated against it, is what makes the store reachable at all.
    assert_eq!(
        after[CANARY_WORD], CANARY_VALUE,
        concat!(
            "canary word {}: seeded {:#010x} and expected the shader's {:#010x}, read back ",
            "{:#010x} - the sentinel here means the store was refused or never reached"
        ),
        CANARY_WORD, SENTINEL, CANARY_VALUE, after[CANARY_WORD]
    );

    // The vertices are untouched, which is the claim that holds whatever the canary did: a
    // window wide enough to reach the canary must not have moved the buffer the other shader
    // reads, and a masked index that folded differently would show up right here.
    for at in VERTEX_WORDS {
        assert_eq!(
            after[at], memory[at],
            "vertex word {at} changed - a wider window must not move what the fetch reads"
        );
    }
}

/// A primitive shader with nothing in it but the four things a mesh module needs.
///
/// # Why this exists
///
/// The console's is 171,095 words translated and draws nothing (worklog 559). This is the
/// bisection: declare the counts, fetch a position per lane, export the primitive, export the
/// positions. If this draws, the difference is what the console's shader does *besides* those
/// four things; if it does not, the fault is in translating the stage and this is where to look.
///
/// Every word is a guest instruction. The two export words are the console's own, taken from
/// oracle record A: `0xf8000941 0x00000001` is its `exp prim` and `0xf80008cf 0x05040302` its
/// `exp pos0`, which is why this fetches into `v[2:5]` - so the same export means the same
/// thing here as there.
fn minimal_primitive_shader() -> Vec<u8> {
    minimal_with_exec(false)
}

/// The same shader, optionally narrowing the execution mask the way the console's does.
///
/// `exec_lo = 7` before the fetch and the exports, which is how a primitive shader says "three
/// of my lanes are vertices". The console's shader does this and draws nothing; this one does
/// not and draws. If adding it here stops the picture, that narrowing is the difference.
fn minimal_with_exec(narrow: bool) -> Vec<u8> {
    minimal_shader(narrow, false)
}

/// The same again, optionally storing a word to guest memory the way the console's does.
///
/// Its canary store is the one thing whose absence was *observed* after the failed draw, so it
/// is the next difference worth isolating.
fn minimal_shader(narrow: bool, store: bool) -> Vec<u8> {
    minimal_full(narrow, store, false)
}

/// The same again, optionally exporting a parameter as well as a position.
///
/// The console's shader exports two, at locations 0 and 1, and a parameter is the one output a
/// mesh module declares that the minimal shader has none of.
fn minimal_full(narrow: bool, store: bool, parameter: bool) -> Vec<u8> {
    minimal_outputs(narrow, store, u32::from(parameter))
}

/// The same again with a chosen number of parameter arrays, so the output shape can be matched
/// to the console's: it exports a position and **two** parameters.
fn minimal_outputs(narrow: bool, store: bool, parameters: u32) -> Vec<u8> {
    minimal_variant(Variant {
        narrow,
        store,
        parameters,
        carry: false,
    })
}

/// The same again, optionally forming the address the way the console does: a carry-producing
/// add over a 64-bit base held in two scalar registers, rather than the register directly.
/// What a variant of the minimal primitive shader includes.
///
/// A struct rather than five parameters, because five booleans at a call site say nothing about
/// which is which - and the point of these variants is that a reader can see what each one adds.
#[derive(Debug, Clone, Copy, Default)]
struct Variant {
    /// Narrow the execution mask to the three lanes that are vertices.
    narrow: bool,
    /// Store a word to guest memory, the way the console's shader stores its canary.
    store: bool,
    /// How many parameter arrays to export alongside the position. The console declares two.
    parameters: u32,
    /// Form the address with a carry-producing add over a scalar base, as the console does.
    carry: bool,
}

fn minimal_variant(variant: Variant) -> Vec<u8> {
    let Variant {
        narrow,
        store,
        parameters,
        carry,
    } = variant;
    let parameter = parameters >= 1;
    let mut words: Vec<u32> = Vec::new();
    if narrow {
        // s_mov_b32 exec_lo, 7 - the three lanes that are vertices.
        words.extend([0xbefe_03ff, 0x0000_0007]);
    }
    words.extend([
        // m0 = three vertices, one primitive - the pair the allocation request declares.
        0xbefc_03ff,
        0x0000_1003,
        // s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbf90_0009,
        // v14 = this lane's index, from a mask of all ones.
        0xd765_000e,
        0x0001_00c1,
        // v15 = lane * 16, v14 = lane * 32, v14 = lane * 48 - twelve words a vertex.
        0x341e_1c84,
        0x341c_1c85,
        0x4a1c_1d0f,
    ]);
    if carry {
        // The console's own address arithmetic: a 64-bit base in two scalar registers, added
        // to the offset with a carry out into `vcc` and a carry in on the high half. Its words,
        // with a base of zero so the window lands where the direct form put it.
        words.extend([0xbe82_03ff, 0x0000_0000]);
        words.extend([0xbe83_03ff, 0x0000_0000]);
        words.push(0x7e02_0280);
        words.extend([0xd70f_6a12, 0x0002_1c02]);
        words.push(0x5026_0203);
        // global_load_dwordx4 v[2:5], v[18:19], off
        words.extend([0xdc38_8000, 0x027d_0012]);
    } else {
        // The vertex, from guest memory at that offset. No scalar base: the address is the
        // register pair, which is the `off` form the console's own loads use.
        words.extend([0xdc38_8000, 0x027d_000e]);
    }
    words.extend([
        0xbf8c_3f70,
        // v1 = vertices 0, 1, 2 with edge flags - the console's packed primitive word.
        0x7e02_02ff,
        0x2028_0600,
        // exp prim, v1, off, off, off done
        0xf800_0941,
        0x0000_0001,
        // exp pos0, v2, v3, v4, v5 done
        0xf800_08cf,
        0x0504_0302,
    ]);
    if parameter {
        // exp param0, v2, v3, v4, v5 - the console's own parameter export word, with the
        // registers it already has.
        words.extend([0xf800_020f, 0x0504_0302]);
    }
    if parameters == 2 {
        // exp param1 as well, so the output shape matches the console's exactly: a position
        // and two parameter arrays.
        words.extend([0xf800_021f, 0x0504_0302]);
    }
    if store {
        // v20 = 0xbeef0001, then global_store_dword v[14:15], v20, off - the console's own
        // canary word through the console's own no-base store form.
        words.extend([0x7e28_02ff, 0xbeef_0001]);
        words.extend([0xdc70_8000, 0x007d_140e]);
        words.push(0xbf8c_3f70);
    }
    words.push(0xbf81_0000);
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// **A primitive shader draws, and the console's address is why its own does not.**
///
/// # What this establishes
///
/// The mesh translation works: twelve guest instructions - declare the counts, fetch a vertex
/// per lane, export the primitive, export the position - draw a triangle on a device. So does
/// the same shader with the execution mask narrowed, with a store to guest memory, with one
/// parameter export and with two, and with the console's own carry-producing address
/// arithmetic. Each was added to find what stopped the console's shader drawing; none of them
/// does (worklog 561).
///
/// **What stops it is the address.** The guest-memory window is the first `MEMORY_WORDS` words
/// of the address space - sixty-four of them, starting at zero - and every access is gated on
/// falling inside it, reading zero and writing nothing when it does not (D147). The console's
/// vertex buffer sat at `0x200900000`, which is not in the first sixty-four words of anything,
/// so its shader fetches zeros, exports three identical vertices, and its canary never lands.
/// The window is behaving exactly as designed and the design has no base yet - which
/// `MEMORY_WORDS` says in its own comment.
///
/// Reported rather than asserted while the pictures are the measurement.
#[test]
fn a_minimal_translated_primitive_shader_draws() {
    use orbistoun_gpu_vulkan::framebuffer::draw_mesh_over;

    if !device_or_skip("a_minimal_translated_primitive_shader_draws") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&minimal_primitive_shader(), &encodings, &operands);
    println!(
        "decoded {} instructions, trustworthy {}",
        decoded.instructions.len(),
        decoded.is_trustworthy()
    );
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    };
    let mesh = match translate_staged(&decoded, &encodings, strategy, Stage::Mesh) {
        Ok(translated) => translated.module,
        Err(e) => {
            println!("refused: {e}");
            return;
        }
    };
    println!("translated: {} words", mesh.len());

    let corners = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [3.0, -1.0, 0.0, 1.0],
        [-1.0, 3.0, 0.0, 1.0],
    ];
    let mut memory = vec![0u32; 64];
    for (vertex, at) in [0usize, 12, 24].into_iter().enumerate() {
        for component in 0..4 {
            memory[at + component] = corners[vertex][component].to_bits();
        }
    }
    let constant = orbistoun_spirv::constant_colour_fragment_module([0.0, 1.0, 0.0, 1.0]);
    let (pixels, _) = draw_mesh_over(&mesh, &constant, [1.0, 0.0, 0.0, 1.0], 8, 5, &memory)
        .expect("the draw ran");
    println!("first pixel: {:?}", pixels.at(0, 0));

    // The same shader with the console's execution-mask narrowing in front of it.
    let narrowed = decode_program(&minimal_with_exec(true), &encodings, &operands);
    let narrowed = translate_staged(&narrowed, &encodings, strategy, Stage::Mesh)
        .expect("the narrowed shader translates")
        .module;
    let (narrow_pixels, _) =
        draw_mesh_over(&narrowed, &constant, [1.0, 0.0, 0.0, 1.0], 8, 5, &memory)
            .expect("the narrowed draw ran");
    println!("first pixel, mask narrowed: {:?}", narrow_pixels.at(0, 0));

    // And with a store to guest memory, which is the difference whose absence was observed.
    let storing = decode_program(&minimal_shader(false, true), &encodings, &operands);
    let storing = translate_staged(&storing, &encodings, strategy, Stage::Mesh)
        .expect("the storing shader translates")
        .module;
    let (store_pixels, after) =
        draw_mesh_over(&storing, &constant, [1.0, 0.0, 0.0, 1.0], 8, 5, &memory)
            .expect("the storing draw ran");
    println!(
        "first pixel, with a store: {:?}; window word 0: {:08x}",
        store_pixels.at(0, 0),
        after[0]
    );

    // And with a parameter export, which is the output a mesh module declares that the minimal
    // shader has none of.
    let with_param = decode_program(&minimal_full(false, false, true), &encodings, &operands);
    let with_param = translate_staged(&with_param, &encodings, strategy, Stage::Mesh)
        .expect("the parameter shader translates")
        .module;
    let (param_pixels, _) =
        draw_mesh_over(&with_param, &constant, [1.0, 0.0, 0.0, 1.0], 8, 5, &memory)
            .expect("the parameter draw ran");
    println!("first pixel, with a parameter: {:?}", param_pixels.at(0, 0));

    // The console's whole output shape: a position and two parameter arrays.
    let both = decode_program(&minimal_outputs(false, false, 2), &encodings, &operands);
    let both = translate_staged(&both, &encodings, strategy, Stage::Mesh)
        .expect("the two-parameter shader translates")
        .module;
    let (both_pixels, _) = draw_mesh_over(&both, &constant, [1.0, 0.0, 0.0, 1.0], 8, 5, &memory)
        .expect("the two-parameter draw ran");
    println!(
        "first pixel, two parameters: {:?} ({} words)",
        both_pixels.at(0, 0),
        both.len()
    );

    // And the console's own address arithmetic, which is the last structural difference left.
    let carried = decode_program(
        &minimal_variant(Variant {
            carry: true,
            ..Variant::default()
        }),
        &encodings,
        &operands,
    );
    let carried = translate_staged(&carried, &encodings, strategy, Stage::Mesh)
        .expect("the carry shader translates")
        .module;
    let (carry_pixels, _) =
        draw_mesh_over(&carried, &constant, [1.0, 0.0, 0.0, 1.0], 8, 5, &memory)
            .expect("the carry draw ran");
    println!("first pixel, carry address: {:?}", carry_pixels.at(0, 0));
}
