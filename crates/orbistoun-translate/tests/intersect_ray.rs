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

/// The descriptor in s[4:7] - base zero, box sorting on, every node index in range, barycentric
/// return, type 8, as Mesa builds it (`radv_nir_rt_common.c:582-585`) - and the ray in v[8:18]:
/// the pointer, extent, origin, direction and inverse direction.
fn set_ray(program: &mut Vec<u32>, pointer: u32, origin: [f32; 3], direction: [f32; 3]) {
    for (register, word) in (4..).zip([0, 1 << 31, u32::MAX, 0x8100_03ff]) {
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
/// are the distance 2 and the second and third vertices' weights 1/4: the weights its flags word
/// (word 15) names, `9` as a node of one triangle holds it (D992).
#[test]
fn a_triangle_node_answers_its_distance_and_barycentrics() {
    let mut node = [0; NODE_WORDS];
    let vertices = [0.0f32, 0.0, -2.0, 4.0, 0.0, -2.0, 0.0, 4.0, -2.0];
    for (word, coordinate) in node.iter_mut().zip(vertices) {
        *word = coordinate.to_bits();
    }
    node[15] = 9;
    let mut program = Vec::new();
    store_node(&mut program, &node);
    set_ray(&mut program, NODE >> 3, [1.0, 1.0, 0.0], [0.0, 0.0, -1.0]);
    program.extend(INTERSECT);
    if let Some(result) = run(program) {
        assert_eq!(result, [32.0f32, 16.0, 4.0, 4.0].map(f32::to_bits));
    }
}

/// The source code of the inline constant `value`, from 0 to 64.
const fn inline(value: u32) -> u32 {
    0x80 + value
}

/// The source code of the condition mask's low half; its high half is the next.
const VCC_LO: u32 = 0x6a;

/// The source code of vector register `register`.
const fn vgpr(register: u32) -> u32 {
    0x100 + register
}

/// Where the wave's mask of lanes whose result is the expected child is stored, low half first:
/// past the node, in the module's window of sixty-four words.
const RESULTS: u32 = 0xc0;

/// Every lane of a wave tests its own ray, each down +z at its own x, so each meets another child:
/// the lanes' results one by one, as a loop over the lanes runs them (D956). Lane `l` is in the box
/// of child `l / 16`, whose pointer is `0x11` times one more than that.
fn four_children_by_lane() -> Vec<u32> {
    let mut node = [0; NODE_WORDS];
    for child in 0..4_u16 {
        let low = 16.0 * f32::from(child) - 0.25;
        let bounds = [low, -1.0, 2.0, low + 15.5, 1.0, 3.0];
        let index = usize::from(child);
        node[index] = 0x11 * (u32::from(child) + 1);
        for (axis, bound) in bounds.into_iter().enumerate() {
            node[4 + 6 * index + axis] = bound.to_bits();
        }
    }
    // v20: the lane, before the node's stores take v0.
    let mut program = vec![head("v_mov_b32_e32") | (20 << 17) | vgpr(0)];
    store_node(&mut program, &node);
    set_ray(&mut program, (NODE >> 3) | 5, [0.0; 3], [0.0, 0.0, 1.0]);
    // The origin's x, v10 after the pointer and the extent, is the lane as a float.
    program.push(head("v_cvt_f32_u32_e32") | (10 << 17) | vgpr(20));
    program.extend(INTERSECT);
    // v22: the child the lane's ray meets, `0x11 * (lane / 16 + 1)`, as `n << 4 + n`.
    program.push(head("v_lshrrev_b32_e32") | (22 << 17) | (20 << 9) | inline(4));
    program.push(head("v_add_nc_u32_e32") | (22 << 17) | (22 << 9) | inline(1));
    program.push(head("v_lshlrev_b32_e32") | (23 << 17) | (22 << 9) | inline(4));
    program.push(head("v_add_nc_u32_e32") | (22 << 17) | (22 << 9) | vgpr(23));
    // vcc: the lanes whose first result is that child, stored whole.
    program.push(head("v_cmp_eq_u32_e32") | (22 << 9) | vgpr(0));
    program.push(head("v_mov_b32_e32") | (23 << 17) | VCC_LO);
    program.push(head("v_mov_b32_e32") | (24 << 17) | (VCC_LO + 1));
    program.extend(v_mov_literal(25, RESULTS));
    program.extend(global_store(25, 23));
    program.extend(v_mov_literal(25, RESULTS + 4));
    program.extend(global_store(25, 24));
    program.push(head("s_endpgm"));
    program
}

fn module_of(program: &[u32]) -> Vec<u32> {
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|word| word.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Wavefront,
        width: Width::default(),
    };
    translate(&decoded, &table, strategy)
        .expect("translates")
        .module
}

#[test]
fn every_lane_tests_its_own_ray() {
    let module = module_of(&four_children_by_lane());
    if let Availability::Unavailable { reason } = probe() {
        println!("!! SKIPPED: no Vulkan device: {reason}; translated shaders were NOT executed");
        return;
    }
    let out = dispatch(&module, PER_FILE * 2, MEMORY_WORDS as usize, [1, 1, 1]).expect("dispatch");
    let at = (RESULTS / 4) as usize;
    let lanes = [out.memory[at], out.memory[at + 1]];
    assert_eq!(
        lanes,
        [u32::MAX; 2],
        concat!(
            "the lanes whose result was their own child, as a mask (lane 0 in bit 0); lane 0's ",
            "v0-v3 were {:x?}",
        ),
        &out.observed[..4]
    );
}

/// One intersect is one loop over the lanes, not a node test emitted for each lane (D956): the
/// module of a wave's intersect stays within a few lanes' worth of words. Grand Theft Auto V's
/// program at `0x74005b7e8600` emitted each of its six intersects 64 times, 1.3 million words
/// apiece, and the driver never finished compiling it.
#[test]
fn an_intersect_is_one_loop_over_the_lanes() {
    let looped = module_of(&four_children_by_lane()).len();
    // The same program without the intersect: what the rest of it costs.
    let mut without = four_children_by_lane();
    let at = without
        .windows(2)
        .position(|pair| pair == INTERSECT)
        .expect("the intersect");
    without.drain(at..at + 2);
    let rest = module_of(&without).len();
    let intersect = looped - rest;
    println!("[intersect] module {looped} words, the intersect {intersect}, the rest {rest}");
    assert!(
        intersect < 8_000,
        "one intersect took {intersect} words: emitted for each lane rather than once in a loop"
    );
}
