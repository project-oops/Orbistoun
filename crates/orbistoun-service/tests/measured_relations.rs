//! The measured relations, asserted as properties rather than as numbers (D545).
//!
//! These checks record counts of what the check did (`018-relational/mutex-handles-distinct` is
//! `0x6`), which mean nothing without obSCEne's C source. The check id states a property instead:
//! handles are distinct, a thread's identity is stable, a held mutex excludes another thread.
//! `018-relational/file-position-tracks-reads` needs a mounted title, and the mount table is
//! process-wide, so it lives in `file_position.rs`.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let found = orbistoun_service::implementation_named(name)
        .unwrap_or_else(|| panic!("{name} is not implemented, so the relation cannot be checked"));
    found(&args)
}

/// The value obSCEne measured for `check`, as the entry for `function` records it, parsed as
/// `measured_refusals.rs` does rather than copied into the test.
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

/// Two live objects of one kind never share a handle.
///
/// Checked across mutexes, condition variables and event flags, three separate allocators. A guest
/// keys every later call on its handle, so a shared handle is one object where the guest believes
/// there are two. Orbistoun's handles are host addresses, so their values are not compared.
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

/// An event flag deleted can be created again.
///
/// The weaker reading of `018-relational/event-flag-handles-reusable`, taken on purpose: whether
/// the same handle value comes back is the host allocator's business, and asserting it would depend
/// on the heap. If obSCEne's check means value reuse, orbistoun's behaviour is undetermined.
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

    // Usable, not merely non-zero: the new flag answers a poll.
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

/// A thread's identity is stable, and two threads do not share one.
///
/// Both halves are needed: a constant passes the first and fails the second, and a fresh value per
/// call fails the first. The value is a host one and is not compared with the hardware's.
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

/// A mutex one thread holds refuses another, with the code the hardware answered.
///
/// `018-relational/mutex-excludes-another-thread` recorded `0x80020010`, EBUSY in the vendor
/// encoding (D398), so a second thread's `trylock` must fail with that code, read from the
/// knowledge base. One thread, one attempt: this shows the refusal and its code, not that the lock
/// is sound under contention.
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
        concat!(
            "a second thread's trylock on a held mutex: the console answered {:#010x}, ",
            "orbistoun {:#010x}"
        ),
        expected, refused
    );

    assert_eq!(
        call("scePthreadMutexUnlock", [at, 0, 0, 0, 0, 0]),
        0,
        "and the holder can still release it"
    );
}
