//! What the worker's private memory is made of, by holder: the guest's own memory, committed as it
//! is touched, and the host's overhead beside it - the heap and the named caches in it, the
//! device's memory by what it is for (a Windows driver charges all of it, local memory included,
//! to the process), the driver's own named holders, and what nothing names.
//!
//! The guest's memory is what a console of 16 GB would hold; the overhead is ours, held to
//! [`OVERHEAD_BUDGET`], and a breakdown over it names its largest holders.

use std::fmt::Write as _;

/// The most host overhead - private memory beyond the guest's committed memory - a run holds
/// before its breakdown is flagged.
pub(crate) const OVERHEAD_BUDGET: u64 = 4 << 30;

/// How many holders a flag names.
const NAMED: usize = 4;

/// Where a holder's bytes come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// The process heap.
    Heap,
    /// Memory the graphics device allocated for this process.
    Device,
    /// The graphics driver's own host memory, beside its device allocations.
    Driver,
}

/// One measurement of what the worker holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Breakdown {
    /// The process's private bytes.
    pub(crate) private: u64,
    /// The guest's memory.
    pub(crate) guest: orbistoun_mem::lazy::GuestCommit,
    /// The heap's live bytes.
    pub(crate) heap: u64,
    /// The named holders, each with its bytes and where they come from.
    pub(crate) holders: Vec<(&'static str, u64, Source)>,
    /// The device-local memory - the card's own - in use and the driver's budget for it, where
    /// the driver says (`VK_EXT_memory_budget`): host memory the device maps directly (D964) is
    /// among the device's holders, and not here (D990).
    pub(crate) local: Option<(u64, u64)>,
}

