//! Choosing a model for a machine.
//!
//! Pure arithmetic over [`Catalog`] and [`Host`] - no filesystem, no network, no
//! accelerator. That is on purpose: this is the decision that spends somebody's
//! bandwidth and then either loads or does not, and it should be checkable without
//! owning any of the machines it decides for.
//!
//! # The rule
//!
//! **The largest auto-eligible model that fits.** A strong machine gets a strong model
//! with no configuration; a weak one stays usable. Sizing is against the pool the
//! model will actually live in - VRAM on an accelerator, system RAM on CPU - because
//! those are different numbers and a model sized against the wrong one is sized
//! against nothing.
//!
//! # The rule when nothing is measurable
//!
//! The catalogue's `default` entry, not the largest and not the smallest.
//!
//! Being wrong upward costs a multi-gigabyte download followed by a load failure,
//! which is the worst place to discover a mistake. Being wrong downward costs
//! quality, quietly and forever. The `default` entry is the choice that makes the
//! first mistake unlikely without making the second one automatic - and because the
//! selection is recorded in the config it can be re-tuned later, whereas a download
//! cannot be un-spent.

use crate::catalog::{Catalog, Offline};
use crate::host::Host;

/// Where a model would run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    /// In system memory, on the CPU.
    Cpu,
    /// In accelerator memory.
    Gpu,
}

/// Why a model was chosen, so the choice can be explained rather than just applied.
///
/// A run report that says "qwen3-4b" tells you nothing about whether that was a
/// measurement or a shrug. This says which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// Sized against a measured accelerator memory figure.
    MeasuredVram(u32),
    /// Sized against measured system memory.
    MeasuredRam(u32),
    /// Sized against a core count, because memory was not reported.
    CoreCount(u32),
    /// Memory was measured and would have allowed more, but the processor would not
    /// have got through it fast enough to be useful.
    CpuSpeedCapped {
        /// System memory, which was not the binding constraint.
        ram_mb: u32,
        /// The core count that was.
        cores: u32,
    },
    /// Nothing was measurable; this is the catalogue's stated default.
    Unmeasured,
}

impl Basis {
    /// One phrase, for the end of a sentence that begins with a model id.
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Self::MeasuredVram(mb) => format!("largest that fits {mb} MB of accelerator memory"),
            Self::MeasuredRam(mb) => format!("largest that fits {mb} MB of system memory"),
            Self::CoreCount(cores) => {
                format!("sized by {cores} cores, because memory was not reported")
            }
            Self::CpuSpeedCapped { ram_mb, cores } => format!(
                concat!(
                    "capped for {} cores; {} MB would have held more than the processor ",
                    "could work through"
                ),
                cores, ram_mb
            ),
            Self::Unmeasured => concat!(
                "the catalogue default; nothing about this machine ",
                "was measurable"
            )
            .to_owned(),
        }
    }
}

/// A model, the device it was sized for, and why.
#[derive(Debug, Clone)]
pub struct Choice<'a> {
    /// The chosen model.
    pub model: &'a Offline,
    /// Where it was sized to run.
    pub device: Device,
    /// What the decision rested on.
    pub basis: Basis,
}

/// How many cores it takes before a CPU-only machine is trusted with the default
/// rather than the smallest model.
///
/// Not a measurement. It is a threshold that has to be *somewhere*, and eight is where
/// a desktop stops being a thin client. Recorded as a named constant so that when it
/// turns out to be wrong there is one place to change and something to point at.
pub const CORES_FOR_DEFAULT: u32 = 8;

/// Picks a model for this machine.
///
/// Returns `None` only when the catalogue holds no offline models at all, which is a
/// catalogue problem rather than a host problem and is reported as one by the caller.
pub fn recommend<'a>(catalog: &'a Catalog, host: &Host) -> Option<Choice<'a>> {
    // An accelerator that reported its memory: size against that pool.
    if let Some(accelerator) = &host.accelerator {
        let vram = accelerator.vram_mb;
        if let Some(model) = largest_fitting(catalog, |m| m.min_vram_mb <= vram) {
            return Some(Choice {
                model,
                device: Device::Gpu,
                basis: Basis::MeasuredVram(vram),
            });
        }
        // An accelerator too small for anything in the catalogue is not a reason to
        // refuse - it is a reason to run on the CPU, which is always present.
    }

    // No accelerator, or one nothing fits in. Size against system memory - and then
    // against how fast that memory can be worked through, which is a different question.
    if let Some(ram) = host.ram_mb {
        let ceiling = cpu_ceiling(catalog, host.cpu_cores);
        if let Some(model) =
            largest_fitting(catalog, |m| m.min_ram_mb <= ram && m.min_ram_mb <= ceiling)
        {
            return Some(Choice {
                model,
                device: Device::Cpu,
                basis: if ceiling < ram {
                    Basis::CpuSpeedCapped {
                        ram_mb: ram,
                        cores: host.cpu_cores.unwrap_or(0),
                    }
                } else {
                    Basis::MeasuredRam(ram)
                },
            });
        }
        // Measured, and too small for even the smallest entry. Say so by falling
        // through to the smallest rather than returning nothing: the model may still
        // load, and refusing outright would make a low-memory machine unusable on the
        // strength of a table this crate wrote about itself.
        if let Some(model) = catalog.smallest_auto() {
            return Some(Choice {
                model,
                device: Device::Cpu,
                basis: Basis::MeasuredRam(ram),
            });
        }
    }

    // Memory unknown. Cores are the only other signal, and they are a weak one.
    if let Some(cores) = host.cpu_cores {
        let model = if cores >= CORES_FOR_DEFAULT {
            catalog.balanced_default()
        } else {
            catalog.smallest_auto()
        };
        if let Some(model) = model {
            return Some(Choice {
                model,
                device: Device::Cpu,
                basis: Basis::CoreCount(cores),
            });
        }
    }

    catalog.balanced_default().map(|model| Choice {
        model,
        device: Device::Cpu,
        basis: Basis::Unmeasured,
    })
}

