//! A draw's bound buffers on a device (D733).
//!
//! A pixel shader builds a raw buffer descriptor in its scalar registers, as radeonsi's clear does
//! (`si_nir_lower_resource.c:27-51`), reads four words through it with one scalar buffer load and
//! exports them as its colour. The buffer is bound through the backend's `BindDrawBuffers`; the
//! words inside the descriptor's record count are the bound bytes, and the words past it read zero,
//! because from GFX8 an out-of-bounds scalar access does not reach memory (Mesa `ac_gpu_info.h:187`,
//! `ac_nir_lower_mem_access_bit_sizes.c:212-217`).

use orbistoun_gpu::{DrawBuffer, RenderBackend, RenderCommand, Resource, ResourceId, ShaderStage};
use orbistoun_gpu_vulkan::VulkanBackend;
use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_spirv::fullscreen_triangle_vertex_module_at_depth;
use orbistoun_translate::wavefront::{MeshPrimitive, Stage, UserData, Window};
use orbistoun_translate::{Fidelity, Strategy, Width, translate_with_user_data};

/// Reports the device, or skips loudly.
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

const TARGET: ResourceId = ResourceId(1 << 63 | 16 << 16 | 16);
const DEPTH: ResourceId = ResourceId(1 << 62 | 0x100);
const TRIANGLE: ResourceId = ResourceId(1);
const PIXEL: ResourceId = ResourceId(2);

/// The pixel shader: a raw descriptor over user-data word 0 with `records` bytes, four words read
/// through it, exported as the colour.
fn pixel_shader(records: u32) -> Vec<u32> {
    assert!(records <= 64, "an inline constant");
    let words = [
        0xbe84_0300,           // s_mov_b32 s4, s0
        0xbe85_0380,           // s_mov_b32 s5, 0
        0xbe86_0380 + records, // s_mov_b32 s6, records
        0xbe87_03ff,           // s_mov_b32 s7, 0x31016fac
        0x3101_6fac,
        0xf428_0202, // s_buffer_load_dwordx4 s[8:11], s[4:7], 0x0
        0xfa00_0000,
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0x7e00_0208, // v_mov_b32 v0, s8
        0x7e02_0209, // v_mov_b32 v1, s9
        0x7e04_020a, // v_mov_b32 v2, s10
        0x7e06_020b, // v_mov_b32 v3, s11
        0xf800_180f, // exp mrt0 v0, v1, v2, v3 done vm
        0x0302_0100,
        0xbf81_0000, // s_endpgm
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode_program(&bytes, &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        },
        (Stage::Fragment, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 1,
            block_offset: 16,
            draw_buffers: true,
            ..UserData::default()
        },
    )
    .expect("the pixel shader translates")
    .module
}

/// The colour the buffer holds: red, green, blue and alpha of one, zero, one, one.
fn buffer() -> DrawBuffer {
    let words = [1.0f32, 0.0, 1.0, 1.0];
    let bytes: Vec<u8> = words
        .iter()
        .flat_map(|w| w.to_bits().to_le_bytes())
        .collect();
    DrawBuffer {
        hash: orbistoun_gpu::content_hash(&words.map(f32::to_bits)),
        bytes: bytes.into(),
        base: 0,
    }
}

/// Draws the covering triangle with the pixel shader over a descriptor of `records` bytes, through
/// the resident attachment when `depth`, and answers the centre pixel.
fn centre(records: u32, depth: bool) -> [u8; 4] {
    let mut backend = VulkanBackend::new();
    let pixel = pixel_shader(records);
    let triangle = fullscreen_triangle_vertex_module_at_depth(0.5);
    for (id, words) in [(TRIANGLE, &triangle), (PIXEL, &pixel)] {
        backend
            .ensure_resident(id, Resource::Shader(words))
            .expect("resident");
    }
    backend
        .ensure_resident(
            TARGET,
            Resource::RenderTarget {
                width: 16,
                height: 16,
                srgb: false,
            },
        )
        .expect("target resident");
    let mut words = [0u32; orbistoun_gpu::USER_DATA_WORDS];
    words[0] = 0x0010_0000;
    let commands = [
        RenderCommand::SetRenderTargets {
            colour: vec![TARGET],
            depth: depth.then_some(DEPTH),
        },
        RenderCommand::SetUserData {
            stage: ShaderStage::Fragment,
            words,
        },
        RenderCommand::BindDrawBuffers {
            stage: ShaderStage::Fragment,
            buffers: vec![buffer()],
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Vertex,
            shader: TRIANGLE,
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Fragment,
            shader: PIXEL,
        },
        RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        },
    ];
    for command in &commands {
        backend.execute(command).expect("the command runs");
    }
    backend
        .last_frame()
        .expect("a frame")
        .at(8, 8)
        .expect("the centre")
}

