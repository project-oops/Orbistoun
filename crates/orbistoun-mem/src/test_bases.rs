//! Addresses for tests that reserve real host memory, handed out rather than chosen.
//!
//! Tests built on this crate reserve host memory at fixed addresses, and two tests using the
//! same base race: within one binary on parallel threads, and across binaries that `cargo test`
//! runs at once. Each crate takes its own [`Range`] from the table in [`crates`], and each test
//! takes the next address from it, so nobody picks a number (D324). It ships rather than
//! living behind `cfg(test)`, which would hide it from the crates that need it.

use std::sync::atomic::{AtomicU64, Ordering};

/// Distance between one crate's test range and the next, far larger than any crate's tests
/// reserve.
pub const RANGE_STRIDE: u64 = 0x0000_0100_0000_0000;

/// Where the first crate's range begins, far from anything a normal process maps.
pub const FIRST_RANGE: u64 = 0x0000_6000_0000_0000;

/// Distance between the address one test gets and the next within a range.
pub const TEST_STRIDE: u64 = 0x0000_0000_0100_0000;

/// One crate's slice of the test address space.
///
/// Declare one `static` per crate and take every base from it.
#[derive(Debug)]
pub struct Range {
    /// Where this crate's addresses start.
    base: u64,
    /// How many have been handed out.
    next: AtomicU64,
}

impl Range {
    /// Claims the `nth` crate range.
    ///
    /// The numbers are assigned in [`crates`]; a literal at the call site is choosing an address
    /// again.
    #[must_use]
    pub const fn nth(nth: u64) -> Self {
        Self {
            base: FIRST_RANGE + nth * RANGE_STRIDE,
            next: AtomicU64::new(0),
        }
    }

    /// An address no other test taking from this instance will get.
    ///
    /// The cursor is atomic, so concurrent takes from one `Range` are safe. A `static` declared
    /// inside a test function is its own instance with its own cursor starting at zero, so
    /// declare one static per test module, not per test (D324).
    pub fn take(&self) -> u64 {
        self.base + self.next.fetch_add(TEST_STRIDE, Ordering::Relaxed)
    }
}

/// Which crate has which range, in one table so the test below can prove they are distinct.
pub mod crates {
    /// `orbistoun-mem`.
    pub const MEM: u64 = 0;
    /// `orbistoun-thunk`.
    pub const THUNK: u64 = 1;
    /// `orbistoun-loader`.
    pub const LOADER: u64 = 2;
    /// `orbistoun-abi`.
    pub const ABI: u64 = 3;
    /// `orbistoun-worker`.
    pub const WORKER: u64 = 4;
    /// `orbistoun-kernel`.
    pub const KERNEL: u64 = 5;

    /// Every range in use, so a test can prove they are distinct.
    pub const ALL: &[(&str, u64)] = &[
        ("mem", MEM),
        ("thunk", THUNK),
        ("loader", LOADER),
        ("abi", ABI),
        ("worker", WORKER),
        ("kernel", KERNEL),
    ];
}

#[cfg(test)]
mod tests {
    use super::{RANGE_STRIDE, Range, TEST_STRIDE, crates};

    /// No two crates share a range.
    #[test]
    fn every_crate_range_is_distinct() {
        let mut seen = std::collections::BTreeMap::new();
        for (name, nth) in crates::ALL {
            if let Some(other) = seen.insert(*nth, *name) {
                panic!("{name} and {other} both claim range {nth}");
            }
        }
        assert_eq!(seen.len(), crates::ALL.len());
    }

    /// Successive takes never repeat, and stay inside their own range.
    #[test]
    fn a_range_hands_out_distinct_addresses_and_does_not_reach_into_its_neighbour() {
        let range = Range::nth(crates::THUNK);
        let first = range.take();
        let second = range.take();

        assert_ne!(first, second);
        assert_eq!(second - first, TEST_STRIDE);

        let neighbour = Range::nth(crates::THUNK + 1);
        assert!(
            second < neighbour.take(),
            "a range must not reach into the next crate's"
        );
    }

    /// Two crates' ranges are far enough apart that neither can grow into the other.
    #[test]
    fn a_range_would_have_to_hand_out_an_absurd_number_to_collide() {
        let per_range = RANGE_STRIDE / TEST_STRIDE;
        assert!(
            per_range >= 65_536,
            "a crate gets {per_range} test addresses before it reaches its neighbour"
        );
    }
}
