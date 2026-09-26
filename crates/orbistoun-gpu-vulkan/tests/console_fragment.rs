//! A pixel shader the hardware ran, on a device (D549).
//!
//! `orbistoun-gpu`'s oracle test translates it and `spirv-val` accepts the module; valid and
//! runnable are different claims, since a module can ask for a feature the device lacks or a
//! descriptor nothing bound. The shader is the GL cube's untextured pixel shader, read from the
//! payload image captured beside the command stream in oracle record A
//! (`crates/orbistoun-gpu/tests/captures`, payload offset `0x300`).

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

/// The payload image from oracle record A, read from the other crate's capture directory so the
/// capture has one copy.
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

/// The hardware's pixel shader, translated for the fragment stage.
fn console_fragment_module(window: orbistoun_translate::wavefront::Window) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let payload = payload();
    // `decode_program`, not `decode`: a shader carries no length and the payload image holds more
    // than this shader, so the terminator says where it ends, as when the submission pipeline reads
    // one from guest memory.
    let decoded = decode_program(&payload[FRAGMENT_OFFSET..], &encodings, &operands);
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    };
    // The window is a parameter: a pixel shader stores to guest memory, and both shaders of a pair
    // must be translated against the same window to agree about what guest memory is.
    orbistoun_translate::translate_windowed(&decoded, &encodings, strategy, Stage::Fragment, window)
        .expect("the console's pixel shader translates")
        .module
}

/// A shader the hardware ran, translated here, draws on a device.
///
/// The shader interpolates attribute zero and exports it, so with all three corners one colour
/// every pixel is that colour exactly. The clear is a colour the shader never writes. The geometry
/// is the oracle's hand-written triangle, isolating the pixel shader. Run it under the Khronos
/// validation layer with `tools/validate-device.sh`: a correct picture can hide validation errors.
#[test]
fn the_consoles_pixel_shader_draws_on_a_device() {
    if !device_or_skip("the_consoles_pixel_shader_draws_on_a_device") {
        return;
    }
    // The default window: this test is about the picture and touches no guest memory.
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

/// The hardware's vertex program, translated for the mesh stage. Reported rather than asserted:
/// what a driver and a validator make of a translated primitive shader is the measurement.
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

/// Where the hardware's vertex program looks for each vertex, in the memory window.
///
/// Its address arithmetic is `vbo + lane * 48`, and the window is addressed by the module's own
/// mask, so a guest address lands at `(address >> 2) & 63`. The vertex buffer at `0x200900000`
/// masks to word zero, and each vertex is twelve words: four of position, four of colour, four of
/// texture coordinates.
const VERTEX_WORDS: [usize; 3] = [0, 12, 24];

/// The low half of the guest address the pixel shader stores its canary at, `0x200920000`.
const CANARY_BASE: u32 = 0x0092_0000;

/// Which word of a window anchored at the vertex buffer the canary is: 128 KiB further on, out of
/// reach of a window sized to the vertex buffer alone.
const CANARY_WORD: usize = ((CANARY_BASE - VERTEX_BUFFER_BASE) / 4) as usize;

/// Words in a window that covers the vertex buffer and the canary.
///
/// The next power of two past the canary's index, because
/// [`Window::spanning`](orbistoun_translate::wavefront::Window::spanning) requires one: the index
/// is kept legal by masking.
const SPANNING_WORDS: u32 = 1 << 16;

/// A value nothing else here writes, so a canary word that changed can only have been stored.
const SENTINEL: u32 = 0x5EED_1234;

/// What the hardware's pixel shader stores at its canary address.
///
/// Measured: the word was seeded with [`SENTINEL`], the draw ran, and this came back, so it is the
/// shader's own constant arriving through a translated store.
const CANARY_VALUE: u32 = 0xBEEF_0001;

/// The window both shaders are translated against: one span covering the vertex buffer the
/// primitive shader reads and the canary the pixel shader writes. A function because
/// [`Window::spanning`](orbistoun_translate::wavefront::Window::spanning) is fallible, and its one
/// failure, a length that is not a power of two, would be a mistake in this file.
fn window() -> orbistoun_translate::wavefront::Window {
    orbistoun_translate::wavefront::Window::spanning(VERTEX_BUFFER_BASE, SPANNING_WORDS)
        .expect("a power-of-two window length")
}

/// The hardware's frame path: its primitive shader and its pixel shader, together.
///
/// Two shaders the hardware ran, translated against one window and drawn in one pipeline over
/// vertices placed where the primitive shader fetches them; the vertices are this test's, since the
/// oracle record holds no vertex buffer. Both halves are asserted: every pixel is the colour the
/// vertex carried, and the canary word holds the shader's constant rather than the seeded sentinel,
/// so the shader computed a guest address and wrote there.
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
        // The window sits where the vertex buffer sat and reaches as far as the canary. Its low
        // half, because a flat access names its address in a register pair and the translation
        // reads the low one.
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

    // A triangle covering the viewport, every corner one colour, so every pixel is that colour
    // exactly.
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
    // The canary word, seeded with something the shader cannot write, so "stored here" and "stored
    // nothing" differ.
    memory[CANARY_WORD] = SENTINEL;

    let red = [1.0, 0.0, 0.0, 1.0];
    // A fragment shader writing one colour whatever it is given, so the picture depends only on the
    // positions the mesh module produced.
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

    // The canary lands: a word 128 KiB past the vertex buffer, seeded by this test, holds the
    // shader's own constant.
    assert_eq!(
        after[CANARY_WORD], CANARY_VALUE,
        concat!(
            "canary word {}: seeded {:#010x} and expected the shader's {:#010x}, read back ",
            "{:#010x} - the sentinel here means the store was refused or never reached"
        ),
        CANARY_WORD, SENTINEL, CANARY_VALUE, after[CANARY_WORD]
    );

    // The vertices are untouched: a window wide enough to reach the canary must not move the buffer
    // the other shader reads.
    for at in VERTEX_WORDS {
        assert_eq!(
            after[at], memory[at],
            "vertex word {at} changed - a wider window must not move what the fetch reads"
        );
    }
}

