//! Every way a run can be changed to find something out.
//!
//! `orbistoun-cli env` lists ten diagnostics, each one a question phrased as an
//! intervention. Only one of them - planting a value at an argument - had ever been
//! swept automatically, and sweeping it exhaustively across a whole title took fifty
//! seconds. The other axes are the same size and the same price.
//!
//! # Each axis is a different question
//!
//! | axis | the question it asks |
//! |---|---|
//! | [`Axis::Write`] | is that argument an out-parameter nobody filled? |
//! | [`Axis::Map`] | does the faulting address become a region the guest wanted? |
//! | [`Axis::Poke`] | does the fault follow a value planted at a known address? |
//! | [`Axis::Fill`] | does the run depend on memory nobody wrote? |
//!
//! They are not interchangeable, and the answers are not comparable: `Fill` changing
//! nothing says the guest does not read uninitialised memory *in that region*, while
//! `Map` changing nothing says something much narrower.
//!
//! # An intervention that moves a wall is not a diagnosis
//!
//! Principle 3, in as many words, and it is the reason [`Change`] distinguishes what it
//! does. Every axis here *alters the program*, so any of them can buy progress with a
//! wrong answer - a poisoned region that shifts a fault has not explained anything, and
//! a mapped region that lets a guest continue may only have postponed the same mistake.
//!
//! So a change is reported as what was observed - the fault moved, the fault went away,
//! the guest reached further - and never as a conclusion. Reading one requires a second
//! observation of a different kind, which is a person's job and is meant to be.

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
    /// *"The last region a poison could not reach"*, as the variable's own description
    /// puts it - which makes it the one most likely to still hold an answer.
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
/// Each variant carries exactly what its variable takes, so a wrong shape is a
/// compile error rather than a run that silently changes nothing. That is not
/// theoretical: passing a `library::symbol` label to [`Axis::Write`], whose value is
/// split on `:`, produced two hundred and seventy-six runs that planted nothing and
/// reported twenty-three clean negatives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Axis {
    /// Plant a value at the address held in an argument.
    Write {
        /// The import, by bare symbol or bare hash - **never `library::symbol`**, which
        /// the value's own `:` delimiter cannot express.
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
    /// **The last channel into a wall that arguments cannot reach.** A function can hand
    /// back a region base rather than filling one in, and no amount of planting in its
    /// arguments will show that. Unlike every other axis this one changes what a function
    /// *does* rather than what it is given, so a run under it is a different program -
    /// which is why the answer is an offset that agrees across two sentinels, not a fault
    /// that moved.
    Return {
        /// The import, by bare symbol or bare hash - **never `library::symbol`**, which
        /// this value's own `:` delimiter cannot express.
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
    /// **The experiment D218 built the apparatus for and never ran.** `MapShape` has had three
    /// variants since then with nothing selecting between them, so the question it exists to
    /// answer - what map will the guest accept - stayed open while the function it blocks took
    /// 67.5% of every guest call recorded (D356).
    MapShape {
        /// The shape, by the name the diagnostic takes.
        shape: &'static str,
    },
    /// Trap on every access to a run of words, and report which instruction made it.
    ///
    /// **The only axis that observes rather than intervenes.** Every other one changes the
    /// program to see what the difference is, and a verdict under those needs the caveat the
    /// run report prints. This one arms a debug register: the guest runs the program it
    /// would have run, and what comes back is where an access came from (D276).
    ///
    /// Four words is the hardware's limit rather than a choice, and the address must be
    /// eight-byte aligned - `orbistoun_worker::watchpoint` refuses anything else with the
    /// reason, rather than rounding it into watching different bytes.
    Watch {
        /// First word. Eight-byte aligned.
        base: u64,
        /// How many words, at most four.
        words: usize,
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
    /// Cleared before each run, so one experiment cannot inherit another's - or the
    /// environment the sweep was launched from. A baseline taken with a stale variable
    /// set is not a baseline.
    ///
    /// **Read from the registry, not listed here.** This was seven hand-written strings, and
    /// `ORBISTOUN_WATCHPOINT` was added to `orbistoun-env` without reaching them - so a sweep
    /// launched from a shell with a watchpoint set would have carried it into every run and
    /// reported a controlled experiment. A second copy of the one list is the exact failure
    /// that crate exists to prevent (D288).
    ///
    /// **Diagnostics only.** A *setting* is how the caller configures the run - the sweep
    /// points each trial at its own temporary trace directory with `ORBISTOUN_DATA_DIR` - and
    /// clearing those would send every run at the machine's real one. That is what `Kind`
    /// distinguishes, and it is load-bearing here rather than descriptive.
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
    /// **A verdict taken under an intervention is not a diagnosis** - a poke, a poison or a
    /// reservation can buy progress with a wrong answer, and needs a second observation of a
    /// different kind saying what the guest did with it (D224, D226, D227). A watchpoint is
    /// the one axis here that leaves the program alone.
    ///
    /// Derived from `orbistoun-env`, which records the effect of every diagnostic, rather
    /// than restated here - the same reason the cleared list is (D288).
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
    /// The most common and least exciting answer, and a real one: whatever this axis
    /// changed, the guest did not depend on it.
    Nothing,
    /// It faulted somewhere else, having got at least as far.
    ///
    /// **Not progress and not a diagnosis.** A fault that moves has been changed, not
    /// explained. This is the version worth a person's time, because the guest was not
    /// simply broken earlier.
    MovedTo {
        /// Where it faulted instead.
        address: u64,
    },
    /// It faulted somewhere else, having got **less far**.
    ///
    /// The intervention broke something before the guest reached what was being asked
    /// about, so the new fault says nothing about the old one. Measured: poisoning
    /// zero-initialised statics on this wall moved the fault to a different address
    /// entirely, and the guest reached eight distinct imports instead of twenty-three.
    ///
    /// Held apart because the address alone reads as a lead. D129 records the same
    /// lesson about the progress verdict - one signal hid a run that had gone backwards.
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
    /// The loudest outcome and the one most worth distrusting: an intervention that
    /// removes a fault can equally have postponed it, and only what the guest does next
    /// says which.
    NoLongerFaulted,
    /// The intervention never took effect, so nothing was measured.
    ///
    /// Held apart from [`Self::Nothing`] for the reason the whole sweep exists: a run
    /// that changed nothing because it *did* nothing is not evidence, and reading it as
    /// evidence is how a wrong variable format turned into twenty-three clean negatives.
    NotApplied,
}

impl Change {
    /// Whether this is worth a person's attention.
    /// Whether this is worth a person's attention.
    ///
    /// A regression is not. It has changed the program without saying anything about the
    /// question that was asked, and putting it beside a real lead is how an afternoon
    /// gets spent on one.
    #[must_use]
    pub const fn is_notable(&self) -> bool {
        matches!(self, Self::MovedTo { .. } | Self::NoLongerFaulted)
    }
}

/// Reads one outcome against a baseline.
///
/// `applied` comes from the run itself rather than from whether the fault moved -
/// inferring it from the result would make "nothing happened" and "nothing was done"
/// the same observation, which is the confusion this exists to prevent.
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
        // **A run that never faulted did not stop faulting.** The wildcard here reported
        // `NoLongerFaulted` for a guest that spins to the time limit under every intervention,
        // so three map shapes each came back as though they had fixed something. Nothing
        // started, so nothing stopped (D356).
        //
        // Fourth time today that a field only meaningful when a fault happened was read on a
        // run where none did - the progress verdict, the sweep's oracle, `Derailed`, and this.
        (Some(_), None) => Change::NoLongerFaulted,
        (Some(before), Some(after)) if before != after => {
            // Two signals, because the address alone cannot tell a lead from a
            // regression - and a regression reported as a lead is a person's afternoon.
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
/// A distinctive pattern rather than zero, because zero is what the region already holds
/// and a fault that follows it would be indistinguishable from one that always happened.
#[must_use]
pub fn fills(byte: u8) -> Vec<Axis> {
    Region::all()
        .into_iter()
        .map(|region| Axis::Fill { region, byte })
        .collect()
}

/// Reserving the page a fault landed in, and the region around it.
///
/// Sized outward from the fault rather than at it: a guest that wanted a megabyte and
/// indexed near its end faults at the far edge, so reserving only the faulting page
/// answers a narrower question than the one worth asking.
#[must_use]
pub fn around(fault: u64) -> Vec<Axis> {
    const PAGE: u64 = 0x1000;
    let page = fault & !(PAGE - 1);
    [PAGE, 0x10_000, 0x100_000]
        .into_iter()
        .map(|length| Axis::Map {
            // Centred on the fault's *page*, not on the fault, and by half of what is
            // left after that page. Centring by half the whole length puts the start
            // below the fault for a single page - the reservation then misses the thing
            // it was made for, and the run reports a clean negative.
            address: page.saturating_sub((length - PAGE) / 2) & !(PAGE - 1),
            length,
        })
        .collect()
}

/// Everything worth trying against a wall, before anything has to be guessed.
///
/// Ordered cheapest question first. Every one of these is a run of about a tenth of a
/// second, so the whole list costs less than describing it.
///
/// # Errors
///
/// Never. The signature is fallible so a caller can chain it with axes that need to read
/// something first.
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
    /// **The invariant behind the whole sweep being controlled.** An axis whose variable is
    /// not cleared leaks into every subsequent run, and the sweep goes on describing itself
    /// as one experiment at a time. That is not a wrong answer, it is a wrong *method*, and
    /// it produces confident results.
    ///
    /// The list is derived from `orbistoun-env` now, so this cannot fail for the reason it
    /// once could - a diagnostic added there and forgotten here (D288). It stays because the
    /// derivation could be narrowed again by somebody filtering it differently, and because a
    /// guard nobody has watched reject something is a guard nobody knows anything about.
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
    /// Checked against `orbistoun-cli env`'s own examples, because a wrong shape is a
    /// run that changes nothing and reports a clean negative - which is exactly what a
    /// mis-rendered `Write` value did across two hundred and seventy-six runs.
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

    /// **Nothing happened and nothing was done are different observations.**
    ///
    /// The distinction the whole sweep rests on. Inferring "applied" from whether the
    /// fault moved would collapse them, and that collapse is how a wrong variable format
    /// became twenty-three clean negatives from runs that changed nothing at all.
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

    /// **A fault that moves after the guest got less far is a regression.**
    ///
    /// The one that had to be run to be found. Poisoning zero-initialised statics on the
    /// live wall moved the fault to a completely different address, which read as a lead,
    /// and the guest had reached eight distinct imports instead of twenty-three. The
    /// poison broke it long before it got anywhere near the question being asked.
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
        // And it must not be offered beside a real lead.
        assert!(!change.is_notable());
    }

    /// Reaching *further* than the baseline is still only an observation.
    ///
    /// The intervention changed the program, so more subsystems reached is not evidence
    /// the change was right - only that it was not simply destructive.
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
    ///
    /// Loud, and the one most worth distrusting: an intervention that removes a fault
    /// may only have postponed it.
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
    ///
    /// A guest that wanted a megabyte and indexed near its end faults at the far edge, so
    /// a reservation starting *at* the fault answers a narrower question than the one
    /// worth asking.
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

    /// A reservation near zero does not wrap.
    ///
    /// The live wall faults at `0xfffe0`, and a megabyte centred on it starts below zero
    /// if the arithmetic is allowed to borrow.
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

    /// **A run that never faulted did not stop faulting.**
    ///
    /// The wildcard reported `NoLongerFaulted` whenever the outcome had no fault, without
    /// asking whether the baseline had one - so a guest that spins to the time limit came back
    /// as though every intervention had fixed it. Three map shapes, three false positives, on
    /// the title where it matters most (D356).
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
