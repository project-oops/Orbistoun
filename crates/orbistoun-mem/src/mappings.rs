//! Every guest mapping, as the device maps guest memory into its own address space (D964).
//!
//! A dispatch whose ranges the host cannot know before it runs reaches guest memory by device
//! address: each guest mapping's readable stretches are mapped whole into the device, and a table
//! turns a guest address into a device address. This keeps the stretches a device may map - each
//! reservation's pages the guest last left readable, one run a maximal stretch of them - and a
//! generation that moves whenever they change, so whoever maps them maps again only then.
//!
//! A run is never left mapped into the device once its host memory is released: the device reads
//! and writes the pages behind the host's back. The hook [`on_release`] installs is called before a
//! reservation's memory goes, and its caller waits for it.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// One reservation's pages by stretch: each stretch's start, to its end and whether the guest left
/// it readable. The stretches cover the reservation and none overlaps another.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Stretches(BTreeMap<u64, (u64, bool)>);

impl Stretches {
    /// `[base, end)` readable or not, splitting the stretches it cuts.
    fn set(&mut self, (base, end): (u64, u64), readable: bool) {
        let cut: Vec<(u64, (u64, bool))> = self
            .0
            .range(..end)
            .filter(|&(_, &(stretch_end, _))| stretch_end > base)
            .map(|(&start, &stretch)| (start, stretch))
            .collect();
        for (start, (stretch_end, was)) in cut {
            self.0.remove(&start);
            if start < base {
                self.0.insert(start, (base, was));
            }
            if stretch_end > end {
                self.0.insert(end, (stretch_end, was));
            }
        }
        self.0.insert(base, (end, readable));
    }

    /// The maximal readable runs, as `(first byte, bytes)`.
    fn readable_runs(&self) -> Vec<(u64, u64)> {
        let mut runs: Vec<(u64, u64)> = Vec::new();
        for (&start, &(end, readable)) in &self.0 {
            if !readable {
                continue;
            }
            match runs.last_mut() {
                Some((first, len)) if *first + *len == start => *len = end - *first,
                _ => runs.push((start, end - start)),
            }
        }
        runs
    }
}

/// The reservations by base, each its pages by stretch, and the generation.
#[derive(Debug, Default)]
struct Registry {
    generation: u64,
    reservations: BTreeMap<u64, (u64, Stretches)>,
}

impl Registry {
    fn reserved(&mut self, base: u64, len: u64, readable: bool) {
        let mut stretches = Stretches::default();
        stretches.set((base, base.saturating_add(len)), readable);
        self.reservations.insert(base, (len, stretches));
        self.generation += 1;
    }

    /// `[base, base + len)` given `readable`, in whichever reservations it lies in.
    fn protected(&mut self, base: u64, len: u64, readable: bool) {
        let end = base.saturating_add(len);
        let mut changed = false;
        for (&start, (held, stretches)) in self.reservations.range_mut(..end) {
            let held_end = start.saturating_add(*held);
            if held_end <= base {
                continue;
            }
            let before = stretches.readable_runs();
            stretches.set((base.max(start), end.min(held_end)), readable);
            changed |= stretches.readable_runs() != before;
        }
        if changed {
            self.generation += 1;
        }
    }

    fn released(&mut self, base: u64) {
        if self.reservations.remove(&base).is_some() {
            self.generation += 1;
        }
    }

