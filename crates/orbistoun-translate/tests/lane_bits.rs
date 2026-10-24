//! The complement, the scalar nor and bit-clear test, a trap with no handler, compares in their SDWA
//! form, and the backward lane permute, run on the device.
//!
//! One group of sixty-four threads, each its lane id in `v0`. Each program leaves four results a
//! lane in `v[4:7]`, which it stores into the window at sixteen bytes a lane through the descriptor
//! in user data words 8-11. Both ways a dispatch runs are held to it: the whole wave in one
//! invocation, and one invocation per lane (D880).

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::{GuestDispatch, dispatch_guest};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
};

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

const WINDOW: u64 = 0x4_0187_0000;
const WINDOW_WORDS: u32 = 4096;
/// What memory held before, so an untouched word is told from a written one.
const BEFORE: u32 = 0xdead_beef;
/// The output descriptor's fourth word: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;
const LITERAL: u32 = 255;
/// The SDWA marker in a short form's first source field.
const SDWA: u32 = 0xf9;
/// The SDWA select of a whole register, and of byte 0, byte 1 and word 1.
const DWORD: u32 = 6;
const BYTE_0: u32 = 0;
const BYTE_1: u32 = 1;
const WORD_1: u32 = 5;
/// The source codes of `exec`'s halves.
const EXEC_LO: u32 = 126;
const EXEC_HI: u32 = 127;

/// The source code of an inline integer.
const fn inline(value: u32) -> u32 {
    128 + value
}
/// The source code of an inline negative integer, -1 to -16.
const fn negative(value: u32) -> u32 {
    192 + value
}
/// A vector register as a source.
const fn vgpr(register: u32) -> u32 {
    256 + register
}

/// The two ways a dispatch runs: the whole wave in one invocation, and one invocation per lane.
const MODELS: [(&str, bool); 2] = [("whole wave", false), ("per lane", true)];

/// A word of the encoding table's family for `name`, its opcode placed, with `fields` ored in.
fn encoded(name: &str, fields: u32) -> u32 {
    let encodings = EncodingTable::builtin().expect("encodings");
    let (family, opcode) = encodings.find_by_name(name).expect("this target has it");
    let encoding = encodings
        .encodings()
        .iter()
        .find(|e| e.name == family)
        .expect("the family it was found in");
    encoding.value | (opcode << encoding.opcode.shift) | fields
}

/// A short vector form `name vDst, <source code>`.
fn vop1(name: &str, dst: u32, source: u32) -> u32 {
    encoded(name, source | (dst << 17))
}

/// A SOP1 instruction `name sDst, <source code>`.
fn sop1(name: &str, dst: u32, source: u32) -> u32 {
    encoded(name, (dst << 16) | source)
}

/// A long form's second word: its three source codes.
const fn sources(a: u32, b: u32, c: u32) -> u32 {
    a | (b << 9) | (c << 18)
}

/// A SOP2 instruction `name sDst, a, b`.
fn sop2(name: &str, dst: u32, a: u32, b: u32) -> u32 {
    encoded(name, (dst << 16) | (b << 8) | a)
}

/// A SOPC instruction `name a, b`.
fn sopc(name: &str, a: u32, b: u32) -> u32 {
    encoded(name, (b << 8) | a)
}

/// `s_mov_b32 sDst, <source code>`.
fn s_mov(dst: u32, source: u32) -> u32 {
    encoded("s_mov_b32", (dst << 16) | source)
}

/// An SDWA modifier word: the first source's code and whether it is in the scalar space, the second
/// source's select and whether its field names a scalar register, each source's select and sign
/// extension, negate and absolute value (bits 20, 21, 28, 29 as `negate_absolute`).
fn sdwa_word(
    (src0, src0_scalar): (u32, bool),
    selects: [(u32, bool); 2],
    src1_scalar: bool,
    negate_absolute: u32,
) -> u32 {
    src0 | (DWORD << 8)
        | (selects[0].0 << 16)
        | (u32::from(selects[0].1) << 19)
        | (u32::from(src0_scalar) << 23)
        | (selects[1].0 << 24)
        | (u32::from(selects[1].1) << 27)
        | (u32::from(src1_scalar) << 31)
        | negate_absolute
}

/// `v_mov_b32 vN, 0` for each of `v[4:7]`, so a lane a compare stops shows zero.
fn cleared() -> Vec<u32> {
    (4..8)
        .map(|n| vop1("v_mov_b32_e32", n, inline(0)))
        .collect()
}

