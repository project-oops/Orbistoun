//! Two tier-1 names of the REQ-cn01 census that are variants of calls already modelled:
//! `sceKernelCheckedReleaseDirectMemory` is the release that refuses a span not allocated, as
//! `sceKernelReleaseDirectMemory` already does (PPSA02664, PPSA03416, PPSA25872), and
//! `sceUltConditionVariableSignal` is `_sceUltConditionVariableSignal` without the underscore
//! (PPSA28061). Assumed, not measured.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn a_checked_release_returns_an_allocated_span_once() {
    let mut start = 0_u64;
    let out = std::ptr::addr_of_mut!(start) as usize as u64;
    assert_eq!(
        call(
            "sceKernelAllocateDirectMemory",
            [0, 0x1_0000_0000, 0x10000, 0x10000, 0, out]
        ),
        0
    );
    let release = [start, 0x10000, 0, 0, 0, 0];
    assert_eq!(call("sceKernelCheckedReleaseDirectMemory", release), 0);
    assert_ne!(
        call("sceKernelCheckedReleaseDirectMemory", release),
        0,
        "a span no longer allocated is refused"
    );
}

#[test]
fn the_plain_signal_wakes_through_a_created_condition_variable() {
    let mut mutex = [0_u8; 0x100];
    let mut cv = [0_u8; 0x100];
    let (m, c) = (
        mutex.as_mut_ptr() as usize as u64,
        cv.as_mut_ptr() as usize as u64,
    );
    let name = c"census".as_ptr() as usize as u64;
    assert_eq!(call("_sceUltMutexCreate", [m, name, 0, 0, 0, 0]), 0);
    assert_eq!(
        call("_sceUltConditionVariableCreate", [c, name, m, 0, 0, 0]),
        0
    );
    assert_eq!(call("sceUltConditionVariableSignal", [c, 0, 0, 0, 0, 0]), 0);
}