    fn readable_runs(&self) -> Vec<(u64, u64)> {
        self.reservations
            .values()
            .flat_map(|(_, stretches)| stretches.readable_runs())
            .collect()
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn locked() -> std::sync::MutexGuard<'static, Registry> {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What is called before a reservation's host memory is released, with its base and length: the
/// device's mappings over it are dropped, waiting for any work that reads them.
pub type ReleaseHook = fn(u64, u64);

static RELEASE_HOOK: OnceLock<ReleaseHook> = OnceLock::new();

/// Installs the hook called before each reservation's host memory is released. First install wins;
/// answers whether this one did.
pub fn on_release(hook: ReleaseHook) -> bool {
    RELEASE_HOOK.set(hook).is_ok()
}

/// The reservation holding `at`, as `(base, len)`: one host allocation, which the write-watch is
/// asked about a span of at a time (D974). Answered from the record, asking the host nothing: the
/// host's own answer walks the allocation's regions and cost a gigabyte reservation 0.25 ms a
/// question, 0.9 ms once the device mapped it.
#[must_use]
pub fn reservation_of(at: u64) -> Option<(u64, u64)> {
    let registry = locked();
    let (&base, &(len, _)) = registry.reservations.range(..=at).next_back()?;
    (at < base.saturating_add(len)).then_some((base, len))
}

/// Records a reservation, readable or not as the guest asked.
pub(crate) fn reserved(base: u64, len: u64, readable: bool) {
    locked().reserved(base, len, readable);
}

/// Records a change of protection over `[base, base + len)`.
pub(crate) fn protected(base: u64, len: u64, readable: bool) {
    locked().protected(base, len, readable);
}

/// Called before the reservation at `base` releases its host memory: the hook first, then the
/// record.
pub(crate) fn releasing(base: u64, len: u64) {
    if let Some(hook) = RELEASE_HOOK.get() {
        hook(base, len);
    }
    locked().released(base);
}

/// The generation the readable runs are at: it moves whenever they change.
#[must_use]
pub fn generation() -> u64 {
    locked().generation
}

/// Moves whenever a host page may have become one the device can map though no readable run
/// changed: a page committed on touch, or given another protection (D968).
static PAGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Notes that a host page may have become one the device can map (D968).
pub(crate) fn pages_changed() {
    PAGES.fetch_add(1, std::sync::atomic::Ordering::Release);
}

/// The host pages' generation: it moves whenever a page is committed or given another protection,
/// so whoever left pages of a run out of a mapping looks at them again only then (D968).
#[must_use]
pub fn pages_generation() -> u64 {
    PAGES.load(std::sync::atomic::Ordering::Acquire)
}

/// The readable runs of every reservation, as `(first byte, bytes)` in address order, with the
/// generation they are at. A run lies in one reservation: two reservations side by side are two
/// runs, since one host mapping may not span both.
#[must_use]
pub fn readable_runs() -> (u64, Vec<(u64, u64)>) {
    let registry = locked();
    (registry.generation, registry.readable_runs())
}

#[cfg(test)]
mod tests {
    use super::Registry;

    /// A reservation readable whole is one run; a protection taking its middle away splits it, and
    /// giving it back joins it again, each change moving the generation.
    #[test]
    fn protection_splits_and_joins_a_reservation_s_runs() {
        let mut registry = Registry::default();
        registry.reserved(0x10_0000, 0x4_0000, true);
        assert_eq!(registry.readable_runs(), vec![(0x10_0000, 0x4_0000)]);
        let before = registry.generation;
        registry.protected(0x11_0000, 0x1_0000, false);
        assert_eq!(
            registry.readable_runs(),
            vec![(0x10_0000, 0x1_0000), (0x12_0000, 0x2_0000)]
        );
        assert!(registry.generation > before);
        registry.protected(0x11_0000, 0x1_0000, true);
        assert_eq!(registry.readable_runs(), vec![(0x10_0000, 0x4_0000)]);
    }

    /// A reservation made with no access has no run until the guest makes part of it readable, as
    /// a pool reserved whole and mapped into piece by piece does.
    #[test]
    fn a_reservation_with_no_access_has_runs_only_where_made_readable() {
        let mut registry = Registry::default();
        registry.reserved(0x100_0000, 0x10_0000, false);
        assert!(registry.readable_runs().is_empty());
        registry.protected(0x104_0000, 0x4000, true);
        assert_eq!(registry.readable_runs(), vec![(0x104_0000, 0x4000)]);
    }

    /// Two reservations side by side stay two runs, and a released one has none.
    #[test]
    fn neighbouring_reservations_stay_apart_and_release_drops_one() {
        let mut registry = Registry::default();
        registry.reserved(0x20_0000, 0x1_0000, true);
        registry.reserved(0x21_0000, 0x1_0000, true);
        assert_eq!(
            registry.readable_runs(),
            vec![(0x20_0000, 0x1_0000), (0x21_0000, 0x1_0000)]
        );
        registry.released(0x20_0000);
        assert_eq!(registry.readable_runs(), vec![(0x21_0000, 0x1_0000)]);
    }

    /// A protection that changes nothing leaves the generation where it was, so nothing remaps.
    #[test]
    fn a_protection_that_changes_nothing_keeps_the_generation() {
        let mut registry = Registry::default();
        registry.reserved(0x30_0000, 0x1_0000, true);
        let generation = registry.generation;
        registry.protected(0x30_0000, 0x4000, true);
        assert_eq!(registry.generation, generation);
    }
}
