//! A guest compute dispatch, run on the device over guest memory.
//!
//! The shader is radeonsi's compute buffer clear as the GL port on the open toolchain compiles it
//! (Mesa's own compiler output, read out of the port's first command buffer): each thread stores
//! the sixteen-byte clear value from user data words 4-7 through the buffer descriptor in words
//! 8-11, at `(workgroup_id.x * 64 + thread_id.x) * 16`.
//!
//! ```text
//! v_lshl_add_u32 v0, s12, 6, v0
//! v_lshrrev_b64 v[4:5], 0, s[4:5]
//! v_lshrrev_b64 v[6:7], 0, s[6:7]
//! v_lshlrev_b32_e32 v0, 4, v0
//! buffer_store_dwordx4 v[4:7], v0, s[8:11], 0 offen
//! s_endpgm
//! ```
//!
//! The clear never meets its wave, so it runs both ways: the whole wave in one invocation, and one
//! invocation per lane (D880). Every test that runs it runs both and holds each to the same words.
//! So does the clear with a branch on its execution mask, which runs per lane with every lane's
//! bits made by ballot or through workgroup memory (D884).

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::dispatch_guest;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, PartialGroups, Stage, UserData, Window, translate_with_user_data,
};
use orbistoun_translate::{Fidelity, Width};

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

const CLEAR: [u32; 11] = [
    0xd746_0000,
    0x0401_0c0c,
    0xd700_0004,
    0x0000_0880,
    0xd700_0006,
    0x0000_0c80,
    0x3400_0084,
    0xe078_1000,
    0x8002_0400,
    0xbf81_0000,
    0xbf9f_0000,
];

/// Where the window sits, and how many words it spans.
const WINDOW: u64 = 0x4_0187_0000;
const WINDOW_WORDS: u32 = 4096;
/// What memory held before, so an untouched word is told from a cleared one.
const BEFORE: u32 = 0xdead_beef;
/// The clear value, one word per component.
const VALUE: [u32; 4] = [0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444];
/// `SQ_BUF_RSRC_WORD3` as the port's descriptor carries it: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;

/// The two ways a dispatch runs: the whole wave in one invocation, and one invocation per lane
/// (D880), by whether the module is asked to run per lane.
const MODELS: [(&str, bool); 2] = [("whole wave", false), ("per lane", true)];

fn module(per_lane: bool) -> Vec<u32> {
    translated(0, per_lane).expect("the clear translates")
}

/// The clear, translated with the user-data words in `unwritten` marked as never written.
fn translated(
    unwritten: u32,
    per_lane: bool,
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    translated_with(unwritten, None, [64, 1, 1], per_lane)
}

/// The clear, translated for a dispatch of groups of `threads` whose last group may be a partial
/// one.
fn translated_with(
    unwritten: u32,
    partial: Option<PartialGroups>,
    threads: [u32; 3],
    per_lane: bool,
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    translated_program(&CLEAR, (unwritten, partial, threads), per_lane)
}

/// The device's subgroup width, which a module run per lane makes its whole masks by where it holds
/// the wave (D884); zero where there is no device.
fn host_subgroup() -> u32 {
    match probe() {
        Availability::Available { properties } => properties.subgroup_size,
        Availability::Unavailable { .. } => 0,
    }
}

/// `program`, translated for groups of `threads`, the last perhaps partial, with the user-data words
/// in `unwritten` marked as never written; one invocation per lane where `per_lane` (D880), on the
/// device's subgroup.
fn translated_program(
    program: &[u32],
    shape: (u32, Option<PartialGroups>, [u32; 3]),
    per_lane: bool,
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    translated_for(
        program,
        shape,
        (per_lane.then(host_subgroup), Width::Wave64),
    )
}

/// [`translated_program`] in waves of `width`, one invocation per lane where `subgroup` is the host's
/// subgroup width to run on (zero where it is not known).
fn translated_for(
    program: &[u32],
    (unwritten, partial, threads): (u32, Option<PartialGroups>, [u32; 3]),
    (subgroup, width): (Option<u32>, Width),
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        width,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            first_register: 0,
            count: 12,
            block_offset: 0,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 1,
                threads,
                unwritten_user_data: unwritten,
                partial,
                integer_images: [false; 2],
                integer_fetched: 0,
                first_thread: 0,
                sampled_buffers: 0,
                scratch_wave_bytes: 0,
                device_addressed: false,
                float_mode: None,
            }),
            per_invocation: subgroup,
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
}

/// The two models a program [`translated_as_dispatched`] runs under: left to choose, where every
/// program here runs one invocation per lane on the host's subgroup (D880, D884, D973); and the
/// wavefront model asked for by name, which keeps the whole wave in one invocation, so each test
/// holds both to its expectations.
const AS_DISPATCHED: [(&str, Fidelity); 2] = [
    ("per lane", Fidelity::Auto),
    ("whole wave", Fidelity::Wavefront),
];

/// `program` translated as the pipeline translates a dispatch: groups of `threads` in waves of
/// `width`, the host's subgroup offered and the model `asked_for` - [`Fidelity::Auto`], left to
/// decide, which must then run it one invocation per lane, or [`Fidelity::Wavefront`], the whole
/// wave - and a local data share of `local_words`.
fn translated_as_dispatched(
    program: &[u32],
    threads: [u32; 3],
    (width, asked_for): (Width, Fidelity),
    local_words: u32,
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let translated = orbistoun_translate::translate_with_user_data(
        &decoded,
        &encodings,
        orbistoun_translate::Strategy::Predicated {
            fidelity: asked_for,
            width,
        },
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            count: 12,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 1,
                threads,
                unwritten_user_data: 0,
                partial: None,
                integer_images: [false; 2],
                integer_fetched: 0,
                first_thread: 0,
                sampled_buffers: 0,
                scratch_wave_bytes: 0,
                device_addressed: false,
                float_mode: None,
            }),
            per_invocation: Some(host_subgroup()),
            local_words: Some(local_words),
            ..UserData::default()
        },
    )?;
    let expected = match asked_for {
        Fidelity::Auto => Fidelity::Lane,
        other => other,
    };
    assert_eq!(
        translated.fidelity, expected,
        "the model asked for: {:?}",
        translated.warnings
    );
    Ok(translated.module)
}

