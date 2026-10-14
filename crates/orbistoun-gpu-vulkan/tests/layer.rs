//! A primitive shader that exports a layer, on a device (D859).
//!
//! `exp pos1` with only its third channel names each vertex's render-target layer. A draw that
//! reads the layer runs once per slice of its view, each pass keeping the primitives in its layer,
//! so a primitive is drawn by the pass of the layer its shader named and by no other.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_mesh_in_layer;
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_spirv::DRAW_LAYER_UNUSED;
use orbistoun_translate::wavefront::{
    Assembly, GeometryInputs, MeshPrimitive, Stage, UserData, Window,
};
use orbistoun_translate::{
    Fidelity, Strategy, Width, translate_windowed_primitive, translate_with_user_data,
};

fn device_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } if properties.mesh_shading => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Available { properties } => {
            println!("[{what}] SKIPPED - {} has no mesh stage", properties.device);
            false
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

/// A primitive shader declaring three vertices and one primitive, fetching each lane's position
/// from guest memory, exporting the primitive `(0, 1, 2)`, the positions, and `layer` as every
/// vertex's layer - `exp pos1 off, off, v20, off`, the form PPSA21564's full-screen pass takes
/// (`0xf80000d4`, its third source the layer) - and the positions again as parameter zero.
fn layered_shader(layer: u32) -> Vec<u8> {
    layered_shader_from(layer, 20)
}

/// [`layered_shader`] exporting register `v{source}` as the layer, after setting `v20` to `layer`.
fn layered_shader_from(layer: u32, source: u32) -> Vec<u8> {
    let words = [
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
        // v20 = layer; exp pos1 off, off, v20, off
        0x7e28_02ff,
        layer,
        0xf800_00d4,
        source << 16,
        // exp param0 v2..v5, which the passthrough fragment module reads, then s_endpgm
        0xf800_020f,
        0x0504_0302,
        0xbf81_0000,
    ];
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn module(layer: u32) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&layered_shader(layer), &encodings, &operands);
    translate_windowed_primitive(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        },
        Stage::Mesh,
        MeshPrimitive::Triangles,
        Window::default(),
    )
    .expect("the layered primitive shader translates")
    .module
}

/// A triangle over the whole viewport.
fn memory() -> Vec<u32> {
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
    memory
}

/// A primitive is drawn by the pass of its own layer and culled from every other; a draw that
/// does not use the layer draws it whatever it names.
#[test]
fn a_primitive_is_drawn_only_in_the_pass_of_its_layer() {
    if !device_or_skip("a_primitive_is_drawn_only_in_the_pass_of_its_layer") {
        return;
    }
    let (width, height) = (8u32, 6u32);
    let fragment = orbistoun_spirv::passthrough_fragment_module();
    let clear = [0.0, 0.0, 1.0, 1.0];
    let blue = [0, 0, 255, 255];
    let covered = |layer: u32, pass: u32| {
        let pixels = draw_mesh_in_layer(
            (&module(layer), &fragment),
            clear,
            (width, height),
            &memory(),
            (0, pass, 1),
        )
        .expect("the draw ran");
        let drawn: Vec<bool> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| pixels.at(x, y).expect("inside") != blue)
            .collect();
        assert!(
            drawn.iter().all(|&d| d) || drawn.iter().all(|&d| !d),
            "a full-screen triangle is drawn whole or not at all (layer {layer}, pass {pass})"
        );
        drawn[0]
    };
    assert!(covered(0, 0), "layer 0 in the pass of layer 0");
    assert!(covered(3, 3), "layer 3 in the pass of layer 3");
    assert!(!covered(3, 0), "layer 3 is not in the pass of layer 0");
    assert!(!covered(0, 1), "layer 0 is not in the pass of layer 1");
    assert!(
        covered(5, DRAW_LAYER_UNUSED),
        "a draw that does not use the layer keeps every primitive"
    );
    // Only bits 10:0 are the layer; the viewport index above them is not.
    assert!(
        covered(2 | 1 << 16, 2),
        "the viewport index is not the layer"
    );
}

/// An instanced draw runs a workgroup per instance, each finding its instance in `v8` (D863): a
/// shader exporting `v8` as its layer draws instance `k`'s triangle in the pass of layer `k`, the
/// way PPSA21564's full-screen pass fills one slice an instance. A draw of one instance has only
/// instance zero.
#[test]
fn each_instance_finds_its_instance_in_v8() {
    if !device_or_skip("each_instance_finds_its_instance_in_v8") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&layered_shader_from(0, 8), &encodings, &operands);
    let module = translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        },
        (Stage::Mesh, MeshPrimitive::Triangles),
        Window::default(),
        UserData {
            geometry: Some(GeometryInputs {
                first_vertex: 0,
                vertices: 0,
                primitives: 0,
                assembly: Assembly::List,
                indices: None,
                passthrough: false,
            }),
            ..UserData::default()
        },
    )
    .expect("the instanced primitive shader translates")
    .module;
    let fragment = orbistoun_spirv::passthrough_fragment_module();
    let clear = [0.0, 0.0, 1.0, 1.0];
    let covered = |instances: u32, pass: u32| {
        let pixels = draw_mesh_in_layer(
            (&module, &fragment),
            clear,
            (8, 6),
            &memory(),
            (3, pass, instances),
        )
        .expect("the draw ran");
        pixels.at(4, 3).expect("inside") != [0, 0, 255, 255]
    };
    assert!(covered(3, 0), "instance 0 draws in layer 0");
    assert!(covered(3, 2), "instance 2 draws in layer 2");
    assert!(!covered(3, 3), "no instance 3 among three");
    assert!(!covered(1, 2), "one instance has no instance 2");
    assert!(covered(1, 0), "one instance is instance 0");
}
