//! A device-addressed dispatch captured and replayed alone (D980): the capture holds every guest
//! chunk the dispatch's lookups found, as it was before the dispatch ran, so a replay walks the same
//! pointers through the same memory and leaves what the run left.
//!
//! Its own test binary, because the capture selection is read from the environment once a process.

use orbistoun_gpu_vulkan::capture::Capture;
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::replay::run_hash;
use orbistoun_gpu_vulkan::{
    Availability, direct, dispatch_guest, note_dispatch, probe, replay_dispatch,
};
use orbistoun_mem::Protection;
use orbistoun_mem::test_bases::{Range, crates};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, USER_DATA_BLOCK_WORDS, UserData, Window,
    translate_with_user_data,
};

static BASES: Range = Range::nth(crates::GPU_VULKAN);

/// The bytes the test reserves.
const RESERVED: u64 = 1 << 20;

/// The guest program address the dispatch is noted with and the capture asks for.
const PROGRAM: u64 = 0x7400_0dca_0000;

/// A window no access of the program reaches.
const WINDOW: u64 = 0x4_0187_0000;

/// Where, past the reservation's base, the user data's pointer, the first pointer it names and the
/// counters the second names lie: each in a 64 KiB page of its own, and only the first's page and
/// the one after it named by the user data.
const NAMED: u64 = 0x1000;
const FIRST: u64 = 0x4_0000;
const COUNTERS: u64 = 0x8_0000;

/// Each lane follows the user data's pointer to a pointer, then to a table of counters, and adds
/// one to its own counter: memory the capture finds only by the lookups the dispatch made, one
/// chunk of it stored into.
///
/// ```text
/// v_mov_b32 v2, s0 ; v_mov_b32 v3, s1
/// global_load_dwordx2 v[4:5], v[2:3], off      the first pointer
/// global_load_dwordx2 v[6:7], v[4:5], off      the counters
/// v_lshlrev_b32 v8, 2, v0 ; v_add_co_u32 v6, vcc, v6, v8 ; v_add_co_ci_u32 v7, vcc, 0, v7, vcc
/// global_load_dword v9, v[6:7], off ; v_add_nc_u32 v9, 1, v9
/// global_store_dword v[6:7], v9, off
/// s_endpgm
/// ```
const PROGRAM_WORDS: [u32; 19] = [
    0x7e04_0200,
    0x7e06_0201,
    0xdc34_8000,
    0x047d_0002,
    0xbf8c_3f70,
    0xdc34_8000,
    0x067d_0004,
    0xbf8c_3f70,
    0x3410_0082,
    0xd70f_6a06,
    0x0002_1106,
    0x500e_0e80,
    0xdc30_8000,
    0x097d_0006,
    0xbf8c_3f70,
    0x4a12_1281,
    0xdc70_8000,
    0x007d_0906,
    0xbf81_0000,
];

fn module() -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = PROGRAM_WORDS.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, 64).expect("a window"),
        UserData {
            count: 2,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [64, 1, 1],
                device_addressed: true,
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
    .expect("the program translates device-addressed")
}

fn write_word(at: u64, value: u32) {
    // SAFETY: every address written lies in the reservation the test holds, committed read-write.
    assert!(unsafe { orbistoun_mem::guest::write_u32(at, value) });
}

fn read_words(at: u64, words: usize) -> Vec<u32> {
    let mut bytes = vec![0_u8; words * 4];
    // SAFETY: as for `write_word`, or a range the test's capture names inside it.
    assert!(unsafe { orbistoun_mem::guest::read_into(at, &mut bytes) });
    bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// A dispatch that walks pointers it loads from guest memory, and stores into what they name, is
/// captured with the chunks it reached as they were before it ran; replayed alone from the capture,
/// it answers the hash the run's own output has.
#[test]
fn a_replayed_device_addressed_capture_walks_the_pointers_the_run_did() {
    let alignment = match probe() {
        Availability::Available { .. } => direct::alignment(),
        Availability::Unavailable { .. } => None,
    };
    if alignment.is_none() {
        println!("!! SKIPPED: no device that addresses guest memory directly");
        return;
    }
    let frames =
        std::env::temp_dir().join(format!("orbistoun-direct-capture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&frames);
    std::fs::create_dir_all(&frames).expect("the frame directory");
    // SAFETY: this binary's one test sets it before any thread reads the environment.
    unsafe { std::env::set_var("ORBISTOUN_FRAME_DIR", &frames) };
    // SAFETY: as above.
    unsafe { std::env::set_var("ORBISTOUN_CAPTURE_DISPATCH", format!("{PROGRAM:#x}")) };
    let dir = frames.join(format!("dispatch-{PROGRAM:x}"));

    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, Protection::READ_WRITE)
        .expect("reserve");
    let (named, first, counters) = (base + NAMED, base + FIRST, base + COUNTERS);
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    let halves = |address: u64| [address as u32, (address >> 32) as u32];
    for (at, word) in [named, named + 4].into_iter().zip(halves(first)) {
        write_word(at, word);
    }
    for (at, word) in [first, first + 4].into_iter().zip(halves(counters)) {
        write_word(at, word);
    }
    for lane in 0..64 {
        write_word(counters + 4 * lane, 1000 * lane as u32);
    }
    let module = module();
    let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    push[..2].copy_from_slice(&halves(named));

    // The capture is written once it holds every chunk as it was before its dispatch, which may be
    // a later dispatch than the first.
    let mut expected = None;
    for dispatched in 1..=4 {
        let counted = read_words(counters, 64);
        note_dispatch(PROGRAM, [1, 1, 1]);
        let done = dispatch_guest(
            &module,
            &[0; 64],
            (&push, [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        if dir.join("dispatch.txt").exists() {
            let capture = Capture::read_from(&dir).expect("the capture reads back");
            assert!(
                capture.guest_before,
                "every chunk as it was before the dispatch"
            );
            let held = capture
                .guest
                .iter()
                .find(|range| {
                    (range.at..range.at + range.words.len() as u64 * 4).contains(&counters)
                })
                .expect("the counters the second pointer names are captured");
            let from = ((counters - held.at) / 4) as usize;
            assert_eq!(
                &held.words[from..from + 64],
                counted.as_slice(),
                "dispatch {dispatched}: the counters as they were before it"
            );
            println!("captured at dispatch {dispatched}");
            let after: Vec<Vec<u32>> = capture
                .guest
                .iter()
                .map(|range| read_words(range.at, range.words.len()))
                .collect();
            expected = Some((run_hash(&done, &after), done.escaped));
            break;
        }
    }
    let (expected, escaped) = expected.expect("the dispatch was captured");
    assert!(!escaped, "the run reached only memory the device maps");
    assert!(space.release(base, RESERVED), "released");

    let replay = replay_dispatch(&dir, 2, None);
    let _ = std::fs::remove_dir_all(&frames);
    let replay = replay.expect("the capture was read");
    println!("{replay}");
    assert!(!replay.lost, "{replay}");
    for run in &replay.runs {
        let output = run.outcome.as_ref().expect("the run answered");
        assert!(
            !output.escaped,
            "the replay reached every chunk the run did: {replay}"
        );
        assert_eq!(
            output.hash, expected,
            "the replay leaves what the run left: {replay}"
        );
    }
}