/// A descriptor covering all four words reads the bound bytes: the colour is the buffer's.
#[test]
fn a_bound_buffer_read_returns_its_bytes() {
    if !device_or_skip("a_bound_buffer_read_returns_its_bytes") {
        return;
    }
    assert_eq!(centre(16, false), [255, 0, 255, 255]);
    assert_eq!(
        centre(16, true),
        [255, 0, 255, 255],
        "and on the resident path"
    );
}

/// A descriptor of eight bytes admits the first two words; the other two are past its record count
/// and read zero though the bound buffer holds them.
#[test]
fn a_read_past_the_record_count_is_zero() {
    if !device_or_skip("a_read_past_the_record_count_is_zero") {
        return;
    }
    assert_eq!(centre(8, false), [255, 0, 0, 0]);
    assert_eq!(centre(8, true), [255, 0, 0, 0], "and on the resident path");
}

/// A pixel shader that reads the bound buffer with `buffer_load_format_xyzw` through a descriptor
/// whose fourth word is `word3`, translated for a draw whose descriptor names that word (D738),
/// and exports the four channels.
fn format_pixel_shader(word3: u32) -> Vec<u32> {
    let program = [
        0xbe84_0300, // s_mov_b32 s4, s0
        0xbe85_0380, // s_mov_b32 s5, 0
        0xbe86_0390, // s_mov_b32 s6, 16
        0xbe87_03ff, // s_mov_b32 s7, word3
        word3,
        0xe00c_0000, // buffer_load_format_xyzw v[0:3], off, s[4:7], 0
        0x8001_0000,
        0xbf8c_0000, // s_waitcnt
        0xf800_180f, // exp mrt0 v0, v1, v2, v3 done vm
        0x0302_0100,
        0xbf81_0000, // s_endpgm
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode_program(&bytes, &encodings, &operands);
    let mut formats = orbistoun_translate::wavefront::BufferFormats::default();
    formats.0[0] = Some(word3);
    translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        },
        (Stage::Fragment, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 1,
            block_offset: 16,
            draw_buffers: true,
            buffer_formats: Some(formats),
            ..UserData::default()
        },
    )
    .expect("the format load translates")
    .module
}

/// Draws with [`format_pixel_shader`] over one word of bytes `[0xff, 0x00, 0x80, 0x40]`, and
/// answers the centre pixel.
fn format_centre(word3: u32) -> [u8; 4] {
    let mut backend = VulkanBackend::new();
    let pixel = format_pixel_shader(word3);
    let triangle = fullscreen_triangle_vertex_module_at_depth(0.5);
    for (id, words) in [(TRIANGLE, &triangle), (PIXEL, &pixel)] {
        backend
            .ensure_resident(id, Resource::Shader(words))
            .expect("resident");
    }
    backend
        .ensure_resident(
            TARGET,
            Resource::RenderTarget {
                width: 16,
                height: 16,
                srgb: false,
            },
        )
        .expect("target resident");
    let mut user_data = [0u32; orbistoun_gpu::USER_DATA_WORDS];
    user_data[0] = 0x0010_0000;
    let texel = [0xff_u8, 0x00, 0x80, 0x40];
    let buffer = DrawBuffer {
        hash: orbistoun_gpu::content_hash(&[u32::from_le_bytes(texel)]),
        bytes: texel.to_vec().into(),
        base: 0,
    };
    let commands = [
        RenderCommand::SetRenderTargets {
            colour: vec![TARGET],
            depth: None,
        },
        RenderCommand::SetUserData {
            stage: ShaderStage::Fragment,
            words: user_data,
        },
        RenderCommand::BindDrawBuffers {
            stage: ShaderStage::Fragment,
            buffers: vec![buffer],
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Vertex,
            shader: TRIANGLE,
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Fragment,
            shader: PIXEL,
        },
        RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        },
    ];
    for command in &commands {
        backend.execute(command).expect("the command runs");
    }
    backend
        .last_frame()
        .expect("a frame")
        .at(8, 8)
        .expect("the centre")
}