/// `s_mov_b64 exec, -1`: every lane runs again.
fn all_lanes() -> u32 {
    encoded("s_mov_b64", (EXEC_LO << 16) | negative(1))
}

/// Stores `v[4:7]` into the window at sixteen bytes a lane; then the end.
fn results() -> Vec<u32> {
    vec![
        all_lanes(),
        // v_lshlrev_b32 v9, 4, v0
        vop1("v_lshlrev_b32_e32", 9, inline(4)),
        // buffer_store_dwordx4 v[4:7], v9, s[8:11], 0 offen
        encoded("buffer_store_dwordx4", 1 << 12),
        9 | (4 << 8) | (2 << 16) | (inline(0) << 24),
        encoded("s_endpgm", 0),
    ]
}

/// `program` translated for a dispatch of one group of sixty-four threads, one invocation per lane
/// where `per_lane`, on the device's subgroup.
fn module(
    program: &[u32],
    per_lane: bool,
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
    let subgroup = per_lane.then(|| match probe() {
        Availability::Available { properties } => properties.subgroup_size,
        Availability::Unavailable { .. } => 0,
    });
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            first_register: 0,
            count: 12,
            block_offset: 0,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [64, 1, 1],
                ..ComputeInputs::default()
            }),
            per_invocation: subgroup,
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
}

