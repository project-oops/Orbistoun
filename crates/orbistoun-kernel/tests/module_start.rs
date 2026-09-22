//! `sceKernelLoadStartModule` does the `Load` half and not the `Start` half, and says so.
//!
//! # Why this is a test and not a gate on the eventual fix
//!
//! Nothing in the tree runs a module's `DT_INIT_ARRAY`. That is the wall PPSA02664 stands at:
//! `Il2CppUserAssemblies.prx` is placed, its exported code runs, and a global its constructors
//! would have filled is read as null three calls later - a fault at a site with no visible
//! connection to the load (D514).
//!
//! What is testable *now* is that the gap is **reported** rather than silent. A handle and a
//! silence are indistinguishable from a module that started, which is the failure principle 3
//! forbids. When something does run the initialisers, this test should be replaced by one
//! asserting they ran - not deleted quietly, which is how a gap outlives its fix (D510).

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
    /// Never read, and required: it owns the bytes `at` points into. Dropping it while the
    /// call is in flight would hand `read_name` a freed buffer.
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
/// # What this asserts and what it cannot
///
/// It asserts the **report**, not the behaviour: that a `/app0` load is recorded and surfaces
/// with its leaf name and handle. It cannot assert that the module's initialisers did not run,
/// because nothing here loads a module - `place_title_modules` does that, before a guest
/// starts, and this crate never sees it.
///
/// The summary is process-wide, so this asserts *containment* rather than the whole string;
/// another test in this binary calling `sceKernelLoadStartModule` would otherwise decide
/// whether this one passed.
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

    // --- and a module the loader *did* record ------------------------------------------
    //
    // Recorded with nothing to run, which is the one shape this can assert without mapping
    // real guest code: starting a module enters an address, and a test cannot manufacture
    // executable relocated guest text. So this pins the **bookkeeping** - that a recorded
    // module is reported as started rather than as missing - and the separate call-out for a
    // start that ran nothing.
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

    // --- and the bulk start, which is a diagnostic rather than behaviour ----------------
    //
    // `ARecordedModule` has nothing to run, so this asserts the **bookkeeping and the
    // wording**: that every recorded module is visited, and that a start nobody asked for is
    // not reported as though a guest had supplied a handle. It cannot assert that a real
    // module's constructors run - that needs placed, relocated, executable guest text.
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
/// # What this asserts, and the thing it deliberately does not
///
/// It asserts the **answer**, because that is what a caller acts on: a scheduling call tested
/// against zero reads a placeholder as "the affinity was refused", which is a lie in the
/// direction that stops a guest (D125, D523).
///
/// It does **not** assert that any affinity was *applied* - nothing here pins a thread and the
/// implementation says so. The set-then-get below shows why a mask coming back would not check that
/// anyway: `scePthreadGetaffinity` reads the thread's creation-time `requested_affinity`, and the
/// running-thread setter drops (D523), so what the getter answers is not what the setter was handed.
#[test]
fn setting_a_threads_affinity_is_accepted_rather_than_refused() {
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = 0x01d1_d900_0960; // a thread handle shaped like the ones a run produces
    regs[1] = 0x1ffb; // the mask PPSA02664 passes

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

    // And reading it straight back does not return what was just set. `scePthreadGetaffinity`
    // answers the thread's creation-time `requested_affinity`, not what a running-thread
    // `scePthreadSetaffinity` was handed - that setter drops (D523). For a handle no thread was
    // registered under, the getter reads zero ("anywhere"), so the `0x1ffb` set above is not
    // observable here - which is the honest limit the doc states rather than a mask promise.
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
/// # What this asserts, and the half it deliberately does not
///
/// It asserts the **shape and the refusal**: two fields written, a vendor-family code for a
/// clock orbistoun cannot answer, and never the POSIX `-1` - registering the POSIX function
/// under the vendor name is the mistake this shape invites and the `stat` pair made once
/// (D525, D536).
///
/// It does **not** assert the values. A wall clock is not reproducible, so what is checked is
/// that the seconds field is past a date already in the past - which distinguishes "the clock
/// answered" from "the field was left alone" without pinning a number no run can repeat.
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
