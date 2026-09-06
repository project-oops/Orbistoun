//! The measured *relations*, asserted as properties rather than as numbers.
//!
//! # Why these could not be claimed the way a refusal can
//!
//! `tests/measured_refusals.rs` compares orbistoun's answer to the console's, because the check
//! id names a refusal and the value is the code. These checks are the other kind. Their
//! recorded values are counts of what the check did - `018-relational/mutex-handles-distinct`
//! is `0x6`, `event-flag-handles-distinct` is `0x8` - and the *number* means nothing without
//! obSCEne's own C source. Copying one into an assertion would pin orbistoun to how many
//! objects a probe happened to make.
//!
//! What the check id does state, and states unambiguously, is the **property**: handles are
//! distinct, a thread's identity is stable, a mutex one thread holds excludes another. Those
//! are properties orbistoun either has or does not, and they need no number at all (D545).
//!
//! # The seventh lives next door
//!
//! `018-relational/file-position-tracks-reads` needs a file, and nothing opens here - `/app0`,
//! a host path and `/dev/stdout` all answer ENOENT with no mounted title. It is in
//! `file_position.rs`, which mounts one: the mount table is process-wide, so a test that
//! touches it decides what every other test in the binary sees and does not belong in this
//! file (D547).

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let found = orbistoun_service::implementation_named(name)
        .unwrap_or_else(|| panic!("{name} is not implemented, so the relation cannot be checked"));
    found(&args)
}

/// The value obSCEne measured for `check`, as the entry for `function` records it.
///
/// The same parse as `measured_refusals.rs` and for the same reason: a constant copied into a
/// test drifts from the capture it came from and then passes for the wrong reason.
fn measured(function: &str, check: &str) -> u32 {
    let entry = orbistoun_hle::knowledge::Knowledge::builtin()
        .functions()
        .find(|f| f.name == function)
        .unwrap_or_else(|| panic!("{function} has no knowledge entry"))
        .clone();
    let head = format!("Measured on hardware: obSCEne `{check}` reported pass, value ");
    let sentence = entry
        .edge_cases
        .iter()
        .find(|e| e.starts_with(&head))
        .unwrap_or_else(|| panic!("{function} records no measurement from {check}"));
    let digits: String = sentence[head.len()..]
        .trim_start_matches("0x")
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    u64::from_str_radix(&digits, 16)
        .unwrap_or_else(|e| panic!("{function}/{check} value {digits:?} is not hex: {e}"))
        as u32
}

/// Creates an object through `initialiser` and returns the handle it wrote.
fn handle_from(initialiser: &str, extra: [u64; 4]) -> u64 {
    let mut handle: u64 = 0;
    let at = std::ptr::from_mut(&mut handle) as u64;
    let rc = call(initialiser, [at, extra[0], extra[1], extra[2], extra[3], 0]);
    assert_eq!(
        rc, 0,
        "{initialiser} refused, so there is no handle to compare"
    );
    assert_ne!(
        handle, 0,
        "{initialiser} reported success and wrote nothing"
    );
    handle
}

/// **Two live objects of one kind never share a handle.**
///
/// # What this asserts
///
/// Across three unrelated subsystems - mutexes, condition variables and event flags - that two
/// objects alive at the same time have different handles. A guest keys every later call on the
/// handle it was given, so two objects sharing one is not a cosmetic collision: it is one
/// object where the guest believes there are two.
///
/// The three are checked together because they are three separate allocators, and a single
/// break in one of them proves nothing about the other two.
///
/// # What it cannot assert
///
/// **That the handles resemble the console's.** Orbistoun's are host addresses; the console's
/// are whatever its own kernel hands out, and the recorded measurement is a *count* of objects
/// the check made rather than a handle. This is the property the check's name states, and the
/// number beside it is deliberately not read.
///
/// And nothing about handles after a delete - see the reuse test below, where the answer is an
/// allocator's and not this project's to promise.
#[test]
fn two_live_objects_of_one_kind_never_share_a_handle() {
    let name = std::ffi::CString::new("relation-probe").expect("no interior nul");
    let named = name.as_ptr() as u64;

    let (mutex, other_mutex) = (
        handle_from("scePthreadMutexInit", [0, 0, 0, 0]),
        handle_from("scePthreadMutexInit", [0, 0, 0, 0]),
    );
    assert_ne!(
        mutex, other_mutex,
        "018-relational/mutex-handles-distinct: two live mutexes share a handle"
    );

    let (condvar, other_condvar) = (
        handle_from("scePthreadCondInit", [0, 0, 0, 0]),
        handle_from("scePthreadCondInit", [0, 0, 0, 0]),
    );
    assert_ne!(
        condvar, other_condvar,
        "015-sync/condvar-handles-distinct: two live condition variables share a handle"
    );

    let (flag, other_flag) = (
        handle_from("sceKernelCreateEventFlag", [named, 0, 0, 0]),
        handle_from("sceKernelCreateEventFlag", [named, 0, 0, 0]),
    );
    assert_ne!(
        flag, other_flag,
        "018-relational/event-flag-handles-distinct: two live event flags share a handle"
    );
}

