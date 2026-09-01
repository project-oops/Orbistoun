//! Finding out what an unnamed function was supposed to *do*.
//!
//! # Why this is not the proposer the backlog describes
//!
//! *Automated stub-semantics search* frames the job as trying candidate semantics and
//! says the constraint is query count: "each query costs a boot and returns one bit. The
//! value of any prior is entirely in reducing the number of queries."
//!
//! **Measured, the space is small and the framing does not hold.** A stub policy
//! expresses `Ok`, `Unimplemented`, or a raw code - three answers and a handful of
//! plausible error values. Forced writes add six argument slots. Both are exhaustively
//! sweepable in minutes, so a prior saves almost nothing.
//!
//! What was actually missing is that **nobody had automated running them**. Every
//! experiment behind the current walls - four answers swept on one, two planted arguments
//! on the others - was a person editing a variable and re-running by hand.
//!
//! # The axis that matters is the side effect, and the return can gate it
//!
//! This was written to sweep planted arguments and explicitly *not* stub returns, quoting
//! `orbistoun-thunk`: *"A stub policy can change what a function **answers**. Nothing could
//! change what a function **does** - and both current walls turned out to be a side effect
//! nobody performed."*
//!
//! **That was half right, and the half it got right is what hid the other half.** The side
//! effect is what matters. But a guest may check the return *before* reading the
//! out-parameter, and then neither intervention alone does anything: plant a base and answer
//! an error, and it takes the failure path without looking; answer success and plant nothing,
//! and it reads a zero. Two clean negatives, which read exactly like proof of absence - and
//! did, for twenty-three functions and several days (D283).
//!
//! So the sweep crosses them. `ORBISTOUN_WRITE` plants a value at the address held in an
//! argument; `ORBISTOUN_RETURN` forces what the call answers. Both are ordinary diagnostics
//! driven through the ordinary run command, so this still adds nothing to the emulator and
//! can still be pointed at a function with no name at all.
//!
//! **The return is a condition here, not a sentinel.** [`RETURN_SENTINELS`] exists for a
//! different question - did the guest compute an address *from* what a call answered - and
//! differencing applies to that. This asks only whether success unlocks the path that reads
//! the slot, and one value answers it (D286).
//!
//! # The oracle is better than one bit
//!
//! A guest that faults computing `base + K` from a `base` it expected filled will fault
//! at `sentinel + K` once a sentinel is planted in the right slot. So the fault address
//! does not merely *change* - it moves **by a knowable amount**.
//!
//! Two sentinels per slot turn that into proof. If planting `S1` faults at `F1` and `S2`
//! faults at `F2`, then `F1 - S1 == F2 - S2` identifies the slot *and* recovers `K`. One
//! sentinel could coincide; two agreeing on an offset is an arithmetic relationship, and
//! that is a different class of evidence from "something moved".

use std::collections::BTreeMap;

use crate::Error;

