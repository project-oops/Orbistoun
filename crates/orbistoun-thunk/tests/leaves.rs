//! Executes leaf-linked stubs (D734).
//!
//! Its own binary because the handler, leaf and diagnostic tables are process-global.

use orbistoun_core::{GUEST_ARG_REGISTERS, GUEST_PAGE_SIZE, GuestFn, LeafFn};
use orbistoun_thunk::{ThunkTable, dispatch, total_calls};

/// How the guest sees a stub: an ordinary System V function of six integers.
type GuestCall = extern "sysv64" fn(u64, u64, u64, u64, u64, u64) -> u64;

/// Reinterprets a stub address as something callable.
fn callable(address: u64) -> GuestCall {
    let pointer: *const () =
        std::ptr::with_exposed_provenance(usize::try_from(address).expect("fits"));
    // SAFETY: `address` came from `ThunkTable::address_of`: the start of a stub the table wrote
    // and mapped read-execute, encoded to the System V convention this type declares. The table
    // outlives the call.
    unsafe { std::mem::transmute::<*const (), GuestCall>(pointer) }
}

/// Far from the other integration binaries' tables.
const TABLE_BASE: u64 = 0x0000_5200_0000;

/// The traced implementation: what a call answers on the traced path.
fn traced(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    args[0].wrapping_add(1)
}

/// The leaf: a different answer, so which path ran is visible. Sums every argument register, so
/// one arriving in the wrong register shows.
extern "sysv64" fn leaf(
    first: u64,
    second: u64,
    third: u64,
    fourth: u64,
    fifth: u64,
    sixth: u64,
) -> u64 {
    0x1ea0_0000
        + first
        + (second << 4)
        + (third << 8)
        + (fourth << 12)
        + (fifth << 16)
        + (sixth << 20)
}

/// A leaf that answers `rsp` mod 16 as it was on entry: eight in a correctly aligned call.
#[unsafe(naked)]
extern "sysv64" fn entry_alignment(_: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    core::arch::naked_asm!("mov rax, rsp", "and rax, 15", "ret")
}

/// Calls `address` the way a guest that ignores the alignment rule would: `rsp` eight past a
/// multiple of sixteen at the `call`, where the rule wants a multiple, so the callee enters with it
/// off by eight.
fn call_misaligned(address: u64) -> u64 {
    let answer: u64;
    // SAFETY: `address` is a mapped stub following the System V convention; the stack pointer is
    // saved in a callee-saved register and restored, and every register the callee may change is
    // declared clobbered.
    unsafe {
        core::arch::asm!(
            "mov r12, rsp",
            "and rsp, -16",
            "sub rsp, 8",
            "call {target}",
            "mov rsp, r12",
            target = in(reg) address,
            out("r12") _,
            out("rax") answer,
            clobber_abi("sysv64"),
        );
    }
    answer
}

/// A stub with a leaf and no diagnostic calls the leaf with the guest's argument registers, is
/// counted, and spends no sequence number; one a diagnostic names keeps the traced path, as does
/// one with no leaf.
#[test]
fn a_leaf_stub_calls_its_leaf_and_a_named_one_stays_traced() {
    let handlers: Vec<Option<GuestFn>> = vec![Some(traced); 4];
    dispatch::install_handlers(handlers);
    let leaves: Vec<Option<LeafFn>> = vec![Some(leaf), None, Some(leaf), Some(entry_alignment)];
    dispatch::install_leaves(leaves);
    // Slot two is named for a dump, so it keeps the traced path.
    dispatch::install_forced_dumps(vec![false, false, true, false]);
    let table = ThunkTable::build(TABLE_BASE, 4, GUEST_PAGE_SIZE).expect("build the table");
    assert_eq!(
        table.link_leaves().expect("linked"),
        2,
        "slots zero and three"
    );

    let before = total_calls();
    let leafed = callable(table.address_of(0).expect("in range"));
    assert_eq!(leafed(1, 2, 3, 4, 5, 6), 0x1ea0_0000 + 0x0065_4321);
    assert_eq!(total_calls(), before, "no sequence number spent");
    assert_eq!(dispatch::call_counts()[0], 1, "but counted");

    // A leaf enters aligned whatever the guest left the stack at.
    let aligned = table.address_of(3).expect("in range");
    assert_eq!(
        callable(aligned)(0, 0, 0, 0, 0, 0),
        8,
        "from an aligned call"
    );
    assert_eq!(call_misaligned(aligned), 8, "from a misaligned one");

    let plain = callable(table.address_of(1).expect("in range"));
    assert_eq!(plain(7, 0, 0, 0, 0, 0), 8, "no leaf: traced");
    let named = callable(table.address_of(2).expect("in range"));
    assert_eq!(named(7, 0, 0, 0, 0, 0), 8, "named by a diagnostic: traced");
    assert_eq!(total_calls(), before + 2);
}

/// Prints the round-trip cost of a leaf call beside a traced one - measured, not asserted.
#[test]
#[ignore = "a measurement, printed rather than asserted"]
fn a_leaf_call_costs_less_than_a_traced_one() {
    let handlers: Vec<Option<GuestFn>> = vec![Some(traced); 2];
    dispatch::install_handlers(handlers);
    dispatch::install_leaves(vec![Some(leaf), None]);
    dispatch::install_forced_dumps(vec![false, false]);
    let table = ThunkTable::build(TABLE_BASE, 2, GUEST_PAGE_SIZE).expect("build the table");
    table.link_leaves().expect("linked");
    for (slot, what) in [(0, "leaf"), (1, "traced")] {
        let call = callable(table.address_of(slot).expect("in range"));
        let calls = 5_000_000u64;
        let started = std::time::Instant::now();
        let mut sum = 0u64;
        for i in 0..calls {
            sum = sum.wrapping_add(call(i, 0, 0, 0, 0, 0));
        }
        let each = started.elapsed().as_nanos() / u128::from(calls);
        println!("one {what} guest call: {each} ns round trip ({sum})");
    }
}
