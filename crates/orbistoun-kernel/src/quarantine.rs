//! Released mapping bases, held back before they are handed out again (D749).
//!
//! A guest that maps and unmaps at a high rate would otherwise walk the bump allocator through the
//! whole mapping range in an hour. A released base is reused, but only once enough newer releases
//! have followed it that a stale pointer into it has had every chance to fault first.

use std::collections::VecDeque;

/// Releases that must follow one before its base is handed out again.
///
/// A policy, not a measurement: deep enough that reuse is far behind any one frame's churn, and
/// shallow enough that the held address space (this many slots) stays small beside the range.
pub(crate) const DEPTH: usize = 65_536;

/// Released slots, oldest first: each a base and the address space its slot spans.
#[derive(Debug, Default)]
pub(crate) struct Quarantine {
    held: VecDeque<(u64, u64)>,
}

impl Quarantine {
    /// Holds a released slot of `span` bytes at `base`.
    pub(crate) fn release(&mut self, base: u64, span: u64) {
        self.held.push_back((base, span));
    }

    /// A base for a slot of `span` bytes: the oldest released one, once more than [`DEPTH`] newer
    /// releases follow it and its slot is wide enough. `None` otherwise, and the caller bumps.
    pub(crate) fn take(&mut self, span: u64) -> Option<u64> {
        if self.held.len() <= DEPTH {
            return None;
        }
        let &(base, held) = self.held.front()?;
        if held < span {
            return None;
        }
        self.held.pop_front();
        Some(base)
    }
}

#[cfg(test)]
mod tests {
    use super::{DEPTH, Quarantine};

    /// A released base comes back only after [`DEPTH`] newer releases, oldest first.
    #[test]
    fn a_base_is_reused_only_after_the_quarantine() {
        let mut q = Quarantine::default();
        for slot in 0..=DEPTH as u64 {
            assert_eq!(q.take(0x2_0000), None, "only {slot} released so far");
            q.release(0x1000_0000 + slot * 0x2_0000, 0x2_0000);
        }
        assert_eq!(q.take(0x2_0000), Some(0x1000_0000), "the oldest");
        assert_eq!(q.take(0x2_0000), None, "and the next waits again");
    }

    /// A slot narrower than the request is not handed out for it.
    #[test]
    fn a_narrow_slot_is_not_reused_for_a_wider_mapping() {
        let mut q = Quarantine::default();
        for slot in 0..=DEPTH as u64 {
            q.release(slot * 0x2_0000, 0x2_0000);
        }
        assert_eq!(q.take(0x4_0000), None);
        assert_eq!(q.take(0x2_0000), Some(0));
    }
}