/// **An event flag deleted can be created again.**
///
/// # What this asserts, and the half it deliberately does not
///
/// That deleting an event flag succeeds and that creating another afterwards still works. That
/// is the weaker of the two readings of `018-relational/event-flag-handles-reusable`, and it is
/// the one taken on purpose.
///
/// The stronger reading - that the *handle value* comes back for reuse - is not asserted,
/// because orbistoun's handles are host allocations and whether one address is handed out
/// twice is the allocator's business. A test of it would pass or fail depending on what the
/// heap did that run, which is the nondeterminism D535 warns about, and it would be asserting
/// something this project never promised.
///
/// **So if obSCEne's check means value-reuse, orbistoun's behaviour is undetermined rather than
/// agreeing** - one run here saw a fresh address. Settling it needs the check's source, which
/// is not readable from inside this repository.
#[test]
fn an_event_flag_deleted_can_be_created_again() {
    let name = std::ffi::CString::new("reuse-probe").expect("no interior nul");
    let named = name.as_ptr() as u64;

    let first = handle_from("sceKernelCreateEventFlag", [named, 0, 0, 0]);
    assert_eq!(
        call("sceKernelDeleteEventFlag", [first, 0, 0, 0, 0, 0]),
        0,
        "a flag this test just made could not be deleted"
    );
    let second = handle_from("sceKernelCreateEventFlag", [named, 0, 0, 0]);

    // Usable, not merely non-zero: the new flag answers a poll rather than being a number
    // nothing is behind.
    let polled = call("sceKernelPollEventFlag", [second, 1, 0, 0, 0, 0]) as u32;
    assert_ne!(
        polled, 0x8002_0003,
        "the flag made after a delete answers ESRCH, so the handle names nothing"
    );
    assert_eq!(
        call("sceKernelDeleteEventFlag", [second, 0, 0, 0, 0, 0]),
        0,
        "and the second one is a real flag too, since it can be deleted"
    );
}

/// **A thread's identity is stable, and two threads do not share one.**
///
/// # What this asserts
///
/// That `scePthreadSelf` answers the same value twice on one thread and a different value on
/// another. Both halves are needed: a stub returning a constant passes the first and fails the
/// second, and one returning a fresh value each call fails the first.
///
/// # What it cannot assert
///
/// That the identity matches the console's. The measurement is `0x880000020`, which is a
/// console address; orbistoun's is a host one, and the check's name is about stability rather
/// than about the value. Nor does it say anything about a thread the guest created through
/// `scePthreadCreate` - these are host threads, which is what the implementation keys on.
#[test]
fn a_thread_identity_is_stable_and_not_shared() {
    let me = call("scePthreadSelf", [0, 0, 0, 0, 0, 0]);
    let me_again = call("scePthreadSelf", [0, 0, 0, 0, 0, 0]);
    assert_eq!(
        me, me_again,
        "018-relational/thread-identity-stable: one thread asked twice got two identities"
    );

    let other = std::thread::spawn(|| call("scePthreadSelf", [0, 0, 0, 0, 0, 0]))
        .join()
        .expect("the second thread ran");
    assert_ne!(
        me, other,
        "two threads share one identity, so a guest cannot tell them apart"
    );
}

/// **A mutex one thread holds refuses another, with the code the console answered.**
///
/// # What this asserts
///
/// The property *and* the value, because this is the one relation whose measurement is a
/// refusal code: `018-relational/mutex-excludes-another-thread` recorded `0x80020010`, which is
/// EBUSY under the vendor encoding D398 measured. So a second thread's `trylock` on a held
/// mutex must both fail and fail with that code, and the code is read from the knowledge base
/// rather than written here.
///
/// This is the relation that would matter most if it were wrong. A `trylock` that succeeded
/// would put two guest threads inside one critical section, and the corruption after it would
/// be blamed on whatever the lock was protecting.
///
/// # What it cannot assert
///
/// That the exclusion is real under contention - one thread, one attempt, no timing. It shows
/// the refusal happens and carries the right code, not that the lock is sound.
#[test]
fn a_held_mutex_refuses_another_thread_with_the_measured_code() {
    let mutex = handle_from("scePthreadMutexInit", [0, 0, 0, 0]);
    let mut holder: u64 = mutex;
    let at = std::ptr::from_mut(&mut holder) as u64;
    assert_eq!(
        call("scePthreadMutexLock", [at, 0, 0, 0, 0, 0]),
        0,
        "the lock has to be held before another thread can be refused it"
    );

    let refused = std::thread::spawn(move || call("scePthreadMutexTrylock", [at, 0, 0, 0, 0, 0]))
        .join()
        .expect("the second thread ran") as u32;
    let expected = measured(
        "scePthreadMutexTrylock",
        "018-relational/mutex-excludes-another-thread",
    );
    assert_eq!(
        refused, expected,
        "a second thread's trylock on a held mutex: the console answered {expected:#010x}, \
         orbistoun {refused:#010x}"
    );

    assert_eq!(
        call("scePthreadMutexUnlock", [at, 0, 0, 0, 0, 0]),
        0,
        "and the holder can still release it"
    );
}