/// The largest footprint worth running on a processor with this many cores.
///
/// **Fitting and being usable are different questions, and only the first is about
/// memory.** Thirty-two gigabytes holds a four-billion-parameter model easily and then
/// works through it at about one token per second - measured, not assumed: a round of
/// three hundred and twenty tokens took four minutes on sixteen cores.
///
/// So memory is a floor and this is a ceiling. Above [`CORES_FOR_DEFAULT`] the
/// catalogue's balanced entry is the most that is worth running; below it, the smallest.
/// Deliberately the same shape as the unmeasured-memory rule, because it is the same
/// judgement - a core count says how much work per second, not how much will fit.
fn cpu_ceiling(catalog: &Catalog, cores: Option<u32>) -> u32 {
    let model = match cores {
        Some(cores) if cores >= CORES_FOR_DEFAULT => catalog.balanced_default(),
        Some(_) | None => catalog.smallest_auto(),
    };
    model.map_or(u32::MAX, |m| m.min_ram_mb)
}

/// The largest auto-eligible model satisfying `fits`.
///
/// Ordered by declared footprint rather than by download size: footprint is what
/// decides whether it loads, and the two are correlated but not identical.
fn largest_fitting(catalog: &Catalog, fits: impl Fn(&Offline) -> bool) -> Option<&Offline> {
    catalog
        .offline
        .iter()
        .filter(|m| m.auto)
        .filter(|m| fits(m))
        .max_by_key(|m| m.min_vram_mb)
}

#[cfg(test)]
mod tests {
    use super::{Basis, CORES_FOR_DEFAULT, Device, recommend};
    use crate::catalog::Catalog;
    use crate::host::{Accelerator, Host};

    fn with_vram(mb: u32) -> Host {
        Host {
            accelerator: Some(Accelerator {
                name: "test".to_owned(),
                vram_mb: mb,
            }),
            ..Host::unmeasured()
        }
    }

    /// A big accelerator gets a big model.
    #[test]
    fn a_large_accelerator_gets_the_largest_that_fits() {
        let catalog = Catalog::default();
        let choice = recommend(&catalog, &with_vram(24_000)).expect("a model");
        assert_eq!(choice.device, Device::Gpu);
        // The largest *auto* entry, not the largest entry: a hand-pick-only model must
        // never be chosen by a machine simply for being big enough.
        assert!(choice.model.auto, "{}", choice.model.id);
        let largest_auto = catalog
            .offline
            .iter()
            .filter(|m| m.auto)
            .max_by_key(|m| m.min_vram_mb)
            .expect("one exists");
        assert_eq!(choice.model.id, largest_auto.id);
    }

    /// A hand-pick-only model is never selected automatically, however big the box.
    ///
    /// This is the whole point of the `auto` flag. Without it the catalogue could only
    /// express "runnable", and "runnable but a bad default" would have nowhere to live.
    #[test]
    fn a_hand_pick_model_is_never_chosen_automatically() {
        let catalog = Catalog::default();
        let choice = recommend(&catalog, &with_vram(80_000)).expect("a model");
        assert!(choice.model.auto);
    }

    /// A tiny accelerator falls through to the CPU rather than refusing.
    ///
    /// An integrated part with 256 MB reported is a real machine. Returning nothing
    /// would make it a machine with no AI at all, when it has a CPU like every other.
    #[test]
    fn an_accelerator_too_small_for_anything_falls_through_to_cpu() {
        let catalog = Catalog::default();
        let host = Host {
            ram_mb: Some(32_000),
            ..with_vram(64)
        };
        let choice = recommend(&catalog, &host).expect("a model");
        assert_eq!(choice.device, Device::Cpu);
        assert!(
            matches!(
                choice.basis,
                Basis::MeasuredRam(32_000) | Basis::CpuSpeedCapped { ram_mb: 32_000, .. }
            ),
            "{:?}",
            choice.basis
        );
    }

