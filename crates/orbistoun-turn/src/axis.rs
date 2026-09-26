//! Every way a run can be changed to find something out.
//!
//! Each axis is a different question and their answers are not comparable: [`Axis::Write`] asks
//! whether an argument is an out-parameter nobody filled, [`Axis::Map`] whether the faulting
//! address is a region the guest wanted, [`Axis::Poke`] whether the fault follows a planted value,
//! and [`Axis::Fill`] whether the run depends on memory nobody wrote.
//!
//! An intervention that moves a wall is not a diagnosis: every intervening axis alters the program
//! and can buy progress with a wrong answer. So a [`Change`] reports what was observed, never a
//! conclusion; reading one needs a second observation of a different kind.

use crate::Error;

/// A region a fill pattern can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Region {
    /// The guest stack.
    Stack,
    /// Every allocation the guest makes.
    Heap,
    /// Zero-initialised static data.
    ///
    /// The region a stack or heap poison cannot reach.
    Bss,
}

impl Region {
    /// The variable that fills it.
    #[must_use]
    pub const fn variable(self) -> &'static str {
        match self {
            Self::Stack => orbistoun_env::STACK_FILL.name,
            Self::Heap => orbistoun_env::HEAP_FILL.name,
            Self::Bss => orbistoun_env::BSS_FILL.name,
        }
    }

    /// Every region, for a sweep.
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::Stack, Self::Heap, Self::Bss]
    }
}

/// One way of changing a run.
///
/// Each variant carries exactly what its variable takes, so a wrong shape is a compile error rather
/// than a run that silently changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Axis {
    /// Plant a value at the address held in an argument.
    Write {
        /// The import, by bare symbol or bare hash, never `library::symbol`, which the value's own
        /// `:` delimiter cannot express.
        target: String,
        /// Which argument.
        slot: u8,
        /// What to plant.
        value: u64,
    },
    /// Reserve a region before the guest starts.
    Map {
        /// Where.
        address: u64,
        /// How much.
        length: u64,
    },
    /// Write a value at a known address before the guest starts.
    Poke {
        /// Where.
        address: u64,
        /// What.
        value: u64,
    },
    /// Force an import to answer a value, and see whether the fault follows it.
    ///
    /// A function can hand back a region base rather than filling one in, which planting in its
    /// arguments never shows. This axis changes what a function does rather than what it is given,
    /// so the answer is an offset that agrees across two sentinels, not a fault that moved.
    Return {
        /// The import, by bare symbol or bare hash, never `library::symbol`, which this value's own
        /// `:` delimiter cannot express.
        target: String,
        /// What to answer.
        value: u64,
    },
    /// Fill a region with a byte, so anything reading it unwritten is visible.
    Fill {
        /// Which region.
        region: Region,
        /// The byte.
        byte: u8,
    },
    /// Show the guest a different shape of physical memory map.
    ///
    /// Answers which map the guest accepts (D357).
    MapShape {
        /// The shape, by the name the diagnostic takes.
        shape: &'static str,
    },
    /// Trap on every access to a run of words, and report which instruction made it.
    ///
    /// It observes rather than intervenes: a debug register is armed, the guest runs the program it
    /// would have run, and what comes back is where an access came from (D276). Four words is the
    /// hardware's limit, and the address must be eight-byte aligned; `orbistoun_worker::watchpoint`
    /// refuses anything else with the reason.
    Watch {
        /// First word. Eight-byte aligned.
        base: u64,
        /// How many words, at most four.
        words: usize,
    },
    /// Read a span of guest memory back once the guest has stopped.
    ///
    /// It observes, like [`Self::Watch`]: it snapshots and prints, so a verdict beside it needs no
    /// caveat. It reads structures longer than the argument dump shows, including ones allocated
    /// while the guest runs.
    Read {
        /// Where the structure starts.
        address: u64,
        /// How many bytes.
        length: u64,
    },
}

