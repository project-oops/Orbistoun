//! `scePthreadAttrSetstack`: a thread created with a caller's stack runs on it.
//!
//! PPSA21564 carves a thread stack from its heap, makes the page below it a guard, and hands the
//! rest to `scePthreadAttrSetstack`; the engine asserts when that is refused.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

fn implementation(name: &str) -> GuestFn {
    orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| panic!("{name} is not implemented"), |(_, f)| *f)
}

fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[..args.len()].copy_from_slice(args);
    implementation(name)(&regs)
}

/// The thread body: answers the stack pointer it was entered with.
extern "sysv64" fn report_stack_pointer(_argument: u64) -> u64 {
    let rsp: u64;
    // SAFETY: reads the stack pointer register and nothing else.
    unsafe { std::arch::asm!("mov {}, rsp", out(reg) rsp) };
    rsp
}

#[test]
fn a_thread_runs_on_the_stack_its_attribute_names() {
    let stack = vec![0_u8; 0x8000];
    let (low, size) = (stack.as_ptr() as u64, stack.len() as u64);

    let mut attr = 0_u64;
    let attr_at = std::ptr::from_mut(&mut attr).expose_provenance() as u64;
    assert_eq!(call("scePthreadAttrInit", &[attr_at]), 0);
    assert_eq!(call("scePthreadAttrSetstack", &[attr_at, low, size]), 0);

    let mut handle = 0_u64;
    let handle_at = std::ptr::from_mut(&mut handle).expose_provenance() as u64;
    let entry = report_stack_pointer as extern "sysv64" fn(u64) -> u64 as usize as u64;
    let name = b"setstack\0";
    assert_eq!(
        call(
            "scePthreadCreate",
            &[handle_at, attr_at, entry, 0, name.as_ptr() as u64]
        ),
        0
    );
    let mut exit = 0_u64;
    let exit_at = std::ptr::from_mut(&mut exit).expose_provenance() as u64;
    assert_eq!(call("scePthreadJoin", &[handle, exit_at]), 0);
    assert!(
        exit > low && exit <= low + size,
        "the thread ran on its caller's stack: rsp {exit:#x}, stack {low:#x}..{:#x}",
        low + size
    );
}

/// A null stack address is refused.
#[test]
fn a_null_stack_is_refused() {
    let mut attr = 0_u64;
    let attr_at = std::ptr::from_mut(&mut attr).expose_provenance() as u64;
    assert_eq!(call("scePthreadAttrInit", &[attr_at]), 0);
    assert_ne!(call("scePthreadAttrSetstack", &[attr_at, 0, 0x8000]), 0);
}

/// The vendor spellings of the read-write lock attribute calls serve the same objects the POSIX
/// ones do: an attribute initialised through them initialises a lock that locks and unlocks.
#[test]
fn a_rwlock_built_from_vendor_attributes_locks() {
    let mut attr = 0_u64;
    let attr_at = std::ptr::from_mut(&mut attr).expose_provenance() as u64;
    assert_eq!(call("scePthreadRwlockattrInit", &[attr_at]), 0);
    assert_ne!(attr, 0, "an attribute handle");
    let mut lock = 0_u64;
    let lock_at = std::ptr::from_mut(&mut lock).expose_provenance() as u64;
    let name = b"rw\0";
    assert_eq!(
        call(
            "scePthreadRwlockInit",
            &[lock_at, attr_at, name.as_ptr() as u64]
        ),
        0
    );
    assert_eq!(call("scePthreadRwlockWrlock", &[lock_at]), 0);
    assert_eq!(call("scePthreadRwlockUnlock", &[lock_at]), 0);
    assert_eq!(call("scePthreadRwlockDestroy", &[lock_at]), 0);
    assert_eq!(call("scePthreadRwlockattrDestroy", &[attr_at]), 0);
}
