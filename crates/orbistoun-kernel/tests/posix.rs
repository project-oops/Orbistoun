//! The kernel's guest-facing calls, driven through the table a resolved import reaches.
//!
//! Most take a pointer to a handle slot: the guest passes the address of a null local and the
//! kernel writes the address of the real object into it. The tests run whole cycles (init,
//! use, destroy), because a wrapper that writes its handle to the wrong place still answers
//! `OK` call by call.
//!
//! Out-parameters are `int` or pointer-width, and each test writes a sentinel beside the
//! out-parameter and asserts it survived (D272). Guest memory is host memory, so a `Vec<u64>`
//! this test owns is a guest object.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_core::GuestFn;

/// Success, as a guest reads it.
const OK: u64 = 0;
/// The caller passed something this could not use.
///
/// Derived from the error type rather than written out, so the tests and the implementation
/// share one constant (D670).
const INVALID_ARGUMENT: u64 = orbistoun_core::GuestError::InvalidArgument.as_raw() as u64;
/// The handle names nothing.
const INVALID_HANDLE: u64 = orbistoun_core::GuestError::InvalidHandle.as_raw() as u64;
/// Releasing something this thread does not hold.
///
/// Measured on the hardware; distinct from a bad argument (D398).
const NOT_OWNER: u64 = 0x8002_0001;
/// The ordinary outcome of asking for something somebody else holds, as measured on the
/// hardware (D398).
const BUSY: u64 = 0x8002_0010;
/// No such object: the vendor `ESRCH` a bad event-flag handle answers on the hardware.
const NO_SUCH: u64 = 0x8002_0003;

/// A guest object: a run of words at a real address.
struct Slot {
    storage: Vec<u64>,
    at: u64,
}

impl Slot {
    /// `words` zeroed machine words.
    fn new(words: usize) -> Self {
        let mut storage = vec![0_u64; words];
        let at = storage.as_mut_ptr().expose_provenance() as u64;
        Self { storage, at }
    }

    /// One word, which is the usual `&handle` case.
    fn one() -> Self {
        Self::new(1)
    }

    fn at(&self) -> u64 {
        self.at
    }

    /// What the guest would read out of word `n`.
    fn read(&self, n: usize) -> u64 {
        // The library wrote through a raw pointer into this same allocation.
        self.storage[n]
    }
}

/// A NUL-terminated name at a real address.
struct Name(Vec<u8>);

impl Name {
    fn new(s: &str) -> Self {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        Self(v)
    }

    fn at(&self) -> u64 {
        self.0.as_ptr().expose_provenance() as u64
    }
}

/// The implementation registered under `name`.
fn implementation(name: &str) -> GuestFn {
    orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(
            || panic!("{name} is not implemented, so no guest can reach it"),
            |(_, f)| *f,
        )
}

/// Calls one, poisoning the argument registers it does not use.
fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0xDEAD_BEEF_DEAD_BEEF_u64; GUEST_ARG_REGISTERS];
    for (slot, value) in regs.iter_mut().zip(args) {
        *slot = *value;
    }
    implementation(name)(&regs)
}

/// Every name appears once, and the table is not empty.
///
/// A duplicate would let the registry pick one of two implementations for the same import.
#[test]
fn the_table_names_each_call_once() {
    let mut seen = std::collections::BTreeSet::new();
    for (name, _) in orbistoun_kernel::implementations() {
        assert!(seen.insert(*name), "{name} appears in the table twice");
    }
    assert!(
        !seen.is_empty(),
        "the table is empty, so this file proves nothing"
    );
}

/// An emptied set contains nothing, and a filled one contains everything.
///
/// `sigismember` needs a real implementation: the placeholder error code is non-zero and a
/// caller reads it as yes.
#[test]
fn an_emptied_signal_set_contains_nothing_and_a_filled_one_everything() {
    let set = Slot::new(2);

    assert_eq!(call("posix_sigemptyset", &[set.at()]), OK);
    for signal in [1_u64, 2, 13, 31, 64, 65, 128] {
        assert_eq!(
            call("posix_sigismember", &[set.at(), signal]),
            0,
            "signal {signal} should not be in an emptied set"
        );
    }

    assert_eq!(call("posix_sigfillset", &[set.at()]), OK);
    for signal in [1_u64, 2, 13, 31, 64, 65, 128] {
        assert_eq!(
            call("posix_sigismember", &[set.at(), signal]),
            1,
            "signal {signal} should be in a filled set"
        );
    }
}

