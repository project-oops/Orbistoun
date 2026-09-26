//! Finding out what an unnamed function was supposed to do, by sweeping planted arguments and
//! forced returns.
//!
//! The space is small: a stub answers `Ok`, `Unimplemented` or a raw code, and forced writes cover
//! six argument slots, so every combination is swept rather than ranked by a prior. The sweep
//! crosses `ORBISTOUN_WRITE` (plant a value at the address an argument holds) with
//! `ORBISTOUN_RETURN` (force the answer), because a guest may check the return before reading the
//! out-parameter, and then either intervention alone reads as a clean negative (D286).
//!
//! The oracle is arithmetic: a guest that faults at `base + K` faults at `sentinel + K` once a
//! sentinel is planted in the right slot. Two sentinels whose faults agree on the same offset
//! identify the slot and recover `K`; one could coincide.

use std::collections::BTreeMap;

use crate::Error;

/// Planting a value in one argument slot of one import.
///
/// Mirrors `ORBISTOUN_WRITE=<import>:<slot>:<value>`, the whole mechanism; nothing is added to the
/// emulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Experiment {
    /// The import, by name where it has one and by hash where it does not.
    pub target: String,
    /// Which argument holds the address to write through.
    pub slot: u8,
    /// What to plant there.
    pub value: u64,
    /// What to force the call to answer, when the plant alone is not enough.
    ///
    /// A condition, not a sentinel: it asks whether answering success lets the guest reach the code
    /// that reads the out-parameter, so it is swept as present or absent (D286).
    pub answer: Option<u64>,
}

impl Experiment {
    /// The environment value the worker parses.
    #[must_use]
    pub fn as_env(&self) -> String {
        self.axes()
            .first()
            .map(|axis| axis.env().1)
            .unwrap_or_default()
    }

    /// The same thing, as diagnostics `orbistoun-cli env` lists.
    ///
    /// An out-parameter experiment is a planted write, so it is rendered by the one place that
    /// knows every variable's shape. Two diagnostics when the plant needs the call to succeed
    /// first.
    #[must_use]
    pub fn axes(&self) -> Vec<crate::axis::Axis> {
        let mut axes = vec![crate::axis::Axis::Write {
            target: self.target.clone(),
            slot: self.slot,
            value: self.value,
        }];
        if let Some(answer) = self.answer {
            axes.push(crate::axis::Axis::Return {
                target: self.target.clone(),
                value: answer,
            });
        }
        axes
    }
}

/// What one run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Where the guest faulted, if it did.
    pub fault: Option<u64>,
    /// Whether the write actually landed.
    ///
    /// A plant that never reached the guest produces a run identical to the baseline, which must
    /// not be read as "this slot is not it".
    pub planted: bool,
    /// Whether the write was attempted and refused.
    ///
    /// A refusal is a result: the address in that argument is not writable guest memory, so the
    /// argument is not a pointer and cannot be the out-parameter. An unattempted write rules out
    /// nothing.
    pub refused: bool,
    /// How many distinct imports the guest reached.
    ///
    /// The second signal: a fault address alone cannot say whether the guest got further or broke
    /// earlier (D129).
    pub reached: usize,
    /// Whether the fault was at an address the guest asked for.
    ///
    /// False makes [`Self::fault`] uncomparable: an illegal instruction, breakpoint or stack
    /// overflow carries no address parameters, so the reporter fills the field with the instruction
    /// pointer.
    pub touched: bool,
}

/// Something that can run one experiment and report what happened.
///
/// A trait so the sweep and its reasoning (which slot a set of outcomes implicates, if any) are
/// testable with no guest and no boot.
pub trait Trial {
    /// Runs once with the experiment applied, or once with nothing applied for a baseline.
    ///
    /// # Errors
    ///
    /// If the run could not be made at all. A run that faults is a result, not an error: the fault
    /// address is the measurement.
    fn run(&mut self, experiment: Option<&Experiment>) -> Result<Outcome, Error>;

    /// Runs once with these axes applied, or with none of them for a baseline.
    ///
    /// On the trait so anything that drives an axis is testable without booting a title.
    ///
    /// # Errors
    ///
    /// As [`Self::run`].
    fn spawn_axes(&mut self, axes: &[crate::axis::Axis]) -> Result<Outcome, Error>;
}

