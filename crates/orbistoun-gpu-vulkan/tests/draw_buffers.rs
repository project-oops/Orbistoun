//! A draw's bound buffers on a device (D732).
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