/// Adding and removing one signal leaves its neighbours alone.
///
/// Signals are numbered from one, so signal n is bit n-1.
#[test]
fn adding_a_signal_leaves_its_neighbours_alone() {
    let set = Slot::new(2);
    assert_eq!(call("posix_sigemptyset", &[set.at()]), OK);

    assert_eq!(call("posix_sigaddset", &[set.at(), 13]), OK);
    assert_eq!(call("posix_sigismember", &[set.at(), 13]), 1);
    assert_eq!(call("posix_sigismember", &[set.at(), 12]), 0);
    assert_eq!(call("posix_sigismember", &[set.at(), 14]), 0);

    // Bit 12 of the first word, which is signal 13 counting from one.
    assert_eq!(set.read(0), 1 << 12);

    assert_eq!(call("posix_sigdelset", &[set.at(), 13]), OK);
    assert_eq!(call("posix_sigismember", &[set.at(), 13]), 0);
    assert_eq!(set.read(0), 0);
}

/// A signal in the second word is reached, which a single-word set would silently drop.
#[test]
fn a_signal_past_the_first_word_still_lands() {
    let set = Slot::new(2);
    assert_eq!(call("posix_sigemptyset", &[set.at()]), OK);

    // Signal 65 is bit 0 of the second word.
    assert_eq!(call("posix_sigaddset", &[set.at(), 65]), OK);
    assert_eq!(set.read(1), 1, "it belongs in the second word");
    assert_eq!(set.read(0), 0, "and not in the first");
    assert_eq!(call("posix_sigismember", &[set.at(), 65]), 1);

    // Signal 128 is the last bit the set can hold.
    assert_eq!(call("posix_sigaddset", &[set.at(), 128]), OK);
    assert_eq!(set.read(1), 1 | (1 << 63));
}

/// A signal number outside the set is an error, not a write past the end of the object.
///
/// The guest's `sigset_t` is a fixed size, so an offset for an out-of-range number would
/// corrupt whatever the guest keeps after it.
#[test]
fn a_signal_number_outside_the_set_is_refused() {
    let set = Slot::new(2);
    call("posix_sigemptyset", &[set.at()]);

    for signal in [0_u64, 129, 1000, u64::MAX] {
        assert_eq!(
            call("posix_sigaddset", &[set.at(), signal]),
            INVALID_ARGUMENT,
            "signal {signal} is not in a set"
        );
        assert_eq!(
            call("posix_sigismember", &[set.at(), signal]),
            INVALID_ARGUMENT
        );
        assert_eq!(
            call("posix_sigdelset", &[set.at(), signal]),
            INVALID_ARGUMENT
        );
    }
    assert_eq!(set.read(0), 0, "and none of them wrote anything");
    assert_eq!(set.read(1), 0);
}

/// A null set is refused by every call that takes one.
#[test]
fn a_null_signal_set_is_refused() {
    for name in ["posix_sigemptyset", "posix_sigfillset"] {
        assert_eq!(call(name, &[0]), INVALID_ARGUMENT, "{name}");
    }
    for name in ["posix_sigaddset", "posix_sigdelset", "posix_sigismember"] {
        assert_eq!(call(name, &[0, 13]), INVALID_ARGUMENT, "{name}");
    }
}

/// The whole mutex cycle, driven the way a guest drives it.
#[test]
fn a_guest_mutex_is_initialised_used_and_destroyed_through_its_own_pointer() {
    let mutex = Slot::one();
    let name = Name::new("frame-lock");

    assert_eq!(call("scePthreadMutexInit", &[mutex.at(), 0, name.at()]), OK);
    assert_ne!(
        mutex.read(0),
        0,
        "a handle must have been written into the slot"
    );

    assert_eq!(call("scePthreadMutexLock", &[mutex.at()]), OK);
    assert_eq!(call("scePthreadMutexUnlock", &[mutex.at()]), OK);

    assert_eq!(call("scePthreadMutexTrylock", &[mutex.at()]), OK);
    assert_eq!(call("scePthreadMutexUnlock", &[mutex.at()]), OK);

    assert_eq!(call("scePthreadMutexDestroy", &[mutex.at()]), OK);
    assert_eq!(
        mutex.read(0),
        0,
        "the slot is cleared, so a use-after-destroy shows up here"
    );
    assert_eq!(
        call("scePthreadMutexDestroy", &[mutex.at()]),
        INVALID_HANDLE,
        "and destroying twice is reported rather than handing back a freed object"
    );
}