/// A user-data word the stream never wrote is refused only when the program reads it: the clear
/// reads words 4 to 12 and never word one.
#[test]
fn an_unwritten_user_data_word_is_refused_only_when_read() {
    for (model, per_lane) in MODELS {
        assert!(
            translated(1 << 1, per_lane).is_ok(),
            "{model}: word one is never read"
        );
        assert!(
            translated(1 << 5, per_lane).is_err(),
            "{model}: word five is the clear value"
        );
        assert!(
            translated(1 << 10, per_lane).is_err(),
            "{model}: word ten is the descriptor's record count"
        );
    }
}

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

/// The clear, with each thread past the thirty-second storing its value's second word as its first:
/// `v_cmp_lt_u32 vcc, 31, v0` after the thread id is formed, and `v_cndmask_b32 v4, v4, v5` after
/// the value is. The comparison writes a per-lane bit and the selection reads it, and nothing reads
/// the mask whole, so one invocation per lane holds its own bit alone (D880).
fn selecting_clear() -> Vec<u32> {
    // VOPC: vsrc1 at bit 9, src0 at 0; inline 31 is code 159. VOP2: vdst at 17 as well.
    let compare = encoded("v_cmp_lt_u32_e32", 159);
    let select = encoded("v_cndmask_b32_e32", (4 << 17) | (5 << 9) | (256 + 4));
    let mut program = CLEAR[..2].to_vec();
    program.push(compare);
    program.extend_from_slice(&CLEAR[2..6]);
    program.push(select);
    program.extend_from_slice(&CLEAR[6..]);
    program
}

