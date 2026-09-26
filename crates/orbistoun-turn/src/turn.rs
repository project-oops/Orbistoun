//! Turning the loop without a person at the wheel.
//!
//! `docs/THE_LOOP.md` leaves two steps to a person: reading the top finding and deciding what to do
//! about it, and writing the code. This module does the first. It is a dispatcher, not a chooser
//! (D231): one boot against a wall costs a fraction of a second, so exhausting a sweep is cheaper
//! than asking a model to choose within it. Each gap the report can name maps to a fixed step, and
//! a sweep step is exhaustive.
//!
//! A model appears in one branch, [`Step::NameAHash`], where the hash is the oracle and model-found
//! names are disjoint from string-harvested ones. A step the loop cannot take is a [`Step::Person`]
//! carrying a specific reason.

use orbistoun_report::diagnose::{Finding, Gap, PRECEDED_BY};

/// One thing the loop can do next, without a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Try to name a bare hash, against the hash itself as the oracle.
    ///
    /// The one branch with a model in it: a proposal is checked by computing the hash, so a wrong
    /// answer cannot survive.
    NameAHash {
        /// The bare hash, as the names database records it.
        hash: String,
    },
    /// Plant a sentinel at every argument of the call that led into a fault.
    ///
    /// Exhaustive rather than ranked: six slots, two sentinels and two conditions is a few seconds
    /// of boots. Crossed with a forced success return, because a guest that checks a call's return
    /// before reading its out-parameter is invisible to either intervention alone (D286).
    SweepArguments {
        /// A bare symbol or bare hash, never `library::symbol`: the variable this reaches splits
        /// its value on `:`.
        target: String,
    },
    /// Ask every other diagnostic against the faulting address.
    ///
    /// Uninitialised memory in each region, and a reservation where the fault landed.
    SweepAxes {
        /// Where the guest died.
        fault: u64,
    },
    /// Trap on the head of the structure the faulting instruction was working on.
    ///
    /// A sweep varies what a call answers; this asks who touched a word, which is left when every
    /// call has been eliminated and the fault has not moved (D276). Mechanical at both ends: the
    /// address comes from the fault's own registers, and what comes back is an instruction offset
    /// in a named region, so the guest's code is never read.
    WatchStructure {
        /// Where the structure starts, from the fault's registers.
        base: u64,
        /// How many eight-byte words, never more than the hardware has.
        words: usize,
    },
    /// Find which unimplemented answer the guest dereferenced, by forcing each to zero.
    ///
    /// A placeholder arriving as an address means the guest followed a "not handled" answer, but
    /// neither finding names which function answered: one names the call that received the code,
    /// the other the import the fault happened inside (D299). So the candidates are swept, and the
    /// oracle is whether the faulting address stops being one of our own codes.
    FindPlaceholderSource {
        /// Every import the run called that nothing implements, in the order it called them.
        candidates: Vec<String>,
    },
    /// Run twice with nothing applied, and see whether the two agree.
    ///
    /// Every other step compares a run under an intervention against one without, which assumes two
    /// runs of one build behave identically; a guest that varies on its own puts its variation into
    /// every difference (D583). Host state such as a heap-allocated handle or a host clock read
    /// makes runs differ (D582). Two runs are enough to know.
    CheckRepeats,
    /// Read back the structure an unimplemented call was handed.
    ///
    /// A finding shows thirty-two bytes at an argument, the dump's fixed window; the structure a
    /// call was given is often longer, with its interesting fields past it. The address comes from
    /// the finding's own evidence and what comes back is bytes. It observes, so a verdict beside it
    /// carries no caveat and it can run on every finding with an address.
    ReadStructure {
        /// Where the structure starts, from an argument the finding recorded.
        address: u64,
        /// How many bytes to read back.
        length: u64,
        /// Which import was handed it, so the report says whose structure this is.
        subject: String,
    },
    /// Nothing here is mechanical.
    Person {
        /// Why not: a statement, never a shrug.
        why: &'static str,
    },
}

impl Step {
    /// Whether the loop can take this step by itself.
    #[must_use]
    pub const fn is_automatic(&self) -> bool {
        !matches!(self, Self::Person { .. })
    }
}

/// The step one finding calls for.
///
/// Total over [`Gap`] by construction, so a new kind of wall is a compile error here rather than a
/// finding that silently produces no work.
#[must_use]
pub fn step(finding: &Finding) -> Step {
    match finding.gap {
        // The oracle is the hash, so a wrong guess is caught rather than believed.
        Gap::Unnamed => finding.subject.as_deref().map_or(
            Step::Person {
                why: "the finding names no import, so there is no hash to work against",
            },
            |subject| Step::NameAHash {
                hash: bare(subject).to_owned(),
            },
        ),
        // Not the subject: a fault's subject is the region the guest died in, and sweeping a region
        // plants nothing. The call that led in is in the evidence.
        Gap::Faulted => preceding_call(finding).map_or(
            Step::Person {
                why: concat!(
                    "the fault records no call leading into it, so there is nothing with ",
                    "arguments to plant in"
                ),
            },
            |target| Step::SweepArguments { target },
        ),
        // A person. A guest entering the kernel through `syscall`, `hlt` or an `int` on an
        // unmeasured vector needs a handler that cannot be written until the vector is
        // characterised on hardware, which is an obSCEne request, not a diagnostic this loop can
        // vary. The finding names it; a person files the measurement and writes the handler.
        Gap::KernelEntryUnimplemented => Step::Person {
            why: concat!(
                "the guest entered the kernel through an instruction orbistoun does not implement; ",
                "the handler needs the vector characterised on the device first (an obSCEne ",
                "request), and writing an interrupt/syscall path is a person's job, not a sweep"
            ),
        },
        // Writing the function stays a person's job and is not automated. Reading the structure it
        // was handed is not implementing it, and informs whoever does.
        Gap::Unimplemented => handed_structure(finding).map_or(
            Step::Person {
                why: concat!(
                    "implementing a function is a person writing code - the loop finds the ",
                    "wall, it does not build what goes behind it"
                ),
            },
            |address| Step::ReadStructure {
                address,
                length: STRUCTURE_BYTES,
                subject: finding.subject.clone().unwrap_or_default(),
            },
        ),
        // Looking is a sweep: every candidate is already in the trace, at a fraction of a second
        // each (D299).
        Gap::ErrorUsedAsPointer => {
            let candidates = unimplemented_calls(finding);
            if candidates.is_empty() {
                Step::Person {
                    why: concat!(
                        "the run recorded no unimplemented call before it, so there is ",
                        "nothing whose answer the guest could have been given"
                    ),
                }
            } else {
                Step::FindPlaceholderSource { candidates }
            }
        }
        // A person: this finding exists because somebody named an import they suspect, and what to
        // conclude from the bytes is the judgement they were making.
        Gap::Captured => Step::Person {
            why: concat!(
                "arguments were captured because somebody asked for them - reading them is ",
                "the question they were asking, not a step the loop can take for them"
            ),
        },
        Gap::AbiViolation => Step::Person {
            why: concat!(
                "how the guest is entered is a property of the thunk, and no diagnostic ",
                "varies it"
            ),
        },
        Gap::ShortRead => Step::Person {
            why: "a short read is a filesystem behaviour, and no diagnostic varies it",
        },
        Gap::GuestGaveUp => Step::Person {
            why: concat!(
                "the guest stopped rather than faulting, so there is no fault address ",
                "for a sweep to compare against - what it was told just before is reading"
            ),
        },
        // Swept: a guest that keeps asking the same question has not accepted the answer, and
        // `ORBISTOUN_RETURN` varies the answer. A spinning guest never faults, so reach is the
        // oracle, which `Finding::Escaped` reads (D351).
        Gap::Spinning => finding.subject.as_deref().map_or(
            Step::Person {
                why: "the finding names no import, so there is nothing to sweep",
            },
            |subject| Step::SweepArguments {
                target: bare(subject).to_owned(),
            },
        ),
        Gap::LinkMismatch => Step::Person {
            why: concat!(
                "a link that differs from the stored plan under the same key is a loader defect, ",
                "and no diagnostic of the guest varies how it was linked"
            ),
        },
        Gap::Submitted => Step::Person {
            why: concat!(
                "a submission is progress, not a wall: what it names - shaders to translate, then ",
                "a backend to run them - is graphics work a person schedules, not a step the loop ",
                "can sweep"
            ),
        },
    }
}