/// A statically initialised lock names nothing, and says so.
///
/// The guest filled the location at compile time and never called init, so the handle there
/// is not one this crate issued. Answering success would let every thread into the critical
/// section at once.
#[test]
fn a_lock_that_was_never_initialised_names_nothing() {
    let mutex = Slot::one(); // still zero: never initialised

    assert_eq!(call("scePthreadMutexLock", &[mutex.at()]), INVALID_HANDLE);
    assert_eq!(call("scePthreadMutexUnlock", &[mutex.at()]), INVALID_HANDLE);
    assert_eq!(
        call("scePthreadMutexTrylock", &[mutex.at()]),
        INVALID_HANDLE
    );
    assert_eq!(
        call("scePthreadMutexDestroy", &[mutex.at()]),
        INVALID_HANDLE
    );

    // And a null pointer is not a lock either.
    assert_eq!(call("scePthreadMutexInit", &[0, 0, 0]), INVALID_ARGUMENT);
    assert_eq!(call("scePthreadMutexLock", &[0]), INVALID_HANDLE);
}

/// `Trylock` reports a lock it could not take as `Busy`, not `OK`.
///
/// A guest branches on that answer, and it is the ordinary outcome of the call rather than a
/// misuse, so it is not an argument error.
#[test]
fn trylock_reports_a_lock_held_by_somebody_else() {
    let mutex = Slot::one();
    let name = Name::new("contended");
    assert_eq!(call("scePthreadMutexInit", &[mutex.at(), 0, name.at()]), OK);

    // Another host thread is another guest thread, so it is another owner.
    let handle = mutex.at();
    std::thread::spawn(move || call("scePthreadMutexLock", &[handle]))
        .join()
        .expect("the other thread takes it");

    assert_eq!(
        call("scePthreadMutexTrylock", &[mutex.at()]),
        BUSY,
        "somebody else holds it, which is an outcome and not a misuse"
    );
    assert_eq!(
        call("scePthreadMutexUnlock", &[mutex.at()]),
        NOT_OWNER,
        "and it is not ours to release"
    );
}

/// The default recursion mode is the strict one, so a double lock is refused, not hung.
///
/// The attribute block is not parsed, so this is the default rather than what the guest asked
/// for, and the trace says so.
#[test]
fn a_guest_mutex_defaults_to_refusing_a_second_lock_by_its_owner() {
    let mutex = Slot::one();
    let name = Name::new("strict");
    call("scePthreadMutexInit", &[mutex.at(), 0, name.at()]);

    assert_eq!(call("scePthreadMutexLock", &[mutex.at()]), OK);
    assert_eq!(
        call("scePthreadMutexLock", &[mutex.at()]),
        INVALID_ARGUMENT,
        "the guest deadlocking against itself is told so rather than made to wait"
    );
    call("scePthreadMutexUnlock", &[mutex.at()]);
    call("scePthreadMutexDestroy", &[mutex.at()]);
}

/// A mutex attribute stores what it was told and reads it back, so `Gettype` writes its
/// out-parameter (D272).
#[test]
fn a_mutex_attribute_stores_what_it_was_told() {
    let attr = Slot::one();
    assert_eq!(call("scePthreadMutexattrInit", &[attr.at()]), OK);
    assert_ne!(attr.read(0), 0, "an object must have been allocated");

    let out = Slot::new(2);
    assert_eq!(call("scePthreadMutexattrSettype", &[attr.at(), 2]), OK);
    assert_eq!(
        call("scePthreadMutexattrGettype", &[attr.at(), out.at()]),
        OK
    );
    assert_eq!(out.read(0) & 0xFFFF_FFFF, 2);

    // The protocol is a separate field, so setting one must not disturb the other.
    assert_eq!(call("scePthreadMutexattrSetprotocol", &[attr.at(), 3]), OK);
    assert_eq!(
        call("scePthreadMutexattrGetprotocol", &[attr.at(), out.at()]),
        OK
    );
    assert_eq!(out.read(0) & 0xFFFF_FFFF, 3);
    assert_eq!(
        call("scePthreadMutexattrGettype", &[attr.at(), out.at()]),
        OK
    );
    assert_eq!(
        out.read(0) & 0xFFFF_FFFF,
        2,
        "the type is still what it was"
    );

    assert_eq!(call("scePthreadMutexattrDestroy", &[attr.at()]), OK);
}

