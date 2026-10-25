//! Guest pages the device left out of its mapping, mapped once they become mappable (D968).
//!
//! The device maps a guest run's committed, read-write pages the first time a device-addressed
//! dispatch runs over it, and leaves the rest out. A page the guest commits on touch or makes
//! writable later changes none of the readable runs, so nothing remapped it: every access to it
//! read zero and stored nothing. A single-pass scan whose groups publish their sums through such a
//! page, and wait on each other's flags there, then waits forever. An access no mapping holds names
//! its page (D971), and a capture keeps the pages user data names so the dispatch replays alone.
//! These tests run in a process of their own, since the device's mappings are the process's.

use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::{Availability, direct, dispatch_guest, probe};
use orbistoun_mem::Protection;
use orbistoun_mem::test_bases::{Range, crates};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, USER_DATA_BLOCK_WORDS, UserData, Window,
    translate_with_user_data,
};

static BASES: Range = Range::nth(crates::GPU_VULKAN);

/// The bytes the test reserves: a megabyte, many import alignments wide.
const RESERVED: u64 = 1 << 20;

/// Whether the device addresses guest memory directly; prints a skip otherwise.
fn direct_or_skip(test: &str) -> bool {
    let reason = match probe() {
        Availability::Available { .. } => match direct::alignment() {
            Some(_) => return true,
            None => {
                "the device lacks VK_EXT_external_memory_host or bufferDeviceAddress".to_owned()
            }
        },
        Availability::Unavailable { reason } => reason,
    };
    println!();
    println!("!! SKIPPED: {test}");
    println!("!! {reason}");
    println!("!! this test did NOT run");
    println!();
    false
}

fn read(at: u64) -> u32 {
    // SAFETY: every address read lies in the test's reservation, committed and readable.
    unsafe { orbistoun_mem::guest::read_u32(at) }.expect("a guest address")
}

fn write(at: u64, value: u32) {
    // SAFETY: every address written lies in the test's reservation, committed read-write then.
    assert!(unsafe { orbistoun_mem::guest::write_u32(at, value) });
}

/// The instruction head of `name`: its family's fixed bits and its opcode.
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

/// `SADDR` naming no scalar base, `off`.
const OFF: u32 = 0x7d;

/// A program that reads the guest word at the address in user data words 0 and 1 and stores it
/// plus one in the word after it:
///
/// ```text
/// v_mov_b32 v0, s0 ; v_mov_b32 v1, s1
/// global_load_dword v2, v[0:1], off
/// v_add_nc_u32 v2, 1, v2
/// global_store_dword v[0:1], v2, off offset:4
/// s_endpgm
/// ```
fn plus_one_program() -> Vec<u32> {
    let v_mov = head("v_mov_b32_e32");
    vec![
        v_mov,
        v_mov | (1 << 17) | 0x1,
        head("global_load_dword"),
        (OFF << 16) | (2 << 24),
        // The inline constant 1 is source code 0x81.
        head("v_add_nc_u32_e32") | (2 << 17) | (2 << 9) | 0x81,
        head("global_store_dword") | 4,
        (2 << 8) | (OFF << 16),
        head("s_endpgm"),
    ]
}

/// `program` translated as a device-addressed dispatch of one group of sixty-four threads, the
/// whole wave in one invocation, with two user-data words.
fn device_addressed(program: &[u32]) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(0x4_0187_0000, 64).expect("a window"),
        UserData {
            count: 2,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [64, 1, 1],
                device_addressed: true,
                ..ComputeInputs::default()
            }),
            local_words: Some(64),
            draw_buffers: true,
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
    .expect("the program translates device-addressed")
}

