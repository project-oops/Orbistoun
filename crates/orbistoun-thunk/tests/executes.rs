//! Executing real thunks.
//!
//! The unit tests assert the *encoding*; this asserts the encoding actually runs. An
//! instruction sequence that is wrong by one bit assembles into a plausible different
//! instruction, so nothing short of executing it proves the table works.
//!
//! Its own integration binary because the recording statics are process-global - a
//! thunk has no register left to carry a context pointer - so assertions about call
//! order only hold in a process that made exactly these calls.

use orbistoun_core::{GUEST_PAGE_SIZE, GuestError};
use orbistoun_thunk::{SUGGESTED_BASE, ThunkTable, recorded_calls, total_calls};

/// How the guest sees a thunk: an ordinary System V function of six integers.
type GuestCall = extern "sysv64" fn(u64, u64, u64, u64, u64, u64) -> u64;

/// Reinterprets a thunk address as something callable.
fn callable(address: u64) -> GuestCall {
    let pointer: *const () =
        std::ptr::with_exposed_provenance(usize::try_from(address).expect("fits"));
    // SAFETY: `address` came from `ThunkTable::address_of`, so it is the start of a
    // stub the table wrote and then mapped read-execute. The stub is encoded to the
    // System V convention this type declares, and the table outlives the call.
    unsafe { std::mem::transmute::<*const (), GuestCall>(pointer) }
}

#[test]
fn thunks_execute_record_their_own_index_and_answer_the_guest() {
    let table = ThunkTable::build(SUGGESTED_BASE, 16, GUEST_PAGE_SIZE).expect("build the table");

    // A stub past the end must not resolve to some other import's, which would produce
    // a call trace that is confidently wrong.
    assert!(table.address_of(16).is_none(), "out of range must be None");
    assert_eq!(table.len(), 16);
    assert!(!table.is_empty());

    // The call the whole design exists for: guest code enters a stub knowing nothing,
    // and comes back with an answer while we learn which import it wanted.
    let answered = callable(table.address_of(3).expect("in range"))(0xAAAA, 2, 3, 4, 5, 6);
    assert_eq!(
        answered,
        u64::from(GuestError::Unimplemented.as_raw()),
        "an unimplemented import must never answer zero, which reads as success"
    );

    // Order is the part that makes a boot trace readable; a histogram alone loses it.
    callable(table.address_of(7).expect("in range"))(0xBBBB, 0, 0, 0, 0, 0);
    callable(table.address_of(0).expect("in range"))(0xCCCC, 0, 0, 0, 0, 0);

    assert_eq!(total_calls(), 3);
    let calls = recorded_calls();
    assert_eq!(calls.len(), 3);

    assert_eq!(
        calls.iter().map(|c| c.index).collect::<Vec<_>>(),
        vec![3, 7, 0],
        "each stub must report its own index, including index zero"
    );
    assert_eq!(
        calls.iter().map(|c| c.arg0).collect::<Vec<_>>(),
        vec![0xAAAA, 0xBBBB, 0xCCCC],
        "the first argument must survive the trip through the trampoline"
    );
    assert_eq!(
        calls.iter().map(|c| c.sequence).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
}