/// An `int` out-parameter is written four bytes wide, not eight.
///
/// An eight-byte write through an `int *` overwrites the caller's neighbouring variable; the
/// sentinel in the upper half is the assertion (D272).
#[test]
fn an_int_out_parameter_does_not_take_its_neighbour_with_it() {
    let attr = Slot::one();
    call("scePthreadMutexattrInit", &[attr.at()]);
    call("scePthreadMutexattrSettype", &[attr.at(), 7]);

    let out = Slot::new(1);
    // Fill the whole word, so an eight-byte write is visible as the loss of the top half.
    // SAFETY: a word inside an allocation this test owns, under the identity mapping.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u64>(out.at() as usize),
            0xAAAA_AAAA_0000_0000,
        );
    }

    assert_eq!(
        call("scePthreadMutexattrGettype", &[attr.at(), out.at()]),
        OK
    );
    assert_eq!(
        out.read(0) & 0xFFFF_FFFF,
        7,
        "the value lands in the low half"
    );
    assert_eq!(
        out.read(0) >> 32,
        0xAAAA_AAAA,
        "and the neighbouring four bytes are untouched"
    );
}

/// A size out-parameter is pointer-width, the deliberate exception to the `int` case above.
///
/// The two widths sit side by side in one family, so a refactor that made them uniform would
/// be wrong one way or the other.
#[test]
fn a_size_out_parameter_is_written_the_full_width() {
    let attr = Slot::one();
    assert_eq!(call("scePthreadAttrInit", &[attr.at()]), OK);

    let big = 0x1_0000_0000_u64; // four gigabytes: nothing an `int` could carry
    assert_eq!(call("scePthreadAttrSetstacksize", &[attr.at(), big]), OK);

    let out = Slot::one();
    assert_eq!(
        call("scePthreadAttrGetstacksize", &[attr.at(), out.at()]),
        OK
    );
    assert_eq!(out.read(0), big, "a size needs all eight bytes");

    // Detach state is an `int` in the same object, so it truncates where the size does not.
    assert_eq!(call("scePthreadAttrSetdetachstate", &[attr.at(), 1]), OK);
    let detach = Slot::one();
    assert_eq!(
        call("scePthreadAttrGetdetachstate", &[attr.at(), detach.at()]),
        OK
    );
    assert_eq!(detach.read(0) & 0xFFFF_FFFF, 1);

    assert_eq!(call("scePthreadAttrDestroy", &[attr.at()]), OK);
    assert_eq!(attr.read(0), 0, "the slot is cleared");
    assert_eq!(
        call("scePthreadAttrDestroy", &[attr.at()]),
        INVALID_ARGUMENT,
        "and destroying twice is reported"
    );
}

/// Each attribute field is separate, so setting one does not disturb the others.
#[test]
fn the_thread_attribute_fields_do_not_overlap() {
    let attr = Slot::one();
    call("scePthreadAttrInit", &[attr.at()]);

    call("scePthreadAttrSetstacksize", &[attr.at(), 0x4000]);
    call("scePthreadAttrSetdetachstate", &[attr.at(), 1]);
    call("scePthreadAttrSetschedparam", &[attr.at(), 42]);

    let out = Slot::one();
    call("scePthreadAttrGetstacksize", &[attr.at(), out.at()]);
    assert_eq!(out.read(0), 0x4000);
    call("scePthreadAttrGetdetachstate", &[attr.at(), out.at()]);
    assert_eq!(out.read(0) & 0xFFFF_FFFF, 1);
    call("scePthreadAttrGetschedparam", &[attr.at(), out.at()]);
    assert_eq!(out.read(0) & 0xFFFF_FFFF, 42);
}