/// A format load converts by its descriptor's format and selects (D738): `8_8_8_8_UNORM` under the
/// identity selects is the four bytes as UNORM; `8_8_UNORM` under `(X, Y, 0, 1)` is the first two
/// and the constants; and the same word read `(Z, Y, X, W)` swaps red and blue.
#[test]
fn a_format_load_converts_by_its_descriptor() {
    if !device_or_skip("a_format_load_converts_by_its_descriptor") {
        return;
    }
    // `OOB_SELECT` raw (3 << 28), `FORMAT` at 18:12, the selects at 11:0.
    let word3 = |format: u32, selects: [u32; 4]| {
        3 << 28 | format << 12 | selects[0] | selects[1] << 3 | selects[2] << 6 | selects[3] << 9
    };
    assert_eq!(format_centre(word3(56, [4, 5, 6, 7])), [255, 0, 128, 64]);
    assert_eq!(format_centre(word3(14, [4, 5, 0, 1])), [255, 0, 0, 255]);
    assert_eq!(format_centre(word3(56, [6, 5, 4, 7])), [128, 0, 255, 64]);
}

/// Without a draw's descriptors a format load is not translated: its conversion is the draw's
/// (D738), and the pipeline translates it again per draw on this refusal.
#[test]
fn a_format_load_without_a_draws_descriptors_asks_for_them() {
    let words: [u32; 10] = [
        0xbe84_0300, // s_mov_b32 s4, s0
        0xbe85_0380, // s_mov_b32 s5, 0
        0xbe86_0390, // s_mov_b32 s6, 16
        0xbe87_0380, // s_mov_b32 s7, 0
        0xe00c_0000, // buffer_load_format_xyzw v[0:3], off, s[4:7], 0
        0x8001_0000,
        0xbf8c_0000,
        0xf800_180f,
        0x0302_0100,
        0xbf81_0000,
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode_program(&bytes, &encodings, &operands);
    assert!(orbistoun_translate::wavefront::reads_buffer_formats(
        &decoded, &encodings
    ));
    let refused = translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        },
        (Stage::Fragment, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 1,
            block_offset: 16,
            draw_buffers: true,
            ..UserData::default()
        },
    );
    assert!(matches!(
        refused,
        Err(orbistoun_translate::TranslateError::NeedsBufferFormats)
    ));
}

