//! The heap at an address the host did not choose.
//!
//! `ORBISTOUN_HEAP_BASE` is read once per process and the property under test is an exact
//! address, which holds only while nothing else has allocated first. So this binary has one
//! test function, and the variable is set before the first call (`docs/TESTING.md`).

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Where this test puts its region, far from the bases the loader and the thunk table use.
const BASE: u64 = 0x0000_6E00_0000_0000;

/// How far the region reaches, mirroring the crate-private `arena::SPAN`.
///
/// The exhaustion step asserts that the summary reports a spill rather than counting
/// allocations, so it holds if the span changes.
const SPAN: u64 = 64 * 1024 * 1024;

/// The alignment `malloc` allocates with, which is also the header size (D128).
const HEADER: u64 = 16;

fn implementation(name: &str) -> GuestFn {
    orbistoun_libc::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(
            || panic!("{name} is not implemented, so nothing can call it"),
            |(_, f)| *f,
        )
}

fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0xDEAD_BEEF_DEAD_BEEF_u64; GUEST_ARG_REGISTERS];
    for (slot, value) in regs.iter_mut().zip(args) {
        *slot = *value;
    }
    implementation(name)(&regs)
}

/// A pointer from the fixed heap is decided by the configured base: the first block lands at
/// exactly `base + 16`, whatever the host allocator would have done.
#[test]
fn a_fixed_heap_hands_out_addresses_the_host_did_not_choose() {
    // SAFETY: set before any thread in this process reads the environment; this binary has
    // exactly one test, and nothing runs before it.
    unsafe { std::env::set_var("ORBISTOUN_HEAP_BASE", format!("{BASE:x}")) };

    // The address is the base's, exactly.
    let first = call("malloc", &[64]);
    assert_eq!(
        first,
        BASE + HEADER,
        concat!(
            "the first block must be the header's width into the region - anything else means ",
            "the host allocator answered"
        )
    );

    let second = call("malloc", &[64]);
    assert!(
        second > first && second < BASE + SPAN,
        "the second block must follow the first inside the region, got {second:#x}"
    );

    // And it is real, writable memory.
    for (offset, byte) in (0..64_u64).zip(0_u8..) {
        // SAFETY: inside the sixty-four bytes `malloc` just returned.
        unsafe {
            std::ptr::write(
                std::ptr::with_exposed_provenance_mut::<u8>((first + offset) as usize),
                byte,
            );
        }
    }
    // SAFETY: the last byte of that same block.
    let last = unsafe {
        std::ptr::read(std::ptr::with_exposed_provenance::<u8>(
            (first + 63) as usize,
        ))
    };
    assert_eq!(last, 63, "the region must be writable and read back");

    // Freeing is a no-op.
    call("free", &[first]);
    let after = call("malloc", &[64]);
    assert_ne!(
        after, first,
        "the region never reuses a block, so a fresh allocation must not land on a freed one"
    );
    // SAFETY: the region never returns a block, so the freed one is still mapped and still
    // holds what was written.
    let survives = unsafe {
        std::ptr::read(std::ptr::with_exposed_provenance::<u8>(
            (first + 63) as usize,
        ))
    };
    assert_eq!(
        survives, 63,
        "free must not have handed the block to the host allocator"
    );

    // Realloc copies across, with both blocks in the region.
    let grown = call("realloc", &[after, 256]);
    assert!(
        (BASE..BASE + SPAN).contains(&grown),
        "a grown block stays in the region, got {grown:#x}"
    );

    // Exhaustion is reported. Sixty-five mebibyte blocks cannot fit in sixty-four mebibytes, so
    // at least one spills to the host heap.
    let mut spilled_any = false;
    for _ in 0..65 {
        let block = call("malloc", &[1024 * 1024]);
        assert_ne!(block, 0, "a spill must still allocate, not fail");
        spilled_any |= !(BASE..BASE + SPAN).contains(&block);
    }
    assert!(spilled_any, "sixty-five mebibytes cannot fit in sixty-four");

    let summary = orbistoun_libc::heap_base_summary().expect("the variable is set");
    assert!(
        summary.contains(&format!("{BASE:#x}")),
        "the report names the base it was given: {summary}"
    );
    assert!(
        summary.contains("did NOT hold every address fixed"),
        concat!(
            "a run that spilled must say so - a partial fixing read as a whole one is the ",
            "conclusion this diagnostic must never support: {}"
        ),
        summary
    );
}