/// An attribute object that was never initialised is refused, as is a null out-parameter.
#[test]
fn an_uninitialised_attribute_object_is_refused() {
    let attr = Slot::one(); // still zero
    let out = Slot::one();

    assert_eq!(call("scePthreadMutexattrInit", &[0]), INVALID_ARGUMENT);
    assert_eq!(call("scePthreadAttrInit", &[0]), INVALID_ARGUMENT);
    assert_eq!(
        call("scePthreadMutexattrSettype", &[attr.at(), 1]),
        INVALID_ARGUMENT
    );
    assert_eq!(
        call("scePthreadAttrGetstacksize", &[attr.at(), out.at()]),
        INVALID_ARGUMENT
    );

    // Initialised, but asked to write the answer nowhere.
    call("scePthreadAttrInit", &[attr.at()]);
    assert_eq!(
        call("scePthreadAttrGetstacksize", &[attr.at(), 0]),
        INVALID_ARGUMENT
    );
    assert_eq!(
        call("scePthreadAttrGetdetachstate", &[attr.at(), 0]),
        INVALID_ARGUMENT
    );
}

/// The condition-variable cycle, including a signal that arrives before the wait.
#[test]
fn a_guest_condition_variable_remembers_a_signal_that_arrived_early() {
    let cond = Slot::one();
    let name = Name::new("ready");
    assert_eq!(call("scePthreadCondInit", &[cond.at(), 0, name.at()]), OK);
    assert_ne!(cond.read(0), 0);

    assert_eq!(call("scePthreadCondSignal", &[cond.at()]), OK);
    assert_eq!(
        call("scePthreadCondWait", &[cond.at(), 0]),
        OK,
        "the owed wake is taken rather than waited for"
    );

    assert_eq!(call("scePthreadCondBroadcast", &[cond.at()]), OK);
    assert_eq!(call("scePthreadCondWait", &[cond.at(), 0]), OK);

    assert_eq!(call("scePthreadCondDestroy", &[cond.at()]), OK);
    assert_eq!(call("scePthreadCondSignal", &[cond.at()]), INVALID_HANDLE);
}

/// A condition variable that was never initialised names nothing.
#[test]
fn an_uninitialised_condition_variable_names_nothing() {
    let cond = Slot::one();
    assert_eq!(call("scePthreadCondSignal", &[cond.at()]), INVALID_HANDLE);
    assert_eq!(
        call("scePthreadCondBroadcast", &[cond.at()]),
        INVALID_HANDLE
    );
    assert_eq!(call("scePthreadCondDestroy", &[cond.at()]), INVALID_HANDLE);
    assert_eq!(call("scePthreadCondInit", &[0, 0, 0]), INVALID_ARGUMENT);
}

/// The read/write lock cycle, and readers sharing where writers do not.
#[test]
fn a_guest_rwlock_shares_between_readers_and_excludes_writers() {
    let lock = Slot::one();
    let name = Name::new("shared");
    assert_eq!(call("scePthreadRwlockInit", &[lock.at(), 0, name.at()]), OK);
    assert_ne!(lock.read(0), 0);

    assert_eq!(call("scePthreadRwlockRdlock", &[lock.at()]), OK);
    assert_eq!(
        call("scePthreadRwlockTryrdlock", &[lock.at()]),
        OK,
        "readers do not queue behind each other"
    );
    assert_eq!(
        call("scePthreadRwlockTrywrlock", &[lock.at()]),
        BUSY,
        "but a writer may not join them"
    );

    assert_eq!(call("scePthreadRwlockUnlock", &[lock.at()]), OK);
    assert_eq!(call("scePthreadRwlockUnlock", &[lock.at()]), OK);
    assert_eq!(call("scePthreadRwlockTrywrlock", &[lock.at()]), OK);
    assert_eq!(
        call("scePthreadRwlockTryrdlock", &[lock.at()]),
        BUSY,
        "and a writer excludes readers too"
    );

    assert_eq!(call("scePthreadRwlockUnlock", &[lock.at()]), OK);
    assert_eq!(
        call("scePthreadRwlockUnlock", &[lock.at()]),
        INVALID_ARGUMENT,
        "releasing one nobody holds is reported"
    );
    assert_eq!(call("scePthreadRwlockDestroy", &[lock.at()]), OK);
}