/// Two values planted per slot, chosen to be far apart and unmistakable.
///
/// Far apart so a matching offset cannot be coincidence, and recognisable in a fault address to a
/// person reading a log.
pub const SENTINELS: [u64; 2] = [0x1100_0000, 0x2200_0000];

/// Two values forced out of a return, chosen the same way and placed higher.
///
/// A forced return may be treated as a region base and indexed far into (`base + 0xfffe0`), so a
/// low sentinel could land inside memory the run already mapped and produce no fault at all.
pub const RETURN_SENTINELS: [u64; 2] = [0x7000_0000_0000, 0x7700_0000_0000];

/// How many argument slots a call can carry.
///
/// Six, the registers the System V convention passes arguments in, which `orbistoun-abi` proves the
/// boundary uses.
pub const SLOTS: u8 = 6;

/// What a sweep concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A slot whose planted value the fault address followed, by a fixed offset.
    ///
    /// The guest computed its faulting address from what was planted, which is what an
    /// out-parameter the guest expected filled looks like.
    OutParameter {
        /// The argument slot.
        slot: u8,
        /// The offset the guest added to it. Never zero.
        offset: i64,
        /// What the call had to answer for the guest to read the slot at all.
        ///
        /// Part of the finding: an out-parameter found under a forced success is not reproducible
        /// without it. `None` is the stronger result: the guest read the slot whatever the call
        /// said (D286).
        answer: Option<u64>,
    },
    /// The plant broke the run rather than moving an address.
    ///
    /// The fault stopped being an address the guest asked for (an illegal instruction, a
    /// breakpoint, a stack overflow), so its address is the faulting instruction and there is
    /// nothing to compare. Reaching fewer imports is not a trigger here: a sentinel in a pointer
    /// the guest follows makes it die at the sentinel before the later wall, which is success.
    Derailed {
        /// The argument slot.
        slot: u8,
        /// Whether the fault was still at an address the guest asked for.
        touched: bool,
        /// Distinct imports reached with the value planted.
        reached: usize,
        /// And without it.
        was: usize,
    },
    /// The fault moved to exactly the planted value.
    ///
    /// An offset of zero means the guest dereferenced the sentinel directly, which is what
    /// overwriting any live pointer does. It says the argument points at a pointer the guest
    /// follows, not that anything was supposed to fill it in, so it is a separate outcome rather
    /// than a weaker success.
    Dereferenced {
        /// The argument slot.
        slot: u8,
    },
    /// Planting moved the fault, but not by a consistent offset.
    ///
    /// Something downstream changed, but not by the arithmetic relationship.
    Moved {
        /// The argument slot.
        slot: u8,
    },
    /// Every slot was reached and none of them moved the fault.
    ///
    /// A slot whose write landed and changed nothing is tested and cleared. A slot whose write was
    /// refused holds something that is not an address, so it could never be the out-parameter.
    Unmoved {
        /// Slots where the write landed and the fault did not move.
        tested: Vec<u8>,
        /// Slots holding something that is not writable memory.
        not_addresses: Vec<u8>,
    },
    /// The guest stopped repeating itself and reached further, with nothing faulting.
    ///
    /// A spinning guest never faults, so comparing fault addresses compares `None` with `None`.
    /// Reach is the signal that survives: a guest that escapes a loop calls imports it was never
    /// getting to (D351).
    Escaped {
        /// Which argument was planted.
        slot: u8,
        /// Distinct imports it reached.
        reached: usize,
        /// What the baseline reached.
        was: usize,
    },
    /// No experiment reached the target at all.
    ///
    /// Separate from [`Self::Unmoved`]: a sweep that never planted anything has measured nothing
    /// and is not evidence against the function.
    NeverPlanted,
}

/// What a call may be forced to answer while a plant is in place.
///
/// `None` first, so a slot that resolves without touching the return is found without it: a finding
/// needing one intervention is stronger than one needing two (D286). `Some(0)` is success as every
/// implemented function here spells it, and the only answer that sends the guest down the path that
/// reads the out-parameter.
pub const ANSWERS: [Option<u64>; 2] = [None, Some(0)];

