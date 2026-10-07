//! Depth, stencil and cull state on a device, through the backend's commands.
//!
//! Two triangles covering the target at different depths: drawn near first and far second, only a
//! depth test keeps the near one on top, since without one the later draw wins. The depth
//! attachment is the host's own and persists across submissions until a clear resets it.

use orbistoun_gpu::depth::{DepthStencilState, decode_cull, depth_stencil_from};
use orbistoun_gpu::{RenderBackend, RenderCommand, Resource, ResourceId, ShaderStage};
use orbistoun_gpu_vulkan::VulkanBackend;
use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_spirv::{
    constant_colour_fragment_module, fullscreen_triangle_vertex_module_at_depth,
};

/// Reports the device, or skips loudly: a test that returns early and reports success would make
/// the suite green where nothing ran.
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
const NEAR: ResourceId = ResourceId(1);
const FAR: ResourceId = ResourceId(2);
const GREEN: ResourceId = ResourceId(3);
const RED: ResourceId = ResourceId(4);
/// A translated pixel shader that exports depth and red.
const DEPTH_WRITER: ResourceId = ResourceId(5);

const GREEN_PIXEL: [u8; 4] = [0, 255, 0, 255];
const RED_PIXEL: [u8; 4] = [255, 0, 0, 255];

/// `DB_DEPTH_CONTROL` with `Z_ENABLE`, `Z_WRITE_ENABLE` and `ZFUNC` `LESS` (`0x16`, gfx103.json's
/// fields), as the translator hands it over.
fn less() -> DepthStencilState {
    depth_stencil_from(|register| (register == 0xA200).then_some(0x16)).expect("depth control")
}

/// A backend holding a 16x16 target, a near and a far covering triangle, and two colours; the
/// target is selected with `depth` beside it.
fn backend(depth: Option<ResourceId>) -> VulkanBackend {
    let mut backend = VulkanBackend::new();
    let modules = [
        (NEAR, fullscreen_triangle_vertex_module_at_depth(0.25)),
        (FAR, fullscreen_triangle_vertex_module_at_depth(0.75)),
        (GREEN, constant_colour_fragment_module([0.0, 1.0, 0.0, 1.0])),
        (RED, constant_colour_fragment_module([1.0, 0.0, 0.0, 1.0])),
    ];
    for (id, words) in &modules {
        backend
            .ensure_resident(*id, Resource::Shader(words))
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
    run(
        &mut backend,
        &[RenderCommand::SetRenderTargets {
            colour: vec![TARGET],
            depth,
        }],
    );
    backend
}

fn run(backend: &mut VulkanBackend, commands: &[RenderCommand]) {
    for command in commands {
        backend.execute(command).expect("the command runs");
    }
}

/// Binds `geometry` and `colour` and draws the triangle.
fn draw(geometry: ResourceId, colour: ResourceId) -> [RenderCommand; 3] {
    [
        RenderCommand::BindShader {
            stage: ShaderStage::Vertex,
            shader: geometry,
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Fragment,
            shader: colour,
        },
        RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        },
    ]
}

fn centre(backend: &mut VulkanBackend) -> [u8; 4] {
    backend
        .last_frame()
        .expect("a frame")
        .at(8, 8)
        .expect("the centre")
}

fn clear_to_far() -> RenderCommand {
    RenderCommand::ClearDepthStencil {
        target: DEPTH,
        depth: Some(1.0),
        stencil: None,
    }
}

/// Near green then far red: with a `LESS` depth test the far triangle fails and green stays; with
/// no depth state the later red covers it.
#[test]
fn a_depth_test_keeps_the_nearer_triangle_whatever_the_order() {
    if !device_or_skip("a_depth_test_keeps_the_nearer_triangle_whatever_the_order") {
        return;
    }
    let mut tested = backend(Some(DEPTH));
    run(
        &mut tested,
        &[clear_to_far(), RenderCommand::SetDepthStencil(less())],
    );
    run(&mut tested, &draw(NEAR, GREEN));
    run(&mut tested, &draw(FAR, RED));
    assert_eq!(
        centre(&mut tested),
        GREEN_PIXEL,
        "the far red draw failed the depth test"
    );

    let mut untested = backend(Some(DEPTH));
    run(&mut untested, &[clear_to_far()]);
    run(&mut untested, &draw(NEAR, GREEN));
    run(&mut untested, &draw(FAR, RED));
    assert_eq!(
        centre(&mut untested),
        RED_PIXEL,
        "with no depth state the last draw wins"
    );

    // Far first, near second: the near triangle passes over the far one.
    let mut reversed = backend(Some(DEPTH));
    run(
        &mut reversed,
        &[clear_to_far(), RenderCommand::SetDepthStencil(less())],
    );
    run(&mut reversed, &draw(FAR, RED));
    run(&mut reversed, &draw(NEAR, GREEN));
    assert_eq!(centre(&mut reversed), GREEN_PIXEL);
}