/// A primitive shader that reads four words of its bound buffer, as [`pixel_shader`] does, and
/// carries them to parameter zero of a triangle covering the target: lane `n`'s corner is
/// `(4 * (n & 1) - 1, 2 * (n & 2) - 1)`, so `(-1, -1)`, `(3, -1)` and `(-1, 3)`.
fn colour_primitive_shader() -> Vec<u32> {
    let words: [u32; 36] = [
        // m0 = three vertices, one primitive; s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbefc_03ff,
        0x0000_1003,
        0xbf90_0009,
        // v14 = lane
        0xd765_000e,
        0x0001_00c1,
        // v2 = float(((v14 & 1) << 2) - 1)
        0x360c_1c81,
        0x340c_0c82,
        0x4a0c_0cc1,
        0x7e04_0b06,
        // v3 = float(((v14 & 2) << 1) - 1)
        0x360e_1c82,
        0x340e_0e81,
        0x4a0e_0ec1,
        0x7e06_0b07,
        // v4 = 0, v5 = 1.0
        0x7e08_0280,
        0x7e0a_02f2,
        // a raw descriptor over user-data word 0 with sixteen bytes; four words into s[8:11]
        0xbe84_0300,
        0xbe85_0380,
        0xbe86_0390,
        0xbe87_03ff,
        0x3101_6fac,
        0xf428_0202,
        0xfa00_0000,
        0xbf8c_c07f,
        0x7e10_0208, // v_mov_b32 v8..v11, s8..s11
        0x7e12_0209,
        0x7e14_020a,
        0x7e16_020b,
        // exp prim v1 = 0 | 1 << 10 | 2 << 20
        0x7e02_02ff,
        0x0020_0400,
        0xf800_0941,
        0x0000_0001,
        0xf800_08cf, // exp pos0 v2..v5 done
        0x0504_0302,
        0xf800_020f, // exp param0 v8..v11
        0x0b0a_0908,
        0xbf81_0000, // s_endpgm
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode_program(&bytes, &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        },
        (Stage::Mesh, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 1,
            draw_buffers: true,
            ..UserData::default()
        },
    )
    .expect("the primitive shader translates")
    .module
}

/// A buffer of the four words `colour`.
fn colour_buffer(colour: [f32; 4]) -> DrawBuffer {
    let words = colour.map(f32::to_bits);
    DrawBuffer {
        hash: orbistoun_gpu::content_hash(&words),
        bytes: words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<u8>>()
            .into(),
        base: 0,
    }
}

/// Two draws through one primitive shader, each with its own buffer, go into one batch and each
/// reads its own (D747): its buffer is a range of an arena its draw's words name, not a set of its
/// own. The second draw covers the first, so the target holds the second's colour; the first,
/// drawn alone, its own.
#[test]
fn batched_primitive_shaders_each_read_their_own_buffer() {
    if !device_or_skip("batched_primitive_shaders_each_read_their_own_buffer") {
        return;
    }
    let draw = |colours: &[[f32; 4]]| {
        let mut backend = VulkanBackend::new();
        let mesh = colour_primitive_shader();
        let pixel = orbistoun_spirv::passthrough_fragment_module();
        for (id, words) in [(TRIANGLE, &mesh), (PIXEL, &pixel)] {
            backend
                .ensure_resident(id, Resource::Shader(words))
                .expect("resident");
        }
        backend
            .ensure_resident(
                TARGET,
                Resource::RenderTarget {
                    width: 16,
                    height: 16,
                    srgb: false,
                },
            )
            .expect("target resident");
        let mut words = [0u32; orbistoun_gpu::USER_DATA_WORDS];
        words[0] = 0x0010_0000;
        let mut commands = vec![
            RenderCommand::SetRenderTargets {
                colour: vec![TARGET],
                depth: None,
            },
            RenderCommand::SetUserData {
                stage: ShaderStage::Vertex,
                words,
            },
            RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: TRIANGLE,
            },
            RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: PIXEL,
            },
        ];
        for &colour in colours {
            commands.push(RenderCommand::BindDrawBuffers {
                stage: ShaderStage::Vertex,
                buffers: vec![colour_buffer(colour)],
            });
            commands.push(RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            });
        }
        for command in &commands {
            backend.execute(command).expect("the command runs");
        }
        backend
            .last_frame()
            .expect("a frame")
            .at(8, 8)
            .expect("the centre")
    };
    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    assert_eq!(draw(&[red]), [255, 0, 0, 255], "the first alone");
    assert_eq!(draw(&[red, blue]), [0, 0, 255, 255], "the second over it");
}

/// A primitive shader that reads its vertex ids from an index buffer (D740) and carries each
/// vertex's id, as a float over eight, to parameter zero's red over the covering triangle of
/// [`colour_primitive_shader`].
fn vertex_id_primitive_shader() -> Vec<u32> {
    vertex_id_primitive_shader_at(Width::Wave64, None)
}