/// Everything a run calls for, in the order the report ranked it, without repeats.
///
/// The report's ranking is kept; this adds deduplication: a hundred calls into one unnamed import
/// is one naming step, and one fault is one axis sweep. A [`Step::SweepAxes`] is appended for the
/// fault, if there was one, because the axes are asked of the address rather than of any call.
#[must_use]
pub fn plan(findings: &[Finding], fault: Option<u64>) -> Vec<Step> {
    // First, and unconditionally: everything below is a comparison between two runs, so it heads
    // the plan as a precondition rather than being ranked among the findings (D583).
    let mut steps: Vec<Step> = vec![Step::CheckRepeats];
    for finding in findings {
        let step = step(finding);
        // A `Person` step is kept once per distinct reason: repeating the same sentence
        // for forty unimplemented functions buries the ones that differ.
        if !steps.contains(&step) {
            steps.push(step);
        }
    }
    if let Some(fault) = fault {
        let axes = Step::SweepAxes { fault };
        if !steps.contains(&axes) {
            steps.push(axes);
        }
        // Asked of the structure rather than of the faulting address, so no finding produces it
        // either. Appended after the axes because it is the more expensive question.
        if let Some(base) = findings.iter().find_map(faulting_object) {
            let watch = Step::WatchStructure {
                base,
                words: WATCHPOINTS,
            };
            if !steps.contains(&watch) {
                steps.push(watch);
            }
        }
    }
    steps
}

