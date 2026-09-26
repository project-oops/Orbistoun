//! Choosing a model for a machine.
//!
//! Pure arithmetic over [`Catalog`] and [`Host`], so the decision is checkable without owning
//! the machines it decides for. The rule is the largest auto-eligible model that fits the pool
//! it will live in: VRAM on an accelerator, system RAM on CPU. When nothing is measurable it is
//! the catalogue's `default` entry: guessing high costs a multi-gigabyte download and a load
//! failure, guessing low costs quality, and the recorded choice can be re-tuned later.

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

/// Why a model was chosen, so a report shows whether the choice was measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// Sized against a measured accelerator memory figure.
    MeasuredVram(u32),
    /// Sized against measured system memory.
    MeasuredRam(u32),
    /// Sized against a core count, because memory was not reported.
    CoreCount(u32),
    /// Memory would have allowed more, but the processor would not work through it fast enough
    /// to be useful.
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

/// How many cores it takes before a CPU-only machine is trusted with the default rather than
/// the smallest model.
///
/// A chosen threshold, not a measurement.
pub const CORES_FOR_DEFAULT: u32 = 8;

/// Picks a model for this machine.
///
/// Returns `None` only when the catalogue holds no offline models, which the caller reports as
/// a catalogue problem.
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
        // An accelerator too small for anything means running on the CPU, not refusing.
    }

    // No accelerator, or one nothing fits in: size against system memory, then cap by how fast
    // the processor works through it.
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
        // Measured and too small for the smallest entry: the smallest is still offered, since it may
        // load and refusing would make a low-memory machine unusable.
        if let Some(model) = catalog.smallest_auto() {
            return Some(Choice {
                model,
                device: Device::Cpu,
                basis: Basis::MeasuredRam(ram),
            });
        }
    }

    // Memory unknown: cores are the only other signal, and a weak one.
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
/// Memory is the floor and this is the ceiling: a large model fits in ample RAM but runs at
/// about a token per second on a CPU. At [`CORES_FOR_DEFAULT`] and above the balanced entry is
/// the most worth running; below it, the smallest.
fn cpu_ceiling(catalog: &Catalog, cores: Option<u32>) -> u32 {
    let model = match cores {
        Some(cores) if cores >= CORES_FOR_DEFAULT => catalog.balanced_default(),
        Some(_) | None => catalog.smallest_auto(),
    };
    model.map_or(u32::MAX, |m| m.min_ram_mb)
}

/// The largest auto-eligible model satisfying `fits`, ordered by declared footprint, which is
/// what decides whether it loads.
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
        // The largest auto entry: a hand-pick-only model is never chosen for a machine being big enough.
        assert!(choice.model.auto, "{}", choice.model.id);
        let largest_auto = catalog
            .offline
            .iter()
            .filter(|m| m.auto)
            .max_by_key(|m| m.min_vram_mb)
            .expect("one exists");
        assert_eq!(choice.model.id, largest_auto.id);
    }

    /// A hand-pick-only model is never selected automatically, however big the machine.
    #[test]
    fn a_hand_pick_model_is_never_chosen_automatically() {
        let catalog = Catalog::default();
        let choice = recommend(&catalog, &with_vram(80_000)).expect("a model");
        assert!(choice.model.auto);
    }

    /// A tiny accelerator falls through to the CPU rather than refusing.
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

    /// A processor is capped by the core count, not by how much memory it has.
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

    /// Sizing on CPU is against system memory, not VRAM.
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

    /// An unmeasured machine gets the catalogue default, and the basis says so.
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