/// Runs `program` as one group over a window that held [`BEFORE`] everywhere.
fn run(program: &[u32], per_lane: bool) -> GuestDispatch {
    let mut block = vec![0_u32; 32];
    block[8] = WINDOW as u32;
    block[9] = (WINDOW >> 32) as u32 & 0xffff;
    block[10] = WINDOW_WORDS * 4;
    block[11] = WORD3;
    dispatch_guest(
        &module(program, per_lane).expect("the program translates"),
        &vec![BEFORE; WINDOW_WORDS as usize],
        (&block, [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("dispatched, not the device lost")
}

/// Asserts each lane's four results at its sixteen bytes.
fn assert_results(memory: &[u32], model: &str, expected: impl Fn(u32) -> [u32; 4]) {
    for lane in 0..64_u32 {
        let at = (lane * 4) as usize;
        assert_eq!(&memory[at..at + 4], &expected(lane), "{model}: lane {lane}");
    }
}

/// `v_not_b32` in its short, long and SDWA forms, and the scalar `s_nor_b32` and `s_bitcmp0_b32`
/// either side of an `s_trap`, which a dispatch with no trap handler runs past, and two cache
/// invalidations (AMD's published
/// RDNA 2 instruction set: `V_NOT_B32` is `~S0`, `S_NOR_B32` `~(S0 | S1)` with the condition code
/// whether it is non-zero, `S_BITCMP0_B32` the condition code whether bit `S1[4:0]` of `S0` is
/// clear).
///
/// - `v4 = ~lane`, the short form.
/// - `v5 = ~lane`, the long form.
/// - `v6 = 127 - lane`: the SDWA form's source is byte 0 of `lane ^ 0xff80`, `lane + 128`,
///   sign-extended to `lane - 128`, whose complement is `127 - lane`.
/// - `v7 = 0xf0f0_ff0f + 5`: `~(0x0f0f_0000 | 0xf0)`, plus its being non-zero (1) or'd with bit 16
///   being clear (no, 0) and bit 4 being clear (yes, 4).
#[test]
fn the_complements_and_the_scalar_bit_tests_compute_as_published() {
    if !device_or_skip("the complements and the scalar bit tests") {
        return;
    }
    let mut program = vec![
        // v_xor_b32 v3, 0xff80, v0
        vop1("v_xor_b32_e32", 3, LITERAL),
        0xff80,
        vop1("v_not_b32_e32", 4, vgpr(0)),
        encoded("v_not_b32_e64", 5),
        vgpr(0),
        // v_not_b32_sdwa v6, sext(v3) src0_sel:BYTE_0
        vop1("v_not_b32_e32", 6, SDWA),
        sdwa_word((3, false), [(BYTE_0, true), (DWORD, false)], false, 0),
        s_mov(15, LITERAL),
        0x0f0f_0000,
        sop2("s_nor_b32", 16, 15, LITERAL),
        0xf0,
        sop2("s_cselect_b32", 17, inline(1), inline(0)),
        sopc("s_bitcmp0_b32", 15, inline(16)),
        sop2("s_cselect_b32", 18, inline(2), inline(0)),
        encoded("s_trap", 2),
        // The cache invalidations, which change nothing a module computes.
        encoded("buffer_gl1_inv", 0),
        0,
        encoded("buffer_gl0_inv", 0),
        0,
        sopc("s_bitcmp0_b32", 15, inline(4)),
        sop2("s_cselect_b32", 19, inline(4), inline(0)),
        sop2("s_or_b32", 17, 17, 18),
        sop2("s_or_b32", 17, 17, 19),
        sop2("s_add_u32", 16, 16, 17),
        vop1("v_mov_b32_e32", 7, 16),
    ];
    program.extend(results());
    for (model, per_lane) in MODELS {
        let done = run(&program, per_lane);
        assert_results(&done.memory, model, |lane| {
            [!lane, !lane, 127_u32.wrapping_sub(lane), 0xf0f0_ff0f + 5]
        });
    }
}

/// Compares in their SDWA form narrow the execution mask, each source the part of its register its
/// select names: `v_cmpx_class_f32` on `-|x|` against a mask in a scalar register, as Grand Theft
/// Auto V's in-world compute programs do (word `0x86360000`); `v_cmpx_ne_u32` of an inline zero in
/// the scalar space (as its `0x86860080`); `v_cmpx_eq_u32` of byte 1 against word 1; and
/// `v_cmpx_lt_i32` of a sign-extended byte against a scalar register (as its `v_cmpx_ge_i32`'s
/// `0x8686006a`).
///
/// - `v4`: `-|float(lane)|` is a negative normal (class bit 3) but at lane 0, a negative zero.
/// - `v5`: `0 != lane`, all but lane 0.
/// - `v6`: byte 1 of `lane << 8`, `lane`, against word 1 of `lane`, zero: lane 0 only.
/// - `v7`: byte 0 of `lane ^ 0xff80` sign-extended, `lane - 128`, below -100: lanes below 28.
#[test]
fn sdwa_compares_into_exec_take_their_parts() {
    if !device_or_skip("SDWA compares into exec") {
        return;
    }
    let mut program = cleared();
    program.extend([
        vop1("v_xor_b32_e32", 3, LITERAL),
        0xff80,
        vop1("v_cvt_f32_u32_e32", 2, vgpr(0)),
        // v_lshlrev_b32 v8, 8, v0
        vop1("v_lshlrev_b32_e32", 8, inline(8)),
        s_mov(14, inline(8)),
        s_mov(16, LITERAL),
        (-100_i32).cast_unsigned(),
        // v_cmpx_class_f32_sdwa -|v2|, s14
        encoded("v_cmpx_class_f32_e32", SDWA | (14 << 9)),
        sdwa_word((2, false), [(DWORD, false); 2], true, 0b11 << 20),
        vop1("v_mov_b32_e32", 4, inline(1)),
        all_lanes(),
        // v_cmpx_ne_u32_sdwa 0, v0
        encoded("v_cmpx_ne_u32_e32", SDWA),
        sdwa_word((inline(0), true), [(DWORD, false); 2], false, 0),
        vop1("v_mov_b32_e32", 5, inline(1)),
        all_lanes(),
        // v_cmpx_eq_u32_sdwa v8, v0 src0_sel:BYTE_1 src1_sel:WORD_1
        encoded("v_cmpx_eq_u32_e32", SDWA),
        sdwa_word((8, false), [(BYTE_1, false), (WORD_1, false)], false, 0),
        vop1("v_mov_b32_e32", 6, inline(1)),
        all_lanes(),
        // v_cmpx_lt_i32_sdwa sext(v3), s16 src0_sel:BYTE_0
        encoded("v_cmpx_lt_i32_e32", SDWA | (16 << 9)),
        sdwa_word((3, false), [(BYTE_0, true), (DWORD, false)], true, 0),
        vop1("v_mov_b32_e32", 7, inline(1)),
    ]);
    program.extend(results());
    for (model, per_lane) in MODELS {
        let done = run(&program, per_lane);
        assert_results(&done.memory, model, |lane| {
            [
                u32::from(lane != 0),
                u32::from(lane != 0),
                u32::from(lane == 0),
                u32::from(lane < 28),
            ]
        });
    }
}

/// `ds_bpermute_b32 vDst, vAddress, vData offset:o`: each lane takes `vData` from lane
/// `(vAddress + o) / 4`, modulo 32 within its own half of the wave, or zero where that lane does
/// not run (AMD's published RDNA 2 instruction set; the half waves as ACO's
/// `emit_bpermute_shared_vgpr` works around them).
///
/// With `v1 = 3 * lane`:
/// - `v4`: from lane `lane ^ 1`.
/// - `v5`: from lane `lane + 40` - past the half, so `(lane + 40) % 32` in the reader's half.
/// - `v6`: from lane `lane ^ 1` with only the even lanes running: an even lane reads an odd one,
///   which does not run, and takes zero; an odd lane does not run and keeps its 0x77.
/// - `v7`: from `lane * 4` with the offset 4, the next lane in the half.
#[test]
fn a_backward_permute_reads_the_lane_its_address_names() {
    if !device_or_skip("a backward permute") {
        return;
    }
    let permute = |dst: u32, address: u32, offset: u32| {
        [
            encoded("ds_bpermute_b32", offset),
            address | (1 << 8) | (dst << 24),
        ]
    };
    let mut program = vec![
        // v_lshl_add_u32 v1, v0, 1, v0
        encoded("v_lshl_add_u32", 1),
        vgpr(0) | (inline(1) << 9) | (vgpr(0) << 18),
        // v2 = (lane ^ 1) * 4; a short form's second source field is zero, `v0`, unless set.
        vop1("v_xor_b32_e32", 2, inline(1)),
        vop1("v_lshlrev_b32_e32", 2, inline(2)) | (2 << 9),
        // v3 = (lane + 40) * 4
        vop1("v_add_nc_u32_e32", 3, inline(40)),
        vop1("v_lshlrev_b32_e32", 3, inline(2)) | (3 << 9),
        // v8 = lane * 4
        vop1("v_lshlrev_b32_e32", 8, inline(2)),
        vop1("v_mov_b32_e32", 6, LITERAL),
        0x77,
    ];
    program.extend(permute(4, 2, 0));
    program.extend(permute(5, 3, 0));
    program.extend(permute(7, 8, 4));
    program.extend([s_mov(EXEC_LO, LITERAL), 0x5555_5555]);
    program.extend([s_mov(EXEC_HI, LITERAL), 0x5555_5555]);
    program.extend(permute(6, 2, 0));
    program.push(encoded("s_waitcnt", 0));
    program.extend(results());
    // One invocation per lane holds only its own registers, so it refuses the permute by name and
    // the dispatch takes the whole wave (D880).
    let refused = module(&program, true).expect_err("refused one invocation per lane");
    assert!(refused.to_string().contains("another lane"), "{refused}");
    {
        let model = "whole wave";
        let done = run(&program, false);
        assert_results(&done.memory, model, |lane| {
            let half = lane & 32;
            [
                3 * (lane ^ 1),
                3 * (half | ((lane + 40) & 31)),
                if lane % 2 == 0 { 0 } else { 0x77 },
                3 * (half | ((lane + 1) & 31)),
            ]
        });
    }
}

/// The sixty-four-bit integer compares over register pairs, into the execution mask and into
/// `vcc` (AMD's published RDNA 2 instruction set, VOPC): the high words decide where they differ,
/// the low words, unsigned, where they do not. Grand Theft Auto V's in-world compute programs test
/// a pointer with `v_cmpx_ne_u64 0, v[0:1]`.
///
/// With `v[2:3]` the pair `(lane, -(lane & 1))` - `lane` for an even lane, `lane - 2^32` for an
/// odd one:
/// - `v4`: `v_cmpx_ne_u64 0, v[2:3]`, all but lane 0.
/// - `v5`: `v_cmp_lt_u64 s[14:15], v[2:3]` with `s[14:15] = 40`: every odd lane, whose high word
///   is all ones unsigned, and the even lanes past 40.
/// - `v6`: `v_cmpx_gt_i64 s[16:17], v[2:3]` with `s[16:17] = 5 - 2^32`: the odd lanes below 5.
/// - `v7`: `v_cmp_ge_i64 -1, v[2:3]`, the inline -1 sign-extended: every odd lane.
#[test]
fn sixty_four_bit_compares_order_register_pairs() {
    if !device_or_skip("sixty-four-bit compares") {
        return;
    }
    let compare = |name: &str, src0: u32| encoded(name, src0 | (2 << 9));
    let mut program = cleared();
    program.extend([
        vop1("v_mov_b32_e32", 2, vgpr(0)),
        // v_and_b32 v3, 1, v0; v_sub_nc_u32 v3, 0, v3
        vop1("v_and_b32_e32", 3, inline(1)),
        vop1("v_sub_nc_u32_e32", 3, inline(0)) | (3 << 9),
        vop1("v_mov_b32_e32", 8, inline(1)),
        s_mov(14, inline(40)),
        s_mov(15, inline(0)),
        s_mov(16, inline(5)),
        s_mov(17, negative(1)),
        compare("v_cmpx_ne_u64_e32", inline(0)),
        vop1("v_mov_b32_e32", 4, inline(1)),
        all_lanes(),
        compare("v_cmp_lt_u64_e32", 14),
        // v_cndmask_b32 v5, 0, v8, vcc
        vop1("v_cndmask_b32_e32", 5, inline(0)) | (8 << 9),
        compare("v_cmpx_gt_i64_e32", 16),
        vop1("v_mov_b32_e32", 6, inline(1)),
        all_lanes(),
        compare("v_cmp_ge_i64_e32", negative(1)),
        vop1("v_cndmask_b32_e32", 7, inline(0)) | (8 << 9),
    ]);
    program.extend(results());
    for (model, per_lane) in MODELS {
        let done = run(&program, per_lane);
        assert_results(&done.memory, model, |lane| {
            let odd = lane % 2 == 1;
            [
                u32::from(lane != 0),
                u32::from(odd || lane > 40),
                u32::from(odd && lane < 5),
                u32::from(odd),
            ]
        });
    }
}

/// The funnel shifts: `v_alignbit_b32 d, a, b, c` is the low word of `{a, b} >> c[4:0]` and
/// `v_alignbyte_b32` of `{a, b} >> (8 * c[1:0])` (AMD's published RDNA 2 instruction set), here
/// with `a = 0x1234_5678`, `b = 0x9abc_def0` and `c` the lane, as Grand Theft Auto V's in-world
/// compute program extracts a field across two words with the first.
#[test]
fn the_funnel_shifts_take_the_low_word_of_the_pair_shifted() {
    if !device_or_skip("the funnel shifts") {
        return;
    }
    let mut program = cleared();
    program.extend([
        s_mov(14, LITERAL),
        0x1234_5678,
        s_mov(15, LITERAL),
        0x9abc_def0,
        encoded("v_alignbit_b32", 4),
        sources(14, 15, vgpr(0)),
        encoded("v_alignbyte_b32", 5),
        sources(14, 15, vgpr(0)),
    ]);
    program.extend(results());
    let pair = 0x1234_5678_9abc_def0_u64;
    for (model, per_lane) in MODELS {
        let done = run(&program, per_lane);
        assert_results(&done.memory, model, |lane| {
            [
                (pair >> (lane & 31)) as u32,
                (pair >> (8 * (lane & 3))) as u32,
                0,
                0,
            ]
        });
    }
}

/// The lowest set bit and the 32-bit saving mask instructions Grand Theft Auto V's in-world compute
/// program uses (AMD's published RDNA 2 instruction set): `v_ffbl_b32` is the index of the lowest
/// set bit, all ones for zero; `s_andn1_saveexec_b32 d, s` saves `exec_lo` and makes it
/// `~s & exec_lo`; `s_orn2_saveexec_b32 d, s` saves it and makes it `s | ~exec_lo`; neither
/// touches `exec_hi`.
///
/// - `v4`: the lowest set bit of `lane << 3`, all ones at lane 0.
/// - `v5`: the two saved masks added, `0xffff_ffff + 0xffff_0000`.
/// - `v6`: 1 where `~0xffff & exec_lo` runs: lanes 16 to 63.
/// - `v7`: 2 where `1 | ~0xffff_0000` runs then: lanes 0 to 15, and 32 to 63 as before.
#[test]
fn the_lowest_set_bit_and_the_narrow_saving_masks_compute_as_published() {
    if !device_or_skip("the lowest set bit and the narrow saving masks") {
        return;
    }
    let mut program = cleared();
    program.extend([
        vop1("v_lshlrev_b32_e32", 8, inline(3)),
        vop1("v_ffbl_b32_e32", 4, vgpr(8)),
        s_mov(21, LITERAL),
        0xffff,
        s_mov(23, inline(1)),
        sop1("s_andn1_saveexec_b32", 20, 21),
        vop1("v_mov_b32_e32", 6, inline(1)),
        sop1("s_orn2_saveexec_b32", 22, 23),
        vop1("v_mov_b32_e32", 7, inline(2)),
        all_lanes(),
        sop2("s_add_u32", 24, 20, 22),
        vop1("v_mov_b32_e32", 5, 24),
    ]);
    program.extend(results());
    for (model, per_lane) in MODELS {
        let done = run(&program, per_lane);
        assert_results(&done.memory, model, |lane| {
            [
                if lane == 0 {
                    u32::MAX
                } else {
                    lane.trailing_zeros() + 3
                },
                0xffff_ffff_u32.wrapping_add(0xffff_0000),
                u32::from(lane >= 16),
                if (16..32).contains(&lane) { 0 } else { 2 },
            ]
        });
    }
}