impl Axis {
    /// The variable this sets, and the value it sets it to.
    #[must_use]
    pub fn env(&self) -> (&'static str, String) {
        match self {
            Self::Write {
                target,
                slot,
                value,
            } => (
                orbistoun_env::WRITE.name,
                format!("{target}:{slot}:{value:#x}"),
            ),
            Self::Map { address, length } => {
                (orbistoun_env::MAP.name, format!("{address:#x}+{length:#x}"))
            }
            Self::Poke { address, value } => {
                (orbistoun_env::POKE.name, format!("{address:#x}:{value:#x}"))
            }
            Self::Return { target, value } => {
                (orbistoun_env::RETURN.name, format!("{target}:{value:#x}"))
            }
            Self::Fill { region, byte } => (region.variable(), format!("{byte:02x}")),
            Self::MapShape { shape } => (orbistoun_env::MAP_SHAPE.name, (*shape).to_owned()),
            Self::Read { address, length } => (
                orbistoun_env::WATCH.name,
                format!("{address:#x}+{length:#x}"),
            ),
            Self::Watch { base, words } => (
                orbistoun_env::WATCHPOINT.name,
                (0..*words)
                    .map(|word| format!("{:#x}", base.saturating_add(word as u64 * 8)))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        }
    }

    /// Every diagnostic a run must not inherit.
    ///
    /// Cleared before each run, so one experiment cannot inherit another's, or the environment the
    /// sweep was launched from. Read from the registry, so a new diagnostic is covered once
    /// declared (D288). Diagnostics only: a setting, such as the `ORBISTOUN_DATA_DIR` each trial is
    /// pointed at, is how the caller configures the run, and `Kind` tells the two apart.
    #[must_use]
    pub fn every_variable() -> Vec<&'static str> {
        orbistoun_env::REGISTRY
            .iter()
            .filter(|var| var.kind == orbistoun_env::Kind::Diagnostic)
            .map(|var| var.name)
            .collect()
    }

    /// One line naming what this asks.
    #[must_use]
    pub fn question(&self) -> String {
        match self {
            Self::Write { target, slot, .. } => {
                format!("is arg{slot} of {target} an out-parameter nobody filled?")
            }
            Self::Map { address, length } => {
                format!("is {address:#x}+{length:#x} a region the guest expected to exist?")
            }
            Self::Poke { address, .. } => {
                format!("does the fault follow a value planted at {address:#x}?")
            }
            Self::Return { target, .. } => {
                format!("does the base come back as the answer to {target}?")
            }
            Self::MapShape { shape } => {
                format!("does the guest accept a {shape} physical memory map?")
            }
            Self::Read { address, length } => {
                format!("what does the guest have at {address:#x}+{length:#x} when it stops?")
            }
            Self::Fill { region, .. } => {
                format!("does the run depend on unwritten {region:?} memory?")
            }
            Self::Watch { base, words } => {
                format!("which instructions touch the {words} words at {base:#x}?")
            }
        }
    }

    /// Whether this changes the program, rather than only observing it.
    ///
    /// A poke, a poison or a reservation can buy progress with a wrong answer, and needs a second
    /// observation of a different kind saying what the guest did with it (D227). A watchpoint
    /// leaves the program alone. Derived from the `orbistoun-env` registry, which records every
    /// diagnostic's effect.
    #[must_use]
    pub fn intervenes(&self) -> bool {
        let name = self.env().0;
        orbistoun_env::REGISTRY
            .iter()
            .find(|var| var.name == name)
            .is_some_and(|var| var.effect.needs_caveat())
    }
}

/// What an intervention did, as observed - never as explained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The run was indistinguishable from the baseline.
    ///
    /// Whatever this axis changed, the guest did not depend on it.
    Nothing,
    /// It faulted somewhere else, having got at least as far.
    ///
    /// Not progress and not a diagnosis: a fault that moves has been changed, not explained. It is
    /// worth a person's time because the guest was not simply broken earlier.
    MovedTo {
        /// Where it faulted instead.
        address: u64,
    },
    /// It faulted somewhere else, having got less far.
    ///
    /// The intervention broke something before the guest reached what was asked about, so the new
    /// fault says nothing about the old one. Held apart because the address alone reads as a lead
    /// (D129).
    BrokeEarlier {
        /// Where it faulted instead.
        address: u64,
        /// Distinct imports reached under the intervention.
        reached: usize,
        /// Distinct imports reached without it.
        was: usize,
    },
    /// It stopped faulting.
    ///
    /// The outcome most worth distrusting: an intervention that removes a fault can equally have
    /// postponed it, and only what the guest does next says which.
    NoLongerFaulted,
    /// The intervention never took effect, so nothing was measured.
    ///
    /// Held apart from [`Self::Nothing`], since a run that changed nothing because it did nothing
    /// is not evidence.
    NotApplied,
}

impl Change {
    /// Whether this is worth a person's attention.
    ///
    /// A regression is not: it changed the program without saying anything about the question
    /// asked.
    #[must_use]
    pub const fn is_notable(&self) -> bool {
        matches!(self, Self::MovedTo { .. } | Self::NoLongerFaulted)
    }
}