/// Planting a value in one argument slot of one import.
///
/// Mirrors `ORBISTOUN_WRITE=<import>:<slot>:<value>` exactly, because that is the whole
/// mechanism - there is nothing else to configure and nothing added to the emulator.
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
    /// **A condition, not a sentinel.** [`RETURN_SENTINELS`] asks whether the guest computed
    /// an address *from* what a call answered, and differencing applies to that. This asks
    /// something else: whether answering **success** lets the guest reach the code that reads
    /// the out-parameter at all. Only one value is interesting for that, so this is swept as
    /// present-or-absent rather than as a pair (D286).
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
    /// An out-parameter experiment *is* a planted write, so it is rendered by the one
    /// place that knows every variable's shape rather than by a second copy of the
    /// format here. Two of them when the plant needs the call to succeed first.
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
    /// **Load-bearing.** A planted value that never reached the guest produces a run
    /// identical to the baseline, and reading that as "this slot is not it" is how an
    /// experiment that never happened gets recorded as a negative result. The worker
    /// says so out loud for the same reason.
    pub planted: bool,
    /// Whether the write was attempted and refused.
    ///
    /// **Not the same as not planted, and the difference is a result.** A refusal means
    /// the address held in that argument is not writable guest memory - so the argument
    /// is not a pointer, and cannot be the out-parameter being looked for. That rules
    /// the slot out for a *reason*, where an unattempted write rules out nothing.
    ///
    /// Measured on the live wall: four of six arguments were refused, because they hold
    /// a size and an alignment rather than addresses.
    pub refused: bool,
    /// How many distinct imports the guest reached.
    ///
    /// **The second signal, and it is not optional.** A fault that moves has been
    /// changed, not explained, and the address alone cannot say whether the guest got
    /// further or was broken earlier. D129 records the same lesson about the progress
    /// verdict: reporting one signal hid a run that reached eight more subsystems behind
    /// an instruction pointer that had gone backwards.
    ///
    /// Measured: poisoning zero-initialised statics moved this wall's fault to a
    /// completely different address, and the guest reached **eight** distinct imports
    /// instead of twenty-three. Without this, that read as a lead.
    pub reached: usize,
    /// Whether the fault was at an address the guest asked for.
    ///
    /// **False makes [`Self::fault`] uncomparable.** An illegal instruction, a breakpoint
    /// or a stack overflow carries no address parameters, so the reporter fills the field
    /// with the instruction pointer - which is a real number that is not somewhere the
    /// guest tried to touch. Comparing it run to run answers a question nobody asked.
    ///
    /// Measured: planting at `arg1` of one import derailed the guest into non-code, and
    /// *both* sentinels produced the identical fault address because both were the same
    /// instruction. Two disagreeing offsets, so not an out-parameter; a changed address,
    /// so reported as an inconsistent move. It was neither.
    pub touched: bool,
}

/// Something that can run one experiment and report what happened.
///
/// A trait so the sweep and its arithmetic are testable with no guest, no title and no
/// twenty-second boot - the same seam, for the same reason, as `Ask` in
/// `orbistoun-llm`. What is worth pinning here is the *reasoning*: which slot a set of
/// outcomes implicates, and whether it implicates one at all.
pub trait Trial {
    /// Runs once with the experiment applied, or once with nothing applied for a
    /// baseline.
    ///
    /// # Errors
    ///
    /// If the run could not be made at all. A run that faults is a *result*, not an
    /// error - faulting is the normal outcome here and the address is the measurement.
    fn run(&mut self, experiment: Option<&Experiment>) -> Result<Outcome, Error>;

    /// Runs once with these axes applied, or with none of them for a baseline.
    ///
    /// **On the trait rather than only on `GuestTrial`**, so anything that drives an axis can
    /// be exercised without booting a commercial title. A dispatcher testable only against a
    /// real guest is a dispatcher with no unit tests (D289).
    ///
    /// # Errors
    ///
    /// As [`Self::run`].
    fn spawn_axes(&mut self, axes: &[crate::axis::Axis]) -> Result<Outcome, Error>;
}

/// Two values planted per slot, chosen to be far apart and unmistakable.
///
/// Far apart so an offset that matches between them cannot be coincidence, and
/// recognisable so a fault address containing one is obvious to a person reading a log
/// rather than only to this code.
pub const SENTINELS: [u64; 2] = [0x1100_0000, 0x2200_0000];

/// Two values forced out of a return, chosen the same way and placed higher.
///
/// **Higher on purpose.** A planted argument only has to be somewhere the guest will
/// dereference. A forced *return* may be treated as a region base and indexed a long way
/// into - the wall this exists for computes `base + 0xfffe0` - so a sentinel low enough
/// to land inside something the run already mapped produces no fault at all, and a
/// silent false negative rather than a measurement.
pub const RETURN_SENTINELS: [u64; 2] = [0x7000_0000_0000, 0x7700_0000_0000];