impl Breakdown {
    /// Measures what the worker holds now, from the process's private bytes. Every holder
    /// publishes its bytes through atomics as it changes, so this takes no lock: it runs inside a
    /// perf window, on whichever thread closes it, while that thread may hold any holder's.
    pub(crate) fn now(private: u64) -> Self {
        let mut holders: Vec<(&'static str, u64, Source)> = orbistoun_gpu::agc_driver::host_held()
            .into_iter()
            .map(|(name, bytes)| (name, bytes, Source::Heap))
            .collect();
        holders.push((
            "draws handed to the device",
            crate::device_thread::handed_over_bytes(),
            Source::Heap,
        ));
        // The device's memory by purpose where the driver reports it; else the host-visible holders
        // the backend counts itself, which the report would include.
        let device = orbistoun_gpu_vulkan::device_memory::held();
        let reported = !device.is_empty();
        if let Some([snapshots, on_host @ ..]) = crate::render::backend_host_held() {
            if !reported {
                holders.push((snapshots.0, snapshots.1, Source::Device));
            }
            holders.extend(
                on_host
                    .into_iter()
                    .map(|(name, bytes)| (name, bytes, Source::Heap)),
            );
        }
        if reported {
            holders.extend(
                device
                    .into_iter()
                    .map(|(name, bytes)| (name, bytes, Source::Device)),
            );
        } else {
            holders.extend(
                orbistoun_gpu_vulkan::spares_held()
                    .into_iter()
                    .map(|(name, bytes)| (name, bytes, Source::Device)),
            );
        }
        holders.push((
            "pipeline cache",
            orbistoun_gpu_vulkan::pipeline_cache::held_bytes(),
            Source::Driver,
        ));
        Self {
            private,
            guest: orbistoun_mem::lazy::census(),
            heap: super::heap::live(),
            holders,
            local: orbistoun_gpu_vulkan::device_memory::local(),
        }
    }

    /// Private bytes beyond the guest's committed memory: the host's overhead.
    pub(crate) fn overhead(&self) -> u64 {
        self.private.saturating_sub(self.guest.committed())
    }

    fn sum(&self, from: Source) -> u64 {
        self.holders
            .iter()
            .filter(|holder| holder.2 == from)
            .map(|holder| holder.1)
            .sum()
    }

    /// The heap's bytes no named holder accounts for.
    fn heap_unnamed(&self) -> u64 {
        self.heap.saturating_sub(self.sum(Source::Heap))
    }

    /// The overhead neither the heap, the device's memory nor the driver's named holders account
    /// for: the rest of the graphics driver's own, thread stacks and the executable's images.
    fn unaccounted(&self) -> u64 {
        self.overhead()
            .saturating_sub(self.heap)
            .saturating_sub(self.sum(Source::Device))
            .saturating_sub(self.sum(Source::Driver))
    }

    /// Every holder of the overhead, the unnamed rests included, largest first.
    fn ranked(&self) -> Vec<(&'static str, u64)> {
        let mut ranked: Vec<(&'static str, u64)> = self
            .holders
            .iter()
            .map(|&(name, bytes, _)| (name, bytes))
            .chain([
                ("heap not named", self.heap_unnamed()),
                ("other", self.unaccounted()),
            ])
            .filter(|&(_, bytes)| bytes > 0)
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        ranked
    }

    /// The breakdown line.
    pub(crate) fn describe(&self) -> String {
        let mut line = format!(
            "worker memory: {} private = guest {} committed (of {} mapped) + host {}: heap {}",
            gib(self.private),
            gib(self.guest.committed()),
            gib(self.guest.lazy_reserved + self.guest.eager),
            gib(self.overhead()),
            gib(self.heap),
        );
        let named = |from: Source| {
            self.holders
                .iter()
                .filter(|holder| holder.2 == from && holder.1 > 0)
                .map(|&(name, bytes, _)| format!("{name} {}", gib(bytes)))
                .collect::<Vec<_>>()
        };
        let mut heap = named(Source::Heap);
        heap.push(format!("not named {}", gib(self.heap_unnamed())));
        let _ = write!(line, " ({})", heap.join(", "));
        for (from, label) in [
            (Source::Device, "device memory"),
            (Source::Driver, "driver"),
        ] {
            let holders = named(from);
            if !holders.is_empty() {
                let _ = write!(
                    line,
                    ", {label} {} ({})",
                    gib(self.sum(from)),
                    holders.join(", ")
                );
            }
        }
        let _ = write!(line, ", other {}", gib(self.unaccounted()));
        if let Some((used, budget)) = self.local {
            let _ = write!(
                line,
                "; device-local {} of a {} budget",
                gib(used),
                gib(budget)
            );
        }
        line
    }

    /// The flag when the overhead is over `budget`, naming its largest holders; `None` within it.
    pub(crate) fn over(&self, budget: u64) -> Option<String> {
        let overhead = self.overhead();
        if overhead <= budget {
            return None;
        }
        let largest: Vec<String> = self
            .ranked()
            .into_iter()
            .take(NAMED)
            .map(|(name, bytes)| format!("{name} {}", gib(bytes)))
            .collect();
        Some(format!(
            "the host's overhead is {} beside the guest's {}, over its {} budget; largest: {}",
            gib(overhead),
            gib(self.guest.committed()),
            gib(budget),
            largest.join(", ")
        ))
    }
}

#[allow(clippy::cast_precision_loss)] // a byte count shown to a hundredth of a GiB
fn gib(bytes: u64) -> String {
    format!("{:.2} GiB", bytes as f64 / f64::from(1u32 << 30))
}

#[cfg(test)]
mod tests {
    use super::{Breakdown, Source};
    use orbistoun_mem::lazy::GuestCommit;

    const GIB: u64 = 1 << 30;

    fn breakdown() -> Breakdown {
        Breakdown {
            private: 20 * GIB,
            guest: GuestCommit {
                lazy_reserved: 14 * GIB,
                lazy_committed: 12 * GIB,
                eager: GIB,
            },
            heap: 4 * GIB,
            holders: vec![
                ("texel cache", GIB, Source::Heap),
                ("translation store", GIB / 2, Source::Heap),
                ("render targets", GIB, Source::Device),
                ("frame snapshots", GIB / 4, Source::Device),
                ("pipeline cache", GIB / 4, Source::Driver),
            ],
            local: None,
        }
    }

    /// The overhead is private memory beyond the guest's committed memory, made of the heap's
    /// named and unnamed bytes, the device's memory, the driver's named holders, and the rest.
    #[test]
    fn the_overhead_is_what_the_guest_does_not_hold() {
        let held = breakdown();
        assert_eq!(held.overhead(), 7 * GIB);
        assert_eq!(held.heap_unnamed(), 4 * GIB - GIB - GIB / 2);
        assert_eq!(
            held.unaccounted(),
            7 * GIB - 4 * GIB - GIB - GIB / 4 - GIB / 4
        );
        assert_eq!(
            held.describe(),
            concat!(
                "worker memory: 20.00 GiB private = guest 13.00 GiB committed (of 15.00 GiB ",
                "mapped) + host 7.00 GiB: heap 4.00 GiB (texel cache 1.00 GiB, translation store ",
                "0.50 GiB, not named 2.50 GiB), device memory 1.25 GiB (render targets 1.00 GiB, ",
                "frame snapshots 0.25 GiB), driver 0.25 GiB (pipeline cache 0.25 GiB), other ",
                "1.50 GiB"
            )
        );
    }

    /// Over its budget the overhead is flagged with its largest holders, the unnamed rests among
    /// them; within it nothing is said.
    #[test]
    fn an_overhead_over_its_budget_names_its_largest_holders() {
        let held = breakdown();
        assert_eq!(held.over(8 * GIB), None);
        assert_eq!(
            held.over(4 * GIB).as_deref(),
            Some(concat!(
                "the host's overhead is 7.00 GiB beside the guest's 13.00 GiB, over its 4.00 ",
                "GiB budget; largest: heap not named 2.50 GiB, other 1.50 GiB, render targets ",
                "1.00 GiB, texel cache 1.00 GiB"
            ))
        );
    }
}