    /// **A processor is capped by what it can work through, not by what it can hold.**
    ///
    /// Thirty-two gigabytes fits every model in the catalogue and runs the larger ones
    /// at about a token per second. Sizing by capacity alone picked a
    /// four-billion-parameter model for a CPU and made a round take four minutes -
    /// measured, on sixteen cores. Memory is the floor; the core count is the ceiling.
    #[test]
    fn a_cpu_is_capped_by_cores_not_by_how_much_memory_it_has() {
        let catalog = Catalog::default();
        let host = Host {
            ram_mb: Some(128_000),
            cpu_cores: Some(16),
            ..Host::unmeasured()
        };
        let choice = recommend(&catalog, &host).expect("a model");
        assert_eq!(choice.device, Device::Cpu);
        assert_eq!(
            choice.model.id,
            catalog.balanced_default().expect("one").id,
            "a huge machine still should not run a huge model on its processor"
        );
        assert!(
            choice.basis.describe().contains("cores"),
            "{:?}",
            choice.basis
        );
    }

    /// Few cores get the smallest model however much memory there is.
    #[test]
    fn few_cores_get_the_smallest_model_whatever_the_memory() {
        let catalog = Catalog::default();
        let host = Host {
            ram_mb: Some(128_000),
            cpu_cores: Some(2),
            ..Host::unmeasured()
        };
        let choice = recommend(&catalog, &host).expect("a model");
        assert_eq!(choice.model.id, catalog.smallest_auto().expect("one").id);
    }

    /// Sizing on CPU is against system memory, not against VRAM.
    ///
    /// The two figures differ by an order of magnitude on an ordinary desktop. Using
    /// the VRAM column for a CPU decision would choose the largest model in the
    /// catalogue on any machine with 16 GB, which is most of them.
    #[test]
    fn a_cpu_machine_is_sized_against_system_memory() {
        let catalog = Catalog::default();
        let host = Host {
            ram_mb: Some(4_000),
            ..Host::unmeasured()
        };
        let choice = recommend(&catalog, &host).expect("a model");
        assert_eq!(choice.device, Device::Cpu);
        let model = catalog.offline(&choice.model.id).expect("in catalogue");
        assert!(model.min_ram_mb <= 4_000, "{} does not fit", model.id);
    }

    /// A machine below the smallest entry still gets something.
    #[test]
    fn a_machine_below_every_entry_still_gets_the_smallest() {
        let catalog = Catalog::default();
        let host = Host {
            ram_mb: Some(256),
            ..Host::unmeasured()
        };
        let choice = recommend(&catalog, &host).expect("a model");
        assert_eq!(choice.model.id, catalog.smallest_auto().expect("one").id);
    }

    /// Nothing measurable gets the catalogue default, and says so.
    ///
    /// The `basis` is the part that matters: a report saying only "qwen3-1.7b" cannot
    /// be told apart from a measured choice, and the two deserve different confidence.
    #[test]
    fn an_unmeasured_machine_gets_the_default_and_says_why() {
        let catalog = Catalog::default();
        let choice = recommend(&catalog, &Host::unmeasured()).expect("a model");
        assert_eq!(choice.model.id, catalog.balanced_default().expect("one").id);
        assert_eq!(choice.basis, Basis::Unmeasured);
        assert!(choice.basis.describe().contains("measurable"));
    }

    /// Cores decide only when memory is unknown, and the threshold is the constant.
    #[test]
    fn cores_are_used_only_when_memory_is_unknown() {
        let catalog = Catalog::default();
        let many = Host {
            cpu_cores: Some(CORES_FOR_DEFAULT),
            ..Host::unmeasured()
        };
        let few = Host {
            cpu_cores: Some(CORES_FOR_DEFAULT - 1),
            ..Host::unmeasured()
        };
        let big = recommend(&catalog, &many).expect("a model");
        let small = recommend(&catalog, &few).expect("a model");
        assert_eq!(big.model.id, catalog.balanced_default().expect("one").id);
        assert_eq!(small.model.id, catalog.smallest_auto().expect("one").id);
        assert!(matches!(big.basis, Basis::CoreCount(_)));
    }

    /// An empty offline table yields no choice rather than a panic.
    #[test]
    fn a_catalogue_with_no_offline_models_yields_nothing() {
        let catalog = Catalog::parse(
            r#"
[[online]]
id = "x"
label = "x"
wire = "openai"
endpoint = "http://localhost/v1/chat/completions"
default_model = "m"
"#,
        )
        .expect("parses");
        assert!(recommend(&catalog, &Host::unmeasured()).is_none());
    }
}