/// How many argument slots a call can carry.
///
/// Six, because that is how many the System V convention passes in registers, which is
/// what `orbistoun-abi` proves the boundary uses.
pub const SLOTS: u8 = 6;

/// What a sweep concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A slot whose planted value the fault address followed, by a fixed offset.
    ///
    /// The strong result: the guest computed its faulting address *from* what was
    /// planted, which is what an out-parameter the guest expected filled looks like.
    OutParameter {
        /// The argument slot.
        slot: u8,
        /// The offset the guest added to it. Never zero.
        offset: i64,
        /// What the call had to answer for the guest to read the slot at all.
        ///
        /// **Carried because the finding is not reproducible without it.** "Slot 0 is an
        /// out-parameter at offset `0xfffe0`" is false on its own - it is true only when the
        /// call also answers success. `None` is the stronger result: the guest read the slot
        /// whatever the call said (D286).
        answer: Option<u64>,
    },
    /// The plant broke the run rather than moving an address.
    ///
    /// **The honest reading of what used to be reported as an inconsistent move.** The
    /// fault stopped being an address the guest asked for - an illegal instruction, a
    /// breakpoint, a stack overflow - so its address is the faulting instruction and there
    /// is nothing to compare against what was planted.
    ///
    /// Note what is deliberately *not* a trigger: the guest reaching fewer imports. In the
    /// axis sweep that is the mark of an intervention that broke the run, and here it is
    /// the mark of one that **worked** - a sentinel planted in a pointer the guest follows
    /// makes it die at the sentinel instead of surviving to the later wall. Importing the
    /// rule from the other sweep reclassified five correct findings.
    ///
    /// Measured on a live title. Planting at `arg1` of `scePthreadMutexattrInit` produced
    /// an *illegal instruction* at a fixed address, identically for both sentinels, with
    /// the guest reaching 19 distinct imports instead of 23. Reported as `Moved`, whose
    /// documentation says "something downstream moved rather than the address being
    /// computed from it" - which claims more than the run supports. Nothing moved. The
    /// guest was derailed into non-code.
    ///
    /// Principle 3 in its third form: an intervention that moves a wall is not a
    /// diagnosis, and this one had not even moved a wall.
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
    /// The fault moved to *exactly* the planted value.
    ///
    /// **Much weaker than it looks, and it was reported as a find until it was run.**
    /// An offset of zero means the guest dereferenced the sentinel directly rather than
    /// computing anything from it - which is what happens whenever a live pointer is
    /// overwritten with something unmapped. It says the argument points at a pointer the
    /// guest follows, and nothing about whether anybody was supposed to fill it in.
    ///
    /// Measured: sweeping every import of one title reported five of these, including
    /// `strlen` at `arg1` - a function that takes one argument. All five were the sweep
    /// breaking the program, not finding its bug.
    ///
    /// Principle 3 names it exactly - *"an intervention that moves a wall is not a
    /// diagnosis"* - so it is a separate outcome rather than a quieter kind of success.
    Dereferenced {
        /// The argument slot.
        slot: u8,
    },
    /// Planting moved the fault, but not by a consistent offset.
    ///
    /// Worth reporting rather than discarding: something downstream changed, which is
    /// not nothing, but it is not the arithmetic relationship either.
    Moved {
        /// The argument slot.
        slot: u8,
    },
    /// Every slot was reached and none of them moved the fault.
    ///
    /// Carries the breakdown, because "none moved" hides two different facts. A slot
    /// whose write *landed* and changed nothing has been tested and cleared. A slot
    /// whose write was *refused* holds something that is not an address, so it could
    /// never have been the out-parameter - which rules it out more firmly, and for a
    /// reason worth reading.
    Unmoved {
        /// Slots where the write landed and the fault did not move.
        tested: Vec<u8>,
        /// Slots holding something that is not writable memory.
        not_addresses: Vec<u8>,
    },
    /// The guest stopped repeating itself and reached further, with nothing faulting.
    ///
    /// **The answer a fault-position oracle cannot give.** A spinning guest never faults, so
    /// comparing fault addresses compares `None` with `None` and answers `Unmoved` however
    /// well the experiment worked - and answers *moved* only when the plant broke a guest that
    /// had been fine, which is the signal inverted.
    ///
    /// Reach is what survives: a guest that escapes a loop starts calling imports it was never
    /// getting to. Already measured on every run, never consulted until now (D351).
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
    /// Separate from [`Self::Unmoved`], and the distinction is the point: a sweep that
    /// never planted anything has measured nothing, and reporting it as "not this
    /// function" is how an experiment that did not happen becomes a negative result.
    NeverPlanted,
}