/// Runs `module` over the guest word at `address`, answering whether an access escaped.
fn run(module: &[u32], address: u64) -> bool {
    let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    push[..2].copy_from_slice(&[address as u32, (address >> 32) as u32]);
    dispatch_guest(
        module,
        &[0; 64],
        (&push, [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("dispatched")
    .escaped
}

/// A page the guest held read-only when the device mapped its run is left out, and once the guest
/// makes it writable - which changes no readable run - the next dispatch reaches it: it reads what
/// the guest wrote there and its store lands, with nothing escaping.
#[test]
fn a_page_made_writable_after_its_run_was_mapped_is_reached() {
    if !direct_or_skip("a_page_made_writable_after_its_run_was_mapped_is_reached") {
        return;
    }
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, Protection::READ_WRITE)
        .expect("reserve");
    let later = base + RESERVED / 2;
    space
        .protect(later, 0x1_0000, Protection::READ_ONLY)
        .expect("held read-only");
    let module = device_addressed(&plus_one_program());

    write(base + 0x1000, 41);
    assert!(!run(&module, base + 0x1000), "the mapped page is held");
    assert_eq!(read(base + 0x1004), 42, "the first dispatch maps the run");

    space
        .protect(later, 0x1_0000, Protection::READ_WRITE)
        .expect("made writable");
    write(later, 7);
    let escaped = run(&module, later);
    assert_eq!(
        read(later + 4),
        8,
        "the page made writable is read and stored by device address"
    );
    assert!(!escaped, "nothing escaped");
    assert!(
        space.release(base, RESERVED),
        "released, its mapping dropped first"
    );
}

/// An access no mapping holds names its guest page in what the dispatch answers (D971): the page
/// of the address it formed, in a reservation the guest may not read, which the device never maps.
#[test]
fn an_escape_names_the_guest_page_it_reached() {
    if !direct_or_skip("an_escape_names_the_guest_page_it_reached") {
        return;
    }
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, Protection::NONE)
        .expect("reserve");
    let module = device_addressed(&plus_one_program());
    let at = base + 0x3_1234;
    let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    push[..2].copy_from_slice(&[at as u32, (at >> 32) as u32]);
    let done = dispatch_guest(
        &module,
        &[0; 64],
        (&push, [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("dispatched");
    assert!(done.escaped, "nothing maps the page");
    assert_eq!(
        done.escaped_page,
        Some(base + 0x3_0000),
        "the page it reached"
    );
    assert!(space.release(base, RESERVED), "released");
}

/// A device-addressed dispatch captured with the guest pages its user data names replays alone:
/// the pages are placed where they lay, the device maps them, the program reads and stores through
/// them as it did, and each run starts from the pages as captured, so each leaves the same output
/// (D971).
#[test]
fn a_device_addressed_capture_replays_over_its_guest_pages() {
    use std::borrow::Cow;

    use orbistoun_gpu_vulkan::capture::{Capture, CapturedGuest, Reason};
    if !direct_or_skip("a_device_addressed_capture_replays_over_its_guest_pages") {
        return;
    }
    let page = BASES.take();
    let at = page + 0x200;
    let mut words = vec![0_u32; 0x4000];
    words[0x80] = 99;
    let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    push[..2].copy_from_slice(&[at as u32, (at >> 32) as u32]);
    let module = device_addressed(&plus_one_program());
    let capture = Capture {
        guest: vec![CapturedGuest {
            at: page,
            words: Cow::Owned(words),
        }],
        ..Capture::of(
            (Some(0x4000_03e1_8000), Reason::Asked),
            &module,
            &[0; 64],
            (&push, [1, 1, 1]),
            &DispatchImages::default(),
            &[],
        )
    };
    let dir = std::env::temp_dir().join(format!("orbistoun-direct-replay-{}", std::process::id()));
    capture.write_to(&dir).expect("the capture was written");
    let out = dir.join("out");
    let replay = orbistoun_gpu_vulkan::replay_dispatch(&dir, 2, Some(&out));
    let after = std::fs::read(out.join("guest-0.bin"));
    let _ = std::fs::remove_dir_all(&dir);
    let replay = replay.expect("the capture was read");
    println!("{replay}");
    assert!(!replay.lost, "{replay}");
    let outputs: Vec<_> = replay
        .runs
        .iter()
        .map(|run| run.outcome.clone().expect("the run answered"))
        .collect();
    assert!(outputs.iter().all(|out| !out.escaped), "{replay}");
    assert_eq!(
        outputs[0].hash, outputs[1].hash,
        "each run starts as captured"
    );
    let after = after.expect("the guest pages as the dispatch left them");
    let word = |index: usize| {
        u32::from_le_bytes(after[index * 4..index * 4 + 4].try_into().expect("a word"))
    };
    assert_eq!(word(0x81), 100, "the store landed in the placed page");
}