/// What taking a step produced.
///
/// Never a bare success: every variant carries what was measured, so a step that found nothing is
/// distinguishable from one that found the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// The argument sweep ran, and this is what it concluded.
    Swept(crate::experiment::Finding),
    /// Every other diagnostic was asked of the faulting address.
    Probed(Vec<(crate::axis::Axis, crate::axis::Change)>),
    /// A watchpoint run was made. What it saw is on the run's error stream, since the guest may die
    /// before any summary, so sites are reported as they happen (D276).
    Watched {
        /// Whether the run still reached the guest at all.
        reached: usize,
    },
    /// A measured out-parameter contract was satisfied, and the guest asked about it.
    ///
    /// It reserves a region, plants its base where the sweep said the guest reads one, and reports
    /// whether that was enough to get further (D289).
    Confirmed {
        /// Where the region was put.
        base: u64,
        /// Where the guest died with it in place, if it still did.
        fault: Option<u64>,
        /// Distinct imports reached with it.
        reached: usize,
        /// And without it.
        was: usize,
    },
    /// The answer the guest was dereferencing came from this function.
    ///
    /// The only auto-keepable outcome: it changes what a function answers and writes no memory, so
    /// `FURTHER` is sufficient evidence (D296).
    Sourced {
        /// Which function answered with the code the guest followed.
        function: String,
        /// Which of the two answers reached further.
        answer: Answer,
        /// Where it died once that function answered zero, if it still did.
        fault: Option<u64>,
        /// Distinct imports reached with the change.
        reached: usize,
        /// And without it.
        was: usize,
    },
    /// Every candidate was tried and the fault stayed a placeholder dereference.
    ///
    /// A measurement: the code the guest followed came from no call the trace recorded before the
    /// fault.
    NotSourced {
        /// How many were tried.
        tried: usize,
    },
    /// Two runs of this build agreed, so every comparison below it means something.
    Repeats {
        /// Where both faulted, if they did.
        fault: Option<u64>,
        /// How many distinct imports both reached.
        reached: usize,
    },
    /// Two runs of this build did not agree.
    ///
    /// This invalidates the rest of the turn, since every comparison would carry the variation.
    /// Carried with both readings, so a reader can check the claim.
    DoesNotRepeat {
        /// The first run's fault and reach.
        first: (Option<u64>, usize),
        /// The second run's.
        second: (Option<u64>, usize),
    },
    /// A structure a call was handed was read back, and its bytes are on the error stream.
    ///
    /// It carries no verdict, because it is an observation, not a comparison.
    Read {
        /// Where it read.
        address: u64,
        /// Whose structure it is.
        subject: String,
        /// Whether the run still reached the guest at all.
        reached: usize,
    },
    /// Automatic in principle, and not runnable from here.
    ///
    /// Distinct from a refusal: `NameAHash` needs a model and a local runtime a sweep does not
    /// start, and is automatic, not declined (D289).
    Elsewhere(&'static str),
    /// Not mechanical, and the step says why.
    Declined(&'static str),
}

impl Taken {
    /// What this outcome says, in one line.
    ///
    /// On the type, so every caller prints the same line.
    #[must_use]
    pub fn say(&self) -> String {
        match self {
            Self::Swept(finding) => format!("swept every argument: {finding:?}"),
            // Names the diagnostics whose answer was not `Nothing`, with their kind, since a fault
            // that moved and a guest broken earlier mean opposite things.
            Self::Probed(results) => {
                use core::fmt::Write as _;

                let quiet = results
                    .iter()
                    .filter(|(_, change)| matches!(change, crate::axis::Change::Nothing))
                    .count();
                let mut said = format!(
                    "asked {} other diagnostics; {quiet} changed nothing",
                    results.len()
                );
                for (axis, change) in results
                    .iter()
                    .filter(|(_, change)| !matches!(change, crate::axis::Change::Nothing))
                {
                    let _ = write!(said, "\n    {axis:?}: {}", describe(change));
                }
                said
            }
            Self::Watched { reached } => {
                format!("armed a watchpoint; the run reached {reached} imports")
            }
            Self::Confirmed {
                base,
                fault,
                reached,
                was,
            } => format!(
                "*** gave it a region at {base:#x}: reached {reached} against {was}, {}",
                ended(*fault)
            ),
            Self::Sourced {
                function,
                answer,
                fault,
                reached,
                was,
            } => format!(
                "*** {function} answered the code the guest followed; {} reaches {reached} against {was}, {}",
                match answer {
                    Answer::Zero => "zero".to_owned(),
                    Answer::Region { bytes } => format!("a {bytes:#x}-byte region"),
                },
                ended(*fault)
            ),
            Self::NotSourced { tried } => format!(
                "tried {tried} answer(s); the code the guest followed came from none of them"
            ),
            Self::Repeats { fault, reached } => format!(
                "two runs agree: {} imports, {}",
                reached,
                fault.map_or_else(|| "no fault".to_owned(), |f| format!("fault {f:#x}"))
            ),
            // Says what it costs: a reader told only that two runs differed would still read the
            // lines below.
            Self::DoesNotRepeat { first, second } => format!(
                concat!(
                    "*** two runs of this build DISAGREE - {} imports/{} against {} ",
                    "imports/{}; every comparison below measures that as well as its own ",
                    "intervention"
                ),
                first.1,
                first
                    .0
                    .map_or_else(|| "no fault".to_owned(), |f| format!("{f:#x}")),
                second.1,
                second
                    .0
                    .map_or_else(|| "no fault".to_owned(), |f| format!("{f:#x}"))
            ),
            Self::Read {
                address,
                subject,
                reached,
            } => format!(
                "read {subject}'s structure at {address:#x} - the bytes are above, {reached} imports reached"
            ),
            Self::Elsewhere(why) => format!("not from here: {why}"),
            Self::Declined(why) => format!("stopped: {why}"),
        }
    }

    /// Whether this outcome is one the loop produced rather than declined.
    #[must_use]
    pub const fn was_taken(&self) -> bool {
        !matches!(self, Self::Declined(_) | Self::Elsewhere(_))
    }
}

/// What one diagnostic changed, in words a person acts on.
///
/// Says which kind of change it was, because they mean opposite things: a fault that moved is worth
/// a person's time, and one that broke the guest earlier says nothing about the wall (D129).
fn describe(change: &crate::axis::Change) -> String {
    use crate::axis::Change;

    match change {
        Change::Nothing => "changed nothing".to_owned(),
        Change::MovedTo { address } => {
            format!("the fault moved to {address:#x} - worth reading, not yet a diagnosis")
        }
        Change::BrokeEarlier {
            address,
            reached,
            was,
        } => format!(
            "broke it earlier: {address:#x}, reaching {reached} against {was} - says nothing about the original wall"
        ),
        Change::NoLongerFaulted => {
            "it stopped faulting - reach has saturated, so the probe is what separates this from a wrong answer".to_owned()
        }
        Change::NotApplied => {
            "**applied zero times** - this measured nothing it was asked to measure".to_owned()
        }
    }
}

/// How a run ended, for a one-line summary.
fn ended(fault: Option<u64>) -> String {
    fault.map_or_else(
        || "and did not fault".to_owned(),
        |address| format!("faulting at {address:#x}"),
    )
}

/// Takes one step, if it is one this can take.
///
/// # Errors
///
/// If a run could not be made at all. A guest that faults is a result.
pub fn take(trial: &mut impl crate::experiment::Trial, step: &Step) -> Result<Taken, crate::Error> {
    match step {
        Step::SweepArguments { target } => Ok(Taken::Swept(
            crate::experiment::investigate(trial, target)?.0,
        )),
        Step::SweepAxes { fault } => {
            let axes = crate::axis::against_a_wall(Some(*fault))?;
            let baseline = trial.run(None)?;
            let mut out = Vec::with_capacity(axes.len());
            for axis in &axes {
                let outcome = trial.spawn_axes(std::slice::from_ref(axis))?;
                let applied = outcome.planted;
                out.push((
                    axis.clone(),
                    crate::axis::compare(&baseline, &outcome, applied),
                ));
            }
            Ok(Taken::Probed(out))
        }
        Step::WatchStructure { base, words } => {
            let outcome = trial.spawn_axes(&[crate::axis::Axis::Watch {
                base: *base,
                words: *words,
            }])?;
            Ok(Taken::Watched {
                reached: outcome.reached,
            })
        }
        Step::FindPlaceholderSource { candidates } => {
            let baseline = trial.run(None)?;
            for candidate in candidates {
                // Stripped here, because this is the variable that cannot express a qualified name;
                // upstream still needs the library.
                let bare_target = bare(candidate).to_owned();
                let outcome = trial.spawn_axes(&[crate::axis::Axis::Return {
                    target: bare_target.clone(),
                    value: 0,
                }])?;
                // The oracle needs no judgement: the faulting address stops being one of our own
                // codes or it does not. A fault that merely moved is not enough, since forcing any
                // answer changes the program (D299).
                let still_a_placeholder = outcome.fault.is_some_and(is_placeholder);
                if still_a_placeholder {
                    continue;
                }
                // Zero is what a caller may test; a function whose answer the guest dereferences
                // may want memory instead. Both are run and the further one wins (D300). Answering
                // with a region is the existing reservation and forced-answer axes applied
                // together.
                let with_region = trial.spawn_axes(&[
                    crate::axis::Axis::Map {
                        address: TRIAL_REGION_BASE,
                        length: TRIAL_REGION,
                    },
                    crate::axis::Axis::Return {
                        target: bare_target.clone(),
                        value: TRIAL_REGION_BASE,
                    },
                ])?;
                let region_is_better = !with_region.fault.is_some_and(is_placeholder)
                    && with_region.reached > outcome.reached;
                return Ok(Taken::Sourced {
                    function: candidate.clone(),
                    answer: if region_is_better {
                        Answer::Region {
                            bytes: TRIAL_REGION,
                        }
                    } else {
                        Answer::Zero
                    },
                    fault: if region_is_better {
                        with_region.fault
                    } else {
                        outcome.fault
                    },
                    reached: outcome.reached.max(with_region.reached),
                    was: baseline.reached,
                });
            }
            Ok(Taken::NotSourced {
                tried: candidates.len(),
            })
        }
        // Two baselines, nothing applied to either, compared on the two signals the rest of this
        // crate compares: where it died and how far it got.
        Step::CheckRepeats => {
            let first = trial.run(None)?;
            let second = trial.run(None)?;
            if first.fault == second.fault && first.reached == second.reached {
                return Ok(Taken::Repeats {
                    fault: first.fault,
                    reached: first.reached,
                });
            }
            Ok(Taken::DoesNotRepeat {
                first: (first.fault, first.reached),
                second: (second.fault, second.reached),
            })
        }
        Step::ReadStructure {
            address,
            length,
            subject,
        } => read_structure(trial, *address, *length, subject),
        Step::NameAHash { .. } => Ok(Taken::Elsewhere(
            "naming needs a model and a local runtime, which a sweep does not start",
        )),
        Step::Person { why } => Ok(Taken::Declined(why)),
    }
}

/// Runs every step a plan holds, in the order it holds them.
///
/// An out-parameter finding is followed through: the next question has no decision left in it
/// (reserve a region, plant its base, ask whether that was enough), so it is asked rather than
/// printed (D289).
///
/// # Errors
///
/// If a run could not be made at all.
pub fn turn(
    trial: &mut impl crate::experiment::Trial,
    plan: &[Step],
) -> Result<Vec<Taken>, crate::Error> {
    let mut out = Vec::with_capacity(plan.len());
    for step in plan {
        let taken = take(trial, step)?;
        if let (
            Step::SweepArguments { target },
            Taken::Swept(crate::experiment::Finding::OutParameter {
                slot,
                offset,
                answer,
            }),
        ) = (step, &taken)
        {
            let follow = satisfy(trial, target, *slot, *offset, *answer)?;
            out.push(taken);
            out.push(follow);
            continue;
        }
        out.push(taken);
    }
    Ok(out)
}

/// Reads a structure back and reports where it looked.
///
/// One observing run. The bytes are printed on the run's error stream as they are read, since a
/// guest may die before any summary.
///
/// # Errors
///
/// If the run could not be made at all.
fn read_structure(
    trial: &mut impl crate::experiment::Trial,
    address: u64,
    length: u64,
    subject: &str,
) -> Result<Taken, crate::Error> {
    let outcome = trial.spawn_axes(&[crate::axis::Axis::Read { address, length }])?;
    Ok(Taken::Read {
        address,
        subject: subject.to_owned(),
        reached: outcome.reached,
    })
}

/// Gives the guest what the sweep says it was missing, and asks whether that was enough.
///
/// The region is sized from the offset the guest indexes by, doubled and rounded up, because the
/// sweep measures where it faulted rather than how much it wanted.
fn satisfy(
    trial: &mut impl crate::experiment::Trial,
    target: &str,
    slot: u8,
    offset: i64,
    answer: Option<u64>,
) -> Result<Taken, crate::Error> {
    /// Where a satisfying region is put: far from anything the loader places, so a fault inside it
    /// is about this and not an overlap.
    const BASE: u64 = 0x5000_0000;
    /// Page size, which a reservation is rounded up to.
    const PAGE: u64 = 0x1000;

    let was = trial.run(None)?.reached;
    // Doubled, because the sweep measures the one access that died, not the extent the guest
    // intends to use, and rounded up to a page so the region covers its own last byte: `0xfffe0`
    // doubled is half a page short.
    let wanted = offset.unsigned_abs().saturating_mul(2).max(0x1_0000);
    let length = wanted.div_ceil(PAGE).saturating_mul(PAGE);
    let mut axes = vec![
        crate::axis::Axis::Map {
            address: BASE,
            length,
        },
        // Through the argument, not at a fixed address: the sweep identifies a slot, and where it
        // points changes between runs.
        crate::axis::Axis::Write {
            target: target.to_owned(),
            slot,
            value: BASE,
        },
    ];
    if let Some(answer) = answer {
        // Targeted, because an empty target matches every import: the worker accepts any substring
        // of a label, and would force the whole run to answer this value.
        axes.push(crate::axis::Axis::Return {
            target: target.to_owned(),
            value: answer,
        });
    }
    let outcome = trial.spawn_axes(&axes)?;
    Ok(Taken::Confirmed {
        base: BASE,
        fault: outcome.fault,
        reached: outcome.reached,
        was,
    })
}

/// What a turn earned about one function, or nothing when it earned nothing.
///
/// Nothing is the common answer: a sweep that concluded `Unmoved` found no slot, and recording that
/// would turn "we looked" into "we know".
#[must_use]
pub fn promote(
    target: &str,
    finding: &crate::experiment::Finding,
    satisfied: bool,
) -> Option<(Option<String>, orbistoun_hle::knowledge::Record)> {
    let crate::experiment::Finding::OutParameter {
        slot,
        offset,
        answer,
    } = finding
    else {
        return None;
    };

    let (library, function) = match target.rsplit_once("::") {
        Some((library, function)) => (Some(library.to_owned()), function.to_owned()),
        None => (None, target.to_owned()),
    };

    let sign = if *offset < 0 { "-" } else { "+" };
    let mut edges = vec![format!(
        "arg{slot} is an out-parameter: the guest reads a value back from it and indexes {sign}{:#x} from what it finds",
        offset.unsigned_abs()
    )];
    if let Some(answer) = answer {
        // Recorded as an edge case: a reimplementation that writes the out-parameter and answers an
        // error is one the guest ignores.
        edges.push(format!(
            "it must answer {answer:#x} for the guest to read arg{slot} at all - answering anything else sends it down a path that never looks"
        ));
    }
    if satisfied {
        edges.push(
            "given a mapped region at that address the guest proceeded past this call".to_owned(),
        );
    }

    Some((
        library,
        orbistoun_hle::knowledge::Record {
            function,
            edge_cases: edges,
            // Everything the sweep did not establish: what the other arguments select, how large a
            // region the guest intends to use, or what the function is for (D291).
            assumptions: vec![
                format!(
                    "what the arguments other than arg{slot} select is not measured - the sweep varies one slot at a time and reads only where the fault lands"
                ),
                "how much space the guest intends to use is not measured - the offset is where it faulted, not the extent it asked for"
                    .to_owned(),
                "what this function is for is not measured; the name is a label on a hash and carries no observed behaviour"
                    .to_owned(),
            ],
            // The vocabulary defines this value as "the guest proceeded when answered this way, and
            // stopped otherwise", which describes a sweep.
            known_by: Some(orbistoun_hle::knowledge::Oracle::GuestObserved),
            ..orbistoun_hle::knowledge::Record::default()
        },
    ))
}

/// What a function whose answer the guest dereferenced should hand back instead.
///
/// Zero is what a caller may test; a region is what one may use. The guest dereferencing an answer
/// is consistent with either, so the loop tries both and keeps whichever reaches further (D300).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Zero, which a caller is entitled to test for (D125).
    Zero,
    /// A mapped region, which a caller can use.
    Region {
        /// How large. Unmeasured: nothing observed says what the guest wanted.
        bytes: u64,
    },
}