/// The POSIX-named entries are the same calls under another name, so a lock taken through
/// one spelling is the lock the other spelling sees.
#[test]
fn the_posix_named_rwlock_entries_reach_the_same_lock() {
    let lock = Slot::one();
    let name = Name::new("aliased");
    assert_eq!(
        call("posix_pthread_rwlock_init", &[lock.at(), 0, name.at()]),
        OK
    );
    let handle = lock.read(0);
    assert_ne!(handle, 0);

    // Taken through the POSIX name, released through the vendor one.
    assert_eq!(call("posix_pthread_rwlock_wrlock", &[lock.at()]), OK);
    assert_eq!(call("scePthreadRwlockUnlock", &[lock.at()]), OK);

    // And the other way round.
    assert_eq!(call("scePthreadRwlockRdlock", &[lock.at()]), OK);
    assert_eq!(call("posix_pthread_rwlock_unlock", &[lock.at()]), OK);

    assert_eq!(call("posix_pthread_rwlock_destroy", &[lock.at()]), OK);
}

/// An uninitialised read/write lock names nothing.
#[test]
fn an_uninitialised_rwlock_names_nothing() {
    let lock = Slot::one();
    assert_eq!(call("scePthreadRwlockRdlock", &[lock.at()]), INVALID_HANDLE);
    assert_eq!(call("scePthreadRwlockWrlock", &[lock.at()]), INVALID_HANDLE);
    assert_eq!(call("scePthreadRwlockUnlock", &[lock.at()]), INVALID_HANDLE);
    assert_eq!(
        call("scePthreadRwlockDestroy", &[lock.at()]),
        INVALID_HANDLE
    );
}

/// A barrier of one releases on arrival and can be reused.
#[test]
fn a_guest_barrier_of_one_releases_on_arrival() {
    let barrier = Slot::one();
    let name = Name::new("solo");
    assert_eq!(
        call("scePthreadBarrierInit", &[barrier.at(), 0, 1, name.at()]),
        OK
    );
    assert_ne!(barrier.read(0), 0);

    assert_eq!(call("scePthreadBarrierWait", &[barrier.at()]), OK);
    assert_eq!(
        call("scePthreadBarrierWait", &[barrier.at()]),
        OK,
        "and again"
    );

    assert_eq!(call("scePthreadBarrierDestroy", &[barrier.at()]), OK);
    assert_eq!(
        call("scePthreadBarrierWait", &[barrier.at()]),
        INVALID_HANDLE
    );
}

/// The event-flag cycle, and the mode bit that separates "all" from "any".
///
/// A poll that misses answers `Busy`, not an error, so a guest does not read an ordinary poll
/// as a broken handle; only a bad handle is a handle error.
#[test]
fn a_guest_event_flag_distinguishes_a_miss_from_a_bad_handle() {
    /// The mode bit meaning every bit of the pattern must be present.
    const WAIT_AND: u64 = 0x01;
    /// The mode bit meaning any bit of the pattern will do.
    ///
    /// Passed explicitly: a mode naming neither `and` nor `or` is an argument error on the hardware.
    const WAIT_OR: u64 = 0x02;

    let flag = Slot::one();
    let name = Name::new("state");
    assert_eq!(
        call(
            "sceKernelCreateEventFlag",
            &[flag.at(), name.at(), 0, 0b0101, 0]
        ),
        OK
    );
    let handle = flag.read(0);
    assert_ne!(handle, 0);

    let result = Slot::one();
    assert_eq!(
        call(
            "sceKernelPollEventFlag",
            &[handle, 0b0100, WAIT_OR, result.at(), 0]
        ),
        OK
    );
    assert_eq!(result.read(0), 0b0101, "the bits at the moment of the test");

    assert_eq!(
        call(
            "sceKernelPollEventFlag",
            &[handle, 0b0111, WAIT_AND, result.at(), 0]
        ),
        BUSY,
        "not all of those bits are set, which is a miss and not a fault"
    );
    assert_eq!(
        call(
            "sceKernelPollEventFlag",
            &[handle, 0b0111, WAIT_OR, result.at(), 0]
        ),
        OK,
        "but some of them are"
    );

    assert_eq!(call("sceKernelSetEventFlag", &[handle, 0b0010]), OK);
    assert_eq!(
        call(
            "sceKernelPollEventFlag",
            &[handle, 0b0111, WAIT_AND, result.at(), 0]
        ),
        OK,
        "and now all of them are"
    );

    assert_eq!(call("sceKernelClearEventFlag", &[handle, 0b0001]), OK);
    assert_eq!(
        call(
            "sceKernelPollEventFlag",
            &[handle, 0b0110, WAIT_OR, result.at(), 0]
        ),
        BUSY,
        "clear keeps only the bits it names"
    );

    assert_eq!(call("sceKernelDeleteEventFlag", &[handle]), OK);
    assert_eq!(
        call(
            "sceKernelPollEventFlag",
            &[handle, 0b0001, WAIT_OR, result.at(), 0]
        ),
        NO_SUCH,
        "which is a different answer from a miss"
    );
    assert_eq!(call("sceKernelSetEventFlag", &[handle, 1]), NO_SUCH);
    assert_eq!(call("sceKernelClearEventFlag", &[handle, 1]), NO_SUCH);
    assert_eq!(call("sceKernelDeleteEventFlag", &[handle]), NO_SUCH);
}