/// A primitive shader with only the four things a mesh module needs: declare the counts, fetch a
/// position per lane, export the primitive, export the positions.
///
/// Every word is a guest instruction. The two export words are from oracle record A: `0xf8000941
/// 0x00000001` is `exp prim` and `0xf80008cf 0x05040302` is `exp pos0`, which is why this fetches
/// into `v[2:5]`.
fn minimal_primitive_shader() -> Vec<u8> {
    minimal_with_exec(false)
}

/// The same shader, optionally narrowing the execution mask as the hardware's does: `exec_lo = 7`
/// before the fetch and the exports, three lanes being vertices.
fn minimal_with_exec(narrow: bool) -> Vec<u8> {
    minimal_shader(narrow, false)
}

/// The same again, optionally storing a word to guest memory as the hardware's shader stores its
/// canary.
fn minimal_shader(narrow: bool, store: bool) -> Vec<u8> {
    minimal_full(narrow, store, false)
}

/// The same again, optionally exporting a parameter as well as a position.
fn minimal_full(narrow: bool, store: bool, parameter: bool) -> Vec<u8> {
    minimal_outputs(narrow, store, u32::from(parameter))
}

/// The same again with a chosen number of parameter arrays; the hardware's shader exports a
/// position and two parameters.
fn minimal_outputs(narrow: bool, store: bool, parameters: u32) -> Vec<u8> {
    minimal_variant(Variant {
        narrow,
        store,
        parameters,
        carry: false,
    })
}

/// What a variant of the minimal primitive shader includes. A struct, so a call site says which
/// part each variant adds.
#[derive(Debug, Clone, Copy, Default)]
struct Variant {
    /// Narrow the execution mask to the three lanes that are vertices.
    narrow: bool,
    /// Store a word to guest memory, the way the console's shader stores its canary.
    store: bool,
    /// How many parameter arrays to export alongside the position. The hardware's shader declares
    /// two.
    parameters: u32,
    /// Form the address with a carry-producing add over a scalar base, as the hardware's shader
    /// does.
    carry: bool,
}

