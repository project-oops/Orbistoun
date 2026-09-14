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
fn console_fragment_module() -> Vec<u32> {
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
    translate_staged(&decoded, &encodings, strategy, Stage::Fragment)
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
/// **That these are the console's pixels.** The console drew this shader over its own vertex
/// program, which does not translate yet (D688), so the geometry here is the oracle's
/// hand-written triangle. This says the shader runs and computes with what it is given; the
/// frame hash in the oracle record is a different claim and needs the other half.
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
    let module = console_fragment_module();
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

/// Where the console's vertex program looks for each vertex, in the memory window.
///
/// Its address arithmetic is `vbo + lane * 48`, and the window is addressed by the module's own
/// mask - so a guest address lands at `(address >> 2) & 63`. The vertex buffer sat at
/// `0x200900000`, which masks to word zero, and each vertex is twelve words: four of position,
/// four of colour, four of texture coordinates.
const VERTEX_WORDS: [usize; 3] = [0, 12, 24];

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
/// # And it does not draw yet, which is the measurement
///
/// Reported rather than asserted, because what it reports is an open question rather than a
/// property to defend. The module is valid - `spirv-val` accepts it and the validation layer
/// is silent on the draw - and **nothing happens**: the attachment keeps its clear, and the
/// guest-memory window still holds exactly what this test seeded, including at the word the
/// shader's own canary store would have overwritten. So the shader is not running, rather than
/// running and producing a degenerate triangle (worklog 559).
///
/// The seeding and the read-back exist so that distinction can be made at all: the attachment
/// says what was drawn and the window says what was stored, and a guest's shaders do both.
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
        translate_staged(&decoded, &encodings, strategy, Stage::Mesh)
            .expect("the vertex program translates")
            .module
    };
    let fragment = console_fragment_module();

    // A triangle covering the viewport, every corner the same colour, so every pixel is that
    // colour exactly and no sample position or rounding enters into it.
    let corners = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [3.0, -1.0, 0.0, 1.0],
        [-1.0, 3.0, 0.0, 1.0],
    ];
    let green = [0.0f32, 1.0, 0.0, 1.0];
    let mut memory = vec![0u32; 64];
    for (vertex, at) in VERTEX_WORDS.into_iter().enumerate() {
        for component in 0..4 {
            memory[at + component] = corners[vertex][component].to_bits();
            memory[at + 4 + component] = green[component].to_bits();
        }
    }

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
    println!("first pixel: {:?}", pixels.at(0, 0));

    // The shader's own canary, which it stores at the end. Its absence is the finding: the
    // seeded word is still there, so nothing the shader does reached memory.
    println!("window word 0 after the draw: {:08x}", after[0]);
    assert_eq!(
        after[0], memory[0],
        "the window changed, so the shader did run - this test is the record of that and wants 
         turning back into an assertion about the picture"
    );
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