/// How much memory a trial hands to a function that looks like it wants some.
///
/// Unmeasured, and one number rather than a sweep: large enough to tell "wants memory" from "wants
/// a value to test".
const TRIAL_REGION: u64 = 0x10_000;

/// Where a trial region is put.
///
/// Far from anything the loader places, so a fault inside one is about this rather than an overlap
/// with the image, the stubs or the stack.
const TRIAL_REGION_BASE: u64 = 0x6000_0000;

/// Whether an address is one of this project's own placeholder codes.
///
/// Read from the range `orbistoun-core` reserves rather than restated. The codes avoid the high
/// bit, so they are never mistaken for a real firmware value.
fn is_placeholder(address: u64) -> bool {
    /// First code this project answers with.
    const LOW: u64 = 0x7FFF_0000;
    /// One past the last.
    const HIGH: u64 = 0x7FFF_0010;

    (LOW..HIGH).contains(&address)
}

/// How many words one run can watch. A property of x86, not a tunable.
const WATCHPOINTS: usize = 4;

/// Below this, a register holds a count or a flag rather than a pointer.
///
/// Watching `0x20` traps nothing and reports "never touched", a false finding, so a smaller value
/// is refused.
const SMALLEST_POINTER: u64 = 0x1_0000;

/// How much of a structure to read back.
///
/// Eight times the argument dump's window. Fixed, so two runs read the same amount and stay
/// comparable.
const STRUCTURE_BYTES: u64 = 0x100;