/// [`vertex_id_primitive_shader`] translated for a wave of `width` lanes, one invocation per lane
/// when `per_invocation` (D760).
fn vertex_id_primitive_shader_at(width: Width, per_invocation: Option<u32>) -> Vec<u32> {
    use orbistoun_translate::wavefront::{Assembly, GeometryInputs, IndexWidth};
    let words: [u32; 38] = [
        // A raw descriptor over user-data word 0 with sixteen bytes; four words into s[8:11],
        // through the bound buffer at slot 0, so the index buffer is slot 1.
        0xbe84_0300,
        0xbe85_0380,
        0xbe86_0390,
        0xbe87_03ff,
        0x3101_6fac,
        0xf428_0202,
        0xfa00_0000,
        // m0 = three vertices, one primitive; s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbefc_03ff,
        0x0000_1003,
        0xbf90_0009,
        // v14 = lane
        0xd765_000e,
        0x0001_00c1,
        // v2 = float(((v14 & 1) << 2) - 1)
        0x360c_1c81,
        0x340c_0c82,
        0x4a0c_0cc1,
        0x7e04_0b06,
        // v3 = float(((v14 & 2) << 1) - 1)
        0x360e_1c82,
        0x340e_0e81,
        0x4a0e_0ec1,
        0x7e06_0b07,
        // v4 = 0, v5 left alone: the vertex id; v12 = 1.0
        0x7e08_0280,
        0x7e18_02f2,
        // v8 = float(v5) / 8, v9 = the buffer's second word, v10 = 0, v11 = 1.0
        0x7e10_0d05, // v_cvt_f32_u32 v8, v5
        0x1010_10f0, // v_mul_f32 v8, 0.5, v8
        0x1010_10f0,
        0x1010_10f0,
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0x7e12_0209, // v_mov_b32 v9, s9
        0x7e14_0280,
        0x7e16_02f2,
        // exp prim v1 = 0 | 1 << 10 | 2 << 20
        0x7e02_02ff,
        0x0020_0400,
        0xf800_0941,
        0x0000_0001,
        0xf800_08cf, // exp pos0 v2, v3, v4, v12 done
        0x0c04_0302,
        0xf800_020f, // exp param0 v8..v11
        0x0b0a_0908,
    ];
    let mut program = words.to_vec();
    program.push(0xbf81_0000); // s_endpgm
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode_program(&bytes, &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width,
        },
        (Stage::Mesh, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 1,
            draw_buffers: true,
            per_invocation,
            geometry: Some(GeometryInputs {
                first_vertex: 0,
                vertices: 0,
                primitives: 0,
                assembly: Assembly::List,
                indices: Some(IndexWidth::Bits32),
                passthrough: false,
            }),
            ..UserData::default()
        },
    )
    .expect("the indexed primitive shader translates")
    .module
}

/// The vertex ids of a chunk counting up from `first`, as the index buffer the pipeline binds for
/// it (D741).
fn counting(first: u32) -> DrawBuffer {
    let ids = [first, first + 1, first + 2];
    DrawBuffer {
        hash: orbistoun_gpu::content_hash(&ids),
        bytes: ids
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<u8>>()
            .into(),
        base: 0,
    }
}

