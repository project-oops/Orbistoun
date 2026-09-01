//! What a thunk answers, and what it does on the way there.
//!
//! # Why a second integration binary
//!
//! Every table here is a `OnceLock` - handlers, stub returns, forced returns, planted
//! writes, readable and writable ranges - because a thunk has no register left to carry a
//! context pointer, so all of it is process-global and installed once. That means **one
//! configuration per process**, and the existing `executes.rs` already owns one. This is a
//! second process with a different one.
//!
//! It also means the stateful assertions live in a single test: two `#[test]` functions
//! installing tables would race to set the same lock, and the loser's install is a silent
//! no-op rather than an error. The pure functions are tested separately, since they touch
//! nothing.
//!
//! # What it is really checking
//!
//! The precedence of the four things that can answer a call, which is the part no unit test
//! can reach: a forced diagnostic answer beats an implementation, a region answer beats a
//! scalar one, a scalar policy answer beats the error code, and the error code is what is
//! left. Every layer here exists because a previous one could not express something, and
//! each was added without removing the last - so the order they are consulted in is the
//! whole behaviour.

use orbistoun_core::{
    GUEST_ARG_REGISTERS, GUEST_FLOAT_REGISTERS, GUEST_PAGE_SIZE, GuestError, GuestFloatFn, GuestFn,
};
use orbistoun_thunk::{ThunkTable, dispatch};

/// How the guest sees a thunk: an ordinary System V function of six integers.
type GuestCall = extern "sysv64" fn(u64, u64, u64, u64, u64, u64) -> u64;

/// Reinterprets a thunk address as something callable.
fn callable(address: u64) -> GuestCall {
    let pointer: *const () =
        std::ptr::with_exposed_provenance(usize::try_from(address).expect("fits"));
    // SAFETY: `address` came from `ThunkTable::address_of`, so it is the start of a stub the
    // table wrote and then mapped read-execute. The stub is encoded to the System V
    // convention this type declares, and the table outlives the call.
    unsafe { std::mem::transmute::<*const (), GuestCall>(pointer) }
}

/// A base far from anything the host is likely to have mapped.
///
/// Not `SUGGESTED_BASE`: the other integration binary uses that, and while these are
/// separate processes, picking a different one keeps the two from ever being confused for
/// each other in a fault report.
const TABLE_BASE: u64 = 0x0000_5000_0000;

/// How many imports this run declares.
const IMPORTS: usize = 8;

// Indices, each configured differently. Named because the assertions below are unreadable
// as bare numbers, and a wrong one would still pass some of them.
/// An ordinary implemented import.
const IMPLEMENTED: usize = 0;
/// One whose answer arrives in a floating-point register.
const FLOATING: usize = 1;
/// Unimplemented, with a scalar answer from the policy.
const SCALAR_ANSWER: usize = 2;
/// Unimplemented, with nothing configured at all.
const BARE: usize = 3;
/// Implemented, with a forced diagnostic answer over the top.
const FORCED_OVER_HANDLER: usize = 4;
/// Unimplemented, and plants a value before answering.
const PLANTS: usize = 5;
/// Unimplemented, used to look at what the guest was pointing at.
const DUMPED: usize = 6;
/// Unimplemented, with both a scalar answer and a region answer.
const REGION_ANSWER: usize = 7;

/// What the implemented import answers: something no other layer would produce.
const HANDLER_ANSWER: u64 = 0x1111_2222_3333_4444;
/// What the forced diagnostic answers instead.
const FORCED_ANSWER: u64 = 0x0FCE_D000_0000_0001;
/// What the policy says the scalar-answering import returns.
const SCALAR_VALUE: u64 = 0;
/// The region base the policy hands back, which must beat the scalar answer.
const REGION_BASE: u64 = 0x0000_7777_0000;
/// The word a plant stores.
const PLANTED: u64 = 0xFEED_FACE_CAFE_BEEF;

/// An implemented import: answers a constant, and proves it ran by seeing its arguments.
fn handler(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    assert_eq!(
        args[0], 0xA11CE,
        "the handler must see the guest's arguments"
    );
    HANDLER_ANSWER
}