/// A guest address an unimplemented call was handed, from the finding's own evidence.
///
/// Only a pointer the run resolved to a region: the dump renders a readable pointer as `->
/// stack+0x7fb458 = ..` and anything else as bare text, so the arrow separates an address from a
/// large integer passed by value. The first such argument, since a call's subject structure is
/// conventionally its first pointer.
fn handed_structure(finding: &Finding) -> Option<u64> {
    finding.evidence.iter().find_map(|line| {
        let (value, rest) = line.split_once(" -> ")?;
        // A region name and an offset, which the dump prints only when it read there.
        if !rest.contains('+') {
            return None;
        }
        let hex = value.rsplit_once("= ").map_or(value, |(_, v)| v).trim();
        let address = u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok()?;
        (address >= SMALLEST_POINTER).then_some(address & !7)
    })
}

/// The structure a faulting instruction was working on, from the fault's own registers.
///
/// `rdi`, by the System V ABI: the first argument, and for a member function the object (D276).
/// Aligned down to a word, because an eight-byte watchpoint needs an eight-byte-aligned address and
/// the hardware refuses rather than rounding.
fn faulting_object(finding: &Finding) -> Option<u64> {
    if finding.gap != Gap::Faulted {
        return None;
    }
    let value = finding
        .evidence
        .iter()
        .flat_map(|line| line.split_whitespace())
        .find_map(|token| token.strip_prefix("rdi="))
        .and_then(|hex| u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok())?;
    (value >= SMALLEST_POINTER).then_some(value & !7)
}

/// The call that led into a wall, as something a diagnostic variable can carry.
///
/// Evidence reads `just before: libkernel::0xabc(0x600000800d38) from 0x400001595d8b`; only the
/// bare symbol is addressable, and the variable this feeds splits its value on `:`.
fn preceding_call(finding: &Finding) -> Option<String> {
    let line = finding
        .evidence
        .iter()
        .find_map(|line| line.strip_prefix(PRECEDED_BY))?;
    let called = line.split(OPEN_PAREN).next().unwrap_or(line).trim();
    (!called.is_empty()).then(|| bare(called).to_owned())
}

/// Every call named in a finding's evidence, bare.
///
/// The trace records the last few calls before a fault. Order is kept: the most recent is the
/// likeliest source and costs the same to try first.
fn unimplemented_calls(finding: &Finding) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in &finding.evidence {
        let Some(call) = line.strip_prefix(PRECEDED_BY) else {
            continue;
        };
        // Qualified: a measurement needs the library to say which knowledge file it belongs in. The
        // bare name `ORBISTOUN_RETURN` needs is stripped where the axis is built.
        let named = call
            .split(OPEN_PAREN)
            .next()
            .unwrap_or(call)
            .trim()
            .to_owned();
        if !bare(&named).is_empty() && !out.contains(&named) {
            out.push(named);
        }
    }
    out
}

/// Where the argument list starts in a recorded call.
const OPEN_PAREN: char = '(';

