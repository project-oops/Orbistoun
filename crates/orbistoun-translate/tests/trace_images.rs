//! The host's concrete wave trace ([`evaluate::wave_trace_with_images`]) over what a ray-traversal
//! pass reads besides memory words: a hierarchy node tested by `image_bvh_intersect_ray`, a texel an
//! `image_load` reads, an `image_store`'s texel, a one-channel format load through a descriptor of
//! words, and a reciprocal square root. Each is what `trace-dispatch` compares a device-addressed
//! dispatch's stores against.

use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::evaluate::{
    LaneProgram, TraceImages, TraceStop, WaveEntry, WaveTrace, wave_trace_with_images,
};

/// `v_mov_b32 v<n>, <literal>`: VOP1, opcode 1, source 255 then the literal.
fn move_literal(register: u32, value: u32) -> [u32; 2] {
    [0x7e00_0000 | register << 17 | 1 << 9 | 0xff, value]
}

/// `v_mov_b32 v<to>, v<from>`.
fn move_vector(to: u32, from: u32) -> u32 {
    0x7e00_0000 | to << 17 | 1 << 9 | (256 + from)
}

const S_ENDPGM: u32 = 0xbf81_0000;

/// Runs `program` as one wave of `lanes` threads over `user_data`, `memory` and `images`.
fn trace(
    program: &[u32],
    user_data: &[u32],
    memory: &dyn Fn(u64) -> Option<u32>,
    images: &mut dyn TraceImages,
) -> WaveTrace {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let waves = [WaveEntry {
        group_ids: vec![0],
        threads: (0..4).map(|t| Some([t, 0, 0])).collect(),
        components: 1,
    }];
    let mut read = |address: u64| memory(address);
    let run = wave_trace_with_images(
        // `DX10_CLAMP` set: a clamp turns a NaN into zero.
        &LaneProgram::new(decoded, &encodings, &[]).clamping(Some(true)),
        &encodings,
        user_data,
        (64, &waves),
        (64, 0, 0),
        (&mut read, images),
    );
    assert!(
        matches!(run.stop, TraceStop::Ended),
        "the host runs it to its end: {:?}",
        run.stop
    );
    run
}

/// No image: what a trace of a program that reads none is handed.
struct Blank;

impl TraceImages for Blank {
    fn load(&mut self, _: [u32; 8], _: [u32; 3], _: u32) -> Option<[u32; 4]> {
        None
    }
    fn sample(&mut self, _: [u32; 8], _: [u32; 4], _: [f32; 3], _: f32) -> Option<[u32; 4]> {
        None
    }
}

/// A ray down +z from the origin against a box node of floats at 0x10000, whose second and third
/// children lie ahead on the ray: without box sorting the hits keep the node's order, a miss
/// answering `0xffffffff`.
#[test]
fn a_hierarchy_node_is_tested_on_the_host() {
    const NODE: u64 = 0x1_0000;
    let mut node = [0_u32; 28];
    let boxes: [(u32, [f32; 3], [f32; 3]); 4] = [
        (0x11, [5.0, 5.0, 0.0], [6.0, 6.0, 1.0]),
        (0x22, [-1.0, -1.0, 8.0], [1.0, 1.0, 9.0]),
        (0x33, [-1.0, -1.0, 2.0], [1.0, 1.0, 3.0]),
        (0x44, [-1.0, -1.0, -5.0], [1.0, 1.0, -4.0]),
    ];
    for (child, (pointer, low, high)) in boxes.into_iter().enumerate() {
        node[child] = pointer;
        for axis in 0..3 {
            node[4 + 6 * child + axis] = low[axis].to_bits();
            node[4 + 6 * child + 3 + axis] = high[axis].to_bits();
        }
    }
    // The ray in v42..v52: the pointer (a box of floats at offset 0), the extent, the origin, the
    // direction and its inverse.
    let ray = [
        5,
        100.0_f32.to_bits(),
        0,
        0,
        0,
        0,
        0,
        1.0_f32.to_bits(),
        f32::INFINITY.to_bits(),
        f32::INFINITY.to_bits(),
        1.0_f32.to_bits(),
    ];
    let mut program: Vec<u32> = (0..11)
        .flat_map(|at| move_literal(42 + at, ray[at as usize]))
        .collect();
    // `image_bvh_intersect_ray v[2:5], v[42:52], s[16:19]`.
    program.extend([0xf198_9f01, 0x0004_022a, S_ENDPGM]);
    // s16..s19: the base 0x10000 in 256-byte units, no box sorting, return mode 1.
    let mut user_data = vec![0; 16];
    user_data.extend([0x100, 0, u32::MAX, 0x8100_03ff]);
    let memory = |address: u64| {
        let word = address.checked_sub(NODE)? / 4;
        node.get(usize::try_from(word).ok()?).copied()
    };
    let run = trace(&program, &user_data, &memory, &mut Blank);
    let lane: Vec<Option<u32>> = run.lanes[0][2..6].to_vec();
    assert_eq!(
        lane,
        [Some(u32::MAX), Some(0x22), Some(0x33), Some(u32::MAX)],
        "the hits in the node's order"
    );
}

