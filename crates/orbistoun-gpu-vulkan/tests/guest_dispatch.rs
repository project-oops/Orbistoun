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

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::dispatch_guest;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, PartialGroups, Stage, UserData, Window, translate_with_user_data,
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

/// `program`, translated for groups of `threads`, the last perhaps partial, with the user-data words
/// in `unwritten` marked as never written; one invocation per lane where `per_lane` (D880).
fn translated_program(
    program: &[u32],
    (unwritten, partial, threads): (u32, Option<PartialGroups>, [u32; 3]),
    per_lane: bool,
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
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
                threads,
                unwritten_user_data: unwritten,
                partial,
                integer_images: [false; 2],
                first_thread: 0,
            }),
            per_invocation: per_lane.then_some(64),
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
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

/// A program that reads a mask whole is refused one invocation per lane, naming why: `s_cbranch_vccz`
/// branches on every lane's bit (D880).
#[test]
fn a_program_reading_a_mask_whole_is_refused_per_lane() {
    // s_cbranch_vccz +0; s_endpgm.
    let refused = translated_program(&[0xbf86_0000, 0xbf81_0000], (0, None, [64, 1, 1]), true)
        .expect_err("refused")
        .to_string();
    assert!(refused.contains("lane mask whole"), "{refused}");
    assert!(
        translated_program(&[0xbf86_0000, 0xbf81_0000], (0, None, [64, 1, 1]), false).is_ok(),
        "the whole wave runs it"
    );
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

/// A group of waves whose program waits at `s_barrier` is refused: its waves meet there, and
/// waves run as separate invocations are not held for one another.
#[test]
fn a_group_of_waves_that_meets_at_a_barrier_is_refused() {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    // s_barrier; s_endpgm.
    let words = [0xbf8a_0000_u32, 0xbf81_0000];
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
                thread_id_components: 1,
                threads: [256, 1, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect_err("refused")
    .to_string();
    assert!(refused.contains("waits at s_barrier"), "{refused}");
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