/// The one whose implementation is shadowed by a forced answer.
///
/// It still records that it ran, because the implementation is supposed to execute and only
/// its *answer* be replaced - skipping it would suppress every side effect too.
static HANDLER_RAN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn shadowed_handler(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    HANDLER_RAN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    0xDEAD
}

/// An import that answers in a floating-point register.
fn float_handler(ints: &[u64; GUEST_ARG_REGISTERS], _floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64 {
    // Reads an integer register, which is what makes this different from a pure maths
    // function and worth carrying both arrays for.
    ints[0].wrapping_add(1)
}

// --- the pure rules ------------------------------------------------------------------------

/// The alignment rule, which is a fact about the convention rather than about a run.
///
/// **Pure so it can be tested without a guest**, and that matters here more than usual: the
/// code that uses it runs inside a naked trampoline, where a mistake is invisible until it
/// is catastrophic.
///
/// A `call` pushes eight bytes onto a sixteen-byte-aligned stack, so a callee's first
/// instruction always sees a remainder of eight. Every other remainder is impossible.
#[test]
fn the_entry_alignment_rule_admits_exactly_one_remainder() {
    assert!(dispatch::entry_alignment_conforms(
        dispatch::EXPECTED_ENTRY_REMAINDER
    ));
    assert!(dispatch::entry_alignment_conforms(0x7FFF_FFF8));
    assert!(dispatch::entry_alignment_conforms(8));

    for bad in [0_u64, 4, 12, 16, 0x7FFF_FFF0] {
        assert!(
            !dispatch::entry_alignment_conforms(bad),
            "{bad:#x} has remainder {} and only 8 conforms",
            bad % 16
        );
    }
}

/// Only a mapped argument carries bytes.
///
/// The three-way distinction is the point: a scalar and an address pointing at nothing this
/// run mapped were once both reported as an empty buffer, so a pointer read as a count and
/// the tool doing the diagnosing was quietly reporting the wrong kind of thing (D217).
#[test]
fn only_a_mapped_argument_is_reported_as_carrying_bytes() {
    assert!(dispatch::Pointing::Mapped.was_read());
    assert!(!dispatch::Pointing::Scalar.was_read());
    assert!(!dispatch::Pointing::Unreadable.was_read());
    assert_ne!(dispatch::Pointing::Scalar, dispatch::Pointing::Unreadable);
}

// --- the whole configuration, in one process ---------------------------------------------------

/// Everything the tables can do to a call, in the order they are consulted.
///
/// One test because the tables are process-global and set once. Assertions are grouped by
/// what they are about rather than by call order, and each says which layer it is pinning.
///
/// Long, and it has to be: splitting it into several `#[test]` functions is exactly what the
/// `OnceLock` tables forbid, and splitting it into helpers would hand each one the buffers,
/// the table and the call closure - more machinery than the thing it would be organising.
#[allow(clippy::too_many_lines)]
#[test]
fn what_answers_a_call_and_in_what_order() {
    // Guest memory this run declares. Held for the whole test, because the addresses handed
    // to the trampoline point into them.
    let mut readable = vec![0_u8; 256];
    for (at, byte) in readable.iter_mut().enumerate() {
        *byte = at as u8;
    }
    let readable_at = readable.as_mut_ptr().expose_provenance() as u64;

    let mut writable = vec![0_u64; 16];
    let writable_at = writable.as_mut_ptr().expose_provenance() as u64;

    // --- install, before anything is called ---------------------------------------------

    let mut handlers: Vec<Option<GuestFn>> = vec![None; IMPORTS];
    handlers[IMPLEMENTED] = Some(handler);
    handlers[FORCED_OVER_HANDLER] = Some(shadowed_handler);
    dispatch::install_handlers(handlers);

    let mut floats: Vec<Option<GuestFloatFn>> = vec![None; IMPORTS];
    floats[FLOATING] = Some(float_handler);
    dispatch::install_float_handlers(floats);

    // The scalar answers first, so the region answers below land in the second table and can
    // be shown to win. Installed in this order deliberately: it is the order a real run
    // installs them, and the reason the second table exists at all (D300).
    let mut stubs: Vec<Option<u64>> = vec![None; IMPORTS];
    stubs[SCALAR_ANSWER] = Some(SCALAR_VALUE);
    stubs[REGION_ANSWER] = Some(SCALAR_VALUE);
    dispatch::install_stub_returns(stubs);
    dispatch::install_policy_returns(vec![(REGION_ANSWER, REGION_BASE)]);

    let mut forced: Vec<Option<u64>> = vec![None; IMPORTS];
    forced[FORCED_OVER_HANDLER] = Some(FORCED_ANSWER);
    dispatch::install_forced_returns(forced);

    dispatch::install_readable_ranges(vec![(readable_at, readable.len() as u64)]);
    dispatch::install_writable_ranges(vec![(writable_at, (writable.len() * 8) as u64)]);

    let mut plants: Vec<dispatch::ForcedWrite> = (0..IMPORTS).map(|_| Vec::new().into()).collect();
    plants[PLANTS] = vec![
        // Lands: argument zero points into the writable buffer.
        dispatch::Plant {
            position: 0,
            offset: 0,
            value: PLANTED,
        },
        // Also lands, one word along, which is what makes a single run able to eliminate
        // several candidate slots at once.
        dispatch::Plant {
            position: 0,
            offset: 8,
            value: PLANTED ^ 1,
        },
        // Refused: argument one is a small number, not an address in the writable range.
        dispatch::Plant {
            position: 1,
            offset: 0,
            value: PLANTED,
        },
        // Refused: there is no seventh argument register to read a pointer from.
        dispatch::Plant {
            position: 9,
            offset: 0,
            value: PLANTED,
        },
    ]
    .into();
    dispatch::install_policy_writes(plants);

    let table = ThunkTable::build(TABLE_BASE, IMPORTS, GUEST_PAGE_SIZE).expect("build the table");
    let at = |index: usize| callable(table.address_of(index).expect("in range"));

    // --- what each layer answers ---------------------------------------------------------

    assert_eq!(
        at(IMPLEMENTED)(0xA11CE, 0, 0, 0, 0, 0),
        HANDLER_ANSWER,
        "an implemented import answers through its handler"
    );

    assert_eq!(
        at(BARE)(0, 0, 0, 0, 0, 0),
        u64::from(GuestError::Unimplemented.as_raw()),
        "with nothing configured, the answer is never zero - which a guest reads as success"
    );

    assert_eq!(
        at(SCALAR_ANSWER)(0, 0, 0, 0, 0, 0),
        SCALAR_VALUE,
        concat!(
            "a policy answer replaces the error code, which for a pointer-returning function ",
            "would be a wild pointer the guest dereferences (D125)"
        )
    );

    assert_eq!(
        at(REGION_ANSWER)(0, 0, 0, 0, 0, 0),
        REGION_BASE,
        concat!(
            "a region answer beats the scalar one: `ok` is what a caller tests, a region is what ",
            "it uses (D300)"
        )
    );

    assert_eq!(
        at(FLOATING)(41, 0, 0, 0, 0, 0),
        42,
        concat!(
            "a function answering in xmm0 also leaves its answer in rax, so a caller reading ",
            "either is not handed a stale one (D268)"
        )
    );

    // --- the forced answer, and the fact the implementation still ran ---------------------

    assert_eq!(
        at(FORCED_OVER_HANDLER)(0, 0, 0, 0, 0, 0),
        FORCED_ANSWER,
        "a forced diagnostic answer beats an implementation's own"
    );
    assert_eq!(
        HANDLER_RAN.load(std::sync::atomic::Ordering::Relaxed),
        1,
        concat!(
            "and the implementation still ran - suppressing it would suppress every side effect ",
            "too, and a moved fault would then say only that the program was changed (D234)"
        )
    );
    assert_eq!(
        dispatch::forced_return_count(),
        1,
        concat!(
            "a forced return that matched nothing must be visible rather than inferred from an ",
            "unchanged run (D230)"
        )
    );

    // --- what a stub does before it answers -------------------------------------------------

    assert_eq!(
        at(PLANTS)(writable_at, 7, 0, 0, 0, 0),
        u64::from(GuestError::Unimplemented.as_raw()),
        "planting changes what the call does, not what it answers"
    );
    assert_eq!(writable[0], PLANTED, "the first plant landed");
    assert_eq!(writable[1], PLANTED ^ 1, "and so did the one at an offset");

    let (done, refused) = dispatch::forced_write_counts();
    assert_eq!(done, 2, "two of the four plants had somewhere to go");
    assert_eq!(
        refused, 2,
        concat!(
            "and the other two are counted - a diagnostic that silently did nothing is ",
            "indistinguishable from one that ran and changed the answer"
        )
    );

    // --- what the guest was pointing at ------------------------------------------------------

    // One unimplemented call with one of each kind of argument.
    let unmapped = readable_at + 0x10_0000;
    at(DUMPED)(readable_at, 5, unmapped, 0, 0, 0);

    let dumps = dispatch::argument_dumps();
    let mine: Vec<_> = dumps
        .iter()
        .filter(|d| d.index as usize == DUMPED)
        .collect();
    assert!(
        !mine.is_empty(),
        "an unimplemented call must have been dumped, or the rest of this proves nothing"
    );

    let slot = |n: u8| {
        mine.iter()
            .find(|d| d.slot == n)
            .unwrap_or_else(|| panic!("argument {n} was not dumped"))
    };

    let pointer = slot(0);
    assert_eq!(pointer.pointing, dispatch::Pointing::Mapped);
    assert_eq!(pointer.address, readable_at);
    assert_eq!(
        pointer.bytes[..4],
        [0, 1, 2, 3],
        "the bytes the guest was pointing at, as they were at the moment of the call"
    );

    assert_eq!(
        slot(1).pointing,
        dispatch::Pointing::Scalar,
        "a size, a flag or a count is evidence in its own right (D198)"
    );
    assert_eq!(slot(1).address, 5);

    assert_eq!(
        slot(2).pointing,
        dispatch::Pointing::Unreadable,
        concat!(
            "address-shaped, and no declared region holds it - either the address is wrong or ",
            "the run did not declare where it points, and neither is a count (D217)"
        )
    );

    // --- what a run can say about itself -------------------------------------------------------

    assert!(
        dispatch::is_implemented(IMPLEMENTED),
        "an integer implementation counts as implemented"
    );
    assert!(
        dispatch::is_implemented(FLOATING),
        concat!(
            "and so does one answering in a floating-point register - checking only the integer ",
            "table reported every maths function as unimplemented while it worked (D268, D290)"
        )
    );
    assert!(!dispatch::is_implemented(BARE));
    assert!(
        !dispatch::is_implemented(IMPORTS + 100),
        "an index past the table is not implemented rather than a panic"
    );
    assert_eq!(
        dispatch::implemented_count(),
        3,
        concat!(
            "two integer handlers and one floating-point one, summed rather than unioned - the ",
            "two tables are disjoint by construction (D268)"
        )
    );

    // --- the calling convention --------------------------------------------------------------

    let conformance = dispatch::abi_conformance();
    assert_eq!(
        conformance.misaligned_calls, 0,
        "every call here came from ordinary Rust, which obeys the convention"
    );
    assert_eq!(conformance.first_misaligned, None);

    // Every call above is in the trace, in order, whether or not anything implemented it.
    let calls = orbistoun_thunk::recorded_calls();
    assert_eq!(
        calls.iter().map(|c| c.index as usize).collect::<Vec<_>>(),
        vec![
            IMPLEMENTED,
            BARE,
            SCALAR_ANSWER,
            REGION_ANSWER,
            FLOATING,
            FORCED_OVER_HANDLER,
            PLANTS,
            DUMPED,
        ],
        concat!(
            "losing visibility of a call the moment it starts working would hide exactly the ",
            "traffic worth understanding"
        )
    );
    assert_eq!(orbistoun_thunk::total_calls(), 8);

    // The buffers have to outlive every call that was handed their addresses.
    drop(readable);
    drop(writable);
}
