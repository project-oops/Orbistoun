//! A guest loop that runs past the cap is cut and reported, run on the device (D949).
//!
//! The program is radeonsi's compute buffer clear (as `orbistoun-gpu-vulkan`'s guest dispatch test
//! has it) with a loop placed after its thread id is formed: each thread stores the sixteen-byte
//! value in user data words 4-7 through the descriptor in words 8-11, once the loop has run. A loop
//! the guest bounds runs as written and the clear lands; one that never ends is cut, the module ends
//! as at `s_endpgm`, and the dispatch names the loop's backward branch. Both ways a dispatch runs
//! are held to it: the whole wave in one invocation, and one invocation per lane (D880).

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::{GuestDispatch, dispatch_guest};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::control::LOOP_CAP;
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

/// `v_lshl_add_u32 v0, s12, 6, v0`: the thread's place in the dispatch.
const THREAD_ID: [u32; 2] = [0xd746_0000, 0x0401_0c0c];
/// The rest of the clear: `v_lshrrev_b64 v[4:5], 0, s[4:5]`, `v_lshrrev_b64 v[6:7], 0, s[6:7]`,
/// `v_lshlrev_b32 v0, 4, v0`, `buffer_store_dwordx4 v[4:7], v0, s[8:11], 0 offen`, `s_endpgm`.
const STORE: [u32; 9] = [
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

const WINDOW: u64 = 0x4_0187_0000;
const WINDOW_WORDS: u32 = 4096;
/// What memory held before, so an untouched word is told from a cleared one.
const BEFORE: u32 = 0xdead_beef;
const VALUE: [u32; 4] = [0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444];
/// `SQ_BUF_RSRC_WORD3`: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;
/// The scalar operand codes for a literal following the instruction, zero and one.
const LITERAL: u32 = 255;
const ZERO: u32 = 128;
const ONE: u32 = 129;
/// The scalar register the loop counts down in: free, past the user data and the workgroup id.
const COUNTER: u32 = 13;

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

/// A branch's signed word offset as its sixteen-bit immediate.
fn words(offset: i16) -> u32 {
    u32::from(u16::from_ne_bytes(offset.to_ne_bytes()))
}

/// The clear with `s_branch` to itself between the thread id and the store: a loop at byte 8 that
/// never ends.
fn endless() -> Vec<u32> {
    let mut program = THREAD_ID.to_vec();
    program.push(encoded("s_branch", words(-1)));
    program.extend_from_slice(&STORE);
    program
}

/// The byte offset of [`endless`]'s branch.
const ENDLESS_BRANCH: u32 = 8;

/// The clear with a loop that runs `times` before the store: the counter set, then decremented and
/// tested until it reaches zero, so the loop jumps back `times - 1` times.
fn counted(times: u32) -> Vec<u32> {
    let mut program = THREAD_ID.to_vec();
    program.push(encoded("s_mov_b32", (COUNTER << 16) | LITERAL));
    program.push(times);
    program.push(encoded("s_sub_u32", (COUNTER << 16) | (ONE << 8) | COUNTER));
    program.push(encoded("s_cmp_lg_u32", (ZERO << 8) | COUNTER));
    // From the instruction after the branch, at byte 28, back to the subtraction at byte 16.
    program.push(encoded("s_cbranch_scc1", words(-3)));
    program.extend_from_slice(&STORE);
    program
}

/// The byte offset of [`counted`]'s branch.
const COUNTED_BRANCH: u32 = 24;

/// `program` translated for a dispatch of groups of sixty-four threads, one invocation per lane
/// where `per_lane`, on the device's subgroup.
fn module(program: &[u32], per_lane: bool) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
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
    .expect("the program translates")
}

/// Runs `program` as one group over a window that held [`BEFORE`] everywhere.
fn run(program: &[u32], per_lane: bool) -> GuestDispatch {
    let mut block = vec![0_u32; 32];
    block[4..8].copy_from_slice(&VALUE);
    block[8] = WINDOW as u32;
    block[9] = (WINDOW >> 32) as u32 & 0xffff;
    block[10] = 64 * 16;
    block[11] = WORD3;
    dispatch_guest(
        &module(program, per_lane),
        &vec![BEFORE; WINDOW_WORDS as usize],
        (&block, [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("dispatched, not the device lost")
}

/// Whether the window holds the clear: the value in each of the sixty-four threads' slots, and
/// nothing past them.
fn cleared(memory: &[u32]) -> bool {
    memory.iter().enumerate().all(|(index, &word)| {
        word == if index < 64 * 4 {
            VALUE[index % 4]
        } else {
            BEFORE
        }
    })
}

/// A loop that never ends is cut rather than losing the device: the dispatch completes, names the
/// loop by its backward branch, and stores nothing, as a program that ended there would.
#[test]
fn an_endless_loop_is_cut_and_named() {
    if !device_or_skip("endless loop") {
        return;
    }
    for (model, per_lane) in MODELS {
        let done = run(&endless(), per_lane);
        assert_eq!(done.loop_cut, Some(ENDLESS_BRANCH), "{model}");
        assert!(!done.escaped, "{model}: a cut is not an access escape");
        assert!(
            done.memory.iter().all(|&word| word == BEFORE),
            "{model}: nothing after the loop ran"
        );
    }
}

/// A loop the guest bounds runs as it wrote it: no cut, and the store after it lands.
#[test]
fn a_bounded_loop_runs_as_written() {
    if !device_or_skip("bounded loop") {
        return;
    }
    for (model, per_lane) in MODELS {
        let done = run(&counted(1000), per_lane);
        assert_eq!(done.loop_cut, None, "{model}");
        assert!(!done.escaped, "{model}");
        assert!(cleared(&done.memory), "{model}: the store after the loop");
    }
}

/// The cap is exact: a loop that jumps back exactly [`LOOP_CAP`] times runs to its end, and one
/// that would jump back once more is cut at its branch. The time the first takes beside a loop of
/// one jump is printed, the measure the cap is set against (the host's watchdog).
#[test]
fn a_loop_at_the_cap_runs_and_one_past_it_is_cut() {
    if !device_or_skip("loop at the cap") {
        return;
    }
    for (model, per_lane) in MODELS {
        let started = std::time::Instant::now();
        run(&counted(2), per_lane);
        let short = started.elapsed();
        let started = std::time::Instant::now();
        let done = run(&counted(LOOP_CAP + 1), per_lane);
        println!(
            "[loop at the cap] {model}: {LOOP_CAP} backward jumps in {:?}, one in {short:?}",
            started.elapsed()
        );
        assert_eq!(done.loop_cut, None, "{model}: at the cap");
        assert!(
            cleared(&done.memory),
            "{model}: at the cap, the store lands"
        );

        let done = run(&counted(LOOP_CAP + 2), per_lane);
        assert_eq!(done.loop_cut, Some(COUNTED_BRANCH), "{model}: one past it");
        assert!(
            done.memory.iter().all(|&word| word == BEFORE),
            "{model}: one past it, nothing stored"
        );
    }
}