/// Every run of a sweep, grouped by the slot and the condition it was made under.
///
/// The two sentinels that share a slot and a condition are the ones whose faults are differenced;
/// grouping by slot alone would compare a forced run against an unforced one (D286).
pub type Grouped = BTreeMap<(u8, Option<u64>), Vec<(u64, Outcome)>>;

/// Every experiment worth running against one target.
///
/// Two sentinels for each slot, each with and without the call forced to succeed. Both dimensions
/// at once, because one at a time cannot see a dependency on the plant and the answer together.
#[must_use]
pub fn sweep(target: &str) -> Vec<Experiment> {
    let mut out = Vec::with_capacity(usize::from(SLOTS) * SENTINELS.len() * ANSWERS.len());
    for slot in 0..SLOTS {
        for answer in ANSWERS {
            for value in SENTINELS {
                out.push(Experiment {
                    target: target.to_owned(),
                    slot,
                    value,
                    answer,
                });
            }
        }
    }
    out
}

/// Runs the whole sweep and says what it found.
///
/// # Errors
///
/// If a run could not be made. A faulting run is a result, not an error.
pub fn investigate(trial: &mut impl Trial, target: &str) -> Result<(Finding, Vec<Outcome>), Error> {
    let baseline = trial.run(None)?;
    let mut outcomes = Vec::new();
    // Keyed by the pair, so only runs sharing a slot and a condition are differenced.
    let mut by_slot: Grouped = BTreeMap::new();

    for experiment in sweep(target) {
        let outcome = trial.run(Some(&experiment))?;
        outcomes.push(outcome.clone());
        by_slot
            .entry((experiment.slot, experiment.answer))
            .or_default()
            .push((experiment.value, outcome));
    }

    Ok((conclude(&baseline, &by_slot), outcomes))
}

/// What a set of sentinels agreed about, if anything.
///
/// The one rule for both sweeps, planted arguments and forced returns: does the fault land at a
/// fixed distance from whatever was planted?
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Agreement {
    /// Every sentinel produced a fault a fixed non-zero distance away.
    ///
    /// The only outcome here that says the value flowed into an address.
    Offset(i64),
    /// Every sentinel produced a fault at exactly the value planted.
    ///
    /// The guest used it as an address rather than computing from it, which is what overwriting any
    /// live pointer looks like.
    Dereferenced,
    /// A sentinel produced a fault that is not at an address the guest asked for.
    ///
    /// An illegal instruction, a breakpoint, a stack overflow. Nothing to subtract from.
    Derailed {
        /// Distinct imports reached.
        reached: usize,
    },
    /// Every run faulted exactly where the baseline did.
    ///
    /// The strongest negative. An unchanged fault yields a different offset from each sentinel, so
    /// the arithmetic alone would read it as [`Self::Inconsistent`] when the guest was indifferent
    /// to what it was handed.
    Unchanged,
    /// The sentinels disagreed, so nothing was computed from them consistently.
    ///
    /// The fault moved differently for each planted value: something downstream shifted rather than
    /// an address being computed.
    Inconsistent,
    /// Nothing landed, so nothing was measured.
    NotApplied,
}

/// What a set of sentinel runs against one target agreed about.
///
/// `applied` is per-run and comes from the run itself, never inferred from whether the fault moved.
#[must_use]
pub fn agreement(baseline: &Outcome, runs: &[(u64, Outcome)]) -> Agreement {
    let applied: Vec<&(u64, Outcome)> = runs.iter().filter(|(_, o)| o.planted).collect();
    if applied.is_empty() {
        return Agreement::NotApplied;
    }
    // Before any arithmetic: a fault not at an address the guest asked for is the faulting
    // instruction, with no offset worth computing. Only where there was a fault at all, because a
    // run that did not fault carries `touched == false` too, and it ran to the time limit rather
    // than derailing (D351).
    if let Some((_, outcome)) = applied
        .iter()
        .find(|(_, o)| o.fault.is_some() && !o.touched)
    {
        return Agreement::Derailed {
            reached: outcome.reached,
        };
    }
    // Before the arithmetic, which would read an unchanged fault measured from two planted values
    // as disagreement.
    if applied
        .iter()
        .all(|(_, outcome)| outcome.fault == baseline.fault)
    {
        return Agreement::Unchanged;
    }
    let offsets: Vec<i64> = applied
        .iter()
        .filter_map(|(value, outcome)| {
            // Wrapping, because a guest that indexes below a planted base faults under it and the
            // difference is legitimately negative.
            Some(outcome.fault?.wrapping_sub(*value) as i64)
        })
        .collect();
    // More than one, all agreeing: a single value has nothing to disagree with.
    if offsets.len() < 2 || offsets.windows(2).any(|pair| pair[0] != pair[1]) {
        return Agreement::Inconsistent;
    }
    if offsets[0] == 0 {
        return Agreement::Dereferenced;
    }
    Agreement::Offset(offsets[0])
}