/// What a call may be forced to answer while a plant is in place.
///
/// `None` first, so a slot that resolves without touching the return is found without one -
/// a finding needing two interventions is strictly weaker than one needing a single
/// intervention, and the ordering makes that automatic (D286).
///
/// `Some(0)` is success as every implemented function here spells it. Nothing else is worth
/// forcing: the question this axis asks is whether the guest reaches the code that reads the
/// out-parameter, and only one answer sends it down that path.
pub const ANSWERS: [Option<u64>; 2] = [None, Some(0)];

/// Every run of a sweep, grouped by the slot **and the condition** it was made under.
///
/// The pair is the key rather than the slot alone: the two sentinels that share a slot *and*
/// a condition are the ones whose faults are differenced, and grouping by slot would compare
/// a forced run against an unforced one and find a relationship between two different
/// experiments (D286).
pub type Grouped = BTreeMap<(u8, Option<u64>), Vec<(u64, Outcome)>>;

/// Every experiment worth running against one target.
///
/// Two sentinels for each slot, each with and without the call forced to succeed - which is
/// every combination, because a boot costs the same whichever it plants and a prior that
/// saves nothing is not worth having.
///
/// **Both dimensions, because one at a time cannot see a two-condition dependency.** The
/// wall at `image+0xafc959` needed the plant *and* the answer: with either alone the fault
/// did not move, and two clean negatives read exactly like proof of absence (D283).
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
    // Keyed by the pair, so the two sentinels that share a slot *and* a condition are the
    // ones compared. Grouping by slot alone would difference a forced run against an
    // unforced one and find a relationship between two different experiments (D286).
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
/// **The rule, in one place.** Two sweeps need it - planting a value at an argument and
/// forcing a value out of a return - and they ask the same arithmetic question of two
/// different interventions: does the fault land at a fixed distance from whatever was
/// planted? A second copy of this would be a second copy of the four corrections that
/// shaped it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Agreement {
    /// Every sentinel produced a fault a fixed non-zero distance away.
    ///
    /// The only outcome here that says the value flowed into an address.
    Offset(i64),
    /// Every sentinel produced a fault at exactly the value planted.
    ///
    /// The guest used it as an address rather than computing from it, which is what
    /// overwriting any live pointer looks like.
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
    /// **The strongest negative available, and it was hiding inside
    /// [`Self::Inconsistent`].** An unchanged fault produces a *different* offset from
    /// each sentinel - it is measured from the planted value, and the planted values
    /// differ - so the arithmetic reads as disagreement when the truth is that the guest
    /// was indifferent to what it was handed. Twenty-two of twenty-three imports reported
    /// disagreement on the first run of the return sweep, which reads as noise; they were
    /// all this, which reads as a result.
    Unchanged,
    /// The sentinels disagreed, so nothing was computed from them consistently.
    ///
    /// Genuinely ambiguous, and rarer than it looked before [`Self::Unchanged`] existed:
    /// the fault moved, differently for each planted value, so something downstream shifted
    /// rather than an address being computed.
    Inconsistent,
    /// Nothing landed, so nothing was measured.
    NotApplied,
}

