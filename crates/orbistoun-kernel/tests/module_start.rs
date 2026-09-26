//! `sceKernelLoadStartModule` reports which modules it started and which it only loaded.
//!
//! A module whose initialisers did not run looks identical to the guest to one that started,
//! so the gap is reported in the module-start summary rather than left silent (D515).

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

fn implementation(name: &str) -> GuestFn {
    orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(
            || panic!("{name} is not implemented, so no guest can reach it"),
            |(_, f)| *f,
        )
}

/// A guest string at a real address, so `read_name` reads what this test wrote.
struct Path {
    /// Never read, and required: it owns the bytes `at` points into for the duration of the call.
    _storage: Vec<u8>,
    at: u64,
}

impl Path {
    fn new(text: &str) -> Self {
        let mut storage = text.as_bytes().to_vec();
        storage.push(0);
        let at = storage.as_mut_ptr().expose_provenance() as u64;
        Self {
            _storage: storage,
            at,
        }
    }
}

/// A module handed a handle is named in the summary, with the fact that it did not start.
///
/// This asserts the report, not the behaviour: nothing here places a module. The summary is
/// process-wide, so the assertions check containment rather than the whole string.
#[test]
fn a_module_given_a_handle_is_reported_as_not_started() {
    assert_eq!(
        orbistoun_kernel::module_start_summary(),
        None,
        concat!(
            "nothing has asked for a module yet, so the run must stay quiet - a report that ",
            "fires on an empty run says nothing about a run that loaded something"
        )
    );

    let path = Path::new("/app0/Media/Modules/ATitleModule.prx");
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = path.at;
    let handle = implementation("sceKernelLoadStartModule")(&regs);
    assert!(
        (handle as i64) >= 0,
        "a title's own module gets a non-negative handle, got {handle:#x}"
    );

    let summary = orbistoun_kernel::module_start_summary()
        .expect("a module was asked for, so the run is no longer quiet");
    assert!(
        summary.contains("ATitleModule.prx"),
        concat!(
            "the report names the module, because a count alone cannot tell a reader which one ",
            "is missing its constructors: {}"
        ),
        summary
    );
    assert!(
        summary.contains(&format!("{handle:#x}")),
        "the report names the handle the guest will key its later calls on: {summary}"
    );
    assert!(
        summary.contains("NOT started"),
        concat!(
            "the report says the module did not start - the whole point, and the half a reader ",
            "would otherwise supply themselves: {}"
        ),
        summary
    );
    drop(path);

    // A module the loader recorded, with nothing to run: a test cannot manufacture executable
    // relocated guest text, so this pins the bookkeeping. A recorded module is reported as
    // started, and a start that ran nothing is called out.
    orbistoun_kernel::note_module_initialisers(
        "ARecordedModule",
        orbistoun_kernel::ModuleInitialisers {
            init: 0,
            array: 0,
            count: 0,
        },
    );
    let recorded = Path::new("/app0/Media/Modules/ARecordedModule.prx");
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = recorded.at;
    let handle = implementation("sceKernelLoadStartModule")(&regs);
    assert!((handle as i64) >= 0, "still a handle, got {handle:#x}");

    let summary = orbistoun_kernel::module_start_summary().expect("two modules asked for now");
    assert!(
        summary.contains("ARecordedModule.prx (0 initialiser(s)"),
        concat!(
            "a module the loader recorded is reported as started - matched on the library ",
            "name with the extension off, which is the comparison that was wrong first ",
            "(D515): {}"
        ),
        summary
    );
    assert!(
        summary.contains("ran NO initialiser"),
        concat!(
            "a start that ran nothing is called out - it produces the same handle as one ",
            "that ran every constructor, and only one of those is a module the guest can ",
            "use: {}"
        ),
        summary
    );
    drop(recorded);

    // The bulk start: every recorded module is visited, and a start nobody asked for is not
    // reported as though a guest supplied a handle.
    let (modules, ran) = orbistoun_kernel::start_every_placed_module();
    assert_eq!(
        modules, 1,
        "every module the loader recorded is visited, and only those"
    );
    assert_eq!(
        ran, 0,
        "this one has nothing to run, and says so rather than counting a start"
    );

    let summary = orbistoun_kernel::module_start_summary().expect("modules were started");
    assert!(
        summary.contains("before entry"),
        concat!(
            "a start nobody asked for has no handle, and inventing one would be a handle ",
            "a guest could be thought to hold: {}"
        ),
        summary
    );
}