/// A program whose lanes each select by their own bit of a comparison stores what the whole wave
/// stores, run per lane: two groups, the first word of every thread's slot the value's first word
/// below thread 32 and its second from there.
#[test]
fn a_per_lane_bit_selects_as_the_whole_wave_does() {
    if !device_or_skip("guest clear, selecting by a lane bit") {
        return;
    }
    for (model, per_lane) in MODELS {
        let module = translated_program(&selecting_clear(), (0, None, [64, 1, 1]), per_lane)
            .expect("the selecting clear translates");
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        let done = dispatch_guest(
            &module,
            &before,
            (&user_data(WINDOW, 2 * 64 * 16), [2, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert!(!done.escaped, "{model}");
        for (index, &word) in done.memory.iter().enumerate() {
            let thread = index / 4;
            let expected = match (thread, index % 4) {
                (128.., _) => BEFORE,
                (32.., 0) => VALUE[1],
                (_, component) => VALUE[component],
            };
            assert_eq!(word, expected, "{model}: word {index}");
        }
    }
}

/// A program that reads a mask whole runs one invocation per lane where its lanes can exchange the
/// mask - a subgroup that holds the wave or half of it (D973), or a group of one wave - and is
/// refused, naming why, in a group of several waves on a subgroup not known (D884):
/// `s_cbranch_vccz` branches on every lane's bit.
#[test]
fn a_program_reading_a_mask_whole_is_refused_only_across_waves() {
    // s_cbranch_vccz +0; s_endpgm.
    let program = [0xbf86_0000, 0xbf81_0000];
    let refused = translated_for(&program, (0, None, [128, 1, 1]), (Some(0), Width::Wave64))
        .expect_err("refused")
        .to_string();
    assert!(refused.contains("lane mask whole"), "{refused}");
    for (threads, subgroup, width) in [
        (64, Some(32), Width::Wave64),
        (128, Some(32), Width::Wave64),
        (64, Some(0), Width::Wave64),
        (128, Some(64), Width::Wave64),
        (128, Some(32), Width::Wave32),
    ] {
        assert!(
            translated_for(&program, (0, None, [threads, 1, 1]), (subgroup, width)).is_ok(),
            "{threads} threads of {width:?} on a subgroup of {subgroup:?}"
        );
    }
    assert!(
        translated_for(&program, (0, None, [128, 1, 1]), (None, Width::Wave64)).is_ok(),
        "the whole wave runs it"
    );
}

/// The clear, with each thread past `threshold` storing as its first word how many of its wave's
/// threads are, by a branch on the execution mask: `v_cmp_lt_u32 vcc, threshold, v0` after the
/// thread id is formed, `s_and_saveexec s20, vcc`, `s_cbranch_execz` over `s_bcnt1_i32_b64 s22`
/// of the mask and `v_mov_b32 v4, s22`, and the mask restored by `s_or exec, exec, s20`. A wave
/// none of whose threads passes the threshold takes the branch. The saved mask, the branch and the
/// count read the mask whole (D884): a lane holding only its own bit would count one. A wave of 32
/// counts its mask copied to `s24` beside a zero `s25`, there being no 32-bit count.
fn branching_clear(width: Width, threshold: u32) -> Vec<u32> {
    // Scalar operand codes: vcc_lo, exec_lo, and the inline zero; an inline integer n is 128 + n.
    const VCC_LO: u32 = 106;
    const EXEC_LO: u32 = 126;
    const ZERO: u32 = 128;
    // Where the count lands.
    const COUNT: u32 = 22;
    // SOP1 and SOP2: sdst at bit 16, ssrc1 at 8, ssrc0 at 0.
    let scalar = |name: &str, (sdst, ssrc1, ssrc0): (u32, u32, u32)| {
        encoded(name, (sdst << 16) | (ssrc1 << 8) | ssrc0)
    };
    // VOPC: vsrc1 at bit 9, src0 at 0.
    let compare = encoded("v_cmp_lt_u32_e32", ZERO + threshold);
    let (save, block, restore) = match width {
        Width::Wave64 => (
            "s_and_saveexec_b64",
            vec![scalar("s_bcnt1_i32_b64", (22, 0, EXEC_LO))],
            "s_or_b64",
        ),
        Width::Wave32 => (
            "s_and_saveexec_b32",
            vec![
                scalar("s_mov_b32", (24, 0, EXEC_LO)),
                scalar("s_mov_b32", (25, 0, ZERO)),
                scalar("s_bcnt1_i32_b64", (22, 0, 24)),
            ],
            "s_or_b32",
        ),
    };
    let mut program = CLEAR[..2].to_vec();
    program.push(compare);
    program.extend_from_slice(&CLEAR[2..6]);
    program.push(scalar(save, (20, 0, VCC_LO)));
    program.push(encoded("s_cbranch_execz", block.len() as u32 + 1));
    program.extend(block);
    // VOP1: vdst at bit 17, src0 at 0.
    program.extend([
        encoded("v_mov_b32_e32", (4 << 17) | COUNT),
        scalar(restore, (EXEC_LO, 20, EXEC_LO)),
    ]);
    program.extend_from_slice(&CLEAR[6..]);
    program
}

/// A program that branches on its execution mask stores, run per lane, what the whole wave stores
/// (D884): in waves of 64, a group of one wave whose mask is made through workgroup memory where the
/// subgroup is narrower; in waves of 32, two waves to a group of 64, each wave a subgroup whose
/// ballot is its mask. A threshold of 63 leaves the first group's waves no thread past it, so they
/// take the branch; a partial last group of 37 threads seeds its mask from its lanes too.
#[test]
fn a_branch_on_the_execution_mask_runs_per_lane_as_the_whole_wave_does() {
    if !device_or_skip("guest clear, branching on the execution mask") {
        return;
    }
    let subgroup = host_subgroup();
    for width in [Width::Wave64, Width::Wave32] {
        for threshold in [31, 63] {
            for partial_threads in [None, Some(37)] {
                let partial = partial_threads.map(|threads| PartialGroups {
                    last: [1, 0, 0],
                    threads: [threads, 1, 1],
                });
                let mut push = user_data(WINDOW, 2 * 64 * 16);
                if let Some(partial) = partial {
                    push.resize(
                        orbistoun_translate::wavefront::PARTIAL_PUSH_WORD as usize,
                        0,
                    );
                    push.extend(partial.push_words());
                }
                let mut results = Vec::new();
                for (model, per_lane) in MODELS {
                    let case = format!("{model}, {width:?}, past {threshold}, {partial_threads:?}");
                    let module = translated_for(
                        &branching_clear(width, threshold),
                        (0, partial, [64, 1, 1]),
                        (per_lane.then_some(subgroup), width),
                    )
                    .unwrap_or_else(|e| panic!("{case}: {e}"));
                    let before = vec![BEFORE; WINDOW_WORDS as usize];
                    let done = dispatch_guest(
                        &module,
                        &before,
                        (&push, [2, 1, 1]),
                        &DispatchImages::default(),
                    )
                    .expect("dispatched");
                    assert!(!done.escaped, "{case}");
                    let threads = 64 + partial_threads.unwrap_or(64) as usize;
                    let lanes = if width == Width::Wave64 { 64 } else { 32 };
                    // How many of a wave's running threads are past the threshold.
                    let past = |thread: usize| {
                        let first = thread / lanes * lanes;
                        (first..first + lanes)
                            .filter(|&t| t < threads && t > threshold as usize)
                            .count() as u32
                    };
                    for (index, &word) in done.memory.iter().enumerate() {
                        let thread = index / 4;
                        let expected = match (thread, index % 4) {
                            (t, _) if t >= threads => BEFORE,
                            (t, 0) if t > threshold as usize => past(t),
                            (_, component) => VALUE[component],
                        };
                        assert_eq!(word, expected, "{case}: word {index}");
                    }
                    results.push(done.memory);
                }
                assert_eq!(results[0], results[1], "both ways leave the same memory");
            }
        }
    }
}

/// The user-data block: words 4-7 the value, 8-11 a raw descriptor at `base` of `records` bytes.
fn user_data(base: u64, records: u32) -> Vec<u32> {
    let mut block = vec![0_u32; 32];
    block[4..8].copy_from_slice(&VALUE);
    block[8] = base as u32;
    block[9] = (base >> 32) as u32 & 0xffff;
    block[10] = records;
    block[11] = WORD3;
    block
}

/// The depth walk of Grand Theft Auto V's tree-rotation program at `0x3dc8900` in its image, run as
/// its dispatch runs it: a group of eight threads in a wave of 32. The mask is saved in `vcc_lo`,
/// each lane leaves the loop as its chain ends (`v_cmpx_ne_u32_e32`), `vcc_hi` counts the wave's
/// trips, and `v2` keeps the count each lane last saw - its depth. Lane `n` halves `n` to zero, so
/// lanes 0 to 7 have depths 0, 1, 2, 2, 3, 3, 3, 3. Each stores its depth, and the mask restored
/// after the loop, which is the group's eight lanes.
#[test]
fn a_group_of_eight_counts_each_lane_s_trips_in_a_wave_scalar() {
    if !device_or_skip("a group of eight counts its trips") {
        return;
    }
    let program = [
        0xbeeb_0380, // s_mov_b32 vcc_hi, 0
        0xbeea_037e, // s_mov_b32 vcc_lo, exec_lo
        0xd765_0001,
        0x0001_00c1, // v_mbcnt_lo_u32_b32 v1, -1, 0
        0x7e04_026b, // v_mov_b32 v2, vcc_hi
        0x7daa_0280, // v_cmpx_ne_u32 0, v1
        0xbf88_0003, // s_cbranch_execz 3
        0x2c02_0281, // v_lshrrev_b32 v1, 1, v1
        0x816b_816b, // s_add_i32 vcc_hi, vcc_hi, 1
        0xbf82_fffa, // s_branch -6
        0xbefe_036a, // s_mov_b32 exec_lo, vcc_lo
        0xd765_0004,
        0x0001_00c1, // v_mbcnt_lo_u32_b32 v4, -1, 0
        0x3408_0882, // v_lshlrev_b32 v4, 2, v4
        0x7e06_027e, // v_mov_b32 v3, exec_lo
        0xe070_1000,
        0x8002_0204, // buffer_store_dword v2, v4, s[8:11], 0 offen
        0xe070_1040,
        0x8002_0304, // buffer_store_dword v3, v4, s[8:11], 0 offen offset:64
        0xbf81_0000, // s_endpgm
    ];
    for (model, per_lane) in MODELS {
        let module = translated_for(
            &program,
            (0, None, [8, 1, 1]),
            (per_lane.then(host_subgroup), Width::Wave32),
        )
        .expect("the walk translates");
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        let done = dispatch_guest(
            &module,
            &before,
            (&user_data(WINDOW + 0x100, 0x80), [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        let depths = &done.memory[0x40..0x48];
        assert_eq!(
            depths,
            [0, 1, 2, 2, 3, 3, 3, 3],
            "{model}: each lane's depth"
        );
        let masks = &done.memory[0x50..0x58];
        assert_eq!(
            masks, [0xff; 8],
            "{model}: the mask restored is the group's"
        );
        assert!(
            done.memory[0x48..0x50].iter().all(|&word| word == BEFORE),
            "{model}: no lane past the group stores"
        );
    }
}

/// The depth walk in the group the tree-rotation program has: 256 threads, eight waves of 32. Each
/// thread halves its id to zero, so its depth is the id's bit length - 0 for thread 0, which never
/// enters, 8 for threads 128 to 255 - and each wave's restored mask is all 32 of its lanes. A lane
/// that has left keeps the count it saw then, however many trips its wave makes after.
#[test]
fn a_group_of_eight_waves_counts_each_lane_s_trips_in_a_wave_scalar() {
    if !device_or_skip("a group of eight waves counts its trips") {
        return;
    }
    let program = [
        0xbeeb_0380, // s_mov_b32 vcc_hi, 0
        0xbeea_037e, // s_mov_b32 vcc_lo, exec_lo
        0x348c_0082, // v_lshlrev_b32 v70, 2, v0
        0x7e02_0300, // v_mov_b32 v1, v0
        0x7e04_026b, // v_mov_b32 v2, vcc_hi
        0x7daa_0280, // v_cmpx_ne_u32 0, v1
        0xbf88_0003, // s_cbranch_execz 3
        0x2c02_0281, // v_lshrrev_b32 v1, 1, v1
        0x816b_816b, // s_add_i32 vcc_hi, vcc_hi, 1
        0xbf82_fffa, // s_branch -6
        0xbefe_036a, // s_mov_b32 exec_lo, vcc_lo
        0x7e06_027e, // v_mov_b32 v3, exec_lo
        0xe070_1000,
        0x8002_0246, // buffer_store_dword v2, v70, s[8:11], 0 offen
        0xe070_1400,
        0x8002_0346, // buffer_store_dword v3, v70, s[8:11], 0 offen offset:1024
        0xbf81_0000, // s_endpgm
    ];
    for (model, asked_for) in AS_DISPATCHED {
        let module =
            translated_as_dispatched(&program, [256, 1, 1], (Width::Wave32, asked_for), 1024)
                .expect("the walk translates");
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        let done = dispatch_guest(
            &module,
            &before,
            (&user_data(WINDOW + 0x100, 0x800), [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        let depths = &done.memory[0x40..0x140];
        let expected: Vec<u32> = (0..256_u32)
            .map(|thread| 32 - thread.leading_zeros())
            .collect();
        assert_eq!(depths, expected, "{model}: each thread's depth");
        assert!(
            done.memory[0x140..0x240]
                .iter()
                .all(|&mask| mask == u32::MAX),
            "{model}: each wave's mask restored"
        );
    }
}

/// A lane that does not run writes nothing to the local data share, though another wave of its group
/// writes the word at the same time (D892). In a group of eight waves of 32, lane `w` of wave `w`
/// writes word `w`, and the other 31 lanes of wave `w`, masked off, address word `w + 1` (mod 8),
/// which the next wave's lane is writing. After the barrier every thread reads word `t mod 8`, which
/// holds `100 + 33 * (t mod 8)`. A masked-off lane that wrote back the word's old value, as one
/// invocation simulating a wave did, raced the next wave's write and could erase it: Grand Theft
/// Auto V's tree-rotation program at `0x3dc8900` in its image lost a worklist entry that way, and its
/// puller rotated the root from the unwritten slot instead.
#[test]
fn a_masked_lane_leaves_another_wave_s_share_word_alone() {
    if !device_or_skip("a masked lane leaves another wave's share word alone") {
        return;
    }
    let program = [
        0x2c02_0085, // v_lshrrev_b32 v1, 5, v0: the wave
        0x3604_009f, // v_and_b32 v2, 31, v0: the lane
        0x4a06_0281, // v_add_nc_u32 v3, 1, v1
        0x3606_0687, // v_and_b32 v3, 7, v3
        0x3406_0682, // v_lshlrev_b32 v3, 2, v3: the next wave's word
        0x3408_0282, // v_lshlrev_b32 v4, 2, v1: the wave's own word
        0x7d84_0501, // v_cmp_eq_u32 vcc_lo, v1, v2
        0x020a_0903, // v_cndmask_b32 v5, v3, v4, vcc_lo
        0x4a0c_00ff,
        0x0000_0064, // v_add_nc_u32 v6, 100, v0
        0xbe94_037e, // s_mov_b32 s20, exec_lo
        0x877e_6a7e, // s_and_b32 exec_lo, exec_lo, vcc_lo
        0xd834_0000,
        0x0000_0605, // ds_write_b32 v5, v6
        0xbefe_0314, // s_mov_b32 exec_lo, s20
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xbf8a_0000, // s_barrier
        0x3610_0087, // v_and_b32 v8, 7, v0
        0x3410_1082, // v_lshlrev_b32 v8, 2, v8
        0xd8d8_0000,
        0x0900_0008, // ds_read_b32 v9, v8
        0x3414_0082, // v_lshlrev_b32 v10, 2, v0
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xe070_1000,
        0x8002_090a, // buffer_store_dword v9, v10, s[8:11], 0 offen
        0xbf81_0000, // s_endpgm
    ];
    let expected: Vec<u32> = (0..256_u32).map(|thread| 100 + 33 * (thread % 8)).collect();
    for (model, asked_for) in AS_DISPATCHED {
        let module =
            translated_as_dispatched(&program, [256, 1, 1], (Width::Wave32, asked_for), 1024)
                .expect("the program translates");
        // A race shows only sometimes, so the dispatch runs many times.
        for run in 0..64 {
            let before = vec![BEFORE; WINDOW_WORDS as usize];
            let done = dispatch_guest(
                &module,
                &before,
                (&user_data(WINDOW + 0x100, 0x400), [1, 1, 1]),
                &DispatchImages::default(),
            )
            .expect("dispatched");
            assert_eq!(&done.memory[0x40..0x140], expected, "{model}: run {run}");
        }
    }
}

/// The depth walk as the tree-rotation program has it, each trip loading the next link of the chain
/// from memory, in its group of eight waves of 32: word `t` holds `t / 2`, so each thread's depth is
/// still its id's bit length, and each wave's restored mask all 32 of its lanes.
#[test]
fn a_group_of_eight_waves_counts_trips_of_a_loaded_chain() {
    if !device_or_skip("a group of eight waves walks a loaded chain") {
        return;
    }
    let program = [
        0xbeeb_0380, // s_mov_b32 vcc_hi, 0
        0xbeea_037e, // s_mov_b32 vcc_lo, exec_lo
        0x348c_0082, // v_lshlrev_b32 v70, 2, v0
        0x7e02_0300, // v_mov_b32 v1, v0
        0x7e04_026b, // v_mov_b32 v2, vcc_hi
        0x7daa_0280, // v_cmpx_ne_u32 0, v1
        0xbf88_0006, // s_cbranch_execz 6
        0x340a_0282, // v_lshlrev_b32 v5, 2, v1
        0xe030_1000,
        0x8002_0105, // buffer_load_dword v1, v5, s[8:11], 0 offen
        0x816b_816b, // s_add_i32 vcc_hi, vcc_hi, 1
        0xbf8c_3f70, // s_waitcnt vmcnt(0)
        0xbf82_fff7, // s_branch -9
        0xbefe_036a, // s_mov_b32 exec_lo, vcc_lo
        0x7e06_027e, // v_mov_b32 v3, exec_lo
        0xe070_1400,
        0x8002_0246, // buffer_store_dword v2, v70, s[8:11], 0 offen offset:1024
        0xe070_1800,
        0x8002_0346, // buffer_store_dword v3, v70, s[8:11], 0 offen offset:2048
        0xbf81_0000, // s_endpgm
    ];
    for (model, asked_for) in AS_DISPATCHED {
        let module =
            translated_as_dispatched(&program, [256, 1, 1], (Width::Wave32, asked_for), 1024)
                .expect("the walk translates");
        let mut before = vec![BEFORE; WINDOW_WORDS as usize];
        for thread in 0..256_u32 {
            before[0x40 + thread as usize] = thread / 2;
        }
        let done = dispatch_guest(
            &module,
            &before,
            (&user_data(WINDOW + 0x100, 0xc00), [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        let depths = &done.memory[0x140..0x240];
        let expected: Vec<u32> = (0..256_u32)
            .map(|thread| 32 - thread.leading_zeros())
            .collect();
        assert_eq!(depths, expected, "{model}: each thread's depth");
        assert!(
            done.memory[0x240..0x340]
                .iter()
                .all(|&mask| mask == u32::MAX),
            "{model}: each wave's mask restored"
        );
    }
}

/// The worklist of Grand Theft Auto V's tree-rotation program at `0x3dc8900` in its image, its
/// instructions as they are there: thread 0 clears a counter in the local data share; after a
/// barrier each selected thread - the odd ones - takes a slot with `ds_add_rtn_u32` and writes its
/// node (`100 + thread`) there; after a second barrier every thread reads the count, and each below
/// it pulls the entry at its own index. Each thread stores the count it read at its own word, and
/// the entry it pulled (or -1) 1024 bytes on.
fn worklist_program() -> [u32; 50] {
    [
        0x348c_0082, // v_lshlrev_b32 v70, 2, v0
        0xbf80_0000, // s_nop
        0x4a88_00ff,
        0x0000_0064, // v_add_nc_u32 v68, 100, v0
        0x3602_0081, // v_and_b32 v1, 1, v0
        0xbe97_037e, // s_mov_b32 s23, exec_lo
        0xbeea_0317, // s_mov_b32 vcc_lo, s23
        0x7da4_0080, // v_cmpx_eq_u32 0, v0
        0xbf88_0003, // s_cbranch_execz 3
        0x7e04_0280, // v_mov_b32 v2, 0
        0xd834_0000,
        0x0000_0202, // ds_write_b32 v2, v2
        0xbefe_036a, // s_mov_b32 exec_lo, vcc_lo
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xbf8a_0000, // s_barrier
        0x7daa_0280, // v_cmpx_ne_u32 0, v1
        0xbf88_0008, // s_cbranch_execz 8
        0x7e02_0280, // v_mov_b32 v1, 0
        0x7e04_0281, // v_mov_b32 v2, 1
        0xd880_0000,
        0x0100_0201, // ds_add_rtn_u32 v1, v1, v2
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0x3402_0282, // v_lshlrev_b32 v1, 2, v1
        0xd834_0004,
        0x0000_4401, // ds_write_b32 v1, v68 offset:4
        0xbefe_036a, // s_mov_b32 exec_lo, vcc_lo
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xbf8a_0000, // s_barrier
        0x7e04_0280, // v_mov_b32 v2, 0
        0x7e02_0280, // v_mov_b32 v1, 0
        0xd8d8_0000,
        0x0200_0002, // ds_read_b32 v2, v2
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0x7e8a_02c1, // v_mov_b32 v69, -1
        0x7d22_0500, // v_cmpx_lt_i32 v0, v2
        0xbf88_0004, // s_cbranch_execz 4
        0x3400_0082, // v_lshlrev_b32 v0, 2, v0
        0x7e02_0281, // v_mov_b32 v1, 1
        0xd8d8_0004,
        0x4500_0000, // ds_read_b32 v69, v0 offset:4
        0xbefe_036a, // s_mov_b32 exec_lo, vcc_lo
        0x7e08_0346, // v_mov_b32 v4, v70
        0xbf80_0000, // s_nop
        0xbf80_0000, // s_nop
        0xe070_1000,
        0x8002_0204, // buffer_store_dword v2, v4, s[8:11], 0 offen
        0xe070_1400,
        0x8002_4504, // buffer_store_dword v69, v4, s[8:11], 0 offen offset:1024
        0xbf8c_3f70, // s_waitcnt vmcnt(0)
        0xbf81_0000, // s_endpgm
    ]
}

/// [`worklist_program`] run as the dispatch runs a group of eight threads in a wave of 32: the
/// count is four, threads 0 to 3 pull the four odd threads' nodes in some order, and threads 4 to 7
/// pull nothing.
#[test]
fn a_group_of_eight_enqueues_through_the_share_and_pulls_by_lane() {
    if !device_or_skip("a group of eight's worklist") {
        return;
    }
    let program = worklist_program();
    for (model, asked_for) in AS_DISPATCHED {
        let module =
            translated_as_dispatched(&program, [8, 1, 1], (Width::Wave32, asked_for), 1024)
                .expect("the worklist translates");
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        let done = dispatch_guest(
            &module,
            &before,
            (&user_data(WINDOW + 0x100, 0x800), [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert_eq!(&done.memory[0x40..0x48], [4; 8], "{model}: the count");
        let mut pulled = done.memory[0x140..0x144].to_vec();
        pulled.sort_unstable();
        assert_eq!(pulled, [101, 103, 105, 107], "{model}: the entries");
        assert_eq!(
            &done.memory[0x144..0x148],
            [u32::MAX; 4],
            "{model}: no entry past the count"
        );
    }
}

/// The same worklist in the group the tree-rotation program has: 256 threads, eight waves of 32,
/// which meet at its barriers (D892). The even threads enqueue - lane 0 of every wave among them,
/// the lane whose entry Grand Theft Auto V's dispatch lost - so the count is 128 and threads 0 to
/// 127 pull every even thread's node once.
#[test]
fn a_group_of_eight_waves_enqueues_through_the_share_and_pulls_by_lane() {
    if !device_or_skip("a group of eight waves' worklist") {
        return;
    }
    let mut program = worklist_program();
    // v_cmpx_eq_u32 0, v1: the even threads.
    program[15] = 0x7da4_0280;
    for (model, asked_for) in AS_DISPATCHED {
        let module =
            translated_as_dispatched(&program, [256, 1, 1], (Width::Wave32, asked_for), 1024)
                .expect("the worklist translates");
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        let done = dispatch_guest(
            &module,
            &before,
            (&user_data(WINDOW + 0x100, 0x800), [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert!(!done.escaped, "{model}");
        let counts = &done.memory[0x40..0x140];
        assert!(
            counts.iter().all(|&count| count == 128),
            "{model}: the count: {counts:?}"
        );
        let mut pulled = done.memory[0x140..0x1c0].to_vec();
        pulled.sort_unstable();
        let even: Vec<u32> = (0..256).step_by(2).map(|thread| 100 + thread).collect();
        assert_eq!(pulled, even, "{model}: each even thread's node once");
        assert!(
            done.memory[0x1c0..0x240]
                .iter()
                .all(|&word| word == u32::MAX),
            "{model}: no entry past the count"
        );
    }
}

/// `v_writelane_b32` writes its lane whether that lane runs or not (D978), as Grand Theft Auto V's
/// ray traversal at `0x74005b7e8600` pushes its node stack: lane `s86` of `v29`-`v31`, `s86` the
/// stack's depth, under an execution mask of the lanes the node's box test sent on. A wave of 64
/// runs only lane 0 while lanes 40 (the second half's) and 7 are written, then runs whole again:
/// every lane stores its `v5`, and the scalar read back from lane 40.
#[test]
fn a_lane_write_reaches_a_lane_that_does_not_run() {
    if !device_or_skip("a lane write to a lane that does not run") {
        return;
    }
    let program = [
        0xb005_1234, // s_movk_i32 s5, 0x1234
        0x7e0a_0280, // v_mov_b32 v5, 0
        0xbe94_047e, // s_mov_b64 s[20:21], exec
        0xbefe_0481, // s_mov_b64 exec, 1
        0xd761_0005,
        0x0001_5005, // v_writelane_b32 v5, s5, 40
        0xd761_0005,
        0x0001_0e05, // v_writelane_b32 v5, s5, 7
        0xbefe_0414, // s_mov_b64 exec, s[20:21]
        0xd760_0006,
        0x0001_5105, // v_readlane_b32 s6, v5, 40
        0x7e0c_0206, // v_mov_b32 v6, s6
        0xd765_0004,
        0x0001_00c1, // v_mbcnt_lo_u32_b32 v4, -1, 0
        0xd766_0004,
        0x0002_08c1, // v_mbcnt_hi_u32_b32 v4, -1, v4
        0x3408_0882, // v_lshlrev_b32 v4, 2, v4
        0xe070_1000,
        0x8002_0504, // buffer_store_dword v5, v4, s[8:11], 0 offen
        0xe070_1100,
        0x8002_0604, // buffer_store_dword v6, v4, s[8:11], 0 offen offset:256
        0xbf81_0000, // s_endpgm
    ];
    let written = 0x1234;
    let expected: Vec<u32> = (0..64)
        .map(|lane| if lane == 7 || lane == 40 { written } else { 0 })
        .collect();
    for (model, asked_for) in AS_DISPATCHED {
        let module = translated_as_dispatched(&program, [64, 1, 1], (Width::Wave64, asked_for), 0)
            .expect("the lane writes translate");
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        let done = dispatch_guest(
            &module,
            &before,
            (&user_data(WINDOW + 0x100, 0x200), [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert_eq!(
            &done.memory[0x40..0x80],
            expected.as_slice(),
            "{model}: each lane's v5"
        );
        assert_eq!(
            &done.memory[0x80..0xc0],
            [written; 64],
            "{model}: lane 40 read back in every lane"
        );
    }
}

/// Three groups clear three kilobytes from the descriptor's base, and nothing else changes.
#[test]
fn a_dispatch_clears_the_buffer_its_descriptor_names() {
    if !device_or_skip("guest clear") {
        return;
    }
    for (model, per_lane) in MODELS {
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        let done = dispatch_guest(
            &module(per_lane),
            &before,
            (&user_data(WINDOW + 0x100, 3 * 64 * 16), [3, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert!(!done.escaped, "{model}: every store landed in the window");
        for (index, &word) in done.memory.iter().enumerate() {
            let expected = if (0x40..0x40 + 3 * 64 * 4).contains(&index) {
                VALUE[index % 4]
            } else {
                BEFORE
            };
            assert_eq!(word, expected, "{model}: word {index}");
        }
    }
}

/// A partial last group runs only its partial count of threads (`PARTIAL_TG_EN`): of two groups
/// of 64, the second runs ten, so the clear reaches 64 + 10 slots and the rest keep what they held.
/// The count is the dispatch's, read where the module finds it (D855): one module, translated once,
/// runs a last group of ten and one of thirty-seven, each as its dispatch says.
#[test]
fn a_partial_last_group_runs_only_its_partial_threads() {
    if !device_or_skip("guest clear, partial last group") {
        return;
    }
    for (model, per_lane) in MODELS {
        let module = translated_with(
            0,
            Some(PartialGroups {
                last: [1, 0, 0],
                threads: [10, 1, 1],
            }),
            [64, 1, 1],
            per_lane,
        )
        .expect("the clear translates");
        runs_partial_groups(model, &module);
    }
}

/// Runs a translated clear over two groups whose second is partial, of ten and of thirty-seven
/// threads, and checks each clears its threads' slots and no other.
fn runs_partial_groups(model: &str, module: &[u32]) {
    for partial_threads in [10, 37] {
        let partial = PartialGroups {
            last: [1, 0, 0],
            threads: [partial_threads, 1, 1],
        };
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        // The user data, then the partial shape where the module reads it.
        let mut push = user_data(WINDOW + 0x100, 2 * 64 * 16);
        push.resize(
            orbistoun_translate::wavefront::PARTIAL_PUSH_WORD as usize,
            0,
        );
        push.extend(partial.push_words());
        let done = dispatch_guest(
            module,
            &before,
            (&push, [2, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert!(!done.escaped, "{model}");
        let end = 0x40 + (64 + partial_threads as usize) * 4;
        for (index, &word) in done.memory.iter().enumerate() {
            let expected = if (0x40..end).contains(&index) {
                VALUE[index % 4]
            } else {
                BEFORE
            };
            assert_eq!(
                word, expected,
                "{model}: {partial_threads} threads, word {index}"
            );
        }
    }
}

/// A thread group of more than one wave runs every wave (D858): one group of 256 threads is four
/// waves and clears 256 slots, and one of 100 is a whole wave and 36 threads of the next and clears
/// 100. The clear waits at no barrier and uses no local data share, so its waves never meet.
#[test]
fn a_group_of_waves_runs_every_wave() {
    if !device_or_skip("guest clear, a group of waves") {
        return;
    }
    for (model, per_lane) in MODELS {
        for threads in [256, 100] {
            let module =
                translated_with(0, None, [threads, 1, 1], per_lane).expect("the clear translates");
            let before = vec![BEFORE; WINDOW_WORDS as usize];
            let done = dispatch_guest(
                &module,
                &before,
                (&user_data(WINDOW, 256 * 16), [1, 1, 1]),
                &DispatchImages::default(),
            )
            .expect("dispatched");
            assert!(!done.escaped, "{model}");
            let end = threads as usize * 4;
            for (index, &word) in done.memory.iter().enumerate() {
                let expected = if index < end {
                    VALUE[index % 4]
                } else {
                    BEFORE
                };
                assert_eq!(word, expected, "{model}: {threads} threads, word {index}");
            }
        }
    }
}

/// A thread group of waves that meet at `s_barrier` through the local data share runs each wave as
/// an invocation and the barrier as the workgroup's, where no branch crosses it: Grand Theft Auto
/// V's scene compute program reduces through the share in groups of four waves, a barrier between
/// each step. Each of 128 threads writes its id to its own share word, waits, and stores the word
/// of thread `id ^ 64` - the other wave's - so only a barrier both waves keep reads every word
/// written.
#[test]
fn a_group_of_waves_meets_at_its_barrier() {
    if !device_or_skip("a group of waves meets at its barrier") {
        return;
    }
    // The source codes of the inline constants 2 and 64.
    let two = 128 + 2_u32;
    let sixty_four = 128 + 64_u32;
    // MUBUF: vaddr at 0, vdata at 8, the descriptor's first register / 4 at 16, soffset 0 at 24.
    let program = [
        encoded("v_lshlrev_b32_e32", (1 << 17) | two), // v1 = id * 4
        encoded("ds_write_b32", 0),
        1,                                                        // address v1, data v0
        0xbf8a_0000,                                              // s_barrier
        encoded("v_xor_b32_e32", (2 << 17) | sixty_four),         // v2 = id ^ 64
        encoded("v_lshlrev_b32_e32", (2 << 17) | (2 << 9) | two), // v2 = v2 * 4
        encoded("ds_read_b32", 0),
        2 | (3 << 24), // v3 = the word at v2
        0xbf8c_c07f,   // s_waitcnt lgkmcnt(0)
        encoded("buffer_store_dword", 0x1000),
        0x8002_0301, // v3 at v1, through s[8:11], offen
        0xbf81_0000,
    ];
    // The whole wave in one invocation, and per lane on the host's subgroup: two lanes an
    // invocation on a subgroup of thirty-two (D973).
    for (model, subgroup) in [("whole wave", None), ("per lane", Some(host_subgroup()))] {
        let module = translated_for(&program, (0, None, [128, 1, 1]), (subgroup, Width::Wave64))
            .expect("waves that meet at an uncrossed barrier translate");
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        let done = dispatch_guest(
            &module,
            &before,
            (&user_data(WINDOW, 128 * 4), [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert!(!done.escaped, "{model}");
        for (index, &word) in done.memory.iter().enumerate() {
            let expected = if index < 128 {
                index as u32 ^ 64
            } else {
                BEFORE
            };
            assert_eq!(word, expected, "{model}: word {index}");
        }
    }
}

/// A group of several waves whose last group is partial runs, per lane, exactly the threads it
/// runs as whole waves (D855, D858, D880): groups of 100 threads, two waves, the second group
/// running 37 of them.
#[test]
fn a_partial_group_of_waves_runs_its_threads_both_ways() {
    if !device_or_skip("guest clear, a partial group of waves") {
        return;
    }
    let partial = PartialGroups {
        last: [1, 0, 0],
        threads: [37, 1, 1],
    };
    let mut push = user_data(WINDOW, 2 * 100 * 16);
    push.resize(
        orbistoun_translate::wavefront::PARTIAL_PUSH_WORD as usize,
        0,
    );
    push.extend(partial.push_words());
    let mut results = Vec::new();
    for (model, per_lane) in MODELS {
        let module =
            translated_with(0, Some(partial), [100, 1, 1], per_lane).expect("the clear translates");
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        let done = dispatch_guest(
            &module,
            &before,
            (&push, [2, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert!(!done.escaped, "{model}");
        // The first group's slots, then the clear's addressing from `workgroup * 64`: the second
        // group starts at thread 64 and its 37 threads reach 101.
        let end = 101 * 4;
        for (index, &word) in done.memory.iter().enumerate() {
            let expected = if index < end {
                VALUE[index % 4]
            } else {
                BEFORE
            };
            assert_eq!(word, expected, "{model}: word {index}");
        }
        results.push(done.memory);
    }
    assert_eq!(results[0], results[1], "both ways leave the same memory");
}

/// A group of waves whose program branches over its `s_barrier` is refused: a wave whose mask is
/// empty would meet the others one time fewer, which the workgroup's barrier cannot stand for
/// (D892).
#[test]
fn a_group_of_waves_that_branches_over_its_barrier_is_refused() {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    // s_cbranch_execz 1 (to the end); s_barrier; s_endpgm.
    let words = [0xbf88_0001_u32, 0xbf8a_0000, 0xbf81_0000];
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let refused = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 1,
                threads: [256, 1, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect_err("refused")
    .to_string();
    assert!(refused.contains("branches over an s_barrier"), "{refused}");
}

/// A raw buffer's bounds hold: stores past its record count are dropped, as the hardware drops
/// them, and that is not an escape.
#[test]
fn stores_past_a_raw_buffers_records_are_dropped() {
    if !device_or_skip("guest clear, short buffer") {
        return;
    }
    for (model, per_lane) in MODELS {
        let before = vec![BEFORE; WINDOW_WORDS as usize];
        // Sixty-three whole stores fit in 1008 bytes; the sixty-fourth lies wholly past them, so
        // how a store straddling the end is checked does not enter into it.
        let done = dispatch_guest(
            &module(per_lane),
            &before,
            (&user_data(WINDOW, 1008), [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert!(!done.escaped, "{model}");
        assert!(
            done.memory[..252]
                .iter()
                .enumerate()
                .all(|(i, &w)| w == VALUE[i % 4]),
            "{model}"
        );
        assert!(
            done.memory[252..].iter().all(|&w| w == BEFORE),
            "{model}: nothing past the records"
        );
    }
}

/// A descriptor naming memory outside the window - in another four-gigabyte span, or past its end
/// - is an escape: the dispatch says so rather than dropping or folding the stores.
#[test]
fn a_descriptor_outside_the_window_is_an_escape() {
    if !device_or_skip("guest clear, escaping") {
        return;
    }
    let before = vec![BEFORE; WINDOW_WORDS as usize];
    for (model, per_lane) in MODELS {
        for base in [
            WINDOW + (1 << 32),
            WINDOW + u64::from(WINDOW_WORDS) * 4 - 0x100,
        ] {
            let done = dispatch_guest(
                &module(per_lane),
                &before,
                (&user_data(base, 64 * 16), [1, 1, 1]),
                &DispatchImages::default(),
            )
            .expect("dispatched");
            assert!(done.escaped, "{model}: {base:#x}");
        }
    }
}