/// The minimal primitive shader with the parts `variant` names.
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
        // m0 = three vertices, one primitive: the pair the allocation request declares.
        0xbefc_03ff,
        0x0000_1003,
        // s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbf90_0009,
        // v14 = this lane's index, from a mask of all ones.
        0xd765_000e,
        0x0001_00c1,
        // v15 = lane * 16, v14 = lane * 32, v14 = lane * 48: twelve words a vertex.
        0x341e_1c84,
        0x341c_1c85,
        0x4a1c_1d0f,
    ]);
    if carry {
        // The hardware shader's address arithmetic: a 64-bit base in two scalar registers, added to
        // the offset with a carry out into `vcc` and a carry in on the high half, with a base of
        // zero.
        words.extend([0xbe82_03ff, 0x0000_0000]);
        words.extend([0xbe83_03ff, 0x0000_0000]);
        words.push(0x7e02_0280);
        words.extend([0xd70f_6a12, 0x0002_1c02]);
        words.push(0x5026_0203);
        // global_load_dwordx4 v[2:5], v[18:19], off
        words.extend([0xdc38_8000, 0x027d_0012]);
    } else {
        // The vertex, from guest memory at that offset, by the register-pair `off` form.
        words.extend([0xdc38_8000, 0x027d_000e]);
    }
    words.extend([
        0xbf8c_3f70,
        // v1 = vertices 0, 1, 2 with edge flags: the packed primitive word.
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
        // exp param0, v2, v3, v4, v5: the hardware shader's parameter export word.
        words.extend([0xf800_020f, 0x0504_0302]);
    }
    if parameters == 2 {
        // exp param1 as well, so the output shape is a position and two parameter arrays.
        words.extend([0xf800_021f, 0x0504_0302]);
    }
    if store {
        // v20 = 0xbeef0001, then global_store_dword v[14:15], v20, off: the canary word through the
        // no-base store form.
        words.extend([0x7e28_02ff, 0xbeef_0001]);
        words.extend([0xdc70_8000, 0x007d_140e]);
        words.push(0xbf8c_3f70);
    }
    words.push(0xbf81_0000);
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// The minimal primitive shader and each variant draw a triangle on a device.
///
/// Twelve guest instructions (declare the counts, fetch a vertex per lane, export the primitive,
/// export the position) draw, as do the variants with a narrowed execution mask, a store to guest
/// memory, one or two parameter exports, and the carry-producing address arithmetic. Accesses
/// outside the guest-memory window read zero and write nothing (D147). Reported rather than
/// asserted: the pictures are the measurement.
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

    // With the execution-mask narrowing in front of it.
    let narrowed = decode_program(&minimal_with_exec(true), &encodings, &operands);
    let narrowed = translate_staged(&narrowed, &encodings, strategy, Stage::Mesh)
        .expect("the narrowed shader translates")
        .module;
    let (narrow_pixels, _) =
        draw_mesh_over(&narrowed, &constant, [1.0, 0.0, 0.0, 1.0], 8, 5, &memory)
            .expect("the narrowed draw ran");
    println!("first pixel, mask narrowed: {:?}", narrow_pixels.at(0, 0));

    // With a store to guest memory.
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

    // With a parameter export.
    let with_param = decode_program(&minimal_full(false, false, true), &encodings, &operands);
    let with_param = translate_staged(&with_param, &encodings, strategy, Stage::Mesh)
        .expect("the parameter shader translates")
        .module;
    let (param_pixels, _) =
        draw_mesh_over(&with_param, &constant, [1.0, 0.0, 0.0, 1.0], 8, 5, &memory)
            .expect("the parameter draw ran");
    println!("first pixel, with a parameter: {:?}", param_pixels.at(0, 0));

    // A position and two parameter arrays.
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

    // With the carry-producing address arithmetic.
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