/// The running-thread affinity setter answers success, not a placeholder.
///
/// A caller testing against zero reads a placeholder as a refusal (D523). No affinity is
/// applied, and `scePthreadGetaffinity` reads the creation-time `requested_affinity`, so the
/// mask set here does not read back.
#[test]
fn setting_a_threads_affinity_is_accepted_rather_than_refused() {
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = 0x01d1_d900_0960; // a thread handle shaped like the ones a run produces
    regs[1] = 0x1ffb; // the mask a title passes

    let answer = implementation("scePthreadSetaffinity")(&regs);
    assert_eq!(
        answer, 0,
        concat!(
            "a call orbistoun can honestly accept must not answer a placeholder - a ",
            "caller testing this against zero would read one as a refusal"
        )
    );
    assert!(
        answer & 0x8000_0000 == 0,
        "and it must not answer a vendor-shaped error either: {answer:#x}"
    );

    // `scePthreadGetaffinity` answers the creation-time `requested_affinity`, not what the
    // running-thread setter was handed; for a handle no thread was registered under it reads zero
    // ("anywhere").
    let mut mask = 0_u64;
    regs[1] = std::ptr::addr_of_mut!(mask) as u64;
    assert_eq!(
        implementation("scePthreadGetaffinity")(&regs),
        0,
        "the getter answers success, not a placeholder"
    );
    assert_eq!(
        mask, 0,
        "the running-thread set dropped (D523), so the get reads the default, not 0x1ffb"
    );
}

/// The vendor clock call fills both fields and refuses a clock it has no source for.
///
/// The refusal is a vendor-family code, never the POSIX `-1` (D525). A wall clock is not
/// reproducible, so the seconds field is checked against a date already in the past rather
/// than a value.
#[test]
fn the_vendor_clock_writes_both_fields_and_refuses_what_it_cannot_answer() {
    let id = |name: &str| {
        orbistoun_hle::constants::abi_constant("clock", name)
            .unwrap_or_else(|| panic!("{name} is in the harvested table"))
    };

    let mut when = [0xA5A5_A5A5_A5A5_A5A5_u64; 2];
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = id("CLOCK_REALTIME") as u64;
    regs[1] = when.as_mut_ptr() as usize as u64;

    let call = implementation("sceKernelClockGettime");
    assert_eq!(call(&regs), 0, "a clock it can answer succeeds");
    assert!(
        when[0] > 1_600_000_000,
        "the seconds field is written, not left as the guard: {:#x}",
        when[0]
    );
    assert_ne!(
        when[1], 0xA5A5_A5A5_A5A5_A5A5,
        concat!(
            "and the nanoseconds field is written too - a call that fills one and leaves ",
            "the other is the shape a guard word exists to catch (D509)"
        )
    );

    let mut refused_regs = [0_u64; GUEST_ARG_REGISTERS];
    refused_regs[0] = id("CLOCK_PROCESS_CPUTIME_ID") as u64;
    refused_regs[1] = when.as_mut_ptr() as usize as u64;
    let refused = call(&refused_regs);
    assert_ne!(refused, 0, "a clock with no honest source is refused");
    assert_ne!(
        refused,
        u64::MAX,
        concat!(
            "and NOT the POSIX -1 - that is the whole reason this is not the POSIX ",
            "function under a second name"
        )
    );
    assert_eq!(
        refused & 0xffff_0000,
        0x8002_0000,
        "it is in the vendor family a caller tests against: {refused:#x}"
    );
}
