//! libc's `setjmp` and `longjmp` leaves, called through stubs as a guest calls them (D734).
//!
//! Its own binary because the handler, leaf and diagnostic tables are process-global.

use orbistoun_core::{GUEST_ARG_REGISTERS, GUEST_PAGE_SIZE, GuestFn, LeafFn};
use orbistoun_mem::test_bases::{Range, crates};
use orbistoun_thunk::{ThunkTable, dispatch};

static RANGE: Range = Range::nth(crates::SERVICE);

fn unused(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

fn leaf(name: &str) -> LeafFn {
    orbistoun_libc::jmp::leaves()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| panic!("{name} is a leaf"), |(_, f)| *f)
}

/// A guest's `if (setjmp(env) == 0) longjmp(env, value);` through the two stubs: the second
/// arrival at the `setjmp` call site answers `value` (1 for 0), with the callee-saved register the
/// code set before `setjmp` back as it was, though it was changed before `longjmp`, and the
/// `jmp_buf` holding that register as a collector scanning it would read it.
#[test]
fn a_longjmp_resumes_where_setjmp_returned() {
    dispatch::install_handlers(vec![Some(unused as GuestFn); 2]);
    dispatch::install_leaves(vec![Some(leaf("setjmp")), Some(leaf("longjmp"))]);
    dispatch::install_forced_dumps(vec![false, false]);
    let table = ThunkTable::build(RANGE.take(), 2, GUEST_PAGE_SIZE).expect("build the table");
    assert_eq!(table.link_leaves().expect("linked"), 2);
    let setjmp = table.address_of(0).expect("in range");
    let longjmp = table.address_of(1).expect("in range");

    for (value, answered) in [(42_u64, 42_u64), (0, 1)] {
        let mut env = [0_u64; 12];
        let resumed: u64;
        let kept: u64;
        // SAFETY: both addresses are leaf-linked stubs following the System V convention. The
        // stack pointer is kept in a callee-saved register, and `longjmp` restores it with the
        // rest, so control and the stack come back to the instruction after the `setjmp` call;
        // everything the callees may change is declared clobbered.
        unsafe {
            core::arch::asm!(
                "mov r12, rsp",
                "and rsp, -16",
                "mov r13, 0x5eed",
                "mov rdi, {env}",
                "call {setjmp}",
                "test rax, rax",
                "jnz 2f",
                "mov r13, 0xbad",
                "mov rdi, {env}",
                "mov rsi, {value}",
                "call {longjmp}",
                "ud2",
                "2:",
                "mov rsp, r12",
                env = in(reg) env.as_mut_ptr(),
                setjmp = in(reg) setjmp,
                longjmp = in(reg) longjmp,
                value = in(reg) value,
                out("r12") _,
                out("r13") kept,
                out("rax") resumed,
                clobber_abi("sysv64"),
            );
        }
        assert_eq!(resumed, answered, "longjmp({value})");
        assert_eq!(kept, 0x5eed, "r13 as it was at setjmp");
        assert_eq!(env[5], 0x5eed, "r13 spilled into the buffer");
        assert_eq!(env[9], orbistoun_libc::jmp::JMP_MAGIC);
    }
}
