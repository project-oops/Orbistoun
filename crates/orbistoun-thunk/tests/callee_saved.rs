//! A recorded call keeps the guest's callee-saved registers.
//!
//! A thread waiting in a loop around one call holds the objects that loop reads in its callee-saved
//! registers, and the call is the only place orbistoun sees them (D832). Its own binary because the
//! recording statics are process-global.

use orbistoun_core::GUEST_PAGE_SIZE;
use orbistoun_thunk::{SUGGESTED_BASE, ThunkTable, recorded_calls};

/// Calls `address` with `rbx`, `rbp` and `r12` to `r15` holding `values`, in that order, as a
/// guest's loop would hold its own state across the call.
fn call_holding(address: u64, values: [u64; 6]) {
    // SAFETY: `address` is a mapped stub following the System V convention. `rbx` and `rbp` are
    // saved and restored around it, the stack pointer is kept in memory and restored, and every
    // other register the callee may change is declared clobbered.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rbp",
            "mov rax, rsp",
            "and rsp, -16",
            "push rax",
            "push rax",
            "mov rbx, qword ptr [{values}]",
            "mov rbp, qword ptr [{values} + 8]",
            "mov r12, qword ptr [{values} + 16]",
            "mov r13, qword ptr [{values} + 24]",
            "mov r14, qword ptr [{values} + 32]",
            "mov r15, qword ptr [{values} + 40]",
            "call {target}",
            "pop rax",
            "mov rsp, rax",
            "pop rbp",
            "pop rbx",
            values = in(reg) values.as_ptr(),
            target = in(reg) address,
            out("r12") _,
            out("r13") _,
            out("r14") _,
            out("r15") _,
            clobber_abi("sysv64"),
        );
    }
}

/// What the guest held in `rbx`, `rbp` and `r12` to `r15` at a call is recorded with it.
#[test]
fn a_recorded_call_keeps_the_guests_callee_saved_registers() {
    let table = ThunkTable::build(SUGGESTED_BASE, 4, GUEST_PAGE_SIZE).expect("build the table");
    let values = [0x1111, 0x2222, 0x3333, 0x4444, 0x5555, 0x6666];
    call_holding(table.address_of(2).expect("in range"), values);
    let calls = recorded_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].index, 2);
    assert_eq!(calls[0].saved, values, "rbx, rbp, r12, r13, r14, r15");
}
