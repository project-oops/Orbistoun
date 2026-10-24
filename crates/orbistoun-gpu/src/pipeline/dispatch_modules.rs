//! Translated dispatch modules kept for the run, the oldest let go past a budget of bytes.
//!
//! A module is translated for its program and for the window it was placed against, so a program
//! whose window moves - a ray-tracing pass over hierarchies built somewhere new each frame - is
//! another module at every move. Kept by key alone, those modules pile up for as long as the run
//! goes on; a module let go is served again from the translation store, or translated afresh.

use std::collections::{BTreeMap, VecDeque};

use orbistoun_translate::wavefront::ImageSources;

use super::DispatchModule;

/// How many bytes of modules are kept: a frame's dispatch programs, each up to a few megabytes of
/// SPIR-V, several times over; one let go is served again from the translation store.
const MODULE_BYTES: usize = 128 << 20;

/// How many modules are kept, however small: a bound on the bookkeeping.
const MODULES: usize = 4096;

/// See the module documentation.
#[derive(Debug)]
pub(super) struct DispatchModules {
    held: BTreeMap<u64, (DispatchModule, ImageSources)>,
    /// The keys held, oldest kept first.
    order: VecDeque<u64>,
    bytes: usize,
    /// The most bytes and modules kept.
    limits: (usize, usize),
}

impl Default for DispatchModules {
    fn default() -> Self {
        Self::within((MODULE_BYTES, MODULES))
    }
}

impl DispatchModules {
    fn within(limits: (usize, usize)) -> Self {
        Self {
            held: BTreeMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            limits,
        }
    }

    /// The bytes of modules kept.
    pub(super) const fn bytes(&self) -> usize {
        self.bytes
    }

    /// The module kept under `key`, with where its images come from.
    pub(super) fn get(&self, key: u64) -> Option<&(DispatchModule, ImageSources)> {
        self.held.get(&key)
    }

    /// Keeps `module` under `key`, newest; the oldest are let go while over the budget, though
    /// never the one just kept.
    pub(super) fn keep(&mut self, key: u64, module: (DispatchModule, ImageSources)) {
        self.bytes += bytes_of(&module);
        if let Some(old) = self.held.insert(key, module) {
            self.bytes -= bytes_of(&old);
            self.order.retain(|held| *held != key);
        }
        self.order.push_back(key);
        let (most_bytes, most_modules) = self.limits;
        while (self.bytes > most_bytes || self.order.len() > most_modules) && self.order.len() > 1 {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(gone) = self.held.remove(&oldest) {
                self.bytes -= bytes_of(&gone);
            }
        }
    }
}

fn bytes_of(((module, _), _): &(DispatchModule, ImageSources)) -> usize {
    module.len() * 4
}

#[cfg(test)]
mod tests {
    use super::DispatchModules;

    fn module(words: usize) -> (super::DispatchModule, super::ImageSources) {
        (
            (vec![0u32; words].into(), words as u64),
            (Vec::new(), Vec::new()),
        )
    }

    /// A program whose window moves every frame makes a module each time; past the budget the
    /// oldest go first, the newest stays, and the bytes held are what is held.
    #[test]
    fn modules_for_moving_windows_stay_within_the_budget() {
        let mut kept = DispatchModules::within((4 * 1000 * 3, 100));
        for frame in 0..50u64 {
            kept.keep(frame, module(1000));
        }
        assert_eq!(kept.held.len(), 3, "three fit the bytes");
        assert!(kept.get(49).is_some() && kept.get(47).is_some());
        assert!(kept.get(46).is_none(), "the oldest went");
        assert_eq!(kept.bytes, 3 * 4000);
        kept.keep(48, module(1000));
        assert_eq!(kept.order.len(), 3, "kept again, it counts once");
        kept.keep(50, module(10_000));
        assert!(kept.get(50).is_some(), "larger than the budget, kept alone");
        assert_eq!((kept.held.len(), kept.bytes), (1, 40_000));
    }

    /// The count bounds small modules the bytes would not.
    #[test]
    fn small_modules_stay_within_the_count() {
        let mut kept = DispatchModules::within((usize::MAX, 8));
        for key in 0..20u64 {
            kept.keep(key, module(1));
        }
        assert_eq!(kept.held.len(), 8);
        assert!(kept.get(19).is_some() && kept.get(11).is_none());
    }
}