/// A poll with nowhere to put the answer still answers.
///
/// The result pointer is optional, and a guest that only wants the yes-or-no passes null.
#[test]
fn an_event_flag_poll_with_no_result_pointer_still_answers() {
    let flag = Slot::one();
    let name = Name::new("e");
    call("sceKernelCreateEventFlag", &[flag.at(), name.at(), 0, 1, 0]);
    let handle = flag.read(0);

    // `0x02` is `or`: a mode of zero names neither and is an argument error on the hardware.
    assert_eq!(call("sceKernelPollEventFlag", &[handle, 1, 0x02, 0, 0]), OK);
    assert_eq!(
        call("sceKernelPollEventFlag", &[handle, 2, 0x02, 0, 0]),
        BUSY
    );

    call("sceKernelDeleteEventFlag", &[handle]);
}

/// Creating an event flag with nowhere to write the handle is refused.
#[test]
fn creating_an_event_flag_with_nowhere_to_put_it_is_refused() {
    assert_eq!(
        call("sceKernelCreateEventFlag", &[0, 0, 0, 0, 0]),
        INVALID_ARGUMENT
    );
}

/// The semaphore cycle through the kernel entry points.
#[test]
fn a_guest_semaphore_is_taken_signalled_and_deleted() {
    let out = Slot::one();
    let name = Name::new("slots");
    assert_eq!(
        call("sceKernelCreateSema", &[out.at(), name.at(), 0, 1, 4, 0]),
        OK
    );
    let handle = out.read(0) & 0xFFFF_FFFF;
    assert_ne!(handle, 0, "a handle must have been written");

    assert_eq!(call("sceKernelPollSema", &[handle, 1]), OK);
    assert_eq!(
        call("sceKernelPollSema", &[handle, 1]),
        BUSY,
        "the count is spent, which is an outcome and not a fault"
    );

    assert_eq!(call("sceKernelSignalSema", &[handle, 1]), OK);
    assert_eq!(
        call("sceKernelWaitSema", &[handle, 1, 0]),
        OK,
        "there is one to take, so this does not block"
    );

    assert_eq!(
        call("sceKernelSignalSema", &[handle, 99]),
        INVALID_ARGUMENT,
        "past the ceiling is refused rather than clamped"
    );

    assert_eq!(call("sceKernelDeleteSema", &[handle]), OK);
    // The count is the second argument, and zero is refused on the hardware rather than granted
    // trivially.
    assert_eq!(call("sceKernelPollSema", &[handle, 1]), INVALID_HANDLE);
    assert_eq!(call("sceKernelDeleteSema", &[handle]), INVALID_HANDLE);
}

/// A semaphore handle of zero names nothing, because zero is how "nothing" is spelled.
#[test]
fn a_semaphore_handle_of_zero_names_nothing() {
    assert_eq!(call("sceKernelPollSema", &[0, 1]), INVALID_HANDLE);
    assert_eq!(call("sceKernelSignalSema", &[0, 1]), INVALID_HANDLE);
    assert_eq!(call("sceKernelDeleteSema", &[0]), INVALID_HANDLE);
}

/// The page size is a real power of two, because guests round against it.
#[test]
fn the_page_size_is_a_usable_power_of_two() {
    let size = call("posix_getpagesize", &[]);
    assert!(size > 0, "rounding against zero would divide by it");
    assert!(size.is_power_of_two(), "{size} is not a power of two");
}