/// Reads a set of outcomes for what they implicate.
///
/// Separate from running them so the reasoning is testable without a guest.
#[must_use]
pub fn conclude(baseline: &Outcome, by_slot: &Grouped) -> Finding {
    // Nothing planted and nothing refused. A refusal is a measurement, so a sweep refused
    // everywhere has learned about every slot.
    if !by_slot
        .values()
        .flatten()
        .any(|(_, outcome)| outcome.planted || outcome.refused)
    {
        return Finding::NeverPlanted;
    }

    let mut moved = None;
    let mut escaped = None;
    let mut derailed = None;
    let mut dereferenced = None;
    let mut tested = Vec::new();
    let mut not_addresses = Vec::new();
    for ((slot, answer), results) in by_slot {
        // Once per slot, not once per condition: each slot appears twice in the map, and "tested"
        // is a fact about the slot.
        if results.iter().any(|(_, outcome)| outcome.planted) {
            if !tested.contains(slot) {
                tested.push(*slot);
            }
        } else if results.iter().any(|(_, outcome)| outcome.refused)
            && !not_addresses.contains(slot)
        {
            not_addresses.push(*slot);
        }
        // The arithmetic lives in `agreement`, shared with the return sweep.
        match agreement(baseline, results) {
            Agreement::Offset(offset) => {
                return Finding::OutParameter {
                    slot: *slot,
                    offset,
                    answer: *answer,
                };
            }
            // Ranked below a real offset: the guest used the sentinel as the address, which is what
            // overwriting any live pointer does.
            Agreement::Dereferenced => {
                dereferenced.get_or_insert(*slot);
                continue;
            }
            Agreement::Derailed { reached } => {
                derailed.get_or_insert((*slot, false, reached));
                continue;
            }
            // `Unchanged` is already carried by `tested` and the `moved` check below, which produce
            // `Unmoved`.
            Agreement::Unchanged | Agreement::Inconsistent | Agreement::NotApplied => {}
        }
        if moved.is_none()
            && results
                .iter()
                .any(|(_, outcome)| outcome.planted && outcome.fault != baseline.fault)
        {
            moved = Some(*slot);
        }
        // When there is no fault to move, reach is the oracle: a spinning guest compares `None`
        // with `None` and would read as unmoved however well the experiment worked (D351).
        if escaped.is_none() && baseline.fault.is_none() {
            if let Some((_, outcome)) = results
                .iter()
                .find(|(_, o)| o.planted && o.fault.is_none() && o.reached > baseline.reached)
            {
                escaped = Some((*slot, outcome.reached));
            }
        }
    }

    // Ordered by how much each says. A consistent non-zero offset has already returned; a bare
    // dereference identifies a pointer the guest follows; an inconsistent move says least.
    if let Some(slot) = dereferenced {
        return Finding::Dereferenced { slot };
    }
    // Above the vague one, because it says something checkable: the plant broke the run. Below the
    // two that identify an argument.
    if let Some((slot, touched, reached)) = derailed {
        return Finding::Derailed {
            slot,
            touched,
            reached,
            was: baseline.reached,
        };
    }
    // Before `Unmoved`: where the baseline never faulted, the fault-position branches have said
    // nothing, and "tested and cleared" would record a non-answer as a negative (D351).
    if let Some((slot, reached)) = escaped {
        return Finding::Escaped {
            slot,
            reached,
            was: baseline.reached,
        };
    }
    moved.map_or(
        Finding::Unmoved {
            tested,
            not_addresses,
        },
        |slot| Finding::Moved { slot },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ANSWERS, Experiment, Finding, Grouped, Outcome, SENTINELS, SLOTS, Trial, conclude,
        investigate,
    };
    use std::collections::BTreeMap;

    /// A guest that computes its faulting address from one slot.
    #[derive(Debug)]
    struct Guest {
        /// The slot that is really the out-parameter, if any.
        out_parameter: Option<u8>,
        /// What the guest adds to it.
        offset: i64,
        /// Whether writes land at all.
        plants: bool,
        /// Slots holding something that is not a writable address.
        not_addresses: &'static [u8],
        /// Where it faults with nothing planted.
        baseline: u64,
        /// How far it gets.
        reached: usize,
        /// Whether its fault is at an address it asked for once something is planted.
        ///
        /// False models the guest being derailed into non-code: the fault address is the faulting
        /// instruction.
        touched: bool,
        /// What the call must answer before this guest reads the out-parameter at all.
        ///
        /// `None` models a guest that reads it unconditionally. `Some(v)` models a guest that
        /// checks the return and on anything but `v` takes the failure path without looking at what
        /// was planted.
        reads_only_when_answered: Option<u64>,
    }

    impl Trial for Guest {
        /// Axes this mock does not model leave the run exactly as it was, so no dispatcher test
        /// passes on a movement no guest has.
        fn spawn_axes(&mut self, axes: &[crate::axis::Axis]) -> Result<Outcome, crate::Error> {
            Ok(Outcome {
                fault: Some(self.baseline),
                planted: !axes.is_empty(),
                refused: false,
                reached: self.reached,
                touched: true,
            })
        }

        fn run(&mut self, experiment: Option<&Experiment>) -> Result<Outcome, crate::Error> {
            let Some(experiment) = experiment else {
                return Ok(Outcome {
                    fault: Some(self.baseline),
                    planted: false,
                    refused: false,
                    reached: self.reached,
                    touched: true,
                });
            };
            if !self.plants {
                return Ok(Outcome {
                    fault: Some(self.baseline),
                    planted: false,
                    refused: false,
                    reached: self.reached,
                    touched: true,
                });
            }
            if self.not_addresses.contains(&experiment.slot) {
                return Ok(Outcome {
                    fault: Some(self.baseline),
                    planted: false,
                    refused: true,
                    reached: self.reached,
                    touched: true,
                });
            }
            let gate_open = self
                .reads_only_when_answered
                .is_none_or(|needed| experiment.answer == Some(needed));
            let fault = if Some(experiment.slot) == self.out_parameter && gate_open {
                experiment.value.wrapping_add(self.offset as u64)
            } else {
                self.baseline
            };
            Ok(Outcome {
                fault: Some(fault),
                planted: true,
                refused: false,
                reached: self.reached,
                // Only a planted run can derail the guest.
                touched: self.touched,
            })
        }
    }

    /// Two sentinels agreeing on an offset identify the slot and the offset the guest applied to
    /// it.
    #[test]
    fn a_slot_the_fault_follows_is_identified_with_its_offset() {
        let mut guest = Guest {
            out_parameter: Some(3),
            // The guest indexes 0x20 below a base it expected filled.
            offset: -0x20,
            plants: true,
            not_addresses: &[],
            baseline: 0xfffe0,
            reached: 23,
            touched: true,
            reads_only_when_answered: None,
        };
        let (finding, outcomes) = investigate(&mut guest, "libkernel::0xabc").expect("runs");
        assert_eq!(
            finding,
            Finding::OutParameter {
                slot: 3,
                offset: -0x20,
                answer: None,
            }
        );
        assert_eq!(
            outcomes.len(),
            usize::from(SLOTS) * SENTINELS.len() * ANSWERS.len()
        );
    }

    /// A plant that derails the guest into non-code has not moved an address, even though both
    /// sentinels fault at the same instruction.
    #[test]
    fn a_plant_that_derails_the_guest_has_not_moved_an_address() {
        let baseline = Outcome {
            fault: Some(0xfffe0),
            planted: false,
            refused: false,
            reached: 23,
            touched: true,
        };
        let mut by_slot = BTreeMap::new();
        by_slot.insert(
            (1_u8, None),
            SENTINELS
                .iter()
                .map(|value| {
                    (
                        *value,
                        Outcome {
                            // The same address for both, because it is the instruction.
                            fault: Some(0x4000_014c_c44e),
                            planted: true,
                            refused: false,
                            reached: 19,
                            touched: false,
                        },
                    )
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            conclude(&baseline, &by_slot),
            Finding::Derailed {
                slot: 1,
                touched: false,
                reached: 19,
                was: 23,
            }
        );
    }

    /// Getting less far does not disqualify a consistent offset: a guest that follows the planted
    /// pointer dies at the sentinel before reaching the baseline's wall.
    #[test]
    fn getting_less_far_does_not_disqualify_a_consistent_offset() {
        let baseline = Outcome {
            fault: Some(0xfffe0),
            planted: false,
            refused: false,
            reached: 23,
            touched: true,
        };
        let mut by_slot = BTreeMap::new();
        by_slot.insert(
            (1_u8, None),
            SENTINELS
                .iter()
                .map(|value| {
                    (
                        *value,
                        Outcome {
                            // A consistent offset, from a run that died at the sentinel and so
                            // never reached what the baseline reached.
                            fault: Some(value.wrapping_add(0xfffe0)),
                            planted: true,
                            refused: false,
                            reached: 8,
                            touched: true,
                        },
                    )
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            conclude(&baseline, &by_slot),
            Finding::OutParameter {
                answer: None,
                slot: 1,
                offset: 0xfffe0,
            },
            "a consistent offset was discarded because the run it came from died earlier"
        );
    }

    /// A fault landing exactly on the sentinel is a dereference, not an out-parameter.
    #[test]
    fn a_fault_landing_on_the_sentinel_itself_is_only_a_dereference() {
        let mut guest = Guest {
            out_parameter: Some(1),
            offset: 0,
            plants: true,
            not_addresses: &[],
            baseline: 0xfffe0,
            reached: 23,
            touched: true,
            reads_only_when_answered: None,
        };
        let (finding, _) = investigate(&mut guest, "libkernel::0xabc").expect("runs");
        assert_eq!(finding, Finding::Dereferenced { slot: 1 });
    }

    /// A real offset outranks a bare dereference, whichever slot comes first.
    #[test]
    fn a_real_offset_outranks_a_bare_dereference() {
        let baseline = Outcome {
            fault: Some(0xfffe0),
            planted: false,
            refused: false,
            reached: 23,
            touched: true,
        };
        let seen = |fault: fn(u64) -> u64| {
            SENTINELS
                .iter()
                .map(|value| {
                    (
                        *value,
                        Outcome {
                            fault: Some(fault(*value)),
                            planted: true,
                            refused: false,
                            reached: 23,
                            touched: true,
                        },
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut by_slot = BTreeMap::new();
        // Slot 0 merely dereferences, and is seen first.
        by_slot.insert((0_u8, None), seen(|value| value));
        by_slot.insert((1_u8, None), seen(|value| value.wrapping_add(0xfffe0)));
        assert_eq!(
            conclude(&baseline, &by_slot),
            Finding::OutParameter {
                answer: None,
                slot: 1,
                offset: 0xfffe0
            }
        );
    }

    /// A guest that ignores every slot says so, rather than implicating one.
    #[test]
    fn a_guest_that_ignores_every_slot_reports_unmoved() {
        let mut guest = Guest {
            out_parameter: None,
            offset: 0,
            plants: true,
            not_addresses: &[],
            baseline: 0xfffe0,
            reached: 23,
            touched: true,
            reads_only_when_answered: None,
        };
        let (finding, _) = investigate(&mut guest, "libkernel::0xabc").expect("runs");
        assert_eq!(
            finding,
            Finding::Unmoved {
                tested: (0..SLOTS).collect(),
                not_addresses: Vec::new()
            }
        );
    }

    /// A slot holding something that is not an address is ruled out, and the report says why.
    #[test]
    fn a_slot_that_is_not_an_address_is_ruled_out_separately() {
        let mut guest = Guest {
            out_parameter: None,
            offset: 0,
            plants: true,
            not_addresses: &[1, 2, 3, 4],
            baseline: 0xfffe0,
            reached: 23,
            touched: true,
            reads_only_when_answered: None,
        };
        let (finding, _) = investigate(&mut guest, "libkernel::0xabc").expect("runs");
        assert_eq!(
            finding,
            Finding::Unmoved {
                tested: vec![0, 5],
                not_addresses: vec![1, 2, 3, 4]
            }
        );
    }

    /// A sweep refused everywhere has still measured every slot, so it is not `NeverPlanted`.
    #[test]
    fn a_sweep_refused_everywhere_is_still_a_result() {
        let mut guest = Guest {
            out_parameter: None,
            offset: 0,
            plants: true,
            not_addresses: &[0, 1, 2, 3, 4, 5],
            baseline: 0xfffe0,
            reached: 23,
            touched: true,
            reads_only_when_answered: None,
        };
        let (finding, _) = investigate(&mut guest, "libkernel::0xabc").expect("runs");
        assert_eq!(
            finding,
            Finding::Unmoved {
                tested: Vec::new(),
                not_addresses: (0..SLOTS).collect()
            }
        );
    }

    /// A sweep that never planted anything is reported as measuring nothing, not as "nothing
    /// moved".
    #[test]
    fn a_sweep_that_never_planted_is_not_a_negative_result() {
        let mut guest = Guest {
            out_parameter: Some(0),
            offset: -0x20,
            plants: false,
            not_addresses: &[],
            baseline: 0xfffe0,
            reached: 23,
            touched: true,
            reads_only_when_answered: None,
        };
        let (finding, _) = investigate(&mut guest, "libkernel::0xabc").expect("runs");
        assert_eq!(finding, Finding::NeverPlanted);
    }

    /// One sentinel agreeing with itself is not a conclusion.
    #[test]
    fn a_single_sentinel_is_not_enough_to_conclude() {
        let baseline = Outcome {
            fault: Some(0xfffe0),
            planted: false,
            refused: false,
            reached: 23,
            touched: true,
        };
        let mut by_slot = BTreeMap::new();
        by_slot.insert(
            (2_u8, None),
            vec![(
                0x1100_0000_u64,
                Outcome {
                    fault: Some(0x1100_0000 - 0x20),
                    planted: true,
                    refused: false,
                    reached: 23,
                    touched: true,
                },
            )],
        );
        assert_eq!(conclude(&baseline, &by_slot), Finding::Moved { slot: 2 });
    }

    /// Sentinels that disagree are a move, not a relationship.
    #[test]
    fn disagreeing_sentinels_are_only_a_move() {
        let baseline = Outcome {
            fault: Some(0xfffe0),
            planted: false,
            refused: false,
            reached: 23,
            touched: true,
        };
        let mut by_slot = BTreeMap::new();
        by_slot.insert(
            (1_u8, None),
            vec![
                (
                    0x1100_0000_u64,
                    Outcome {
                        fault: Some(0x1100_0000 - 0x20),
                        planted: true,
                        refused: false,
                        reached: 23,
                        touched: true,
                    },
                ),
                (
                    0x2200_0000_u64,
                    Outcome {
                        // A different offset: whatever moved, it was not this arithmetic.
                        fault: Some(0x2200_0000 - 0x40),
                        planted: true,
                        refused: false,
                        reached: 23,
                        touched: true,
                    },
                ),
            ],
        );
        assert_eq!(conclude(&baseline, &by_slot), Finding::Moved { slot: 1 });
    }

    /// A guest that reads its out-parameter only when the call succeeded is found, with the answer
    /// it needs.
    #[test]
    fn a_slot_read_only_after_success_is_found_with_the_condition_it_needs() {
        let mut guest = Guest {
            out_parameter: Some(0),
            offset: -0x20,
            plants: true,
            not_addresses: &[],
            baseline: 0xfffe0,
            reached: 23,
            touched: true,
            reads_only_when_answered: Some(0),
        };
        let (finding, _) = investigate(&mut guest, "libkernel::0xabc").expect("runs");
        assert_eq!(
            finding,
            Finding::OutParameter {
                slot: 0,
                offset: -0x20,
                // Carried: the guest does not read this slot unconditionally, and the finding is
                // not reproducible without it.
                answer: Some(0),
            }
        );
    }

    /// The same guest is invisible to a sweep that never forces the return.
    #[test]
    fn the_same_guest_reads_as_a_clean_negative_without_the_second_axis() {
        let mut guest = Guest {
            out_parameter: Some(0),
            offset: -0x20,
            plants: true,
            not_addresses: &[],
            baseline: 0xfffe0,
            reached: 23,
            touched: true,
            reads_only_when_answered: Some(0),
        };
        let baseline = guest.run(None).expect("a baseline");
        let mut by_slot = BTreeMap::new();
        for slot in 0..SLOTS {
            let mut results = Vec::new();
            for value in SENTINELS {
                let experiment = Experiment {
                    target: "libkernel::0xabc".to_owned(),
                    slot,
                    value,
                    // No forced answer, ever.
                    answer: None,
                };
                results.push((value, guest.run(Some(&experiment)).expect("a run")));
            }
            by_slot.insert((slot, None), results);
        }
        assert!(
            matches!(conclude(&baseline, &by_slot), Finding::Unmoved { .. }),
            "one axis at a time must miss this - that is what it was blind to"
        );
    }

    /// The environment value is exactly what the worker parses; a wrong format would plant nothing
    /// on every run.
    #[test]
    fn the_environment_value_matches_what_the_worker_reads() {
        let experiment = Experiment {
            target: "0x6abac2f3dc6f8cee".to_owned(),
            slot: 0,
            value: 0x1100_0000,
            answer: None,
        };
        assert_eq!(experiment.as_env(), "0x6abac2f3dc6f8cee:0:0x11000000");
    }

    /// A negative offset survives the round trip: the offset is a wrapping subtraction, not a
    /// saturating one.
    #[test]
    fn a_fault_below_the_planted_value_reads_as_a_negative_offset() {
        let mut guest = Guest {
            out_parameter: Some(1),
            offset: -0x2000,
            plants: true,
            not_addresses: &[],
            baseline: 0xfffe0,
            reached: 23,
            touched: true,
            reads_only_when_answered: None,
        };
        let (finding, _) = investigate(&mut guest, "x").expect("runs");
        assert_eq!(
            finding,
            Finding::OutParameter {
                answer: None,
                slot: 1,
                offset: -0x2000
            }
        );
    }

    /// A spinning guest that reaches further with a plant is reported as an escape, not as unmoved
    /// (D351).
    #[test]
    fn a_guest_that_escapes_a_loop_is_not_reported_as_unmoved() {
        let baseline = Outcome {
            fault: None,
            planted: false,
            refused: false,
            reached: 4,
            touched: false,
        };
        let mut by_slot: Grouped = BTreeMap::new();
        by_slot.insert(
            (0, None),
            vec![(
                0xdead_0000,
                Outcome {
                    planted: true,
                    reached: 31,
                    ..baseline.clone()
                },
            )],
        );

        assert_eq!(
            conclude(&baseline, &by_slot),
            Finding::Escaped {
                slot: 0,
                reached: 31,
                was: 4,
            }
        );
    }

    /// A spinning guest that reaches no further is not an escape.
    #[test]
    fn planting_without_reaching_further_is_not_an_escape() {
        let baseline = Outcome {
            fault: None,
            planted: false,
            refused: false,
            reached: 4,
            touched: false,
        };
        let mut by_slot: Grouped = BTreeMap::new();
        by_slot.insert(
            (0, None),
            vec![(
                0xdead_0000,
                Outcome {
                    planted: true,
                    reached: 4,
                    ..baseline.clone()
                },
            )],
        );

        assert!(
            !matches!(conclude(&baseline, &by_slot), Finding::Escaped { .. }),
            "the guest asked the same question the same number of times"
        );
    }

    /// A run that did fault is judged on the fault, not on reach.
    #[test]
    fn a_faulting_baseline_still_uses_the_fault_position() {
        let baseline = Outcome {
            fault: Some(0x1000),
            planted: false,
            refused: false,
            reached: 4,
            touched: true,
        };
        let mut by_slot: Grouped = BTreeMap::new();
        by_slot.insert(
            (0, None),
            vec![(
                0xdead_0000,
                Outcome {
                    fault: Some(0x2000),
                    planted: true,
                    reached: 31,
                    ..baseline.clone()
                },
            )],
        );

        assert!(
            !matches!(conclude(&baseline, &by_slot), Finding::Escaped { .. }),
            "reach is the oracle only where there was no fault to compare"
        );
    }
}
