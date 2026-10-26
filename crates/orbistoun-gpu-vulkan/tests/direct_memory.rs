//! Guest memory imported into the device and addressed by pointer (D964), proved on memory reserved
//! as a run reserves it.
//!
//! The guest's own pages are the device's: a word the guest writes before a submission is what the
//! module reads, and a word the module stores is in the guest's pages once the dispatch returns,
//! with nothing uploaded or written back. A missing device, or one without direct addressing, skips
//! with a line `bin/orbistoun check` surfaces.

use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::direct::{self, Imported};
use orbistoun_gpu_vulkan::{Availability, dispatch_guest, dispatch_pushing, probe};
use orbistoun_mem::Protection;
use orbistoun_mem::test_bases::{Range, crates};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_spirv::{DirectStep, direct_address_module};
use orbistoun_translate::Width;
use orbistoun_translate::device_address::{LOCAL_APERTURE_HIGH, Mapped, Table};
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, USER_DATA_BLOCK_WORDS, UserData, Window,
    translate_with_user_data,
};

static BASES: Range = Range::nth(crates::GPU_VULKAN);

/// The bytes each test reserves: a megabyte, many import alignments wide.
const RESERVED: u64 = 1 << 20;

/// Whether the device addresses guest memory directly, with the import alignment; prints a skip
/// otherwise.
fn direct_or_skip(test: &str) -> Option<u64> {
    let reason = match probe() {
        Availability::Available { .. } => match direct::alignment() {
            Some(alignment) => return Some(alignment),
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
    None
}

const fn read_write() -> Protection {
    Protection::READ_WRITE
}

fn read(at: u64) -> u32 {
    // SAFETY: every address read lies in a reservation the test holds, committed read-write.
    unsafe { orbistoun_mem::guest::read_u32(at) }.expect("a guest address")
}

fn write(at: u64, value: u32) {
    // SAFETY: as for `read`.
    assert!(unsafe { orbistoun_mem::guest::write_u32(at, value) });
}

/// Runs `steps` against the device address of guest address `at`.
fn run(at: u64, import: &Imported, steps: &[DirectStep]) {
    let address = import.device_address_of(at).expect("the import holds it");
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    let push = [address as u32, (address >> 32) as u32];
    dispatch_pushing(&direct_address_module(steps), (1, 1), &push, [1, 1, 1]).expect("dispatch");
}

/// A word the guest wrote before the import and one it wrote after are both what the module reads,
/// what the module stores is in the guest's pages when the dispatch returns, and a second dispatch
/// over the same import reads what the guest wrote between the two.
#[test]
fn a_guest_range_is_read_and_written_by_device_address() {
    let Some(_) = direct_or_skip("a_guest_range_is_read_and_written_by_device_address") else {
        return;
    };
    let base = BASES.take();
    let held = orbistoun_mem::platform::reserve(base, RESERVED, read_write()).expect("reserve");
    write(base + 0x1000, 0x1234_5678);
    // SAFETY: the reservation is committed whole and outlives the import, dropped first.
    let import = unsafe { Imported::import(base, RESERVED) }.expect("import");
    assert_eq!(import.base(), base);
    write(base + 0x2000, 41);
    let steps = [
        DirectStep {
            from: 0x2000,
            to: 0x2004,
            add: 1,
        },
        DirectStep {
            from: 0x1000,
            to: 0x3004,
            add: 0,
        },
        DirectStep {
            from: 0x3008,
            to: 0x3000,
            add: 0x00C0_FFEE,
        },
    ];
    run(base, &import, &steps);
    assert_eq!(read(base + 0x2004), 42, "written after the import, read");
    assert_eq!(
        read(base + 0x3004),
        0x1234_5678,
        "written before the import, read"
    );
    assert_eq!(
        read(base + 0x3000),
        0x00C0_FFEE,
        "the device's store landed"
    );
    assert_eq!(read(base + 0x300c), 0, "nothing else was written");

    write(base + 0x2000, 99);
    run(base, &import, &steps[..1]);
    assert_eq!(
        read(base + 0x2004),
        100,
        "the second dispatch read the guest's newer word"
    );
    drop(import);
    drop(held);
}

/// A range imported from inside a reservation is addressed from its own base: the guest address
/// a module reaches is the import's device address plus its distance from the range's start.
#[test]
fn a_range_imported_from_inside_a_reservation_addresses_from_its_own_base() {
    let Some(alignment) =
        direct_or_skip("a_range_imported_from_inside_a_reservation_addresses_from_its_own_base")
    else {
        return;
    };
    let base = BASES.take();
    let held = orbistoun_mem::platform::reserve(base, RESERVED, read_write()).expect("reserve");
    let start = base + alignment.max(0x1_0000);
    let len = RESERVED - (start - base);
    // SAFETY: the range lies in the reservation, committed whole, and the import is dropped first.
    let import = unsafe { Imported::import(start, len) }.expect("import");
    assert_eq!(
        import.device_address_of(start - 4),
        None,
        "before the range"
    );
    assert_eq!(import.device_address_of(start + len), None, "past it");
    let at = start + 0x1230;
    write(at, 7);
    run(
        at,
        &import,
        &[DirectStep {
            from: 0,
            to: 0x10,
            add: 3,
        }],
    );
    assert_eq!(read(at + 0x10), 10);
    drop(import);
    drop(held);
}

/// A range reserved to commit on touch imports once committed, and is addressed as any other. It
/// must be committed first: the driver touches the pages it imports from the calling thread, and
/// on an uncommitted page that touch faulted the test process rather than being refused.
#[test]
fn a_range_committed_on_touch_imports_once_committed() {
    let Some(_) = direct_or_skip("a_range_committed_on_touch_imports_once_committed") else {
        return;
    };
    let base = BASES.take();
    let held =
        orbistoun_mem::platform::reserve_lazily(base, RESERVED, read_write()).expect("reserve");
    orbistoun_mem::lazy::commit(base, RESERVED);
    // SAFETY: committed just above, and the import is dropped before the reservation.
    let import = unsafe { Imported::import(base, RESERVED) }.expect("import once committed");
    write(base + 0x8000, 5);
    run(
        base,
        &import,
        &[DirectStep {
            from: 0x8000,
            to: 0x9000,
            add: 6,
        }],
    );
    assert_eq!(read(base + 0x9000), 11);
    drop(import);
    drop(held);
}

/// A device store passes the host's write-watch by: the pages it changed are not reported written,
/// so whoever caches guest memory must be told of a direct dispatch's writes apart (D964).
#[test]
fn a_device_store_is_not_seen_by_the_write_watch() {
    let Some(_) = direct_or_skip("a_device_store_is_not_seen_by_the_write_watch") else {
        return;
    };
    let base = BASES.take();
    let held = orbistoun_mem::platform::reserve(base, RESERVED, read_write()).expect("reserve");
    // SAFETY: the reservation is committed whole and outlives the import.
    let import = unsafe { Imported::import(base, RESERVED) }.expect("import");
    let page = base + 0x4_0000;
    let Some(since) = orbistoun_mem::watch::mark(page, 0x1000) else {
        println!("the host does not watch writes here: nothing to show");
        return;
    };
    run(
        page,
        &import,
        &[DirectStep {
            from: 0x10,
            to: 0,
            add: 0x5150,
        }],
    );
    assert_eq!(read(page), 0x5150, "the device stored");
    assert_eq!(
        orbistoun_mem::watch::written_since(page, 0x1000, since),
        Some(false),
        "the write-watch saw a device store"
    );
    drop(import);
    drop(held);
}

/// Pages made read-only for the CPU after their import, as a write-watch guard makes them (D824),
/// can still be read by the device (D974): it reads what the CPU wrote before the protection, and
/// stores it in pages left writable. Made writable again, the CPU's next write is what the device
/// reads, over the same import.
///
/// The device must not store into them while they are read-only: measured on an RTX 5070 Ti, the
/// same program storing into the protected pages lost the device. So no guard stands over pages
/// the device maps (D974).
///
/// A measurement, run alone with `ORBISTOUN_DEVICE_PROTECT` set: run beside the other tests it lost
/// the device too, a protection change on pages another test's dispatch maps, and took every
/// test after it down.
#[test]
fn an_import_s_read_only_pages_are_still_read() {
    if std::env::var_os("ORBISTOUN_DEVICE_PROTECT").is_none() {
        return;
    }
    let Some(_) = direct_or_skip("an_import_s_read_only_pages_are_still_read") else {
        return;
    };
    let base = BASES.take();
    let held = orbistoun_mem::platform::reserve(base, RESERVED, read_write()).expect("reserve");
    write(base + 0x1000, 7);
    // SAFETY: the reservation is committed whole and outlives the import, dropped first.
    let import = unsafe { Imported::import(base, RESERVED) }.expect("import");
    let half = RESERVED / 2;
    orbistoun_mem::platform::protect(base, half, Protection::READ_ONLY).expect("protected");
    let step = [DirectStep {
        from: 0x1000,
        to: half + 0x2000,
        add: 1,
    }];
    run(base, &import, &step);
    assert_eq!(read(base + half + 0x2000), 8, "read under the protection");
    orbistoun_mem::platform::protect(base, half, read_write()).expect("writable");
    write(base + 0x1000, 41);
    run(base, &import, &step);
    assert_eq!(read(base + half + 0x2000), 42, "the same import, after");
    drop(import);
    drop(held);
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
/// The flat encoding's flat and global segments, bits 15:14.
const FLAT: u32 = 0;
const GLOBAL: u32 = 2;

/// `v_mov_b32 vN, sM`.
fn v_mov_scalar(dst: u32, scalar: u32) -> u32 {
    head("v_mov_b32_e32") | (dst << 17) | scalar
}

/// `v_mov_b32 vN, <literal>`.
fn v_mov_literal(dst: u32, value: u32) -> [u32; 2] {
    [head("v_mov_b32_e32") | (dst << 17) | 255, value]
}

/// A flat-encoded load of one word into `dst` from the pair at `address`, in `segment`.
fn load(segment: u32, dst: u32, address: u32) -> [u32; 2] {
    [
        head("global_load_dword") & !(3 << 14) | segment << 14,
        address | (OFF << 16) | (dst << 24),
    ]
}

/// A flat-encoded store of `data` to the pair at `address` plus `offset`, in `segment`.
fn store(segment: u32, address: u32, data: u32, offset: u32) -> [u32; 2] {
    [
        head("global_store_dword") & !(3 << 14) | segment << 14 | offset,
        address | (data << 8) | (OFF << 16),
    ]
}

/// Where the program's words lie from the guest address in user data words 0 and 1: the word it
/// reads, then what it stores.
const SOURCE: u64 = 0;
const THROUGH_SHARE: u64 = 4;
const CONSTANT: u64 = 8;
const PLUS_ONE: u64 = 12;
const UNMAPPED: u64 = 16;

/// A program reading and writing guest memory and the local data share by flat and global
/// addressing (D964):
///
/// ```text
/// v_mov_b32 v0, s0 ; v_mov_b32 v1, s1           the guest address A
/// global_load_dword v2, v[0:1], off             the word at A
/// v_mov_b32 v3, 0xc0ffee
/// global_store_dword v[0:1], v3, off offset:8
/// v_mov_b32 v4, 0x40 ; v_mov_b32 v5, 0x80000000 the share's byte 0x40, through its aperture
/// flat_store_dword v[4:5], v2
/// flat_load_dword v6, v[4:5]
/// global_store_dword v[0:1], v6, off offset:4   the word at A, by way of the share
/// flat_load_dword v7, v[0:1]                     the word at A, a flat access to guest memory
/// v_add_nc_u32 v7, 1, v7
/// flat_store_dword v[0:1], v7 offset:12
/// v_mov_b32 v8, 0x10 ; v_mov_b32 v9, 0           an address nothing maps
/// global_load_dword v10, v[8:9], off
/// global_store_dword v[0:1], v10, off offset:16  zero, and an escape
/// s_endpgm
/// ```
fn flat_program() -> Vec<u32> {
    let mut words = vec![v_mov_scalar(0, 0), v_mov_scalar(1, 1)];
    words.extend(load(GLOBAL, 2, 0));
    words.extend(v_mov_literal(3, 0x00c0_ffee));
    words.extend(store(GLOBAL, 0, 3, CONSTANT as u32));
    words.extend(v_mov_literal(4, 0x40));
    words.extend(v_mov_literal(5, LOCAL_APERTURE_HIGH));
    words.extend(store(FLAT, 4, 2, 0));
    words.extend(load(FLAT, 6, 4));
    words.extend(store(GLOBAL, 0, 6, THROUGH_SHARE as u32));
    words.extend(load(FLAT, 7, 0));
    // v_add_nc_u32 v7, 1, v7: the inline constant 1 is source code 129.
    words.push(head("v_add_nc_u32_e32") | (7 << 17) | (7 << 9) | 0x81);
    words.extend(store(FLAT, 0, 7, PLUS_ONE as u32));
    words.extend(v_mov_literal(8, 0x10));
    words.extend(v_mov_literal(9, 0));
    words.extend(load(GLOBAL, 10, 8));
    words.extend(store(GLOBAL, 0, 10, UNMAPPED as u32));
    words.push(head("s_endpgm"));
    words
}

/// A window no access of the program reaches: a device-addressed dispatch reads guest memory by
/// device address, not through it.
const WINDOW: u64 = 0x4_0187_0000;

/// `program` translated as a device-addressed dispatch of one group of sixty-four threads, with two
/// user-data words; one invocation per lane where `per_lane` (D880).
fn device_addressed(program: &[u32], per_lane: bool) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
    let subgroup = per_lane.then(|| match probe() {
        Availability::Available { properties } => properties.subgroup_size,
        Availability::Unavailable { .. } => 0,
    });
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
            per_invocation: subgroup,
            local_words: Some(64),
            draw_buffers: true,
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
    .expect("the program translates device-addressed")
}

/// `vN = src0 op vM`, a vector instruction's short form: `src0` a source code (a scalar register's
/// number, an inline constant's 128 + value, a vector register's 256 + number).
fn short_form(name: &str, dst: u32, src0: u32, vsrc1: u32) -> u32 {
    head(name) | (dst << 17) | (vsrc1 << 9) | src0
}

/// The bytes between one lane's accesses and the next's, one group's and the next's, and one step
/// of the chain and the next, in a benchmark dispatch: every access its own line, every step 2 MiB
/// past the last, so a long chain reaches more memory than the device's last-level cache holds and
/// its accesses miss it, as a hierarchy walk over gigabytes does.
const BENCH_LANE_BYTES: u32 = 256;
/// See [`BENCH_LANE_BYTES`].
const BENCH_GROUP_BYTES: u32 = 64 * BENCH_LANE_BYTES;
/// See [`BENCH_LANE_BYTES`].
const BENCH_STEP_BYTES: u32 = BENCH_GROUPS * BENCH_GROUP_BYTES;

/// The groups a benchmark dispatch runs.
const BENCH_GROUPS: u32 = 128;

/// A program of `accesses` device-addressed accesses in a dependent chain, as a hierarchy walk
/// reaches memory: every load's address adds the word the previous one read (zero), so none can
/// issue before the last returns, and then a step's bytes ([`BENCH_LANE_BYTES`]). Where `storing`,
/// each step also stores the word it read beside it.
///
/// ```text
/// v_lshlrev_b32 v4, 8, v0 ; v_mov_b32 v6, s2 ; v_lshlrev_b32 v6, 14, v6
/// v_add_nc_u32 v4, v6, v4 ; v_add_nc_u32 v4, s0, v4 ; v_mov_b32 v5, s1
/// repeated: global_load_dword v2, v[4:5], off ; v_add_nc_u32 v4, v2, v4
///           [global_store_dword v[4:5], v2, off offset:8]
///           v_add_nc_u32 v4, 0x200000, v4
/// s_endpgm
/// ```
fn chained_program(accesses: u32, storing: bool) -> Vec<u32> {
    let mut words = vec![
        short_form(
            "v_lshlrev_b32_e32",
            4,
            128 + BENCH_LANE_BYTES.trailing_zeros(),
            0,
        ),
        v_mov_scalar(6, 2),
        short_form(
            "v_lshlrev_b32_e32",
            6,
            128 + BENCH_GROUP_BYTES.trailing_zeros(),
            6,
        ),
        short_form("v_add_nc_u32_e32", 4, 256 + 6, 4),
        short_form("v_add_nc_u32_e32", 4, 0, 4),
        v_mov_scalar(5, 1),
    ];
    for _ in 0..accesses {
        words.extend(load(GLOBAL, 2, 4));
        words.push(short_form("v_add_nc_u32_e32", 4, 256 + 2, 4));
        if storing {
            words.extend(store(GLOBAL, 4, 2, 8));
        }
        // The literal follows its instruction: source code 255.
        words.push(short_form("v_add_nc_u32_e32", 4, 255, 4));
        words.push(BENCH_STEP_BYTES);
    }
    words.push(head("s_endpgm"));
    words
}

/// The least time of `runs` dispatches of `module` over the guest address `address`, after two
/// that warm the pipeline and the mappings.
fn least_dispatch_time(module: &[u32], address: u64, runs: u32) -> std::time::Duration {
    let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    push[..2].copy_from_slice(&[address as u32, (address >> 32) as u32]);
    let once = || {
        let started = std::time::Instant::now();
        dispatch_guest(
            module,
            &[0; 64],
            (&push, [BENCH_GROUPS, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        started.elapsed()
    };
    once();
    once();
    (0..runs).map(|_| once()).min().expect("a run")
}

/// What asking the write-watch costs over guest memory the device maps (D974): many small runs
/// marked one by one, the same runs covered by one mark of the range holding them, both before the
/// range is imported and after. Run with `ORBISTOUN_DEVICE_BENCH` set; it prints the figures.
#[test]
fn write_watch_cost_over_an_import() {
    if std::env::var_os("ORBISTOUN_DEVICE_BENCH").is_none() {
        return;
    }
    let Some(_) = direct_or_skip("write_watch_cost_over_an_import") else {
        return;
    };
    // A reservation as large as a title's heaps: a gigabyte, in consecutive test bases.
    let base = BASES.take();
    let len: u64 = 1 << 30;
    let mut taken = 0x100_0000;
    while taken < len {
        if BASES.take() != base + taken {
            println!("the test bases are not consecutive: run it alone");
            return;
        }
        taken += 0x100_0000;
    }
    let mut space = orbistoun_mem::AddressSpace::new();
    space.reserve(base, len, read_write()).expect("reserve");
    // 256 runs of one page, 4 MiB apart: as the copies' runs lie, across the reservation.
    let runs: Vec<(u64, u64)> = (0..256).map(|i| (base + i * 0x40_0000, 0x1000)).collect();
    let time = |what: &str| {
        for &(at, _) in &runs {
            write(at, 1);
        }
        let started = std::time::Instant::now();
        for &(at, run) in &runs {
            let _ = orbistoun_mem::watch::mark(at, run);
        }
        let each = started.elapsed();
        for &(at, _) in &runs {
            write(at, 2);
        }
        let started = std::time::Instant::now();
        let _ = orbistoun_mem::watch::mark(base, len);
        let whole = started.elapsed();
        for &(at, _) in &runs {
            write(at, 3);
        }
        let started = std::time::Instant::now();
        let since = orbistoun_mem::watch::mark_all(&runs).expect("watched");
        let together = started.elapsed();
        write(runs[7].0, 4);
        assert!(orbistoun_mem::watch::harvest_all(&runs));
        assert!(orbistoun_mem::watch::recorded_since(
            runs[7].0, 0x1000, since
        ));
        assert!(!orbistoun_mem::watch::recorded_since(
            runs[8].0, 0x1000, since
        ));
        println!(
            "D974 write-watch, {what}: {} runs marked one by one {each:?}, {:?} each; the range holding them marked once {whole:?}; the runs marked together {together:?}",
            runs.len(),
            each / u32::try_from(runs.len()).unwrap_or(1)
        );
    };
    time("not imported");
    orbistoun_mem::lazy::commit(base, len);
    // SAFETY: the reservation is committed whole just above and outlives the import, dropped first.
    let import = unsafe { Imported::import(base, len) }.expect("import");
    time("imported");
    drop(import);
    assert!(space.release(base, len), "released");
}

/// What one device-addressed access costs on this device (D964): the difference between dispatches
/// of a long and a short dependent chain of accesses, over the accesses between them. Two figures
/// each: the chain's step, the time a lane waits on one access, which a hierarchy walk pays per
/// node; and the device's throughput, the time over every lane's access. Both ways a dispatch runs,
/// loads alone and loads each beside a store. Run with `ORBISTOUN_DEVICE_BENCH` set; it prints the
/// figures and checks only that the accesses landed.
#[test]
fn device_addressed_access_cost() {
    if std::env::var_os("ORBISTOUN_DEVICE_BENCH").is_none() {
        return;
    }
    let Some(_) = direct_or_skip("device_addressed_access_cost") else {
        return;
    };
    let (short, long) = (4_u32, 36_u32);
    // The chain's reach, in whole test bases taken in a row; a run with other tests taking bases
    // between them, or whose bases' low halves would carry, measures nothing.
    let reach = u64::from(long + 1) * u64::from(BENCH_STEP_BYTES);
    let base = BASES.take();
    let mut reserved = 0x100_0000;
    while reserved < reach {
        if BASES.take() != base + reserved {
            println!("the benchmark's test bases are not consecutive: run it alone");
            return;
        }
        reserved += 0x100_0000;
    }
    if (base & 0xFFFF_FFFF) + reserved > 1 << 32 {
        println!(
            "the benchmark's reservation crosses a 4 GiB line its 32-bit adds would carry over"
        );
        return;
    }
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, reserved, read_write())
        .expect("reserve");
    for copying in [false, true] {
        direct::set_copying(copying);
        for (model, per_lane) in [("whole wave", false), ("per lane", true)] {
            for storing in [false, true] {
                // A module of its own for each round, so each records what it touches anew.
                let time = |accesses| {
                    let mut program = chained_program(accesses, storing);
                    program.insert(0, v_mov_scalar(7, u32::from(copying)));
                    let module = device_addressed(&program, per_lane);
                    least_dispatch_time(&module, base, 5)
                };
                let (brief, lengthy) = (time(short), time(long));
                let steps = f64::from(long - short);
                let step = lengthy.saturating_sub(brief).as_secs_f64() * 1e9 / steps;
                let lanes = f64::from(BENCH_GROUPS * 64);
                println!(
                    concat!(
                        "D964 {model}, {}{}: {short} accesses {brief:?}, {long} {lengthy:?}; ",
                        "{step:.0} ns a chained step, {:.2} ns an access over every lane",
                    ),
                    if storing { "load + store" } else { "load" },
                    if copying { ", copied (D974)" } else { "" },
                    step / lanes,
                    model = model,
                    short = short,
                    brief = brief,
                    long = long,
                    lengthy = lengthy,
                    step = step,
                );
                if storing {
                    assert_eq!(read(base + 8), 0, "{model}: the stored word, zero, landed");
                }
            }
        }
        if copying {
            println!(
                "D974: {} MiB of guest memory copied",
                direct::copied_bytes() >> 20
            );
        }
    }
    direct::set_copying(true);
    assert!(space.release(base, reserved), "released");
}

/// A device-addressed module forms pointers from device addresses. With
/// `ORBISTOUN_WRITE_DEVICE_MODULE` set to a path, the module is written there for `spirv-val`.
#[test]
fn a_device_addressed_dispatch_forms_device_pointers() {
    let module = device_addressed(&flat_program(), false);
    // `OpConvertUToPtr`, by its opcode in an instruction's first word.
    let forms_pointers = module.iter().skip(5).any(|word| word & 0xffff == 120);
    assert!(forms_pointers, "the module reads through device addresses");
    assert!(
        direct::addresses_directly(&module),
        "told by its capability"
    );
    assert!(
        !direct::addresses_directly(
            &direct_address_module(&[])
                .into_iter()
                .take(5)
                .collect::<Vec<_>>()
        ),
        "a header alone declares nothing"
    );
    if let Some(path) = std::env::var_os("ORBISTOUN_WRITE_DEVICE_MODULE") {
        let bytes: Vec<u8> = module.iter().flat_map(|w| w.to_le_bytes()).collect();
        std::fs::write(path, bytes).expect("written");
    }
}

/// How many table slots `module` loads: a slot is one load of sixteen bytes, aligned so
/// (`OpLoad` type, result, pointer, `Aligned`, 16), and nothing else a module loads is.
fn slot_loads(module: &[u32]) -> usize {
    let mut at = 5;
    let mut count = 0;
    while at < module.len() {
        let length = (module[at] >> 16) as usize;
        if module[at] & 0xffff == 61
            && length == 6
            && module[at + 4] == orbistoun_spirv::MEMORY_ALIGNED
            && module[at + 5] == 16
        {
            count += 1;
        }
        at += length.max(1);
    }
    count
}

/// How many `OpFunctionCall`s `module` makes.
fn calls(module: &[u32]) -> usize {
    let mut at = 5;
    let mut count = 0;
    while at < module.len() {
        let length = (module[at] >> 16) as usize;
        count += usize::from(module[at] & 0xffff == 57);
        at += length.max(1);
    }
    count
}

/// An access's words share their lookups, and every lookup is one helper function (D969): a
/// four-word load and a four-word store each call it twice, at their first word and their last,
/// whose chunks hold every word between, where a lookup for each word would call it four times as
/// often; and the module reads table slots only in the helper, `PROBES` of them. One invocation
/// per lane, so the module holds each access once.
#[test]
fn an_access_s_words_share_two_lookups() {
    // v_mov_b32 v0, s0 ; v_mov_b32 v1, s1
    // global_load_dwordx4 v[2:5], v[0:1], off
    // global_store_dwordx4 v[0:1], v[2:5], off offset:16
    // s_endpgm
    let mut program = vec![v_mov_scalar(0, 0), v_mov_scalar(1, 1)];
    program.extend([
        head("global_load_dwordx4") & !(3 << 14) | GLOBAL << 14,
        (OFF << 16) | (2 << 24),
    ]);
    program.extend([
        head("global_store_dwordx4") & !(3 << 14) | GLOBAL << 14 | 16,
        (2 << 8) | (OFF << 16),
    ]);
    program.push(head("s_endpgm"));
    let module = device_addressed(&program, true);
    let probes = orbistoun_translate::device_address::PROBES as usize;
    assert_eq!(calls(&module), 2 * 2, "two lookups an access");
    assert_eq!(
        slot_loads(&module),
        probes,
        "the slots read once, in the helper"
    );
}

/// Every word of a multi-word device-addressed access lands (D969): a four-word global load, its
/// words stored back by a four-word global store, and a four-word scalar load's words stored the
/// same way, both ways a dispatch runs.
///
/// ```text
/// The scalar load's base is a pointer the program reads from guest memory, so the host cannot
/// resolve it and binds no buffer for it: the load goes by device address too. (A base from the
/// user data the host resolves and binds, which this test's dispatch does not.)
///
/// ```text
/// v_mov_b32 v0, s0 ; v_mov_b32 v1, s1
/// global_load_dwordx4 v[2:5], v[0:1], off
/// global_store_dwordx4 v[0:1], v[2:5], off offset:16
/// global_load_dwordx2 v[10:11], v[0:1], off offset:48     the pointer stored there, A
/// v_readfirstlane_b32 s8, v10 ; v_readfirstlane_b32 s9, v11
/// s_load_dwordx4 s[4:7], s[8:9], 0x0
/// v_mov_b32 v6, s4 ; v_mov_b32 v7, s5 ; v_mov_b32 v8, s6 ; v_mov_b32 v9, s7
/// global_store_dwordx4 v[0:1], v[6:9], off offset:32
/// s_endpgm
/// ```
#[test]
fn every_word_of_a_wide_access_lands() {
    let Some(_) = direct_or_skip("every_word_of_a_wide_access_lands") else {
        return;
    };
    let mut program = vec![v_mov_scalar(0, 0), v_mov_scalar(1, 1)];
    program.extend([
        head("global_load_dwordx4") & !(3 << 14) | GLOBAL << 14,
        (OFF << 16) | (2 << 24),
    ]);
    program.extend([
        head("global_store_dwordx4") & !(3 << 14) | GLOBAL << 14 | 16,
        (2 << 8) | (OFF << 16),
    ]);
    program.extend([
        head("global_load_dwordx2") & !(3 << 14) | GLOBAL << 14 | 0x30,
        (OFF << 16) | (10 << 24),
    ]);
    // v_readfirstlane_b32 sN, vM: its scalar destination in the vector destination's field.
    program.push(short_form("v_readfirstlane_b32", 8, 256 + 10, 0));
    program.push(short_form("v_readfirstlane_b32", 9, 256 + 11, 0));
    // s_load_dwordx4 s[4:7], s[8:9], 0: SDATA bits 12:6, SBASE (a pair's index) bits 5:0, and
    // no offset register (`SOFFSET` 0x7d, bits 31:25).
    program.extend([head("s_load_dwordx4") | (4 << 6) | (8 >> 1), 0x7d << 25]);
    program.extend((0..4).map(|word| v_mov_scalar(6 + word, 4 + word)));
    program.extend([
        head("global_store_dwordx4") & !(3 << 14) | GLOBAL << 14 | 32,
        (6 << 8) | (OFF << 16),
    ]);
    program.push(head("s_endpgm"));
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, read_write())
        .expect("reserve");
    let address = base + 0x3000;
    for (model, per_lane) in [("whole wave", false), ("per lane", true)] {
        let module = device_addressed(&program, per_lane);
        for word in 0..4u64 {
            write(
                address + 4 * word,
                0x100 + u32::try_from(word).expect("small"),
            );
            write(address + 16 + 4 * word, 0x5555_5555);
            write(address + 32 + 4 * word, 0x5555_5555);
        }
        #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
        let halves = [address as u32, (address >> 32) as u32];
        write(address + 48, halves[0]);
        write(address + 52, halves[1]);
        let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
        push[..2].copy_from_slice(&halves);
        dispatch_guest(
            &module,
            &[0; 64],
            (&push, [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        let words = |at: u64| [0, 4, 8, 12].map(|step| read(at + step));
        assert_eq!(
            words(address + 16),
            [0x100, 0x101, 0x102, 0x103],
            "{model}: a global load's words, stored back"
        );
        assert_eq!(
            words(address + 32),
            [0x100, 0x101, 0x102, 0x103],
            "{model}: a scalar load's words, stored"
        );
    }
    assert!(space.release(base, RESERVED), "released");
}

/// A device-addressed module reads and writes the guest's words with `Volatile` loads and stores:
/// run 336 of Grand Theft Auto V lost the device to a single-pass scan whose look-back loop polls
/// the previous group's flag, a read the driver kept out of the loop.
#[test]
fn a_device_addressed_dispatch_reads_guest_words_each_time() {
    let module = device_addressed(&flat_program(), false);
    // (opcode, where the memory operands sit): `OpLoad` type, result, pointer, operands;
    // `OpStore` pointer, object, operands.
    let volatile = |opcode: u32, operands: usize| {
        let mut at = 5;
        let mut count = 0;
        while at < module.len() {
            let length = (module[at] >> 16) as usize;
            if module[at] & 0xffff == opcode
                && length > operands
                && module[at + operands] & orbistoun_spirv::MEMORY_VOLATILE != 0
            {
                count += 1;
            }
            at += length.max(1);
        }
        count
    };
    assert!(volatile(61, 4) > 0, "a guest word loaded each time");
    assert!(volatile(62, 3) > 0, "and stored each time");
}

/// The formats the host would read from each bound buffer's descriptor, stood in for: each slot a
/// format access goes through, as many 32-bit floats as the widest such access moves, `X Y Z W` in
/// order (`OOB_SELECT` 3). One format for every slot refuses whichever access is wider than it, or a
/// store narrower; a descriptor the program computes converts by its own as the module runs (D967).
fn placeholder_formats(
    decoded: &orbistoun_shader::Decode,
    encodings: &EncodingTable,
    user_data: &UserData,
) -> orbistoun_translate::wavefront::BufferFormats {
    let mut formats = orbistoun_translate::wavefront::BufferFormats::default();
    let Ok(traced) = orbistoun_translate::wavefront::draw_buffers_for(
        decoded,
        encodings,
        Stage::Compute,
        *user_data,
    ) else {
        return formats;
    };
    let mut widest: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    for instruction in &decoded.instructions {
        let name = instruction
            .encoding
            .and_then(|index| encodings.encodings().get(usize::from(index)))
            .and_then(|family| encodings.mnemonic_for(&family.name, instruction.opcode));
        if let (Some((_, channels)), Some(&slot)) = (
            name.and_then(|name| name.rsplit_once("_format_")),
            traced.served.get(&instruction.offset),
        ) {
            let channels = u32::try_from(channels.len()).unwrap_or(4);
            let entry = widest.entry(slot).or_insert(channels);
            *entry = (*entry).max(channels);
        }
    }
    for (slot, channels) in widest {
        // `32_FLOAT`, `32_32_FLOAT`, `32_32_32_FLOAT`, `32_32_32_32_FLOAT`.
        let code = [22, 64, 74, 77][channels.clamp(1, 4) as usize - 1];
        formats.set(slot as usize, Some(0x3100_0fac | code << 12));
    }
    formats
}

/// Guest programs read from files translated device-addressed, to see whether each translates and
/// why not: `ORBISTOUN_DEVICE_PROGRAMS` lists `path|user words|group id components|thread id
/// components|scratch bytes a wave[|program address in hex]` entries separated by `;`, each file
/// the program's bytes from its first instruction; the address, where given, is where it was
/// loaded, which a program reading its own address needs (D842). Nothing runs without it.
#[test]
fn listed_programs_translate_device_addressed() {
    let Ok(listed) = std::env::var("ORBISTOUN_DEVICE_PROGRAMS") else {
        return;
    };
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let mut failed = Vec::new();
    for entry in listed.split(';').filter(|entry| !entry.is_empty()) {
        let fields: Vec<&str> = entry.split('|').collect();
        let (path, count, ids, components, scratch, loaded_at) = match fields[..] {
            [path, count, ids, components, scratch] => {
                (path, count, ids, components, scratch, None)
            }
            [path, count, ids, components, scratch, at] => (
                path,
                count,
                ids,
                components,
                scratch,
                Some(u64::from_str_radix(at.trim_start_matches("0x"), 16).expect("an address")),
            ),
            _ => panic!("an entry is path|user words|ids|components|scratch[|address]: {entry}"),
        };
        let number = |text: &str| text.parse::<u32>().expect("a number");
        let bytes = std::fs::read(path).expect("the program's bytes");
        let decoded = orbistoun_shader::decode_program(&bytes, &encodings, &operands);
        let ids = number(ids);
        let user_data = UserData {
            count: number(count),
            compute: Some(ComputeInputs {
                workgroup_ids: [ids > 0, ids > 1, ids > 2],
                thread_id_components: number(components),
                threads: [64, 1, 1],
                scratch_wave_bytes: number(scratch),
                device_addressed: true,
                ..ComputeInputs::default()
            }),
            local_words: Some(16384),
            draw_buffers: true,
            dx10_clamp: Some(true),
            program_address: loaded_at,
            ..UserData::default()
        };
        let formats = placeholder_formats(&decoded, &encodings, &user_data);
        let translated = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::spanning_address(WINDOW, 64).expect("a window"),
            UserData {
                buffer_formats: Some(formats),
                ..user_data
            },
        );
        match translated {
            Ok((module, _, _)) => {
                println!(
                    "{path}: {} instructions, {} words",
                    decoded.instructions.len(),
                    module.len()
                );
                if let Some(directory) = std::env::var_os("ORBISTOUN_WRITE_DEVICE_MODULES") {
                    let name = std::path::Path::new(path).file_stem().expect("a name");
                    let mut out = std::path::PathBuf::from(directory);
                    out.push(name);
                    out.set_extension("spv");
                    let bytes: Vec<u8> = module.iter().flat_map(|w| w.to_le_bytes()).collect();
                    std::fs::write(out, bytes).expect("written");
                }
            }
            Err(why) => {
                println!("{path}: refused: {why}");
                failed.push(path);
            }
        }
    }
    assert!(failed.is_empty(), "refused: {failed:?}");
}

/// A device-address table built over one guest range finds its bytes at the import's device
/// addresses, as the module's lookup will.
#[test]
fn a_table_over_an_import_finds_its_device_addresses() {
    let Some(_) = direct_or_skip("a_table_over_an_import_finds_its_device_addresses") else {
        return;
    };
    let base = BASES.take();
    let held = orbistoun_mem::platform::reserve(base, RESERVED, read_write()).expect("reserve");
    // SAFETY: the reservation is committed whole and outlives the import, dropped first.
    let import = unsafe { Imported::import(base, RESERVED) }.expect("import");
    let table = Table::build(&[Mapped {
        guest: base,
        len: RESERVED,
        device: import.device_address(),
    }])
    .expect("a table");
    for at in [base, base + 0x1234, base + RESERVED - 4] {
        assert_eq!(table.lookup(at), import.device_address_of(at));
    }
    assert_eq!(table.lookup(base + RESERVED), None);
    drop(import);
    drop(held);
}

/// A chunk a recording dispatch only read is copied into device memory, and its copy stays the
/// guest's (D974): the next dispatches reach it there, read what the CPU wrote between them, and
/// leave what they stored in the guest's pages when they return, every time.
#[test]
fn a_copied_chunk_stays_coherent_with_its_home_pages() {
    let Some(_) = direct_or_skip("a_copied_chunk_stays_coherent_with_its_home_pages") else {
        return;
    };
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, read_write())
        .expect("reserve");
    direct::set_copying(true);
    let address = base + 0x1000;
    let module = device_addressed(&flat_program(), true);
    let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    push[..2].copy_from_slice(&[address as u32, (address >> 32) as u32]);
    // A program that only reads the chunk records it, and it is copied: a chunk a recording
    // dispatch stores into is not.
    let reader = device_addressed(&chained_program(1, false), true);
    dispatch_guest(
        &reader,
        &[0; 64],
        (&push, [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("dispatched");
    assert!(direct::copied_bytes() > 0, "the chunk it read is copied");
    for (round, source) in [5_u32, 70, 900, 12_345].into_iter().enumerate() {
        write(address + SOURCE, source);
        for at in [THROUGH_SHARE, CONSTANT, PLUS_ONE] {
            write(address + at, 0x5555_5555);
        }
        dispatch_guest(
            &module,
            &[0; 64],
            (&push, [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert_eq!(
            read(address + PLUS_ONE),
            source + 1,
            "round {round}: read, stored"
        );
        assert_eq!(read(address + THROUGH_SHARE), source, "round {round}");
        assert_eq!(read(address + CONSTANT), 0x00c0_ffee, "round {round}");
    }
    assert!(space.release(base, RESERVED), "released");
}

/// Run on the device over a guest reservation the run's address space made, which the device maps
/// whole, the program reads the guest's word where it lies, stores a constant beside it, takes the
/// word through the local data share and back by flat addressing, reads it again by a flat access
/// to guest memory and stores it plus one, and reads zero from an address nothing maps - which the
/// dispatch reports as an escape. The chunk its stores reached comes back as written, for the
/// caches the write-watch cannot tell. Both ways a dispatch runs: the whole wave in one invocation,
/// and one invocation per lane.
#[test]
fn a_device_addressed_dispatch_reaches_guest_memory_and_the_share() {
    let Some(_) = direct_or_skip("a_device_addressed_dispatch_reaches_guest_memory_and_the_share")
    else {
        return;
    };
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, read_write())
        .expect("reserve");
    let address = base + 0x1000;
    for (model, per_lane) in [("whole wave", false), ("per lane", true)] {
        write(address + SOURCE, 0x0bad_f00d);
        for at in [THROUGH_SHARE, CONSTANT, PLUS_ONE, UNMAPPED] {
            write(address + at, 0x5555_5555);
        }
        let module = device_addressed(&flat_program(), per_lane);
        let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
        #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
        push[..2].copy_from_slice(&[address as u32, (address >> 32) as u32]);
        let _ = orbistoun_gpu::device_addressed::take_written();
        let done = dispatch_guest(
            &module,
            &[0; 64],
            (&push, [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        assert_eq!(
            read(address + CONSTANT),
            0x00c0_ffee,
            "{model}: a global store"
        );
        assert_eq!(
            read(address + THROUGH_SHARE),
            0x0bad_f00d,
            "{model}: through the share by flat addressing"
        );
        assert_eq!(
            read(address + PLUS_ONE),
            0x0bad_f00e,
            "{model}: a flat access to guest memory"
        );
        assert_eq!(read(address + UNMAPPED), 0, "{model}: nothing maps it");
        assert!(done.escaped, "{model}: an unmapped read is an escape");
        let written = orbistoun_gpu::device_addressed::take_written();
        assert!(
            written
                .iter()
                .any(|&(first, len)| first <= address && address + PLUS_ONE < first + len),
            "{model}: the stores' chunk comes back written: {written:x?}"
        );
    }
    assert!(
        space.release(base, RESERVED),
        "released, its mapping dropped first"
    );
}

/// A program whose lanes diverge as a wave's do on the hardware, its words as `llvm-mc` assembles
/// them for this target (wave of sixty-four):
///
/// ```text
///   v_and_b32 v1, 7, v0                 n = lane & 7
///   v_mov_b32 v2, 0
///   s_mov_b64 s[4:5], exec
/// loop:                                 a loop each lane leaves after its own n passes
///   v_cmp_ne_u32 vcc, 0, v1
///   s_and_b64 exec, exec, vcc
///   s_cbranch_execz loop_end
///   v_add_nc_u32 v2, 3, v2
///   v_add_nc_u32 v1, -1, v1
///   s_branch loop
/// loop_end:
///   s_mov_b64 exec, s[4:5]
///   v_and_b32 v3, 1, v0                 an if and an else on the lane's own bit
///   v_cmp_eq_u32 vcc, 0, v3
///   s_and_saveexec_b64 s[6:7], vcc
///   s_cbranch_execz else
///   v_or_b32 v2, 0x100, v2
/// else:
///   s_andn2_b64 exec, s[6:7], exec
///   s_cbranch_execz join
///   v_or_b32 v2, 0x200, v2
/// join:
///   s_mov_b64 exec, s[6:7]
///   s_mov_b32 s10, 0                    a block no lane enters, skipped with its scalar work
///   v_cmp_lt_u32 vcc, 63, v0
///   s_and_saveexec_b64 s[8:9], vcc
///   s_cbranch_execz none
///   s_movk_i32 s10, 0x1000
///   v_or_b32 v2, 0x400, v2
/// none:
///   s_mov_b64 exec, s[8:9]
///   s_mov_b32 s11, 0                    a branch on the condition mask one lane sets
///   v_cmp_eq_u32 vcc, 5, v0
///   s_cbranch_vccz past
///   s_movk_i32 s11, 0x800
/// past:
///   v_or_b32 v2, s10, v2
///   v_or_b32 v2, s11, v2
///   v_lshlrev_b32 v4, 2, v0
///   global_store_dword v4, v2, s[0:1]   each lane's word at the guest address plus 4 * lane
///   s_endpgm
/// ```
const DIVERGENT_PROGRAM: [u32; 39] = [
    0x3602_0087,
    0x7e04_0280,
    0xbe84_047e,
    0x7d8a_0280,
    0x87fe_6a7e,
    0xbf88_0003,
    0x4a04_0483,
    0x4a02_02c1,
    0xbf82_fffa,
    0xbefe_0404,
    0x3606_0081,
    0x7d84_0680,
    0xbe86_246a,
    0xbf88_0002,
    0x3804_04ff,
    0x0000_0100,
    0x8afe_7e06,
    0xbf88_0002,
    0x3804_04ff,
    0x0000_0200,
    0xbefe_0406,
    0xbe8a_0380,
    0x7d82_00bf,
    0xbe88_246a,
    0xbf88_0003,
    0xb00a_1000,
    0x3804_04ff,
    0x0000_0400,
    0xbefe_0408,
    0xbe8b_0380,
    0x7d84_0085,
    0xbf86_0001,
    0xb00b_0800,
    0x3804_040a,
    0x3804_040b,
    0x3408_0082,
    0xdc70_8000,
    0x0000_0204,
    0xbf81_0000,
];

/// What lane `lane` of [`DIVERGENT_PROGRAM`] stores, as the hardware runs it: three for each pass
/// of its own loop, its if or its else, nothing of the block no lane enters, and the vcc branch's
/// word, since one lane set the mask.
fn divergent_lane(lane: u32) -> u32 {
    let parity = if lane % 2 == 0 { 0x100 } else { 0x200 };
    (3 * (lane & 7)) | parity | 0x800
}

/// A device-addressed dispatch needs nothing of its control flow from the host (D964, D970): its
/// module carries the wave's branches on the execution mask, on the condition mask and on the
/// scalar condition, and a loop whose lanes leave it after different counts, as the hardware runs
/// them. Every lane's word is the hardware's, both ways a dispatch runs.
#[test]
fn a_device_addressed_dispatch_diverges_per_lane() {
    let Some(_) = direct_or_skip("a_device_addressed_dispatch_diverges_per_lane") else {
        return;
    };
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, read_write())
        .expect("reserve");
    let address = base + 0x2000;
    for (model, per_lane) in [("whole wave", false), ("per lane", true)] {
        for lane in 0..64 {
            write(address + 4 * lane, 0x5555_5555);
        }
        let module = device_addressed(&DIVERGENT_PROGRAM, per_lane);
        let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
        #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
        push[..2].copy_from_slice(&[address as u32, (address >> 32) as u32]);
        let _ = orbistoun_gpu::device_addressed::take_written();
        dispatch_guest(
            &module,
            &[0; 64],
            (&push, [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        let stored: Vec<u32> = (0..64).map(|lane| read(address + 4 * lane)).collect();
        let wanted: Vec<u32> = (0..64).map(divergent_lane).collect();
        assert_eq!(
            stored, wanted,
            "{model}: each lane's word as the hardware's"
        );
    }
    assert!(
        space.release(base, RESERVED),
        "released, its mapping dropped first"
    );
}

/// A format load through a descriptor the program reads from an address it computes (D967):
///
/// ```text
/// v_mov_b32 v1, s0 ; v_readfirstlane_b32 s10, v1 ; s_mov_b32 s11, s1   the block's address B,
///                                                                        which the host cannot know
/// s_load_dwordx4 s[4:7], s[10:11], 0x0                                  the descriptor at B
/// buffer_load_format_xyzw v[4:7], v0, s[4:7], 0 idxen                   the lane's element
/// v_lshlrev_b32 v8, 4, v0 ; v_add_nc_u32 v8, 0x400, v8
/// global_store_dwordx4 v8, v[4:7], s[0:1]                               at B + 0x400 + lane * 16
/// s_endpgm
/// ```
fn format_program() -> Vec<u32> {
    let mut words = vec![
        v_mov_scalar(1, 0),
        head("v_readfirstlane_b32") | (10 << 17) | (256 + 1),
        head("s_mov_b32") | (11 << 16) | 1,
    ];
    // SMEM: the destination in bits 12:6, the base pair's first register halved in 5:0; the offset
    // in the second word, `SOFFSET` null.
    words.extend([head("s_load_dwordx4") | (4 << 6) | (10 >> 1), 0xfa00_0000]);
    // MUBUF `idxen`: the address register, data, descriptor's register over four and the inline
    // constant zero as the scalar offset in the second word.
    words.extend([head("buffer_load_format_xyzw") | 1 << 13, 0x8001_0400]);
    // v_lshlrev_b32 v8, 4, v0: the inline constant 4 is source code 132, 0x84.
    words.push(head("v_lshlrev_b32_e32") | (8 << 17) | 0x84);
    // v_add_nc_u32 v8, 0x400, v8: a literal, source code 255.
    words.extend([head("v_add_nc_u32_e32") | (8 << 17) | (8 << 9) | 255, 0x400]);
    // global_store_dwordx4 v8, v[4:7], s[0:1]: `SADDR` s0, a 32-bit offset in v8.
    words.extend([
        head("global_store_dwordx4") & !(3 << 14) | GLOBAL << 14,
        8 | (4 << 8),
    ]);
    words.push(head("s_endpgm"));
    words
}

/// A buffer descriptor's words: `base`, `stride`, `records`, the four selects `x y z w`, the
/// format code, the out-of-bounds mode, and where given the index stride's code, with swizzling and
/// adding the lane's id (`gfx10-rsrc.json`).
fn format_descriptor(
    (base, stride, records): (u64, u32, u32),
    selects: [u32; 4],
    (format, oob): (u32, u32),
    swizzle: Option<u32>,
) -> [u32; 4] {
    let (swizzled, index_stride, adds_lane) = match swizzle {
        Some(code) => (1, code, 1),
        None => (0, 0, 0),
    };
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    [
        base as u32,
        (base >> 32) as u32 & 0xffff | stride << 16 | swizzled << 31,
        records,
        selects[0]
            | selects[1] << 3
            | selects[2] << 6
            | selects[3] << 9
            | format << 12
            | index_stride << 21
            | adds_lane << 23
            | 1 << 24
            | oob << 28,
    ]
}

/// What a lane of a [`FormatCase`] writes.
type Expected = fn(u32) -> [u32; 4];

/// One case of [`a_format_load_decodes_its_descriptor_as_it_runs`]: the descriptor's words, the
/// data words it reads, each lane's four words, and whether the dispatch escapes.
struct FormatCase {
    name: &'static str,
    descriptor: [u32; 4],
    data: Vec<u32>,
    expected: Expected,
    escapes: bool,
}

/// An IEEE half's value as a single's bits, for the normal halves and zero the cases use.
fn half_bits(half: u32) -> u32 {
    let sign = (half & 0x8000) << 16;
    if half.trailing_zeros() >= 15 {
        return sign;
    }
    let exponent = (half >> 10 & 0x1f) + 127 - 15;
    sign | exponent << 23 | (half & 0x3ff) << 13
}

/// `1.0`'s bits.
const ONE: u32 = 0x3f80_0000;

/// `value / 255` as an eight-bit normalised component reads.
#[expect(clippy::cast_precision_loss, reason = "a byte")]
fn unorm8(value: u32) -> u32 {
    (value as f32 / 255.0).to_bits()
}

/// `lane + 0.5`.
#[expect(clippy::cast_precision_loss, reason = "a lane")]
fn half_past(lane: u32) -> u32 {
    (lane as f32 + 0.5).to_bits()
}

/// The cases, each one's data `data_at` its number.
fn format_cases(data_at: impl Fn(u64) -> u64) -> Vec<FormatCase> {
    vec![
        FormatCase {
            name: "32_FLOAT, x 0 0 1, sixty records by index",
            descriptor: format_descriptor((data_at(0), 4, 60), [4, 0, 0, 1], (22, 1), None),
            data: (0..64).map(half_past).collect(),
            expected: |lane| [if lane < 60 { half_past(lane) } else { 0 }, 0, 0, ONE],
            escapes: false,
        },
        FormatCase {
            name: "8_8_8_8_UNORM, z y x w",
            descriptor: format_descriptor((data_at(1), 4, 64), [6, 5, 4, 7], (56, 1), None),
            data: (0..64u32)
                .map(|lane| lane | 0xff << 8 | (255 - lane) << 16 | 51 << 24)
                .collect(),
            expected: |lane| [unorm8(255 - lane), unorm8(255), unorm8(lane), unorm8(51)],
            escapes: false,
        },
        FormatCase {
            name: "16_16_SINT, x y 1 0",
            descriptor: format_descriptor((data_at(2), 4, 64), [4, 5, 1, 0], (28, 1), None),
            data: (0..64u32)
                .map(|lane| (lane.wrapping_neg() & 0xffff) | (lane * 3) << 16)
                .collect(),
            expected: |lane| [lane.wrapping_neg(), lane * 3, 1, 0],
            escapes: false,
        },
        FormatCase {
            name: "16_16_16_16_FLOAT",
            descriptor: format_descriptor((data_at(3), 8, 64), [4, 5, 6, 7], (71, 1), None),
            data: (0..64).flat_map(|_| [0xc100_3c00, 0x0000_3800]).collect(),
            expected: |_| [half_bits(0x3c00), half_bits(0xc100), half_bits(0x3800), 0],
            escapes: false,
        },
        FormatCase {
            // An index stride of sixteen and a stride of eight: element `i` lies at
            // `(i / 16) * 128 + (i % 16) * 4`, and `i` is the lane's index plus its id.
            name: "32_UINT swizzled, adding the lane's id",
            descriptor: format_descriptor((data_at(4), 8, 128), [4, 0, 0, 1], (20, 1), Some(1)),
            data: (0..256u32).map(|word| 0x1000 + word).collect(),
            expected: |lane| {
                let index = 2 * lane;
                let byte = (index / 16) * 128 + (index % 16) * 4;
                [0x1000 + byte / 4, 0, 0, 1]
            },
            escapes: false,
        },
        FormatCase {
            name: "a format code with no meaning",
            descriptor: format_descriptor((data_at(5), 4, 64), [4, 0, 0, 1], (0, 1), None),
            data: vec![0x1234_5678; 64],
            expected: |_| [0, 0, 0, ONE],
            escapes: true,
        },
    ]
}

/// Run on the device over guest memory a run reserves, a format load through a descriptor the
/// program reads from an address it computes converts by the format that descriptor names, decoded
/// as the module runs (D967): a 32-bit float bounded by its records, eight-bit normalised channels
/// with their selects reordered, sixteen-bit signed integers with an integer one, halves, a
/// swizzled buffer that adds each lane's id to its index, and a format code with no meaning, which
/// reads zero and is an escape. Both ways a dispatch runs.
#[test]
fn a_format_load_decodes_its_descriptor_as_it_runs() {
    let Some(_) = direct_or_skip("a_format_load_decodes_its_descriptor_as_it_runs") else {
        return;
    };
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, read_write())
        .expect("reserve");
    let data_at = |case: u64| base + 0x8_0000 + case * 0x1000;
    let cases = format_cases(data_at);
    for (model, per_lane) in [("whole wave", false), ("per lane", true)] {
        let module = device_addressed(&format_program(), per_lane);
        for (case, at) in cases.iter().zip(0u64..) {
            let block = base + 0x1000 + at * 0x1000;
            for (word, value) in (0u64..).zip(case.descriptor) {
                write(block + word * 4, value);
            }
            for (word, &value) in (0u64..).zip(&case.data) {
                write(data_at(at) + word * 4, value);
            }
            for word in 0..256 {
                write(block + 0x400 + word * 4, 0x5555_5555);
            }
            let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
            #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
            push[..2].copy_from_slice(&[block as u32, (block >> 32) as u32]);
            let done = dispatch_guest(
                &module,
                &[0; 64],
                (&push, [1, 1, 1]),
                &DispatchImages::default(),
            )
            .expect("dispatched");
            for lane in 0..64u32 {
                let out = block + 0x400 + u64::from(lane) * 16;
                let got = [0, 4, 8, 12].map(|step| read(out + step));
                assert_eq!(
                    got,
                    (case.expected)(lane),
                    "{model}: {}: lane {lane}",
                    case.name
                );
            }
            assert_eq!(done.escaped, case.escapes, "{model}: {}", case.name);
        }
    }
    let _ = orbistoun_gpu::device_addressed::take_written();
    assert!(
        space.release(base, RESERVED),
        "released, its mapping dropped first"
    );
}

/// Neighbouring lanes' byte stores by device address each keep the others' bytes of a word
/// (D977): lane `n` stores the byte `0x80 + n` at byte `n` of the block in user data words 0 and
/// 1, four lanes to a word; and every lane the half `0xcafe` at byte `0x43`, across words 16 and
/// 17. Both ways a dispatch runs.
///
/// ```text
/// v_add_nc_u32_e32 v1, 0x80, v0
/// global_store_byte v0, v1, s[0:1]
/// v_mov_b32 v2, 0x43 ; v_mov_b32 v3, 0xcafe
/// global_store_short v2, v3, s[0:1]
/// s_endpgm
/// ```
#[test]
fn neighbouring_lanes_store_bytes_into_one_guest_word() {
    let Some(_) = direct_or_skip("neighbouring_lanes_store_bytes_into_one_guest_word") else {
        return;
    };
    let mut program = vec![head("v_add_nc_u32_e32") | (1 << 17) | 255, 0x80];
    // The global segment, `SADDR` s0: a 32-bit offset in the address register.
    let store = |name: &str, vaddr: u32, vdata: u32| {
        [head(name) & !(3 << 14) | GLOBAL << 14, vaddr | (vdata << 8)]
    };
    program.extend(store("global_store_byte", 0, 1));
    program.extend(v_mov_literal(2, 0x43));
    program.extend(v_mov_literal(3, 0xcafe));
    program.extend(store("global_store_short", 2, 3));
    program.push(head("s_endpgm"));
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, read_write())
        .expect("reserve");
    let block = base + 0x1000;
    for (model, per_lane) in [("whole wave", false), ("per lane", true)] {
        for word in 0..32 {
            write(block + word * 4, 0);
        }
        let module = device_addressed(&program, per_lane);
        let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
        #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
        push[..2].copy_from_slice(&[block as u32, (block >> 32) as u32]);
        let done = dispatch_guest(
            &module,
            &[0; 64],
            (&push, [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        for word in 0..16u32 {
            let byte = |n: u32| (0x80 + 4 * word + n) << (8 * n);
            let wanted = byte(0) | byte(1) | byte(2) | byte(3);
            let at = block + u64::from(word) * 4;
            assert_eq!(read(at), wanted, "{model}: word {word}");
        }
        let (low, high) = (read(block + 64), read(block + 68));
        assert_eq!(low, 0xfe00_0000, "{model}: the half's low byte");
        assert_eq!(high, 0xca, "{model}: its high byte, in the next word");
        assert!(!done.escaped, "{model}: every byte is mapped");
    }
    let _ = orbistoun_gpu::device_addressed::take_written();
    assert!(
        space.release(base, RESERVED),
        "released, its mapping dropped first"
    );
}

/// What a byte store's compare-and-swap costs by device address (D977), set beside a word store
/// over the same lanes: 4096 groups of 64 lanes, each storing at its own place - a byte
/// (`global_store_byte`), four lanes a word, or a word (`global_store_dword`) - and every lane
/// storing a byte into one of four bytes of a single word, all contending. The median of eleven
/// dispatches each, whole wave and per lane. Runs only with `ORBISTOUN_DEVICE_BENCH` set.
#[test]
fn a_byte_store_s_cost_beside_a_word_store() {
    const GROUPS: u32 = 4096;
    if std::env::var_os("ORBISTOUN_DEVICE_BENCH").is_none() {
        return;
    }
    let Some(_) = direct_or_skip("a_byte_store_s_cost_beside_a_word_store") else {
        return;
    };
    // v1 = s2 << shift + (v0 << lane_shift), or v0 & 3 where every lane contends; v2 the data.
    let program = |name: &str, (group_shift, lane_shift): (u32, u32), contended: bool| {
        let mut words = vec![v_mov_scalar(1, 2)];
        words.push(head("v_lshlrev_b32_e32") | (1 << 17) | (1 << 9) | (0x80 + group_shift));
        words.push(head("v_lshlrev_b32_e32") | (3 << 17) | (0x80 + lane_shift));
        words.push(head("v_add_nc_u32_e32") | (1 << 17) | (1 << 9) | (0x100 + 3));
        if contended {
            words.push(head("v_and_b32_e32") | (1 << 17) | (0x80 + 3));
        }
        words.extend(v_mov_literal(2, 0x5a));
        words.extend([head(name) & !(3 << 14) | GLOBAL << 14, 1 | (2 << 8)]);
        words.push(head("s_endpgm"));
        words
    };
    let cases = [
        ("word stores", program("global_store_dword", (8, 2), false)),
        ("byte stores", program("global_store_byte", (6, 0), false)),
        ("contended", program("global_store_byte", (6, 0), true)),
    ];
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, read_write())
        .expect("reserve");
    let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    push[..2].copy_from_slice(&[base as u32, (base >> 32) as u32]);
    for (model, per_lane) in [("whole wave", false), ("per lane", true)] {
        for (name, words) in &cases {
            let module = device_addressed(words, per_lane);
            let mut times: Vec<std::time::Duration> = (0..11)
                .map(|_| {
                    let started = std::time::Instant::now();
                    let done = dispatch_guest(
                        &module,
                        &[0; 64],
                        (&push, [GROUPS, 1, 1]),
                        &DispatchImages::default(),
                    )
                    .expect("dispatched");
                    assert!(!done.escaped, "{model}: {name}: nothing escapes");
                    started.elapsed()
                })
                .collect();
            times.sort();
            println!("{model}: {name}: median {:?}", times[times.len() / 2]);
        }
        let contended = read(base) & 0xff;
        assert_eq!(contended, 0x5a, "{model}: the contended word's byte");
    }
    let _ = orbistoun_gpu::device_addressed::take_written();
    assert!(
        space.release(base, RESERVED),
        "released, its mapping dropped first"
    );
}

/// Lanes storing the same byte leave one lane's value, never several merged (D977): every lane
/// stores the byte `1 << (lane % 8)` at byte 0 of the block, so a merge of two would set two bits;
/// the bytes beside it stay zero. Each lane its own invocation, storing at once, and the whole wave.
///
/// ```text
/// v_mov_b32 v1, 0 ; v_mov_b32 v4, 1 ; v_and_b32 v3, 7, v0 ; v_lshlrev_b32 v2, v3, v4
/// global_store_byte v1, v2, s[0:1]
/// s_endpgm
/// ```
#[test]
fn lanes_storing_one_byte_leave_one_lane_s_value() {
    let Some(_) = direct_or_skip("lanes_storing_one_byte_leave_one_lane_s_value") else {
        return;
    };
    let mut program = vec![
        head("v_mov_b32_e32") | (1 << 17) | 0x80,
        head("v_mov_b32_e32") | (4 << 17) | 0x81,
        head("v_and_b32_e32") | (3 << 17) | 0x87,
        head("v_lshlrev_b32_e32") | (2 << 17) | (4 << 9) | 0x103,
    ];
    program.extend([
        head("global_store_byte") & !(3 << 14) | GLOBAL << 14,
        1 | (2 << 8),
    ]);
    program.push(head("s_endpgm"));
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, read_write())
        .expect("reserve");
    let block = base + 0x1000;
    let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    push[..2].copy_from_slice(&[block as u32, (block >> 32) as u32]);
    for (model, per_lane) in [("whole wave", false), ("per lane", true)] {
        let module = device_addressed(&program, per_lane);
        for _ in 0..16 {
            write(block, 0);
            let done = dispatch_guest(
                &module,
                &[0; 64],
                (&push, [4, 1, 1]),
                &DispatchImages::default(),
            )
            .expect("dispatched");
            let word = read(block);
            let set = (word & 0xff).count_ones();
            assert_eq!(set, 1, "{model}: one lane's byte, {word:#x}");
            assert_eq!(word >> 8, 0, "{model}: the bytes beside it untouched");
            assert!(!done.escaped, "{model}: no store gave up");
        }
    }
    let _ = orbistoun_gpu::device_addressed::take_written();
    assert!(
        space.release(base, RESERVED),
        "released, its mapping dropped first"
    );
}

/// 64-bit buffer atomics through a descriptor the program reads from an address it computes, by
/// device address (D955's forms): each lane exchanges its own pair, answering what the pair held,
/// and every lane ors `1 << (lane % 32)` and `lane / 32` into one shared pair, which ends all ones
/// below and one above. The descriptor at the block `B` in user data words 0 and 1 names
/// `B + 0x800`, its records `0x400` bytes, untyped.
///
/// ```text
/// v_mov_b32 v1, s0 ; v_readfirstlane_b32 s10, v1 ; s_mov_b32 s11, s1   B, which the host cannot know
/// s_load_dwordx4 s[4:7], s[10:11], 0x0                                  the descriptor at B
/// v_lshlrev_b32 v8, 3, v0                                               the lane's pair, lane * 8
/// v_mov_b32 v2, v0 ; v_add_nc_u32 v3, 0x1000, v0
/// buffer_atomic_swap_x2 v[2:3], v8, s[4:7], 0 offen glc
/// v_mov_b32 v9, 1 ; v_lshlrev_b32 v6, v0, v9 ; v_lshrrev_b32 v7, 5, v0
/// buffer_atomic_or_x2 v[6:7], off, s[4:7], 0 offset:0x200
/// v_add_nc_u32 v10, 0xc00, v8
/// global_store_dwordx2 v10, v[2:3], s[0:1]                              what the pair held
/// s_endpgm
/// ```
#[test]
fn sixty_four_bit_buffer_atomics_land_by_device_address() {
    const GLC: u32 = 1 << 14;
    const OFFEN: u32 = 1 << 12;
    let mut program = vec![
        v_mov_scalar(1, 0),
        head("v_readfirstlane_b32") | (10 << 17) | (256 + 1),
        head("s_mov_b32") | (11 << 16) | 1,
    ];
    program.extend([head("s_load_dwordx4") | (4 << 6) | (10 >> 1), 0xfa00_0000]);
    // v_lshlrev_b32 v8, 3, v0: the inline constant 3 is source code 131.
    program.push(head("v_lshlrev_b32_e32") | (8 << 17) | 0x83);
    program.push(v_mov_scalar(2, 256));
    program.extend([head("v_add_nc_u32_e32") | (3 << 17) | 255, 0x1000]);
    // MUBUF: the address register, data, the descriptor's register over four, and the inline
    // constant zero as the scalar offset.
    program.extend([head("buffer_atomic_swap_x2") | OFFEN | GLC, 0x8001_0208]);
    // v_mov_b32 v9, 1: the inline constant 1 is source code 129.
    program.push(v_mov_scalar(9, 129));
    program.push(head("v_lshlrev_b32_e32") | (6 << 17) | (9 << 9) | 256);
    // v_lshrrev_b32 v7, 5, v0: the inline constant 5 is source code 133.
    program.push(head("v_lshrrev_b32_e32") | (7 << 17) | 0x85);
    program.extend([head("buffer_atomic_or_x2") | 0x200, 0x8001_0600]);
    program.extend([
        head("v_add_nc_u32_e32") | (10 << 17) | (8 << 9) | 255,
        0xc00,
    ]);
    program.extend([
        head("global_store_dwordx2") & !(3 << 14) | GLOBAL << 14,
        0xa | (2 << 8),
    ]);
    program.push(head("s_endpgm"));
    // Translated before any device is asked for: the translation holds everywhere.
    let modules = [false, true].map(|per_lane| device_addressed(&program, per_lane));
    let Some(_) = direct_or_skip("sixty_four_bit_buffer_atomics_land_by_device_address") else {
        return;
    };
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, read_write())
        .expect("reserve");
    let block = base + 0x1000;
    let data = block + 0x800;
    let descriptor = format_descriptor((data, 0, 0x400), [4, 5, 6, 7], (20, 3), None);
    for (model, module) in ["whole wave", "per lane"].into_iter().zip(&modules) {
        for (word, value) in (0u64..).zip(descriptor) {
            write(block + word * 4, value);
        }
        for lane in 0..64u32 {
            let pair = data + u64::from(lane) * 8;
            write(pair, 0x5555_0000 | lane);
            write(pair + 4, 0xaaaa_0000 | lane);
        }
        write(data + 0x200, 0);
        write(data + 0x204, 0);
        let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
        #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
        push[..2].copy_from_slice(&[block as u32, (block >> 32) as u32]);
        let done = dispatch_guest(
            module,
            &[0; 64],
            (&push, [1, 1, 1]),
            &DispatchImages::default(),
        )
        .expect("dispatched");
        for lane in 0..64u32 {
            let pair = data + u64::from(lane) * 8;
            assert_eq!(
                [read(pair), read(pair + 4)],
                [lane, 0x1000 + lane],
                "{model}: lane {lane}'s pair exchanged"
            );
            let held = block + 0xc00 + u64::from(lane) * 8;
            assert_eq!(
                [read(held), read(held + 4)],
                [0x5555_0000 | lane, 0xaaaa_0000 | lane],
                "{model}: lane {lane} answered what its pair held"
            );
        }
        assert_eq!(
            [read(data + 0x200), read(data + 0x204)],
            [0xffff_ffff, 1],
            "{model}: every lane's or, both words"
        );
        assert!(!done.escaped, "{model}: every pair is mapped");
    }
    let _ = orbistoun_gpu::device_addressed::take_written();
    assert!(
        space.release(base, RESERVED),
        "released, its mapping dropped first"
    );
}