/// The depth attachment keeps what the draws left until a clear: a far draw after the near one,
/// in a later submission, fails the test; after a clear to the far plane it passes.
#[test]
fn depth_persists_until_a_clear() {
    if !device_or_skip("depth_persists_until_a_clear") {
        return;
    }
    let mut backend = backend(Some(DEPTH));
    run(
        &mut backend,
        &[clear_to_far(), RenderCommand::SetDepthStencil(less())],
    );
    run(&mut backend, &draw(NEAR, GREEN));
    backend.submit_draws().expect("submitted");
    assert_eq!(centre(&mut backend), GREEN_PIXEL);

    run(&mut backend, &draw(FAR, RED));
    assert_eq!(
        centre(&mut backend),
        GREEN_PIXEL,
        "the near depth from the last submission holds"
    );

    run(&mut backend, &[clear_to_far()]);
    run(&mut backend, &draw(FAR, RED));
    assert_eq!(
        centre(&mut backend),
        RED_PIXEL,
        "after the clear the far draw passes"
    );
}

/// Culling: the covering triangle winds clockwise on the target, so culling back faces with
/// counter-clockwise front (`PA_SU_SC_MODE_CNTL` `0x2`) discards it and with clockwise front
/// (`0x6`) keeps it.
#[test]
fn cull_state_discards_the_back_faces_it_names() {
    if !device_or_skip("cull_state_discards_the_back_faces_it_names") {
        return;
    }
    let mut culled = backend(None);
    run(&mut culled, &[RenderCommand::SetCull(decode_cull(0x2))]);
    run(&mut culled, &draw(NEAR, GREEN));
    assert_ne!(
        centre(&mut culled),
        GREEN_PIXEL,
        "a back face was drawn with back faces culled"
    );

    let mut kept = backend(None);
    run(&mut kept, &[RenderCommand::SetCull(decode_cull(0x6))]);
    run(&mut kept, &draw(NEAR, GREEN));
    assert_eq!(centre(&mut kept), GREEN_PIXEL);
}

/// A guest pixel shader exporting depth to `mrtz` replaces the triangle's own: Mesa's MRTZ export
/// carries Z in its first channel (`ac_get_spi_shader_z_format`), as 2 Ship 2 Harkinian's
/// `gl_FragDepth = prim_depth` compiles to `exp mrtz v27`. The far triangle (0.75) exports 0.125
/// and red, so under `LESS` it now passes over the near green (0.25) - without the replacement it
/// would fail.
#[test]
fn a_pixel_shader_exporting_depth_replaces_the_triangle_s() {
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
    use orbistoun_translate::wavefront::{Stage, Window};
    use orbistoun_translate::{Fidelity, Strategy, Width};
    if !device_or_skip("a_pixel_shader_exporting_depth_replaces_the_triangle_s") {
        return;
    }
    // v_mov_b32 v0, 0.125; v_mov_b32 v1, 1.0; v_mov_b32 v2, 0; exp mrtz v0 (X only);
    // exp mrt0 v1, v2, v2, v1 done vm; s_endpgm. 0.125 is a literal, 1.0 and 0 inline codes.
    let words: [u32; 10] = [
        0x7e00_02ff,
        0x3e00_0000,
        0x7e02_02f2,
        0x7e04_0280,
        0xf800_0081,
        0x0000_0000,
        0xf800_180f,
        0x0102_0201,
        0xbf81_0000,
        0,
    ];
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&bytes, &encodings, &operands);
    let module = orbistoun_translate::translate_windowed(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::default(),
        },
        Stage::Fragment,
        Window::default(),
    )
    .expect("a depth export translates")
    .module;
    let mut backend = backend(Some(DEPTH));
    backend
        .ensure_resident(DEPTH_WRITER, Resource::Shader(&module))
        .expect("resident");
    run(
        &mut backend,
        &[clear_to_far(), RenderCommand::SetDepthStencil(less())],
    );
    run(&mut backend, &draw(NEAR, GREEN));
    run(&mut backend, &draw(FAR, DEPTH_WRITER));
    assert_eq!(
        centre(&mut backend),
        RED_PIXEL,
        "the exported 0.125 passes LESS over the near 0.25"
    );
}