/// The machine reports as a retail unit and not as a development one.
///
/// Exactly one of the three can be true, and a guest branches on the answer. The spelling is
/// part of the assertion: guests import `sceKernelIsDevkit`, a different symbol from
/// `sceKernelIsDevKit` (D393).
#[test]
fn the_console_reports_itself_as_retail() {
    let retail = call("sceKernelIsCex", &[]);
    let devkit = call("sceKernelIsDevkit", &[]);
    let testkit = call("sceKernelIsTestKit", &[]);
    let neo = call("sceKernelIsNeoMode", &[]);
    let development = call("sceKernelIsDevelopmentMode", &[]);

    assert_ne!(retail, 0, "this presents as a retail unit");
    assert_eq!(devkit, 0);
    assert_eq!(testkit, 0);
    assert_eq!(neo, 0, "and the base hardware revision");
    assert_eq!(development, 0);
}

/// Each of these is a boolean, so none may answer the placeholder.
///
/// The placeholder is non-zero and reads as yes; asserting the answer is small enough to be a
/// boolean catches a function dropped from the table (D393).
#[test]
fn no_console_kind_answers_a_placeholder() {
    for name in [
        "sceKernelIsCex",
        "sceKernelIsDevkit",
        "sceKernelIsTestKit",
        "sceKernelIsNeoMode",
        "sceKernelIsDevelopmentMode",
    ] {
        let answered = call(name, &[]);
        assert!(
            answered <= 1,
            concat!(
                "{} answered {:#x}, which is not a boolean - an unimplemented ",
                "one answers a placeholder and every placeholder reads as true"
            ),
            name,
            answered
        );
    }
}

/// The counter runs forward, and its reported frequency is usable.
///
/// A guest divides by the frequency, so zero is not an answer.
#[test]
fn the_timestamp_counter_runs_forward_at_a_stated_frequency() {
    let frequency = call("sceKernelGetTscFrequency", &[]);
    assert!(frequency > 0, "a guest divides by this");

    let first = call("sceKernelReadTsc", &[]);
    let second = call("sceKernelReadTsc", &[]);
    assert!(second >= first, "the counter must not go backwards");
}

/// Process time is monotonic and measured from this process, not from an epoch.
///
/// A wall clock would run backwards when the host's clock is corrected, and an epoch would
/// make two runs incomparable.
#[test]
fn process_time_is_monotonic_and_starts_near_zero() {
    // A century of microseconds, which an epoch-based value would exceed and a process-relative
    // one cannot reach.
    const A_CENTURY: u64 = 100 * 365 * 24 * 60 * 60 * 1_000_000;

    let first = call("sceKernelGetProcessTime", &[]);
    let second = call("sceKernelGetProcessTime", &[]);
    assert!(second >= first, "process time must not go backwards");

    assert!(
        first < A_CENTURY,
        "{first} looks like an epoch rather than time since this process began"
    );
}

/// A sleep of nothing returns rather than blocking.
#[test]
fn a_sleep_of_nothing_returns_at_once() {
    let started = std::time::Instant::now();
    assert_eq!(call("posix_usleep", &[0]), OK);
    assert_eq!(call("sceKernelUsleep", &[0]), OK);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "a zero sleep should not have waited"
    );
}

/// `sceKernelIsStack` reports the span through its out-parameters and answers zero.
///
/// On the hardware it answers `0` for a local and a static alike and writes the stack bounds
/// to the two words after the address. The span is process-wide, so this is one test; with
/// no span recorded there is nothing to report.
#[test]
fn is_stack_reports_the_span_that_was_noted_and_answers_zero() {
    let base = 0x7000_0000_u64;
    let len = 0x1_0000_u64;
    orbistoun_kernel::note_stack_span(base, len);

    let mut low = 0xA5A5_A5A5_A5A5_A5A5_u64;
    let mut high = 0xA5A5_A5A5_A5A5_A5A5_u64;
    let (low_at, high_at) = (
        std::ptr::from_mut(&mut low) as u64,
        std::ptr::from_mut(&mut high) as u64,
    );

    assert_eq!(
        call("sceKernelIsStack", &[base, low_at, high_at]),
        0,
        "a successful call answers zero"
    );
    assert_eq!(low, base, "the low bound is the base of the span");
    assert_eq!(high, base + len, "and the high bound is one past its end");

    // The same for an address outside the stack: the hardware does not distinguish.
    low = 0;
    high = 0;
    assert_eq!(call("sceKernelIsStack", &[base - 1, low_at, high_at]), 0);
    assert_eq!((low, high), (base, base + len));

    // Null out-parameters are how a caller says it wants neither, and must not fault.
    assert_eq!(call("sceKernelIsStack", &[base, 0, 0]), 0);
}
