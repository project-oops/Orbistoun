//! The fixed heap handing blocks out downward.
//!
//! The direction is read once per process, from the same variable as the base, so it has its
//! own binary. Reversing only the order blocks come out in separates a guest depending on
//! fixed addresses from one depending on their ordering; this asserts the direction really
//! reverses.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Where this test puts its region, a different base from the ascending test's.
const BASE: u64 = 0x0000_6E01_0000_0000;

/// How far the region reaches, mirroring `arena::SPAN`.
const SPAN: u64 = 64 * 1024 * 1024;

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

/// Downward, every later block is at a lower address, and every block is still real memory.
#[test]
fn the_descending_region_really_descends() {
    // SAFETY: set before any thread in this process reads the environment; this binary has
    // exactly one test, and nothing runs before it.
    unsafe { std::env::set_var("ORBISTOUN_HEAP_BASE", format!("{BASE:x}-down")) };

    let mut previous = u64::MAX;
    for step in 0..8_u64 {
        let block = call("malloc", &[64]);
        assert!(
            (BASE..BASE + SPAN).contains(&block),
            "block {step} left the region: {block:#x}"
        );
        assert!(
            block < previous,
            concat!(
                "block {} at {:#x} must be below the one before it at {:#x} - ",
                "a region that did not reverse would pass every other assertion here"
            ),
            step,
            block,
            previous
        );
        assert_eq!(block % 16, 0, "still aligned like `malloc` must be");
        // The whole block is writable, which a start rounded down past the region's floor would
        // not be.
        for offset in 0..64_u64 {
            // SAFETY: inside the sixty-four bytes just returned.
            unsafe {
                std::ptr::write(
                    std::ptr::with_exposed_provenance_mut::<u8>((block + offset) as usize),
                    step as u8,
                );
            }
        }
        // SAFETY: the last byte of the same block.
        let last = unsafe {
            std::ptr::read(std::ptr::with_exposed_provenance::<u8>(
                (block + 63) as usize,
            ))
        };
        assert_eq!(last, step as u8, "the block must be writable memory");
        previous = block;
    }

    // Blocks do not overlap: eight distinct 64-byte blocks need at least 8 * 64 bytes between
    // the first and last.
    let first = call("malloc", &[64]);
    assert!(
        first < previous,
        "the ninth block continues downward: {first:#x} then {previous:#x}"
    );

    let summary = orbistoun_libc::heap_base_summary().expect("the variable is set");
    assert!(
        summary.contains(&format!("{BASE:#x}")),
        "the report names the base it was given: {summary}"
    );
    assert!(
        !summary.contains("did NOT hold every address fixed"),
        "nothing here should have spilled: {summary}"
    );
}