/// Reads one outcome against a baseline.
///
/// `applied` comes from the run itself, not from whether the fault moved, so "nothing happened" and
/// "nothing was done" stay different observations.
#[must_use]
pub fn compare(
    baseline: &crate::experiment::Outcome,
    outcome: &crate::experiment::Outcome,
    applied: bool,
) -> Change {
    if !applied {
        return Change::NotApplied;
    }
    match (baseline.fault, outcome.fault) {
        // A run that never faulted did not stop faulting: with no baseline fault, nothing started,
        // so nothing stopped (D356).
        (Some(_), None) => Change::NoLongerFaulted,
        (Some(before), Some(after)) if before != after => {
            // Two signals, because the address alone cannot tell a lead from a regression.
            if outcome.reached < baseline.reached {
                Change::BrokeEarlier {
                    address: after,
                    reached: outcome.reached,
                    was: baseline.reached,
                }
            } else {
                Change::MovedTo { address: after }
            }
        }
        _ => Change::Nothing,
    }
}

/// The fill experiments: one byte per region.
///
/// A distinctive pattern rather than zero, because zero is what the region already holds.
#[must_use]
pub fn fills(byte: u8) -> Vec<Axis> {
    Region::all()
        .into_iter()
        .map(|region| Axis::Fill { region, byte })
        .collect()
}

/// Reserving the page a fault landed in, and the region around it.
///
/// Sized outward from the fault: a guest that wanted a megabyte and indexed near its end faults at
/// the far edge, so reserving only the faulting page answers a narrower question.
#[must_use]
pub fn around(fault: u64) -> Vec<Axis> {
    const PAGE: u64 = 0x1000;
    let page = fault & !(PAGE - 1);
    [PAGE, 0x10_000, 0x100_000]
        .into_iter()
        .map(|length| Axis::Map {
            // Centred on the fault's page, by half of what is left after that page, so a single
            // page still starts at the fault.
            address: page.saturating_sub((length - PAGE) / 2) & !(PAGE - 1),
            length,
        })
        .collect()
}

