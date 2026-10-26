//! The multiply-adds the instruction set defines as fused round once, and those it defines as
//! unfused round the product first (AMD's published RDNA 2 instruction set; `Model::f32_fma`), on
//! the device and in the host's concrete run alike. Each form runs on operands where the two
//! differ, and is held to the value worked out by hand:
//!
//! - single precision: `a = b = 1 + 2^-12`, `c = -(1 + 2^-11)`. The product is
//!   `1 + 2^-11 + 2^-24`, which rounds to `1 + 2^-11` (a tie, to even). Fused, the result is
//!   `2^-24` (`0x33800000`); unfused, it is zero.
//! - half precision: `a = 2^-11 * (1 + 2^-10)` (`0x1001`), `b = 1 - 2^-10` (`0x3bfe`),
//!   `c = 1 + 2^-10` (`0x3c01`). The exact result is `1 + 3 * 2^-11 - 2^-31`, just below the tie
//!   between `0x3c01` and `0x3c02`, so rounded once it is `0x3c01`. Rounded first to single
//!   precision it lands on the tie, which rounds to the even `0x3c02`.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::dispatch_guest;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::evaluate::{LaneProgram, TraceStop, WaveEntry, wave_trace};
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
};

/// Where the window sits, and how many words it spans.
const WINDOW: u64 = 0x4_0187_0000;
const WINDOW_WORDS: u32 = 64;
/// The group's threads, one wave of thirty-two.
const THREADS: u32 = 32;
/// User-data words: 8-11 the descriptor, then the group id in `s12`.
const USER_WORDS: usize = 12;
/// `SQ_BUF_RSRC_WORD3`: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;
/// What memory held before.
const BEFORE: u32 = 0xdead_beef;

/// The single-precision operands, and the results fused and unfused.
const ONE_PLUS_2_12: u32 = 0x3f80_0800;
const MINUS_ONE_PLUS_2_11: u32 = 0xbf80_1000;
const FUSED: u32 = 0x3380_0000;
const UNFUSED: u32 = 0;
/// The half-precision operands and the result rounded once.
const HALF_A: u32 = 0x1001;
const HALF_B: u32 = 0x3bfe;
const HALF_C: u32 = 0x3c01;
const HALF_FUSED: u32 = 0x3c01;

/// `v_mov_b32 v<n>, <literal>`.
fn move_literal(register: u32, value: u32) -> [u32; 2] {
    [0x7e00_0000 | register << 17 | 1 << 9 | 0xff, value]
}

/// `v_mov_b32 v2, v<n>`.
fn move_to_result(register: u32) -> u32 {
    0x7e00_0000 | 2 << 17 | 1 << 9 | (256 + register)
}

/// `v_and_b32 v2, 0xffff, v2`: the low half alone.
const LOW_HALF: [u32; 2] = [0x3604_04ff, 0xffff];

/// The program around `body`: `v3`, `v4`, `v5` the operands, then `body`, which leaves its result
/// in `v2`, stored at the thread's word.
fn around(operands: [u32; 3], body: &[u32]) -> Vec<u32> {
    // v_lshlrev_b32 v70, 2, v0: the thread's byte.
    let mut program = vec![0x348c_0082];
    for (register, value) in (3..).zip(operands) {
        program.extend(move_literal(register, value));
    }
    program.extend_from_slice(body);
    // buffer_store_dword v2, v70, s[8:11], 0 offen.
    program.extend([0xe070_1000, 0x8002_0246, 0xbf81_0000]);
    program
}

