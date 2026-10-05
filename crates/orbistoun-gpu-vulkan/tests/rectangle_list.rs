//! A primitive shader emitting a rectangle list, on a device.
//!
//! radeonsi's blits set `VGT_GS_OUT_PRIM_TYPE` to `RECTLIST` (`si_pipe.h:2110`) and give each
//! rectangle three corners - `(x1, y1)`, `(x1, y2)`, `(x2, y1)`, vertex ids 0, 1, 2
//! (`si_nir_lower_vs_inputs.c:97-122`) - so the rectangle is the parallelogram those three span, its
//! fourth corner `v1 + v2 - v0`. The mesh module draws it as two triangles over that fourth vertex,
//! with every parameter carried the same way, so the parameters stay one plane across the whole
//! rectangle.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::{draw_mesh_of_vertices_over, draw_mesh_over};
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::wavefront::{MeshPrimitive, Stage, Window};
use orbistoun_translate::{Fidelity, Strategy, Width, translate_windowed_primitive};

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

/// A primitive shader declaring three vertices and one primitive, fetching each lane's vertex from
/// guest memory, and exporting the primitive `(0, 1, 2)`, the positions, and the positions again as
/// parameter zero.
///
/// The instruction words are those of `console_fragment.rs`'s minimal primitive shader: the export
/// words are oracle record A's.
fn primitive_shader() -> Vec<u8> {
    let words: [u32; 17] = [
        // m0 = three vertices, one primitive; s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbefc_03ff,
        0x0000_1003,
        0xbf90_0009,
        // v14 = lane * 48: twelve words a vertex
        0xd765_000e,
        0x0001_00c1,
        0x341e_1c84,
        0x341c_1c85,
        0x4a1c_1d0f,
        // global_load_dwordx4 v[2:5], v[14:15], off
        0xdc38_8000,
        0x027d_000e,
        0xbf8c_3f70,
        // v1 = vertices 0, 1, 2 with edge flags; exp prim
        0x7e02_02ff,
        0x2028_0600,
        0xf800_0941,
        0x0000_0001,
        // exp pos0 v2..v5
        0xf800_08cf,
        0x0504_0302,
    ];
    let mut bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    // exp param0 v2..v5, then s_endpgm
    for word in [0xf800_020f_u32, 0x0504_0302, 0xbf81_0000] {
        bytes.extend(word.to_le_bytes());
    }
    bytes
}

fn mesh_module(primitive: MeshPrimitive) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&primitive_shader(), &encodings, &operands);
    translate_windowed_primitive(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        },
        Stage::Mesh,
        primitive,
        Window::default(),
    )
    .expect("the primitive shader translates")
    .module
}

/// Three corners of the whole viewport in radeonsi's order: `(x1, y1)`, `(x1, y2)`, `(x2, y1)`.
fn memory() -> Vec<u32> {
    let corners = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [-1.0, 1.0, 0.0, 1.0],
        [1.0, -1.0, 0.0, 1.0],
    ];
    let mut memory = vec![0u32; 64];
    for (vertex, at) in [0usize, 12, 24].into_iter().enumerate() {
        for component in 0..4 {
            memory[at + component] = corners[vertex][component].to_bits();
        }
    }
    memory
}