/// Draws chunks of an indexed draw through `mesh`, one per first id, and reads the centre: the
/// harness of [`indexed_chunks_each_read_their_own_vertex_ids`].
fn draw_vertex_ids(mesh: &Vec<u32>, firsts: &[u32]) -> [u8; 4] {
    let mut backend = VulkanBackend::new();
    let pixel = orbistoun_spirv::passthrough_fragment_module();
    for (id, words) in [(TRIANGLE, mesh), (PIXEL, &pixel)] {
        backend
            .ensure_resident(id, Resource::Shader(words))
            .expect("resident");
    }
    backend
        .ensure_resident(
            TARGET,
            Resource::RenderTarget {
                width: 16,
                height: 16,
                srgb: false,
            },
        )
        .expect("target resident");
    let mut words = [0u32; orbistoun_gpu::USER_DATA_WORDS];
    words[0] = 0x0010_0000;
    let mut commands = vec![
        RenderCommand::SetRenderTargets {
            colour: vec![TARGET],
            depth: None,
        },
        RenderCommand::SetUserData {
            stage: ShaderStage::Vertex,
            words,
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Vertex,
            shader: TRIANGLE,
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Fragment,
            shader: PIXEL,
        },
    ];
    for &first in firsts {
        commands.push(RenderCommand::BindDrawBuffers {
            stage: ShaderStage::Vertex,
            buffers: vec![colour_buffer([0.0, 0.5, 0.0, 0.0]), counting(first)],
        });
        commands.push(RenderCommand::DrawIndexed {
            indices: 3,
            instances: 1,
            first_index: 0,
            index_buffer: Some(orbistoun_gpu::IndexBuffer::Counting { first }),
        });
    }
    for command in &commands {
        backend.execute(command).expect("the command runs");
    }
    backend
        .last_frame()
        .expect("a frame")
        .at(8, 8)
        .expect("the centre")
}

/// Chunks of an indexed draw each read their own vertex ids (D740, D741): a chunk of ids 0 to 2
/// colours the centre by their weighted mean, and a second chunk of ids 3 to 5 drawn over it, in
/// the same batch, by its own. At the centre of the covering triangle the weights are a half and
/// two quarters, so red is (0.5 * a + 0.25 * b + 0.25 * c) / 8. The index buffer is the second
/// slot, after a buffer the shader reads through a descriptor, whose second word is green.
#[test]
fn indexed_chunks_each_read_their_own_vertex_ids() {
    if !device_or_skip("indexed_chunks_each_read_their_own_vertex_ids") {
        return;
    }
    let mesh = vertex_id_primitive_shader();
    let draw = |firsts: &[u32]| draw_vertex_ids(&mesh, firsts);
    let red = |ids: [f32; 3]| ((0.5 * ids[0] + 0.25 * ids[1] + 0.25 * ids[2]) / 8.0 * 255.0) as i32;
    let near = |got: u8, want: i32| (i32::from(got) - want).abs() <= 3;
    let first = draw(&[0]);
    assert!(
        near(first[0], red([0.0, 1.0, 2.0])),
        "ids 0 to 2: {first:?}"
    );
    assert!(near(first[1], 128), "green from slot 0: {first:?}");
    let second = draw(&[0, 3]);
    assert!(
        near(second[0], red([3.0, 4.0, 5.0])),
        "ids 3 to 5: {second:?}"
    );
    assert!(near(second[1], 128), "green from slot 0: {second:?}");
}

/// A primitive shader run one invocation per lane (D760) draws what one invocation simulating the
/// whole wave draws: each lane its own vertex id from the index buffer, its lane from
/// `v_mbcnt_lo`, its vertex and primitive written at its own slot - for a wave of thirty-two lanes
/// on a subgroup as wide, and for one of sixty-four exchanging through workgroup memory.
#[test]
fn a_primitive_shader_run_per_lane_draws_what_the_wave_draws() {
    let Availability::Available { properties } = probe() else {
        println!("[per-lane primitive shader] SKIPPED - no device");
        return;
    };
    if properties.subgroup_size != 32 {
        println!(
            "[per-lane primitive shader] SKIPPED - subgroup {} wide, not 32",
            properties.subgroup_size
        );
        return;
    }
    // A wave the subgroup holds takes ballots and shuffles; one twice as wide exchanges through
    // workgroup memory.
    for width in [Width::Wave32, Width::Wave64] {
        let wave = vertex_id_primitive_shader_at(width, None);
        let per_lane = vertex_id_primitive_shader_at(width, Some(32));
        for firsts in [&[0][..], &[0, 3]] {
            let expected = draw_vertex_ids(&wave, firsts);
            assert_ne!(expected, [0, 0, 0, 0], "the wave model draws");
            assert_eq!(
                draw_vertex_ids(&per_lane, firsts),
                expected,
                "{width:?}, chunks {firsts:?}"
            );
        }
    }
}