/// The image a test reads: texel `(x, y)` answers `[10x, 10y, 0x1234, 1.0]`.
struct Ramp;

impl TraceImages for Ramp {
    fn load(&mut self, _: [u32; 8], [x, y, _]: [u32; 3], level: u32) -> Option<[u32; 4]> {
        (level == 0).then_some([10 * x, 10 * y, 0x1234, 1.0_f32.to_bits()])
    }
    fn sample(&mut self, _: [u32; 8], _: [u32; 4], _: [f32; 3], _: f32) -> Option<[u32; 4]> {
        None
    }
}

/// An `image_load` of texel (3, 4) reads what the images answer, its data mask's components in
/// order; an `image_store` of one of them at (5, 6) is a texel the trace records, under the store's
/// descriptor and byte.
#[test]
fn an_image_load_reads_the_texel_and_a_store_records_it() {
    let mut program = Vec::new();
    program.extend(move_literal(6, 3));
    program.extend(move_literal(7, 4));
    // `image_load v[6:9], v[6:7], s[8:15] dmask:0xf dim:SQ_RSRC_IMG_2D`.
    program.extend([0xf000_0f08, 0x0002_0606]);
    program.push(move_vector(2, 8));
    program.extend(move_literal(10, 5));
    program.extend(move_literal(11, 6));
    // `image_store v2, v[10:11], s[8:15] dmask:0x1 dim:SQ_RSRC_IMG_2D`.
    let store_at = 4 * program.len() as u32;
    program.extend([0xf020_0108, 0x0002_020a, S_ENDPGM]);
    let descriptor: [u32; 8] = std::array::from_fn(|word| 0x100 + word as u32);
    let mut user_data = vec![0; 8];
    user_data.extend(descriptor);
    let run = trace(&program, &user_data, &|_| None, &mut Ramp);
    assert_eq!(
        run.lanes[1][6..10],
        [Some(30), Some(40), Some(0x1234), Some(1.0_f32.to_bits())]
    );
    let stored = run.texels.get(&(descriptor, [5, 6, 0], 0));
    assert_eq!(stored, Some(&(vec![(0, Some(0x1234))], store_at)));
}

/// `buffer_load_format_x` through a descriptor of `32_UINT` elements reads the word at the lane's
/// index, as a dword load does.
#[test]
fn a_format_load_of_words_reads_the_word() {
    const BASE: u64 = 0x2_0000;
    let mut program = Vec::new();
    program.extend(move_literal(2, 3));
    // `buffer_load_format_x v2, v2, s[0:3], 0 idxen`.
    program.extend([0xe000_2000, 0x8000_0202, S_ENDPGM]);
    // Base 0x20000, stride 4, 64 records, format 20 (`32_UINT`).
    let user_data = [0x2_0000, 4 << 16, 64, 0x0001_4204];
    let memory = |address: u64| {
        let word = address.checked_sub(BASE)? / 4;
        (word < 64).then(|| 0x100 + word as u32)
    };
    let run = trace(&program, &user_data, &memory, &mut Blank);
    assert_eq!(run.lanes[0][2], Some(0x103));
}