/// A primitive shader that stores its geometry-engine inputs to guest memory: each lane's `v0`,
/// `v1`, `v5` and `v2` at `lane * 16`, then `s2` and `s3` at `0x800`.
fn storing_inputs_shader() -> Vec<u8> {
    let words = [
        // m0 = three vertices, one primitive; s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbefc_03ff,
        0x0000_1003,
        0xbf90_0009,
        // v14 = lane * 16, v15 = 0
        0xd765_000e,
        0x0001_00c1,
        0x341c_1c84,
        0x7e1e_0280,
        // global_store_dword v[14:15], v0 / v1 / v5 / v2, off offset:0 / 4 / 8 / 12
        0xdc70_8000,
        0x007d_000e,
        0xdc70_8004,
        0x007d_010e,
        0xdc70_8008,
        0x007d_050e,
        0xdc70_800c,
        0x007d_020e,
        // v16 = 0x800, v17 = 0
        0x7e20_02ff,
        0x0000_0800,
        0x7e22_0280,
        // v20 = s2; store; v20 = s3; store offset:4
        0x7e28_0202,
        0xdc70_8000,
        0x007d_1410,
        0x7e28_0203,
        0xdc70_8004,
        0x007d_1410,
        0xbf8c_3f70,
        // exp prim v21 = 0 | 1 << 10 | 2 << 20; exp pos0 v23, v23, v23, v22 = (0, 0, 0, 1)
        0x7e2a_02ff,
        0x0020_0400_u32,
        0xf800_0941,
        0x0000_0015,
        0x7e2c_02f2,
        0x7e2e_0280,
        0xf800_08cf,
        0x1617_1717,
        0xbf81_0000,
    ];
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A primitive shader given a draw's geometry finds the geometry engine's inputs where GFX10
/// puts them (D730): three vertices and one primitive leave gs_tg_info `3 << 12 | 1 << 22` in
/// `s2` and merged_wave_info `3 | 1 << 8 | 1 << 28` in `s3`; primitive thread zero has vertices
/// 0 and 1 in `v0` and 2 in `v1` and its id in `v2`; each vertex thread its id in `v5`; any other
/// thread zeros. Without the geometry the same shader is refused, since it reads what nothing
/// seeded. The counts are the draw's, from its words (D745): the module is translated with none,
/// as the pipeline translates it, and a second draw of six vertices through the same module
/// finds two primitives.
#[test]
fn a_primitive_shader_given_a_draw_s_geometry_finds_its_inputs() {
    use orbistoun_translate::wavefront::{GeometryInputs, UserData};
    if !device_or_skip("a_primitive_shader_given_a_draw_s_geometry_finds_its_inputs") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&storing_inputs_shader(), &encodings, &operands);
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::Wave64,
    };
    let with = |geometry| {
        orbistoun_translate::translate_with_user_data(
            &decoded,
            &encodings,
            strategy,
            (Stage::Mesh, MeshPrimitive::Rectangles),
            // Wide enough for every lane's four words and the two scalars at 0x800.
            Window::spanning(0, 1024).expect("a power of two"),
            UserData {
                geometry,
                ..UserData::default()
            },
        )
    };
    assert!(matches!(
        with(None),
        Err(orbistoun_translate::TranslateError::ReadsGeometryInputs)
    ));
    let module = with(Some(GeometryInputs {
        first_vertex: 0,
        vertices: 0,
        primitives: 0,
        assembly: orbistoun_translate::wavefront::Assembly::List,
        indices: None,
    }))
    .expect("translates with its geometry")
    .module;
    let fragment = orbistoun_spirv::constant_colour_fragment_module([0.0, 1.0, 0.0, 1.0]);
    let (_, six) = draw_mesh_of_vertices_over((&module, &fragment), 6, (4, 4), &vec![0u32; 1024])
        .expect("the six-vertex draw ran");
    assert_eq!(
        &six[0x800 / 4..0x800 / 4 + 2],
        [6 << 12 | 2 << 22, 6 | 2 << 8 | 1 << 28],
        "the second draw's counts, through the same module"
    );
    assert_eq!(&six[4..8], [3 | 4 << 16, 5, 1, 1], "primitive thread one");
    let (_, memory) =
        draw_mesh_of_vertices_over((&module, &fragment), 3, (4, 4), &vec![0u32; 1024])
            .expect("the draw ran");
    let lane = |n: usize| &memory[n * 4..n * 4 + 4];
    assert_eq!(lane(0), [1 << 16, 2, 0, 0], "v0, v1, v5, v2 of thread zero");
    assert_eq!(lane(1), [0, 0, 1, 0], "a vertex thread only");
    assert_eq!(lane(2), [0, 0, 2, 0]);
    assert_eq!(lane(3), [0, 0, 0, 0], "neither");
    assert_eq!(
        memory[0x200..0x202],
        [(3 << 12) | (1 << 22), 3 | (1 << 8) | (1 << 28)],
        "gs_tg_info and merged_wave_info"
    );
}