/// Everything worth trying against a wall, before anything has to be guessed.
///
/// Ordered cheapest question first; each is a short run.
///
/// # Errors
///
/// Never. The signature is fallible so a caller can chain it with axes that need to read something
/// first.
pub fn against_a_wall(fault: Option<u64>) -> Result<Vec<Axis>, Error> {
    let mut out = fills(0xA5);
    if let Some(fault) = fault {
        out.extend(around(fault));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{Axis, Change, Region, around, compare, fills};

    /// Every axis this crate can set is one the next run clears.
    ///
    /// The invariant behind the sweep being controlled: an axis whose variable is not cleared leaks
    /// into every later run. The list is derived from the registry (D288); this guards against the
    /// derivation being narrowed.
    #[test]
    fn no_axis_survives_into_the_next_run() {
        let cleared = Axis::every_variable();
        let every_axis = [
            Axis::Write {
                target: "x".to_owned(),
                slot: 0,
                value: 1,
            },
            Axis::Map {
                address: 0x1000,
                length: 0x1000,
            },
            Axis::Poke {
                address: 0x1000,
                value: 1,
            },
            Axis::Return {
                target: "x".to_owned(),
                value: 0,
            },
            Axis::Fill {
                region: Region::Stack,
                byte: 0xAA,
            },
            Axis::Fill {
                region: Region::Heap,
                byte: 0xAA,
            },
            Axis::Fill {
                region: Region::Bss,
                byte: 0xAA,
            },
        ];
        for axis in every_axis {
            let (variable, _) = axis.env();
            assert!(
                cleared.contains(&variable),
                "{variable} is set by an axis and not cleared before the next run"
            );
        }
    }

    /// Every axis renders the shape its variable documents.
    ///
    /// Checked against `orbistoun-cli env`'s own examples, because a wrong shape is a run that
    /// changes nothing and reports a clean negative.
    #[test]
    fn each_axis_renders_the_shape_its_variable_documents() {
        let cases = [
            (
                Axis::Write {
                    target: "0x6abac2f3dc6f8cee".to_owned(),
                    slot: 0,
                    value: 0x1100_0000,
                },
                ("ORBISTOUN_WRITE", "0x6abac2f3dc6f8cee:0:0x11000000"),
            ),
            (
                Axis::Map {
                    address: 0xf_0000,
                    length: 0x1_0000,
                },
                ("ORBISTOUN_MAP", "0xf0000+0x10000"),
            ),
            (
                Axis::Poke {
                    address: 0x4000_019e_9cb0,
                    value: 0x1100_0000,
                },
                ("ORBISTOUN_POKE", "0x4000019e9cb0:0x11000000"),
            ),
            (
                Axis::Fill {
                    region: Region::Stack,
                    byte: 0x5a,
                },
                ("ORBISTOUN_STACK_FILL", "5a"),
            ),
            (
                Axis::Fill {
                    region: Region::Heap,
                    byte: 0xa5,
                },
                ("ORBISTOUN_HEAP_FILL", "a5"),
            ),
        ];
        for (axis, expected) in cases {
            let (name, value) = axis.env();
            assert_eq!((name, value.as_str()), expected, "{axis:?}");
        }
    }

    /// Nothing happened and nothing was done are different observations.
    #[test]
    fn an_intervention_that_never_applied_is_not_a_negative_result() {
        assert_eq!(
            compare(&ran(Some(0xfffe0), 23), &ran(Some(0xfffe0), 23), false),
            Change::NotApplied
        );
        assert_eq!(
            compare(&ran(Some(0xfffe0), 23), &ran(Some(0xfffe0), 23), true),
            Change::Nothing
        );
    }

    /// A fault that moves is reported as moved, not as explained.
    #[test]
    fn a_moved_fault_is_reported_as_an_observation() {
        assert_eq!(
            compare(&ran(Some(0xfffe0), 23), &ran(Some(0x1100_0000), 23), true),
            Change::MovedTo {
                address: 0x1100_0000
            }
        );
    }

    /// A fault that moves after the guest got less far is a regression, not a lead.
    #[test]
    fn a_fault_that_moves_after_getting_less_far_is_a_regression() {
        let change = compare(&ran(Some(0xfffe0), 23), &ran(Some(0xffff_ffff), 8), true);
        assert_eq!(
            change,
            Change::BrokeEarlier {
                address: 0xffff_ffff,
                reached: 8,
                was: 23,
            }
        );
        // It must not be offered beside a real lead.
        assert!(!change.is_notable());
    }

    /// Reaching further than the baseline is still only an observation, not evidence the change was
    /// right.
    #[test]
    fn getting_further_is_reported_as_a_move_not_a_regression() {
        assert_eq!(
            compare(&ran(Some(0xfffe0), 23), &ran(Some(0x1100_0000), 31), true),
            Change::MovedTo {
                address: 0x1100_0000
            }
        );
    }

    /// A fault that goes away is its own outcome.
    #[test]
    fn a_fault_that_goes_away_is_its_own_outcome() {
        let gone = compare(&ran(Some(0xfffe0), 23), &ran(None, 23), true);
        assert_eq!(gone, Change::NoLongerFaulted);
        assert!(gone.is_notable());
        assert!(!Change::Nothing.is_notable());
        assert!(!Change::NotApplied.is_notable());
    }

    /// One run, as the sweep saw it.
    fn ran(fault: Option<u64>, reached: usize) -> crate::experiment::Outcome {
        crate::experiment::Outcome {
            fault,
            planted: true,
            refused: false,
            reached,
            touched: true,
        }
    }

    /// A fill covers every region, and each names a different variable.
    #[test]
    fn a_fill_covers_every_region() {
        let axes = fills(0xa5);
        assert_eq!(axes.len(), 3);
        let mut names: Vec<&str> = axes.iter().map(|a| a.env().0).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 3, "two regions shared a variable");
    }

    /// A reservation is sized outward from the fault, and stays page-aligned.
    #[test]
    fn a_reservation_is_sized_outward_and_aligned() {
        let axes = around(0xfffe0);
        assert!(!axes.is_empty());
        for axis in &axes {
            let Axis::Map { address, length } = axis else {
                panic!("{axis:?} is not a reservation");
            };
            assert_eq!(address % 0x1000, 0, "{address:#x} is not page-aligned");
            assert!(
                *address <= 0xfffe0 && 0xfffe0 < address + length,
                "{address:#x}+{length:#x} does not contain the fault"
            );
        }
    }

    /// A reservation near zero does not wrap below zero.
    #[test]
    fn a_reservation_near_zero_does_not_wrap() {
        for axis in around(0x100) {
            let Axis::Map { address, length } = axis else {
                panic!("not a reservation");
            };
            assert!(address < 0x1000, "{address:#x} wrapped");
            assert!(length > 0);
        }
    }

    /// A run that never faulted did not stop faulting, whatever the intervention (D356).
    #[test]
    fn a_guest_that_never_faulted_has_not_stopped_faulting() {
        let quiet = crate::experiment::Outcome {
            fault: None,
            planted: true,
            refused: false,
            reached: 4,
            touched: false,
        };

        assert_eq!(
            compare(&quiet, &quiet, true),
            Change::Nothing,
            "neither run faulted, so nothing changed"
        );
    }

    /// And a run that did stop faulting still says so.
    #[test]
    fn a_fault_that_disappears_is_still_reported() {
        let faulted = crate::experiment::Outcome {
            fault: Some(0x1000),
            planted: true,
            refused: false,
            reached: 4,
            touched: true,
        };
        let quiet = crate::experiment::Outcome {
            fault: None,
            ..faulted.clone()
        };

        assert_eq!(compare(&faulted, &quiet, true), Change::NoLongerFaulted);
    }
}