/// A reciprocal square root, which the module spells as an extended instruction, is computed on
/// the host in a trace: 1/sqrt(4) is one half.
#[test]
fn a_reciprocal_square_root_is_computed_in_a_trace() {
    let mut program = Vec::new();
    program.extend(move_literal(3, 4.0_f32.to_bits()));
    // `v_rsq_f32_e32 v2, v3`: VOP1 opcode 0x2e.
    program.extend([0x7e00_0000 | 2 << 17 | 0x2e << 9 | (256 + 3), S_ENDPGM]);
    let run = trace(&program, &[0; 4], &|_| None, &mut Blank);
    assert_eq!(run.lanes[0][2], Some(0.5_f32.to_bits()));
}

/// The inline operand `1/(2*pi)`, which `v_sin_f32`'s callers scale by, and `src_scc`, the
/// condition code read as a word, read their values in a trace.
#[test]
fn inline_reciprocal_two_pi_and_the_condition_code_read_their_values() {
    let mut program = Vec::new();
    program.extend(move_literal(3, 2.0_f32.to_bits()));
    // `v_mul_f32_e32 v2, 0.15915494, v3`: VOP2 opcode 8, source 248.
    program.push(8 << 25 | 2 << 17 | 3 << 9 | 0xf8);
    // `s_cmp_eq_u32 s0, s0`, then `v_mov_b32_e32 v4, src_scc` (source 253).
    program.push(0xbf06_0000);
    program.push(0x7e00_0000 | 4 << 17 | 1 << 9 | 0xfd);
    program.push(S_ENDPGM);
    let run = trace(&program, &[7; 4], &|_| None, &mut Blank);
    assert_eq!(
        run.lanes[0][2],
        Some((2.0_f32 * f32::from_bits(0x3e22_f983)).to_bits())
    );
    assert_eq!(run.lanes[0][4], Some(1));
}

/// An output clamp, which the shader's `DX10_CLAMP` mode decides for a NaN, is computed once the
/// program says that mode: 2 * 3 clamped is one.
#[test]
fn a_clamped_result_is_computed_under_the_program_s_clamp_mode() {
    let mut program = Vec::new();
    program.extend(move_literal(3, 2.0_f32.to_bits()));
    program.extend(move_literal(4, 3.0_f32.to_bits()));
    // `v_mul_f32_e64 v2, v3, v4 clamp`: VOP3 opcode 0x108, clamp bit 15.
    program.extend([0xd508_8002, 0x103 | 0x104 << 9, S_ENDPGM]);
    let run = trace(&program, &[0; 4], &|_| None, &mut Blank);
    assert_eq!(run.lanes[0][2], Some(1.0_f32.to_bits()));
}

/// A global access's offset is a signed twelve-bit number: `global_load_dword v3, v[6:7], off
/// offset:-4` reads the word before the address its registers hold.
#[test]
fn a_global_offset_is_signed() {
    const BASE: u64 = 0x3_0000;
    let mut program = Vec::new();
    program.extend(move_literal(6, 0x3_0010));
    program.extend(move_literal(7, 0));
    // `0xffc` in the offset field: -4.
    program.extend([0xdc30_8ffc, 0x037d_0006, S_ENDPGM]);
    let memory = |address: u64| {
        let word = address.checked_sub(BASE)? / 4;
        (word < 64).then(|| 0x500 + word as u32)
    };
    let run = trace(&program, &[0; 4], &memory, &mut Blank);
    assert_eq!(run.lanes[0][3], Some(0x503), "the word at 0x3000c");
}