/// Window-space positions (D731): with `PA_CL_VTE_CNTL` in radeonsi's window-space form the
/// position is already in pixels and its fourth component is `1/W`, taken as is - Gallium's
/// `VS_WINDOW_SPACE_POSITION`. Drawn under the viewport [`WINDOW_SPACE_SCALE`] asks for, a
/// rectangle with corners at pixels `(2, 1)`, `(2, 5)` and `(6, 1)` covers exactly the pixels
/// `2..6` by `1..5`, whatever its fourth component, since nothing divides by it.
#[test]
fn a_window_space_rectangle_covers_the_pixels_its_corners_name() {
    use orbistoun_gpu_vulkan::framebuffer::draw_mesh_over_viewport;
    use orbistoun_translate::wavefront::{UserData, WINDOW_SPACE_SCALE};
    if !device_or_skip("a_window_space_rectangle_covers_the_pixels_its_corners_name") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&primitive_shader(), &encodings, &operands);
    let module = orbistoun_translate::translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        },
        (Stage::Mesh, MeshPrimitive::Rectangles),
        Window::default(),
        UserData {
            window_space: true,
            ..UserData::default()
        },
    )
    .expect("translates with window-space positions")
    .module;
    let fragment = orbistoun_spirv::constant_colour_fragment_module([0.0, 1.0, 0.0, 1.0]);
    let viewport = orbistoun_gpu::ViewportTransform {
        x_scale: WINDOW_SPACE_SCALE,
        x_offset: 0.0,
        y_scale: WINDOW_SPACE_SCALE,
        y_offset: 0.0,
        depth: orbistoun_gpu::DepthMapping::IDENTITY,
    };
    for reciprocal_w in [1.0f32, 0.5] {
        let corners = [[2.0f32, 1.0], [2.0, 5.0], [6.0, 1.0]];
        let mut memory = vec![0u32; 64];
        for (vertex, at) in [0usize, 12, 24].into_iter().enumerate() {
            let [x, y] = corners[vertex];
            for (component, value) in [x, y, 0.0, reciprocal_w].into_iter().enumerate() {
                memory[at + component] = value.to_bits();
            }
        }
        let (pixels, _) = draw_mesh_over_viewport(
            &module,
            &fragment,
            [0.0, 0.0, 1.0, 1.0],
            (8, 6),
            &memory,
            viewport,
        )
        .expect("the window-space draw ran");
        for y in 0..6 {
            for x in 0..8 {
                let inside = (2..6).contains(&x) && (1..5).contains(&y);
                let expected = if inside {
                    [0, 255, 0, 255]
                } else {
                    [0, 0, 255, 255]
                };
                assert_eq!(
                    pixels.at(x, y),
                    Some(expected),
                    "pixel ({x}, {y}) with 1/W {reciprocal_w}"
                );
            }
        }
    }
}

/// The rectangle covers every pixel, and parameter zero - the position - is one plane across it:
/// at each pixel centre it is that pixel's own clip-space position, clamped to the target's unorm
/// range. As a triangle list the same three vertices leave the far corner at the clear colour.
#[test]
fn a_rectangle_list_covers_the_parallelogram_its_three_corners_span() {
    if !device_or_skip("a_rectangle_list_covers_the_parallelogram_its_three_corners_span") {
        return;
    }
    let (width, height) = (8u16, 6u16);
    let fragment = orbistoun_spirv::passthrough_fragment_module();
    let clear = [0.0, 0.0, 1.0, 1.0];

    let (pixels, _) = draw_mesh_over(
        &mesh_module(MeshPrimitive::Rectangles),
        &fragment,
        clear,
        u32::from(width),
        u32::from(height),
        &memory(),
    )
    .expect("the rectangle draw ran");
    let unorm = |ndc: f32| ndc.clamp(0.0, 1.0) * 255.0;
    for y in 0..height {
        for x in 0..width {
            let ndc_x = (f32::from(x) + 0.5) * 2.0 / f32::from(width) - 1.0;
            let ndc_y = (f32::from(y) + 0.5) * 2.0 / f32::from(height) - 1.0;
            let got = pixels.at(u32::from(x), u32::from(y)).expect("inside");
            let expected = [unorm(ndc_x), unorm(ndc_y), 0.0, 255.0];
            for (channel, (&g, e)) in got.iter().zip(expected).enumerate() {
                assert!(
                    (f32::from(g) - e).abs() <= 2.0,
                    "pixel ({x}, {y}) channel {channel}: {got:?}, expected {expected:?}"
                );
            }
        }
    }

    let (triangle, _) = draw_mesh_over(
        &mesh_module(MeshPrimitive::Triangles),
        &fragment,
        clear,
        u32::from(width),
        u32::from(height),
        &memory(),
    )
    .expect("the triangle draw ran");
    assert_eq!(
        triangle.at(u32::from(width) - 1, u32::from(height) - 1),
        Some([0, 0, 255, 255]),
        "a triangle leaves the fourth corner clear"
    );
}
