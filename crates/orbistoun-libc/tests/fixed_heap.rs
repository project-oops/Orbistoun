//! The heap at an address the host did not choose.
//!
//! # Why this is one test in its own binary
//!
//! `ORBISTOUN_HEAP_BASE` is read once per process and the region is built once, so a test
//! that sets it changes every other test in the same binary. More than that, the property
//! under test is **an exact address**, which only holds while nothing else has allocated
//! through the guest allocator first.
//!
//! So: one binary, one test function, and the variable set before the first call. Splitting
//! it into several `#[test]`s would run them on parallel threads in an order nobody
//! controls, and the first assertion below would then be asserting about whichever test won
//! the race (`docs/TESTING.md`).

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Where this test puts its region.
///
/// Far from the bases the loader and the thunk table suggest, so a failure here is never a
/// conflict with something else that happened to be mapped.
const BASE: u64 = 0x0000_6E00_0000_0000;

/// How far the region reaches, mirroring `arena::SPAN`, which is crate-private.
///
/// Written out rather than exported: a constant exported only for a test is a claim the
/// test makes about the implementation rather than about behaviour. If the module's span
/// changes and this does not, the exhaustion step below stops exhausting - which is why
/// that step asserts the *summary says it spilled* rather than counting allocations.
const SPAN: u64 = 64 * 1024 * 1024;

/// The alignment `malloc` allocates with, which is also the header size (D190).
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

/// A pointer from the fixed heap is decided by the base, not by where the host put its heap.
///
/// # What this proves and what it cannot
///
/// It proves the address is a **function of the configured base** - the first block lands at
/// exactly `base + 16` on every run, on every machine, whatever the host allocator was going
/// to do. That is the whole point: D499's surviving candidate is a guest branching on a
/// pointer value, and a pointer value that is host-chosen differs run to run.
///
/// It cannot prove the guest's *own* run becomes deterministic. Only twelve runs of the
/// guest can say that, and this test is the instrument those runs use rather than the
/// finding itself.
#[test]
fn a_fixed_heap_hands_out_addresses_the_host_did_not_choose() {
    // SAFETY: set before any thread in this process reads the environment - this binary has
    // exactly one test, and nothing runs before it.
    unsafe { std::env::set_var("ORBISTOUN_HEAP_BASE", format!("{BASE:x}")) };

    // --- the address is the base's, exactly -------------------------------------------
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

    // --- and it is real, writable memory ----------------------------------------------
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

    // --- freeing is a no-op, which is the documented behaviour ------------------------
    call("free", &[first]);
    let after = call("malloc", &[64]);
    assert_ne!(
        after, first,
        "the region never reuses a block, so a fresh allocation must not land on a freed one"
    );
    // SAFETY: the region never returns a block, so the freed one is still mapped and still
    // holds what was written. Reading it is the assertion.
    let survives = unsafe {
        std::ptr::read(std::ptr::with_exposed_provenance::<u8>(
            (first + 63) as usize,
        ))
    };
    assert_eq!(
        survives, 63,
        "free must not have handed the block to the host allocator"
    );

    // --- realloc copies across, with both blocks in the region ------------------------
    let grown = call("realloc", &[after, 256]);
    assert!(
        (BASE..BASE + SPAN).contains(&grown),
        "a grown block stays in the region, got {grown:#x}"
    );

    // --- exhaustion is reported, never hidden -----------------------------------------
    // Sixty-five mebibyte blocks cannot fit in sixty-four mebibytes, so at least one of
    // these must spill to the host heap whatever the region's exact span is.
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