/// What a set of sentinel runs against one target agreed about.
///
/// `applied` is per-run and comes from the run itself, never inferred from whether the
/// fault moved - collapsing "nothing happened" into "nothing was done" is how a wrong
/// variable format became twenty-three clean negatives.
#[must_use]
pub fn agreement(baseline: &Outcome, runs: &[(u64, Outcome)]) -> Agreement {
    let applied: Vec<&(u64, Outcome)> = runs.iter().filter(|(_, o)| o.planted).collect();
    if applied.is_empty() {
        return Agreement::NotApplied;
    }
    // Before any arithmetic. A fault that is not at an address the guest asked for has no
    // offset worth computing - the number is the faulting instruction.
    // **And only where there was a fault at all.** `touched` says whether a fault landed
    // somewhere the guest asked for, which is a question about a fault - and a run that did
    // not fault carries `false` for the same reason an empty list carries no first element.
    // Reading that as "derailed into non-code" reports a guest that ran to the time limit as
    // one that crashed in the weeds (D351).
    if let Some((_, outcome)) = applied
        .iter()
        .find(|(_, o)| o.fault.is_some() && !o.touched)
    {
        return Agreement::Derailed {
            reached: outcome.reached,
        };
    }
    // Before the arithmetic, because the arithmetic cannot see it. An unchanged fault is
    // measured from two different planted values, so it yields two different offsets and
    // reads as disagreement - which is the opposite of what it means.
    if applied
        .iter()
        .all(|(_, outcome)| outcome.fault == baseline.fault)
    {
        return Agreement::Unchanged;
    }
    let offsets: Vec<i64> = applied
        .iter()
        .filter_map(|(value, outcome)| {
            // Wrapping, because a guest that indexes below a planted base produces a
            // fault *under* it and the difference is legitimately negative.
            Some(outcome.fault?.wrapping_sub(*value) as i64)
        })
        .collect();
    // More than one, and all agreeing. A single agreeing value is a coincidence with
    // nothing to disagree with.
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
/// Separated from running them so the reasoning is testable without a guest, which is
/// the only part of this that can be wrong in a quiet way.
#[must_use]
pub fn conclude(baseline: &Outcome, by_slot: &Grouped) -> Finding {
    // Nothing planted **and** nothing refused. A refusal is a measurement - it says the
    // argument holds something that is not writable memory - so a sweep that was refused
    // everywhere has learned about every slot, where one that was never attempted has
    // learned about none.
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
        // **Once per slot, not once per condition.** A slot appears in this map twice - with
        // and without a forced answer - and "tested" is a fact about the slot rather than
        // about the pair. Listing it twice would read as twelve slots on a six-argument call,
        // which is a report claiming more than the sweep did (D286).
        if results.iter().any(|(_, outcome)| outcome.planted) {
            if !tested.contains(slot) {
                tested.push(*slot);
            }
        } else if results.iter().any(|(_, outcome)| outcome.refused)
            && !not_addresses.contains(slot)
        {
            not_addresses.push(*slot);
        }
        // The arithmetic itself lives in `agreement`, because the return sweep asks the
        // identical question of a different intervention and a second copy of this would
        // be a second copy of the four corrections that shaped it.
        match agreement(baseline, results) {
            Agreement::Offset(offset) => {
                return Finding::OutParameter {
                    slot: *slot,
                    offset,
                    answer: *answer,
                };
            }
            // Ranked below a real offset, so it cannot be read as the answer: the guest
            // used the sentinel *as* the address, which is what overwriting any live
            // pointer does rather than what an unfilled out-parameter does.
            Agreement::Dereferenced => {
                dereferenced.get_or_insert(*slot);
                continue;
            }
            Agreement::Derailed { reached } => {
                derailed.get_or_insert((*slot, false, reached));
                continue;
            }
            // `Unchanged` is already carried by `tested` and the `moved` check below,
            // which together produce `Unmoved` - the same statement in this sweep's own
            // vocabulary. Nothing to add here.
            Agreement::Unchanged | Agreement::Inconsistent | Agreement::NotApplied => {}
        }
        if moved.is_none()
            && results
                .iter()
                .any(|(_, outcome)| outcome.planted && outcome.fault != baseline.fault)
        {
            moved = Some(*slot);
        }
        // **The oracle when there is no fault to move.** A guest that spins never faults, so
        // `fault != fault` compares `None` with `None` and answers "unmoved" however well the
        // experiment worked - and answers *moved* only when the plant broke a guest that had
        // been fine, which is the signal inverted (D351).
        //
        // Reach is what changes when a guest escapes a loop: it starts calling imports it was
        // never getting to. Already measured, never consulted.
        if escaped.is_none() && baseline.fault.is_none() {
            if let Some((_, outcome)) = results
                .iter()
                .find(|(_, o)| o.planted && o.fault.is_none() && o.reached > baseline.reached)
            {
                escaped = Some((*slot, outcome.reached));
            }
        }
    }

    // Ordered by how much each says. A consistent non-zero offset has already returned
    // above; a bare dereference at least identifies a pointer the guest follows; an
    // inconsistent move says least.
    if let Some(slot) = dereferenced {
        return Finding::Dereferenced { slot };
    }
    // Above the vague one, because it says something checkable: the plant broke the run.
    // Below the two that identify an argument, because it does not.
    if let Some((slot, touched, reached)) = derailed {
        return Finding::Derailed {
            slot,
            touched,
            reached,
            was: baseline.reached,
        };
    }
    // **Before `Unmoved`, because on a run with no fault `Unmoved` is not a measurement.**
    // Where the baseline never faulted there was no position to move, so the fault-position
    // branches above have said nothing - and reporting "tested and cleared" from them would
    // record a non-answer as a negative result (D229, D230, D351).
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
        /// False models the guest being derailed into non-code: the fault is an illegal
        /// instruction, so its address is the faulting instruction rather than anywhere
        /// the guest tried to touch.
        touched: bool,
        /// What the call must answer before this guest reads the out-parameter at all.
        ///
        /// `None` models a guest that reads it unconditionally. `Some(v)` models the real
        /// wall: the guest checks the return, and on anything but `v` takes the failure path
        /// and never looks at what was planted - so the plant alone changes nothing and reads
        /// as a clean negative (D283).
        reads_only_when_answered: Option<u64>,
    }

    impl Trial for Guest {
        /// Axes this mock does not model leave the run exactly as it was.
        ///
        /// Honest rather than convenient: a mock that invented a movement would let a
        /// dispatcher test pass on behaviour no guest has.
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
                // Only a planted run can derail the guest, so this is the only outcome
                // whose fault kind an experiment gets to change.
                touched: self.touched,
            })
        }
    }

    /// **The result the whole module exists for.** The slot is identified, and so is the
    /// offset the guest applied to it.
    ///
    /// One sentinel could coincide; two agreeing on an offset is an arithmetic
    /// relationship, which is a different class of evidence from "something moved".
    #[test]
    fn a_slot_the_fault_follows_is_identified_with_its_offset() {
        let mut guest = Guest {
            out_parameter: Some(3),
            // The shape the real wall has: the guest indexes 0x20 below a base it
            // expected filled.
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

    /// **A plant that derails the guest has not moved an address.**
    ///
    /// The one that had to be run against a live title to be found. Planting at `arg1` of
    /// `scePthreadMutexattrInit` produced an *illegal instruction* - so the fault address
    /// is the faulting instruction, identical for both sentinels because both are the same
    /// instruction. Two disagreeing offsets, so not an out-parameter; a changed address, so
    /// it came out as `Moved`, whose documentation claims something downstream moved.
    /// Nothing moved.
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

    /// **Getting less far does not disqualify a consistent offset - it is the point.**
    ///
    /// Written the other way round first, by importing the axis sweep's rule that a run
    /// reaching fewer imports has broken rather than answered. The two sweeps ask opposite
    /// questions. Poisoning a region and getting less far means the poison broke the run.
    /// Planting a sentinel in a pointer the guest follows and getting less far is what
    /// success looks like: it now dies at the sentinel rather than surviving to the wall
    /// that prompted the experiment. The wrong rule reclassified five correct findings on
    /// a live title before this test existed.
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
                            // A consistent offset, from a run that died at the sentinel
                            // and so never reached what the baseline reached.
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

    /// **A fault landing exactly on the sentinel is not an out-parameter.**
    ///
    /// It is what overwriting any live pointer looks like: the guest dereferences what
    /// was planted instead of computing from it. Sweeping one title reported five of
    /// these - including `strlen` at `arg1`, which takes one argument - and every one was
    /// the sweep breaking the program rather than finding its bug.
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

    /// A real offset outranks a bare dereference.
    ///
    /// Both can appear in one sweep, and the arithmetic relationship is the one worth
    /// acting on - so it must win rather than depend on which slot came first.
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

    /// **A slot holding something that is not an address is ruled out, and says why.**
    ///
    /// Measured on the live wall: four of six arguments were refused because they hold a
    /// size and an alignment rather than pointers. That rules them out more firmly than a
    /// planted write that changed nothing - they could never have been the out-parameter -
    /// and reporting both as a bare "nothing moved" throws the reason away.
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

    /// A sweep refused everywhere has still measured every slot.
    ///
    /// `NeverPlanted` means *nothing was attempted*. Refusals were attempted and answered,
    /// so reading them as "nothing was measured" would discard a complete result.
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

    /// **A sweep that never planted anything has measured nothing.**
    ///
    /// Reported separately from "nothing moved", because reading a write that never
    /// landed as evidence against a slot is how an experiment that did not happen
    /// becomes a negative result - the exact failure the worker prints a warning for.
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
    ///
    /// The whole strength of the test is two values agreeing on an offset. A single
    /// observation has nothing to disagree with it, and calling that an out-parameter
    /// would turn a coincidence into a finding.
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

    /// The environment value is exactly what the worker parses.
    ///
    /// A guest that reads its out-parameter only when the call succeeded.
    ///
    /// **The case a one-dimensional sweep is structurally blind to**, and the one the real
    /// wall turned out to be. Planting alone leaves the fault exactly where it was, because
    /// the guest checks the return first and takes the failure path. Forcing the return alone
    /// leaves it too, because there is nothing in the slot to read. Each half is a clean
    /// negative, and two clean negatives read as proof of absence (D283).
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
                // Carried, because without it the finding is false: the guest does not read
                // this slot unconditionally, and a report saying so cannot be reproduced.
                answer: Some(0),
            }
        );
    }

    /// And the same guest is invisible when only one axis moves.
    ///
    /// **The negative half, and the reason the second dimension exists.** Sweeping the old
    /// way - every slot, both sentinels, never touching the return - reports that no argument
    /// of this function reaches the address. That is exactly what was believed about
    /// twenty-three functions.
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
                    // The old sweep, reconstructed: no forced answer, ever.
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

    /// A format this gets wrong produces a sweep where every run silently plants
    /// nothing, and twenty-four boots later reports that the function is not the one.
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

    /// A negative offset survives the round trip.
    ///
    /// The real wall indexes *below* its base - `0x100000 - 0x20` - so an unsigned
    /// subtraction that saturated or panicked here would miss the case this was built
    /// for.
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

    /// **A spinning guest never faults, so the fault oracle answers nothing.**
    ///
    /// Both runs carry `fault: None`, so `outcome.fault != baseline.fault` is false however
    /// well the experiment worked - and `Unmoved` would record a non-answer as "tested and
    /// cleared", which is the failure D229 and D230 both name. Reach is the signal that
    /// survives (D351).
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

    /// **And reaching no further is still not an escape.**
    ///
    /// The guard has to refuse as well as fire, or a spinning title reports an escape on every
    /// slot it plants and the finding means nothing.
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

    /// A run that *did* fault is judged on the fault, not on reach.
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
