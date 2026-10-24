//! `image_bvh_intersect_ray` translated and run on a device: a node written into guest memory, a
//! ray in the address registers, and the four result registers read back. The node tests' own
//! arithmetic is unit-tested on the host in `model/bvh.rs`; this checks the instruction's wiring -
//! its operands, the node address, the window read and the result registers. A missing device
//! skips loudly, as in `execute.rs`.

use orbistoun_gpu_vulkan::{Availability, dispatch, probe};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::predicated::MEMORY_WORDS;
use orbistoun_translate::{Fidelity, Strategy, Width, translate};

/// Registers copied out of each file: the vector file then the scalar file.
const PER_FILE: usize = 8;

/// The node's byte address, inside the module's window, which a pointer names in 8-byte units with
/// its type in the low three bits.
const NODE: u32 = 0x40;

/// The words a box node of floats occupies up to its last box.
const NODE_WORDS: usize = 28;

/// `image_bvh_intersect_ray v[0:3], v[8:18], s[4:7]`, as the reference assembler encodes the same
/// instruction with other registers (`operands-bvh.out`): data mask 0xf, unnormalised, a 128-bit
/// descriptor, and the opcode's eighth bit in bit 0. Address register 8 in the second word's low
/// byte, the result at 0 above it, the descriptor's quarter-register 1 above that.
const INTERSECT: [u32; 2] = [0xf198_9f01, 0x0001_0008];

fn head(name: &str) -> u32 {
    let table = EncodingTable::builtin().expect("encodings");
    let (family, opcode) = table
        .find_by_name(name)
        .unwrap_or_else(|| panic!("this target has no instruction named {name}"));
    let encoding = table
        .encodings()
        .iter()
        .find(|encoding| encoding.name == family)
        .unwrap_or_else(|| panic!("no encoding family named {family}"));
    encoding.value | (opcode << encoding.opcode.shift)
}

/// `v_mov_b32_e32 vN, <literal>`.
fn v_mov_literal(dst: u32, value: u32) -> [u32; 2] {
    [head("v_mov_b32_e32") | (dst << 17) | 255, value]
}

/// `s_mov_b32 sN, <literal>`.
fn s_mov_literal(dst: u32, value: u32) -> [u32; 2] {
    [head("s_mov_b32") | (dst << 16) | 255, value]
}

/// `global_store_dword vAddr, vData, off`.
fn global_store(vaddr: u32, vdata: u32) -> [u32; 2] {
    [
        head("global_store_dword"),
        vaddr | (vdata << 8) | (0x7d << 16),
    ]
}

/// Stores `node` at [`NODE`], a word at a time through v0 and v1.
fn store_node(program: &mut Vec<u32>, node: &[u32; NODE_WORDS]) {
    for (address, &value) in (NODE..).step_by(4).zip(node) {
        program.extend(v_mov_literal(0, address));
        program.extend(v_mov_literal(1, value));
        program.extend(global_store(0, 1));
    }
}

/// The descriptor in s[4:7] - base zero, box sorting on, barycentric return, type 8 - and the
/// ray in v[8:18]: the pointer, extent, origin, direction and inverse direction.
fn set_ray(program: &mut Vec<u32>, pointer: u32, origin: [f32; 3], direction: [f32; 3]) {
    for (register, word) in (4..).zip([0, 1 << 31, 0, 0x8100_0000]) {
        program.extend(s_mov_literal(register, word));
    }
    let inverse = direction.map(|component| 1.0 / component);
    let floats = [100.0]
        .into_iter()
        .chain(origin)
        .chain(direction)
        .chain(inverse);
    let words = std::iter::once(pointer).chain(floats.map(f32::to_bits));
    for (register, word) in (8..).zip(words) {
        program.extend(v_mov_literal(register, word));
    }
}

/// Translates and runs `program`, answering v0-v3.
fn run(mut program: Vec<u32>) -> Option<[u32; 4]> {
    if let Availability::Unavailable { reason } = probe() {
        println!("!! SKIPPED: no Vulkan device: {reason}; translated shaders were NOT executed");
        return None;
    }
    program.push(head("s_endpgm"));
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|word| word.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Wavefront,
        width: Width::default(),
    };
    let translated = translate(&decoded, &table, strategy).expect("translates");
    let out = dispatch(
        &translated.module,
        PER_FILE * 2,
        MEMORY_WORDS as usize,
        [1, 1, 1],
    )
    .expect("dispatch");
    Some([0, 1, 2, 3].map(|register| out.observed[register]))
}

/// A box node of floats whose third and second children lie on a ray down +z answers them in that
/// order, nearest first, then two misses.
#[test]
fn a_box_node_answers_the_children_the_ray_enters_nearest_first() {
    let boxes: [(u32, [f32; 6]); 4] = [
        (0x11, [5.0, 5.0, 0.0, 6.0, 6.0, 1.0]),
        (0x22, [-1.0, -1.0, 8.0, 1.0, 1.0, 9.0]),
        (0x33, [-1.0, -1.0, 2.0, 1.0, 1.0, 3.0]),
        (0x44, [-1.0, -1.0, -5.0, 1.0, 1.0, -4.0]),
    ];
    let mut node = [0; NODE_WORDS];
    for (index, (pointer, bounds)) in boxes.into_iter().enumerate() {
        node[index] = pointer;
        for (axis, bound) in bounds.into_iter().enumerate() {
            node[4 + 6 * index + axis] = bound.to_bits();
        }
    }
    let mut program = Vec::new();
    store_node(&mut program, &node);
    set_ray(&mut program, (NODE >> 3) | 5, [0.0; 3], [0.0, 0.0, 1.0]);
    program.extend(INTERSECT);
    if let Some(result) = run(program) {
        assert_eq!(result, [0x33, 0x22, u32::MAX, u32::MAX]);
    }
}

/// A triangle node hit at (1,1) by a ray down -z answers `t`, `det`, `v` and `w` whose quotients
/// are the distance 2 and the second and third vertices' weights 1/4.
#[test]
fn a_triangle_node_answers_its_distance_and_barycentrics() {
    let mut node = [0; NODE_WORDS];
    let vertices = [0.0f32, 0.0, -2.0, 4.0, 0.0, -2.0, 0.0, 4.0, -2.0];
    for (word, coordinate) in node.iter_mut().zip(vertices) {
        *word = coordinate.to_bits();
    }
    let mut program = Vec::new();
    store_node(&mut program, &node);
    set_ray(&mut program, NODE >> 3, [1.0, 1.0, 0.0], [0.0, 0.0, -1.0]);
    program.extend(INTERSECT);
    if let Some(result) = run(program) {
        assert_eq!(result, [32.0f32, 16.0, 4.0, 4.0].map(f32::to_bits));
    }
}