/// Each form, its operands, its body and what it must leave, by name. Words as `llvm-mc` encodes
/// them for this target.
fn forms() -> Vec<(&'static str, [u32; 3], Vec<u32>, u32)> {
    let single = [ONE_PLUS_2_12, ONE_PLUS_2_12, MINUS_ONE_PLUS_2_11];
    let half = [HALF_A, HALF_B, HALF_C];
    let packed = half.map(|half| half | half << 16);
    vec![
        // v_fma_f32 v2, v3, v4, v5
        ("v_fma_f32", single, vec![0xd54b_0002, 0x0416_0903], FUSED),
        // v_mad_f32 v2, v3, v4, v5
        ("v_mad_f32", single, vec![0xd541_0002, 0x0416_0903], UNFUSED),
        // v_fmac_f32_e32 v5, v3, v4
        (
            "v_fmac_f32",
            single,
            vec![0x560a_0903, move_to_result(5)],
            FUSED,
        ),
        // v_mac_f32_e32 v5, v3, v4
        (
            "v_mac_f32",
            single,
            vec![0x3e0a_0903, move_to_result(5)],
            UNFUSED,
        ),
        // v_fmaak_f32 v2, v3, v4, -(1 + 2^-11)
        (
            "v_fmaak_f32",
            single,
            vec![0x5a04_0903, MINUS_ONE_PLUS_2_11],
            FUSED,
        ),
        // v_madak_f32 v2, v3, v4, -(1 + 2^-11)
        (
            "v_madak_f32",
            single,
            vec![0x4204_0903, MINUS_ONE_PLUS_2_11],
            UNFUSED,
        ),
        // v_fma_mix_f32 v2, v3, v4, v5: every source single precision.
        (
            "v_fma_mix_f32",
            single,
            vec![0xcc20_0002, 0x0416_0903],
            FUSED,
        ),
        // s_mov_b32 vcc_lo, 0; s_mov_b32 vcc_hi, 0; v_div_fmas_f32 v2, v3, v4, v5: no lane scaled.
        (
            "v_div_fmas_f32",
            single,
            vec![0xbeea_0380, 0xbeeb_0380, 0xd56f_0002, 0x0416_0903],
            FUSED,
        ),
        // v_fma_f16 v2, v3, v4, v5
        (
            "v_fma_f16",
            half,
            [&[0xd74b_0002, 0x0416_0903][..], &LOW_HALF].concat(),
            HALF_FUSED,
        ),
        // v_fmac_f16_e32 v5, v3, v4
        (
            "v_fmac_f16",
            half,
            [&[0x6c0a_0903, move_to_result(5)][..], &LOW_HALF].concat(),
            HALF_FUSED,
        ),
        // v_pk_fma_f16 v2, v3, v4, v5: both halves.
        (
            "v_pk_fma_f16",
            packed,
            vec![0xcc0e_4002, 0x1c16_0903],
            HALF_FUSED | HALF_FUSED << 16,
        ),
        // v_fma_mixlo_f16 v2, v3, v4, v5 op_sel_hi:[1,1,1]: every source a half.
        (
            "v_fma_mixlo_f16",
            half,
            [&[0xcc21_4002, 0x1c16_0903][..], &LOW_HALF].concat(),
            HALF_FUSED,
        ),
    ]
}

/// `program` translated for one wave of [`THREADS`] threads.
fn translated(program: &[u32]) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave32,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            count: USER_WORDS as u32,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [THREADS, 1, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
    .expect("the program translates")
}

/// The user-data block: words 8-11 a raw descriptor over the window's start.
fn user_data() -> Vec<u32> {
    let mut block = vec![0_u32; 32];
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    {
        block[8] = WINDOW as u32;
        block[9] = (WINDOW >> 32) as u32 & 0xffff;
    }
    block[10] = THREADS * 4;
    block[11] = WORD3;
    block
}

/// What the host's concrete run of `program` stores at thread 0's word.
fn host(program: &[u32]) -> Option<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let waves = [WaveEntry {
        group_ids: vec![0],
        threads: (0..THREADS).map(|t| Some([t, 0, 0])).collect(),
        components: 1,
    }];
    let block = user_data();
    let mut read = |address: u64| {
        let word = address.checked_sub(WINDOW)? / 4;
        (word < u64::from(WINDOW_WORDS)).then_some(BEFORE)
    };
    let run = wave_trace(
        &LaneProgram::new(decoded, &encodings, &[]),
        &encodings,
        &block[..USER_WORDS],
        (THREADS, &waves),
        (0, 0, 0),
        &mut read,
    );
    assert!(matches!(run.stop, TraceStop::Ended), "{:?}", run.stop);
    run.written.get(&WINDOW).and_then(|(value, _)| *value)
}

/// The host's concrete run computes each form as the instruction set defines it.
#[test]
fn the_host_rounds_a_fused_multiply_add_once() {
    let wrong: Vec<String> = forms()
        .into_iter()
        .filter_map(|(name, operands, body, expected)| {
            let stored = host(&around(operands, &body));
            (stored != Some(expected))
                .then(|| format!("{name}: {stored:x?}, expected {expected:#x}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The device computes each form as the instruction set defines it: fused forms rounded once,
/// unfused ones with the product rounded first.
#[test]
fn the_device_rounds_a_fused_multiply_add_once() {
    match probe() {
        Availability::Available { properties } => println!("device: {}", properties.device),
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            return;
        }
    }
    let mut wrong = Vec::new();
    for (name, operands, body, expected) in forms() {
        let module = translated(&around(operands, &body));
        let done = dispatch_guest(
            &module,
            &vec![BEFORE; WINDOW_WORDS as usize],
            (&user_data(), [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        let stored = &done.memory[..THREADS as usize];
        if stored.iter().any(|word| *word != expected) {
            wrong.push(format!("{name}: {:#x}, expected {expected:#x}", stored[0]));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
