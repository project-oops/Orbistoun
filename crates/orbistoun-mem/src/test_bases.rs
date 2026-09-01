//! Addresses for tests that reserve real host memory, handed out rather than chosen.
//!
//! # Why this is not a comment telling people to be careful
//!
//! Anything built on this crate reserves **real host memory at fixed addresses**, so two
//! tests using the same base race and one of them fails. That has now happened twice, and
//! both times the address was picked by a person reading a file and choosing a gap:
//!
//! - Inside `orbistoun-mem`, a new test chose `+0x300_0000` and a test three functions
//!   later was already using it. It failed about one run in ten - often enough to be real,
//!   rare enough to be dismissed.
//! - Inside `orbistoun-thunk`, four tests for [`crate`]-backed storage all took the shipped
//!   base, and whichever arrived second got `Conflict` (D323).
//!
//! `stack.rs` fixed the first by handing out bases from a counter, which removes the choice
//! *within one binary*. `docs/BACKLOG.md` recorded that the same hazard **between** crates
//! was still open, because a per-binary counter cannot see another binary - and `cargo test`
//! runs several at once.
//!
//! This closes it. A crate takes its own [`Range`], and a test takes the next address from
//! it. Nobody picks a number at either level.
//!
//! # Why it ships rather than hiding behind `cfg(test)`
//!
//! `#[cfg(test)]` items are invisible to other crates, so every dependent would define its
//! own - which is exactly the duplication that lets two of them choose the same range. It
//! is three constants and a counter, and it costs a crate that ignores it nothing.

use std::sync::atomic::{AtomicU64, Ordering};

/// Distance between one crate's test range and the next.
///
/// Enormous relative to anything a test reserves, so a crate cannot run out and reach into
/// its neighbour however many tests it grows.
pub const RANGE_STRIDE: u64 = 0x0000_0100_0000_0000;

/// Where the first crate's range begins.
///
/// Far from anything a normal process maps, so a test is about the mechanism rather than
/// about luck.
pub const FIRST_RANGE: u64 = 0x0000_6000_0000_0000;

/// Distance between the address one test gets and the next within a range.
pub const TEST_STRIDE: u64 = 0x0000_0000_0100_0000;

/// One crate's slice of the test address space.
///
/// **Declare one `static` per crate and take every base from it.** Two tests in one binary
/// run on parallel threads and two binaries run at once, so an address used twice fails
/// either way - and neither failure looks like an address problem when it arrives.
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
    /// **The numbers are assigned in [`crates`], not chosen at the call site.** A crate
    /// passing a literal here is choosing an address again, one level up, which is the
    /// thing this module exists to stop.
    #[must_use]
    pub const fn nth(nth: u64) -> Self {
        Self {
            base: FIRST_RANGE + nth * RANGE_STRIDE,
            next: AtomicU64::new(0),
        }
    }

    /// An address no other test taking from **this instance** will get.
    ///
    /// # The distinction is the whole trap
    ///
    /// The cursor is atomic, so concurrent takes from one `Range` are safe. But a test that
    /// writes `static RANGE: Range = Range::nth(crates::MINE);` *inside its own function* gets
    /// its own instance - and a second test in the same crate doing the same gets another,
    /// whose cursor also starts at zero and therefore hands out the same addresses.
    ///
    /// It cost a flaky gate: two loader tests reserved identical pages and whichever ran
    /// second failed, so it passed alone and passed most of the time in a suite. Declare one
    /// static per **test module**, not per test (D399).
    pub fn take(&self) -> u64 {
        self.base + self.next.fetch_add(TEST_STRIDE, Ordering::Relaxed)
    }
}

/// Which crate has which range, in one place so no two can disagree.
///
/// A table rather than a constant per crate, because the property that matters is that they
/// are **distinct** - and that is checkable here and nowhere else. A crate adding itself
/// appends a line and the test below proves it collides with nothing.
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

    /// Every range in use, so a test can prove they are distinct.
    pub const ALL: &[(&str, u64)] = &[
        ("mem", MEM),
        ("thunk", THUNK),
        ("loader", LOADER),
        ("abi", ABI),
        ("worker", WORKER),
    ];
}

#[cfg(test)]
mod tests {
    use super::{RANGE_STRIDE, Range, TEST_STRIDE, crates};

    /// **No two crates share a range**, which is the whole claim.
    ///
    /// Asserted rather than trusted to review: the table is the kind of thing somebody
    /// appends to in a hurry, and a duplicate would reintroduce exactly the intermittent
    /// cross-binary failure this replaced.
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