/// The part of a qualified name that a diagnostic variable can actually carry.
///
/// `libkernel::0xabc` becomes `0xabc`: the value is split on `:`, so the qualified form addresses
/// nothing.
pub(crate) fn bare(subject: &str) -> &str {
    subject.rsplit("::").next().unwrap_or(subject)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbistoun_report::diagnose::Confidence;

    /// Every kind of wall, so a new one cannot be added without being classified here.
    const EVERY_GAP: [Gap; 8] = [
        Gap::Unimplemented,
        Gap::Unnamed,
        Gap::ErrorUsedAsPointer,
        Gap::GuestGaveUp,
        Gap::Faulted,
        Gap::Spinning,
        Gap::AbiViolation,
        Gap::ShortRead,
    ];

    /// A fault, shaped the way the report shapes one: region as subject, call in evidence.
    fn faulted() -> Finding {
        let mut fault = finding(Gap::Faulted, Some("image"));
        fault.evidence = vec![format!("{PRECEDED_BY}libkernel::0xfff(0x1) from 0x2")];
        fault
    }

    /// One finding, as the report would hand it over.
    fn finding(gap: Gap, subject: Option<&str>) -> Finding {
        Finding {
            gap,
            confidence: Confidence::Certain,
            subject: subject.map(ToOwned::to_owned),
            what: String::new(),
            evidence: Vec::new(),
            action: None,
            weight: 1,
        }
    }

    /// Every kind of wall the report can name either acts or declines with a reason.
    #[test]
    fn every_gap_the_report_can_name_produces_a_step() {
        for gap in EVERY_GAP {
            if let Step::Person { why } = step(&finding(gap, Some("libkernel::0xabc"))) {
                assert!(
                    why.len() > 30,
                    "{gap:?} declines with a shrug rather than a reason"
                );
            }
        }
    }

    /// A qualified name is stripped, because the variable cannot carry one.
    #[test]
    fn a_qualified_name_is_stripped_before_it_reaches_a_variable() {
        assert_eq!(
            step(&finding(
                Gap::Unnamed,
                Some("libSceNet::0xd652cde431670c7e")
            )),
            Step::NameAHash {
                hash: "0xd652cde431670c7e".to_owned()
            }
        );
    }

    /// A finding with no subject declines rather than sweeping nothing.
    ///
    /// A sweep with an empty target plants nothing and reports negatives.
    #[test]
    fn a_finding_with_nothing_to_work_on_declines() {
        assert!(
            !step(&finding(Gap::Unnamed, None)).is_automatic(),
            "an unnamed import with no hash produced an automatic step with nothing in it"
        );
    }

    /// A fault is swept through the call that led in, never through its subject, which is a region
    /// with no argument list.
    #[test]
    fn a_fault_is_swept_through_the_call_that_led_in() {
        let mut fault = finding(Gap::Faulted, Some("image"));
        fault.evidence = vec![
            "write to 0xfffe0 is an address in no region this run mapped".to_owned(),
            format!("{PRECEDED_BY}libkernel::0x6abac2f3dc6f8cee(0x600000800d38) from 0x4001595d8b"),
        ];
        assert_eq!(
            step(&fault),
            Step::SweepArguments {
                target: "0x6abac2f3dc6f8cee".to_owned()
            }
        );
    }

    /// A fault with no call recorded before it declines, rather than sweeping its region.
    #[test]
    fn a_fault_with_no_call_before_it_declines() {
        let mut fault = finding(Gap::Faulted, Some("image"));
        fault.evidence = vec!["write to 0xfffe0 is an address in no region".to_owned()];
        assert!(
            !step(&fault).is_automatic(),
            "a fault with no preceding call produced a sweep with nothing to plant in"
        );
    }

    /// Writing the implementation is not automated, by policy.
    #[test]
    fn implementing_a_function_stays_with_a_person() {
        assert!(!step(&finding(Gap::Unimplemented, Some("libkernel::sceFoo"))).is_automatic());
    }

    /// The model appears in exactly one branch, where the hash checks the guess.
    #[test]
    fn only_the_branch_with_an_oracle_behind_it_proposes_anything() {
        let proposing: Vec<_> = EVERY_GAP
            .into_iter()
            .filter(|gap| {
                matches!(
                    step(&finding(*gap, Some("libkernel::0xabc"))),
                    Step::NameAHash { .. }
                )
            })
            .collect();
        assert_eq!(proposing, vec![Gap::Unnamed]);
    }

    /// A hundred calls into one import is one naming step.
    #[test]
    fn repeated_findings_do_not_repeat_the_work() {
        let findings = vec![
            finding(Gap::Unnamed, Some("libkernel::0xabc")),
            finding(Gap::Unnamed, Some("libkernel::0xabc")),
            finding(Gap::Unnamed, Some("libkernel::0xdef")),
        ];
        assert_eq!(
            plan(&findings, None),
            vec![
                Step::CheckRepeats,
                Step::NameAHash {
                    hash: "0xabc".to_owned()
                },
                Step::NameAHash {
                    hash: "0xdef".to_owned()
                },
            ]
        );
    }

    /// The report's ranking is kept, not re-derived.
    ///
    /// It ranks by how many calls each finding concerns, a fact about the run.
    #[test]
    fn the_reports_ranking_survives() {
        let findings = vec![faulted(), finding(Gap::Unnamed, Some("libkernel::0xabc"))];
        let plan = plan(&findings, None);
        // The repeatability check leads, and the report's own order follows it unchanged.
        assert!(matches!(plan[0], Step::CheckRepeats));
        assert!(matches!(plan[1], Step::SweepArguments { .. }));
        assert!(matches!(plan[2], Step::NameAHash { .. }));
    }

    /// The axes are asked of the address, so no finding produces them; they are appended for a
    /// fault.
    #[test]
    fn a_fault_adds_an_axis_sweep_that_no_finding_asked_for() {
        let findings = vec![faulted()];
        assert_eq!(
            plan(&findings, Some(0xfffe0)).last(),
            Some(&Step::SweepAxes { fault: 0xfffe0 })
        );
        // A run that did not fault does not ask about an address it never had.
        assert!(
            !plan(&findings, None)
                .iter()
                .any(|step| matches!(step, Step::SweepAxes { .. }))
        );
    }

    /// A guest whose out-parameter the sweep can find, for driving the runner.
    struct Gated {
        /// How many runs it has been asked for.
        runs: usize,
    }

    impl crate::experiment::Trial for Gated {
        fn run(
            &mut self,
            experiment: Option<&crate::experiment::Experiment>,
        ) -> Result<crate::experiment::Outcome, crate::Error> {
            self.runs += 1;
            let Some(experiment) = experiment else {
                return Ok(outcome(0xfffe0, false, 23));
            };
            // Slot 0, and only once the call is forced to succeed.
            let found = experiment.slot == 0 && experiment.answer == Some(0);
            let fault = if found {
                experiment.value.wrapping_sub(0x20)
            } else {
                0xfffe0
            };
            Ok(outcome(fault, true, 23))
        }

        fn spawn_axes(
            &mut self,
            axes: &[crate::axis::Axis],
        ) -> Result<crate::experiment::Outcome, crate::Error> {
            self.runs += 1;
            // A region behind the base lets it past the wall, which is what `satisfy` asks.
            let satisfied = axes
                .iter()
                .any(|axis| matches!(axis, crate::axis::Axis::Map { .. }));
            Ok(outcome(if satisfied { 0 } else { 0xfffe0 }, true, 24))
        }
    }

    /// One run's result, with the fields this module cares about.
    fn outcome(fault: u64, planted: bool, reached: usize) -> crate::experiment::Outcome {
        crate::experiment::Outcome {
            fault: Some(fault),
            planted,
            refused: false,
            reached,
            touched: true,
        }
    }

    /// A found out-parameter is satisfied, not merely reported: the region, the slot and the forced
    /// answer all come out of the sweep (D289).
    #[test]
    fn a_found_out_parameter_is_followed_through_without_being_asked() {
        let mut guest = Gated { runs: 0 };
        let plan = vec![Step::SweepArguments {
            target: "libkernel::0xabc".to_owned(),
        }];
        let taken = turn(&mut guest, &plan).expect("the runner runs");

        assert!(
            matches!(
                taken.first(),
                Some(Taken::Swept(
                    crate::experiment::Finding::OutParameter { .. }
                ))
            ),
            "the sweep should have found slot 0: {taken:?}"
        );
        let Some(Taken::Confirmed { reached, was, .. }) = taken.get(1) else {
            panic!("a found out-parameter must be followed through: {taken:?}");
        };
        assert!(reached > was, "and the follow-up reports what it bought");
    }

    /// A sweep that found nothing is not followed through.
    ///
    /// Reserving a region after an inconclusive sweep would be an intervention nothing measured.
    #[test]
    fn nothing_is_satisfied_when_the_sweep_concluded_nothing() {
        /// A guest no plant ever moves.
        struct Deaf;
        impl crate::experiment::Trial for Deaf {
            fn run(
                &mut self,
                experiment: Option<&crate::experiment::Experiment>,
            ) -> Result<crate::experiment::Outcome, crate::Error> {
                Ok(outcome(0xfffe0, experiment.is_some(), 23))
            }
            fn spawn_axes(
                &mut self,
                _axes: &[crate::axis::Axis],
            ) -> Result<crate::experiment::Outcome, crate::Error> {
                Ok(outcome(0xfffe0, true, 23))
            }
        }
        let plan = vec![Step::SweepArguments {
            target: "libkernel::0xabc".to_owned(),
        }];
        let taken = turn(&mut Deaf, &plan).expect("the runner runs");
        assert!(
            !taken.iter().any(|t| matches!(t, Taken::Confirmed { .. })),
            "an inconclusive sweep must not be followed through: {taken:?}"
        );
    }

    /// "Nobody can run this here" and "nobody should run this" are different facts.
    ///
    /// The naming loop is automatic but not runnable here, not a policy refusal (D289).
    #[test]
    fn a_step_this_cannot_run_is_not_the_same_as_one_it_declines() {
        let mut guest = Gated { runs: 0 };
        let elsewhere = take(
            &mut guest,
            &Step::NameAHash {
                hash: "0xabc".to_owned(),
            },
        )
        .expect("no run is needed");
        assert!(matches!(elsewhere, Taken::Elsewhere(_)));

        let declined = take(&mut guest, &Step::Person { why: "because" }).expect("no run");
        assert!(matches!(declined, Taken::Declined("because")));
    }

    /// A dereferenced placeholder is traced to whichever call answered with it.
    ///
    /// The oracle is that the fault stops being one of our own codes, not that it moved (D299).
    #[test]
    fn the_answer_the_guest_followed_is_found_by_forcing_each_to_zero() {
        /// A guest that dereferences whatever `sceSecond` answers.
        struct Follows;
        impl crate::experiment::Trial for Follows {
            fn run(
                &mut self,
                _experiment: Option<&crate::experiment::Experiment>,
            ) -> Result<crate::experiment::Outcome, crate::Error> {
                Ok(outcome(0x7fff_0001, false, 12))
            }
            fn spawn_axes(
                &mut self,
                axes: &[crate::axis::Axis],
            ) -> Result<crate::experiment::Outcome, crate::Error> {
                let forced = axes.iter().find_map(|axis| match axis {
                    crate::axis::Axis::Return { target, .. } => Some(target.as_str()),
                    _ => None,
                });
                // Only the real source stops the guest following a placeholder.
                if forced == Some("sceSecond") {
                    Ok(outcome(0x1234, true, 14))
                } else {
                    Ok(outcome(0x7fff_0001, true, 12))
                }
            }
        }

        let step = Step::FindPlaceholderSource {
            candidates: vec!["sceFirst".to_owned(), "sceSecond".to_owned()],
        };
        let taken = take(&mut Follows, &step).expect("the sweep runs");
        let Taken::Sourced {
            function, reached, ..
        } = taken
        else {
            panic!("the source is findable: {taken:?}");
        };
        assert_eq!(function, "sceSecond");
        assert_eq!(reached, 14, "and it reports what the change bought");
    }

    /// Every candidate tried and none of them it: a measurement, not a shrug.
    #[test]
    fn a_placeholder_from_nowhere_recorded_is_reported_as_such() {
        /// A guest nothing on the list satisfies.
        struct Stubborn;
        impl crate::experiment::Trial for Stubborn {
            fn run(
                &mut self,
                _experiment: Option<&crate::experiment::Experiment>,
            ) -> Result<crate::experiment::Outcome, crate::Error> {
                Ok(outcome(0x7fff_0001, false, 12))
            }
            fn spawn_axes(
                &mut self,
                _axes: &[crate::axis::Axis],
            ) -> Result<crate::experiment::Outcome, crate::Error> {
                Ok(outcome(0x7fff_0001, true, 12))
            }
        }

        let step = Step::FindPlaceholderSource {
            candidates: vec!["sceFirst".to_owned(), "sceSecond".to_owned()],
        };
        assert_eq!(
            take(&mut Stubborn, &step).expect("the sweep runs"),
            Taken::NotSourced { tried: 2 },
            "having looked and found nothing is different from having looked nowhere"
        );
    }

    /// A measured contract becomes an entry whose provenance is the measurement.
    #[test]
    fn a_measured_contract_is_promoted_as_guest_observed() {
        let finding = crate::experiment::Finding::OutParameter {
            slot: 0,
            offset: 0xfffe0,
            answer: Some(0),
        };
        let (library, learned) = promote("libkernel::sceKernelReserveVirtualRange", &finding, true)
            .expect("a found out-parameter is promotable");

        assert_eq!(learned.function, "sceKernelReserveVirtualRange");
        assert_eq!(library.as_deref(), Some("libkernel"));
        assert_eq!(
            learned.known_by,
            Some(orbistoun_hle::knowledge::Oracle::GuestObserved)
        );

        let edges = learned.edge_cases.join(" | ");
        assert!(edges.contains("arg0"), "{edges}");
        assert!(edges.contains("0xfffe0"), "{edges}");
        // An edge case rather than a note: an implementation that writes the slot and answers an
        // error is one the guest ignores.
        assert!(edges.contains("must answer 0x0"), "{edges}");
    }

    /// What the sweep did not establish is recorded as assumed, so the entry never reads as though
    /// it measured the other arguments, the region's size or the function's purpose (D291).
    #[test]
    fn everything_the_sweep_did_not_measure_is_recorded_as_an_assumption() {
        let finding = crate::experiment::Finding::OutParameter {
            slot: 0,
            offset: 0xfffe0,
            answer: Some(0),
        };
        let (_, learned) = promote("libkernel::sceFoo", &finding, true).expect("promotable");
        let assumed = learned.assumptions.join(" | ");

        assert!(!learned.assumptions.is_empty());
        assert!(assumed.contains("other than arg0"), "{assumed}");
        assert!(assumed.contains("how much space"), "{assumed}");
        assert!(
            assumed.contains("what this function is for"),
            "the name is a label on a hash and carries no observed behaviour: {assumed}"
        );
    }

    /// A sweep that concluded nothing is promoted to nothing.
    ///
    /// "We looked and found none" is not knowledge.
    #[test]
    fn a_sweep_that_found_nothing_records_nothing() {
        for finding in [
            crate::experiment::Finding::Unmoved {
                tested: vec![0, 1, 2],
                not_addresses: vec![],
            },
            crate::experiment::Finding::NeverPlanted,
            crate::experiment::Finding::Dereferenced { slot: 1 },
        ] {
            assert!(
                promote("libkernel::sceFoo", &finding, false).is_none(),
                "{finding:?} establishes nothing about the function"
            );
        }
    }

    /// A fault with an object in `rdi` earns a watchpoint step, aligned to a word.
    ///
    /// The address comes from the registers, since the address the guest died on is the one place
    /// certain to hold nothing.
    #[test]
    fn a_fault_inside_an_object_asks_who_touched_the_object() {
        let mut fault = faulted();
        // As `Registers::lines()` writes them, four to a line, so a format change fails this.
        fault
            .evidence
            .push("rsi=0x0 rdi=0x4000019e9ca6 rbp=0x600000800ee0 rsp=0x600000800e20".to_owned());
        assert_eq!(
            plan(&[fault], Some(0xfffe0)).last(),
            // Aligned down to a word: the hardware refuses an unaligned eight-byte watchpoint.
            Some(&Step::WatchStructure {
                base: 0x4000_019e_9ca0,
                words: WATCHPOINTS,
            })
        );
    }

    /// A small `rdi` is a count, and watching it would report "never touched" as a finding.
    #[test]
    fn a_register_holding_a_count_is_not_proposed_as_a_structure() {
        let mut fault = faulted();
        fault
            .evidence
            .push("rsi=0x0 rdi=0x20 rbp=0x600000800ee0 rsp=0x600000800e20".to_owned());
        assert!(
            !plan(&[fault], Some(0xfffe0))
                .iter()
                .any(|step| matches!(step, Step::WatchStructure { .. }))
        );
    }

    /// A fault whose registers were never recorded proposes nothing rather than zero.
    #[test]
    fn a_fault_without_registers_asks_nothing_about_a_structure() {
        assert!(
            !plan(&[faulted()], Some(0xfffe0))
                .iter()
                .any(|step| matches!(step, Step::WatchStructure { .. }))
        );
    }

    /// Forty identical refusals are one refusal.
    #[test]
    fn the_same_reason_is_not_repeated_forty_times() {
        let findings: Vec<_> = (0..40)
            .map(|_| finding(Gap::Unimplemented, Some("libkernel::sceFoo")))
            .collect();
        // The repeatability check that leads every plan, and one refusal for forty identical
        // findings.
        assert_eq!(plan(&findings, None).len(), 2);
    }

    /// The diagnostic that said something is the one that gets named.
    #[test]
    fn a_probe_that_changed_something_is_named_rather_than_counted() {
        use crate::axis::{Axis, Change, Region};

        let said = Taken::Probed(vec![
            (
                Axis::Fill {
                    region: Region::Stack,
                    byte: 0xa5,
                },
                Change::Nothing,
            ),
            (
                Axis::Fill {
                    region: Region::Heap,
                    byte: 0xa5,
                },
                Change::MovedTo {
                    address: 0x00af_cc08,
                },
            ),
        ])
        .say();

        assert!(said.contains("1 changed nothing"), "{said}");
        assert!(
            said.contains("0xafcc08"),
            "the address it moved to is the finding: {said}"
        );
        assert!(said.contains("Heap"), "and which axis found it: {said}");
    }

    /// A diagnostic that applied zero times is reported as measuring nothing, distinct from one
    /// that applied and changed nothing.
    #[test]
    fn a_diagnostic_that_never_applied_says_so_rather_than_reading_as_a_negative() {
        use crate::axis::{Axis, Change, Region};

        let said = Taken::Probed(vec![(
            Axis::Fill {
                region: Region::Bss,
                byte: 0,
            },
            Change::NotApplied,
        )])
        .say();

        assert!(
            said.contains("applied zero times"),
            "an unapplied diagnostic must not read as a clean negative: {said}"
        );
    }

    /// A guest that varies on its own is reported, not swept past: here it faults at the same
    /// address both times and reaches one more import the second, so comparing only the fault would
    /// pass (D583).
    #[test]
    fn a_run_that_disagrees_with_itself_is_the_first_thing_said() {
        /// A guest that reaches one more import the second time and dies in the same place.
        struct Drifts {
            runs: usize,
        }
        impl crate::experiment::Trial for Drifts {
            fn run(
                &mut self,
                _experiment: Option<&crate::experiment::Experiment>,
            ) -> Result<crate::experiment::Outcome, crate::Error> {
                self.runs += 1;
                Ok(outcome(0xa0, false, 192 + self.runs % 2))
            }
            fn spawn_axes(
                &mut self,
                _axes: &[crate::axis::Axis],
            ) -> Result<crate::experiment::Outcome, crate::Error> {
                Ok(outcome(0xa0, true, 193))
            }
        }
        let taken = take(&mut Drifts { runs: 0 }, &Step::CheckRepeats).expect("two runs");
        let Taken::DoesNotRepeat { first, second } = taken else {
            panic!("a drifting guest was reported as repeatable: {taken:?}");
        };
        assert_ne!(first.1, second.1, "the two reaches must be what differed");
        assert!(
            taken_says_it_costs_something(&Taken::DoesNotRepeat { first, second }),
            "the line must say what the disagreement costs, not only that it happened"
        );
    }

    /// Whether the sentence names the consequence rather than only the fact.
    fn taken_says_it_costs_something(taken: &Taken) -> bool {
        taken.say().contains("every comparison below")
    }

    /// A guest that agrees with itself says so, so the rest of the turn can be read.
    #[test]
    fn a_run_that_agrees_with_itself_says_what_both_runs_saw() {
        let mut guest = Gated { runs: 0 };
        let taken = take(&mut guest, &Step::CheckRepeats).expect("two runs");
        assert!(
            matches!(taken, Taken::Repeats { .. }),
            "a steady guest was reported as drifting: {taken:?}"
        );
    }

    /// The check leads every plan, including one with no findings at all.
    ///
    /// A plan that ranked it would put findings above the line saying they cannot be read.
    #[test]
    fn every_plan_checks_that_the_run_repeats_first() {
        assert_eq!(
            plan(&[], None).first(),
            Some(&Step::CheckRepeats),
            "a turn with nothing to do still has to establish the run is readable"
        );
    }
}
