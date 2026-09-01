//! Turning the loop without a person at the wheel.
//!
//! `docs/THE_LOOP.md` has exactly two steps a person performs: **read the top finding and
//! decide what to do about it**, and **write the code**. The second is out of scope by
//! policy - *"step 18 is still a person writing code, and there is no plan here to have
//! something generate it unverified"*. This module is the first.
//!
//! # It is a dispatcher, and that is the finding
//!
//! The obvious design was a model that understands every tool and picks between them.
//! Measured against what the tools actually cost, that is the wrong shape:
//!
//! | | cost |
//! |---|---|
//! | one boot of the guest against a wall | **~0.13 s** |
//! | every argument of every import, exhaustively | 552 boots, 100 s |
//! | every other diagnostic axis against one fault | 6 boots, **1 s** |
//! | one answer out of a local model, on the GPU | 5-20 s |
//!
//! A chooser is slower than running everything it would choose between. So nothing here
//! chooses: each gap the report can name maps to a fixed step, and where the step is a
//! sweep the sweep is exhaustive. The same measurement retired a model-driven suspect
//! ranker in this crate, for the same reason.
//!
//! # Where the model is, and why only there
//!
//! One branch: [`Step::NameAHash`]. That is not a preference, it is the only place a
//! result was measured. Of the names a model earned against the hash oracle, **not one
//! exists whole in any module string**, and none of its words appears in any
//! vendor-prefixed string - so the string harvester could not have found them, and the
//! two sources are disjoint rather than redundant. Every other branch is a rule, and a
//! rule does not need a model to read it.
//!
//! # A step it cannot take is named, not guessed
//!
//! [`Step::Person`] carries *why*, and the why is a specific statement rather than an
//! absence of one. Principle 3: a dispatcher that quietly did something plausible with a
//! gap it has no rule for would be the same failure as a stub returning success.

use orbistoun_report::diagnose::{Finding, Gap, PRECEDED_BY};

/// One thing the loop can do next, without a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Try to name a bare hash, against the hash itself as the oracle.
    ///
    /// **The one branch with a model in it.** A proposal is checked by computing the
    /// hash, so a wrong answer cannot survive - which is what makes it safe to have
    /// something guess here and nowhere else.
    NameAHash {
        /// The bare hash, as the names database records it.
        hash: String,
    },
    /// Plant a sentinel at every argument of the call that led into a fault.
    ///
    /// Exhaustive rather than ranked: six slots, two sentinels and two conditions is
    /// twenty-four boots, a little over three seconds, and a prior that saves nothing is not
    /// worth having.
    ///
    /// **Two dimensions, because one at a time cannot see a two-condition dependency.** A
    /// guest that checks a call's return before reading its out-parameter is invisible to
    /// either intervention alone, and was read as twenty-three clean negatives (D283, D286).
    SweepArguments {
        /// **A bare symbol or bare hash, never `library::symbol`.**
        ///
        /// The variable this reaches splits its value on `:`, so a qualified name cannot
        /// be expressed in it. Passing one produced two hundred and seventy-six runs
        /// that planted nothing and read as twenty-three clean negatives.
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
    /// **The step that answers a question the argument sweep structurally cannot.** A sweep
    /// varies what a *call* answers; this asks who touched a *word*. At the
    /// `image+0xafc959` wall every call had been eliminated - twenty-three of them, on their
    /// return values and their offset-zero out-parameters - and the fault did not move, so
    /// there was nothing left for a sweep to vary. The first watchpoint run found three
    /// reads of `rdi+0x00` in the fifty bytes before the faulting instruction, and that the
    /// slot every hypothesis had been about was never touched at all (D276).
    ///
    /// Mechanical in both directions: the address comes from the fault's own registers, and
    /// what comes back is an instruction offset in a named region. Neither end reads the
    /// guest's code, which is what keeps it inside principle 1.
    WatchStructure {
        /// Where the structure starts, from the fault's registers.
        base: u64,
        /// How many eight-byte words, never more than the hardware has.
        words: usize,
    },
    /// Find which unimplemented answer the guest dereferenced, by forcing each to zero.
    ///
    /// **The report knows the fault and not the cause.** A placeholder arriving as an address
    /// means the guest asked something, got "not handled", and followed it - and D125 already
    /// says what the fix is. What neither finding names is *which* function answered, because
    /// one names the call that received the code and the other the import the fault happened
    /// inside (D299).
    ///
    /// So the candidates are swept. The oracle needs no judgement: the faulting address stops
    /// being one of our own codes, or it does not.
    FindPlaceholderSource {
        /// Every import the run called that nothing implements, in the order it called them.
        candidates: Vec<String>,
    },
    /// Nothing here is mechanical.
    Person {
        /// Why not - a statement, never a shrug.
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
/// Total over [`Gap`] by construction - a new kind of wall is a compile error here rather
/// than a finding that silently produces no work.
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
        // **Not the subject.** A fault's subject is the region the guest died in, and
        // sweeping a region plants nothing at all - measured, across every argument, and
        // it would have read as a clean negative. The call that led in is in the evidence.
        Gap::Faulted => preceding_call(finding).map_or(
            Step::Person {
                why: concat!(
                    "the fault records no call leading into it, so there is nothing with ",
                    "arguments to plant in"
                ),
            },
            |target| Step::SweepArguments { target },
        ),
        // Deliberately not automated. The loop is allowed to find the wall; writing what
        // goes behind it is a person's job, and generating it unverified is not planned.
        Gap::Unimplemented => Step::Person {
            why: concat!(
                "implementing a function is a person writing code - the loop finds the ",
                "wall, it does not build what goes behind it"
            ),
        },
        // **Looking is a sweep.** The finding's own action says "find what answered with that
        // code just before", which is an instruction to go looking - and every candidate is
        // already in the trace, at a tenth of a second each (D299).
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
        // **Swept, and it used to be declined.** The decline said "what it is waiting for is
        // not varied by any diagnostic", and `ORBISTOUN_RETURN` varies exactly that - the
        // finding's own evidence says so: *a guest that keeps asking the same question has not
        // accepted the answer*. The report and the dispatcher disagreed and the report was
        // right.
        //
        // It could not be swept until the sweep had an oracle for it. A spinning guest never
        // faults, so the fault-position comparison answered `Unmoved` whatever happened; reach
        // is what moves when a guest escapes a loop, and `Finding::Escaped` reads it (D351).
        Gap::Spinning => finding.subject.as_deref().map_or(
            Step::Person {
                why: "the finding names no import, so there is nothing to sweep",
            },
            |subject| Step::SweepArguments {
                target: bare(subject).to_owned(),
            },
        ),
    }
}

/// Everything a run calls for, in the order the report ranked it, without repeats.
///
/// The report ranks by how much each finding concerns, and that order is kept - this adds
/// nothing to it. What it does add is **deduplication**: a hundred calls into one unnamed
/// import is one naming step, not a hundred, and one fault is one axis sweep.
///
/// A [`Step::SweepAxes`] is appended for the fault, if there was one, because the axes are
/// asked of the address rather than of any call - so no finding produces it.
#[must_use]
pub fn plan(findings: &[Finding], fault: Option<u64>) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
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
        // Asked of the *structure* rather than of the address that faulted, so no finding
        // produces it either. Appended after the axes because it is the more expensive
        // question and only worth asking once the cheap ones have not moved anything.
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
/// **Never a bare success.** Every variant carries what was measured, because a runner that
/// reported only "done" would make a step that ran and found nothing indistinguishable from
/// one that ran and found the answer - and the second is the entire output of the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// The argument sweep ran, and this is what it concluded.
    Swept(crate::experiment::Finding),
    /// Every other diagnostic was asked of the faulting address.
    Probed(Vec<(crate::axis::Axis, crate::axis::Change)>),
    /// A watchpoint run was made. What it saw is on the run's error stream, by design:
    /// the guest may die before any summary is reached, so sites are reported as they
    /// happen (D276).
    Watched {
        /// Whether the run still reached the guest at all.
        reached: usize,
    },
    /// A measured out-parameter contract was **satisfied**, and the guest asked about it.
    ///
    /// The one step that does not merely observe the shape of a gap. It reserves a region,
    /// plants its base where the sweep said the guest reads one, and reports whether that
    /// was enough to get further (D284, D289).
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
    /// **Auto-keepable, and the only outcome here that is.** It changes what a function
    /// answers and writes no memory, so `FURTHER` is sufficient evidence by the rule in D296 -
    /// a wrong answer that buys progress shows up as a wall that moved.
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
    /// **A measurement, not a shrug.** It says the code the guest followed did not come from
    /// any call the trace recorded before the fault - which is worth knowing, and different
    /// from having looked nowhere.
    NotSourced {
        /// How many were tried.
        tried: usize,
    },
    /// Automatic in principle, and not runnable from here.
    ///
    /// **Distinct from a refusal.** `NameAHash` needs a model and a local runtime that a
    /// sweep has no business starting; reporting that as a policy decision would describe
    /// the naming loop as something nobody is willing to automate, which is the opposite of
    /// true (D289).
    Elsewhere(&'static str),
    /// Not mechanical, and the step says why.
    Declined(&'static str),
}

impl Taken {
    /// What this outcome says, in one line.
    ///
    /// **On the type, because two callers had a copy each.** A shim printed one match over
    /// every variant and the integration test printed another, which is the same drift the
    /// dispatcher itself was rescued from - a second copy of the thing that ships proves
    /// nothing about the thing that ships (D289).
    #[must_use]
    pub fn say(&self) -> String {
        match self {
            Self::Swept(finding) => format!("swept every argument: {finding:?}"),
            // **Names the ones that said something.** This counted the silent ones and
            // stopped - "asked 6 other diagnostics; 5 changed nothing" leaves the sixth, the
            // only one carrying a signal, unnamed and unreported. The whole reason to run a
            // diagnostic is the answer that is not `Nothing`, and the summary was reporting
            // the negative space (D331).
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
/// **Says which kind of change it was, because they mean opposite things.** A fault that
/// moved is worth a person's time; one that broke the guest *earlier* says nothing about the
/// wall being asked about, and reading the second as the first is the mistake D129 records
/// about the progress verdict.
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
                // Stripped here, because this is the variable that cannot express a
                // qualified name - not upstream, where the library is still needed (D355).
                let bare_target = bare(candidate).to_owned();
                let outcome = trial.spawn_axes(&[crate::axis::Axis::Return {
                    target: bare_target.clone(),
                    value: 0,
                }])?;
                // **The oracle, and it needs no judgement.** Either the faulting address stops
                // being one of our own codes or it does not. A fault that merely *moved* is
                // not enough: forcing any answer changes the program, and a moved wall is not
                // a diagnosis (D224, D299).
                let still_a_placeholder = outcome.fault.is_some_and(is_placeholder);
                if still_a_placeholder {
                    continue;
                }
                // **Zero worked; that does not make it right.** It is what a caller may
                // *test*, and a function whose answer the guest dereferences may want memory
                // instead. Both are now sayable, so both are run and the further one wins -
                // rather than keeping whichever the rule happened to reach for (D300).
                // **Two existing axes, not a new one.** Reserving a region and forcing an
                // answer already exist; "answer with a region" is those two applied together,
                // which is exactly the pairing a one-at-a-time sweep could not express (D283).
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
        Step::NameAHash { .. } => Ok(Taken::Elsewhere(
            "naming needs a model and a local runtime, which a sweep does not start",
        )),
        Step::Person { why } => Ok(Taken::Declined(why)),
    }
}

/// Runs every step a plan holds, in the order it holds them.
///
/// **Follows an out-parameter finding through.** When the sweep concludes that a slot is one,
/// the next question has no decision left in it - reserve a region, plant its base, ask the
/// guest whether that was enough - so it is asked rather than printed as a suggestion (D289).
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

/// Gives the guest what the sweep says it was missing, and asks whether that was enough.
///
/// The region is sized from the offset the guest indexes by, rounded up and doubled, because
/// the sweep measures where it *faulted* rather than how much it wanted - and a reservation
/// exactly as large as the one access it made would answer a narrower question than the one
/// worth asking, which is the reasoning `axis::around` already uses.
fn satisfy(
    trial: &mut impl crate::experiment::Trial,
    target: &str,
    slot: u8,
    offset: i64,
    answer: Option<u64>,
) -> Result<Taken, crate::Error> {
    /// Where a satisfying region is put. Far from anything the loader places, so a fault
    /// inside it is unmistakably about this and not about an overlap.
    const BASE: u64 = 0x5000_0000;
    /// Page size, which a reservation is rounded up to.
    const PAGE: u64 = 0x1000;

    let was = trial.run(None)?.reached;
    // Sized from where the guest faulted rather than from what it asked for, so doubled: the
    // sweep measures the one access that died, not the extent the guest intends to use, and
    // a region exactly as large as that access answers a narrower question than the one worth
    // asking - the same reasoning `axis::around` gives for sizing outward.
    //
    // **Rounded up to a page**, because it is not otherwise: `0xfffe0` doubled is `0x1fffc0`,
    // which is half a page short of covering its own last byte. The first run of this reserved
    // exactly that and the guest faulted *inside the region it had just been given*, at
    // `base + 0xfffe0` - a result that reads as "the base was not the problem" and is really
    // "the reservation stopped forty bytes early" (D289).
    let wanted = offset.unsigned_abs().saturating_mul(2).max(0x1_0000);
    let length = wanted.div_ceil(PAGE).saturating_mul(PAGE);
    let mut axes = vec![
        crate::axis::Axis::Map {
            address: BASE,
            length,
        },
        // **Through the argument, not at a fixed address.** The sweep identifies a *slot*;
        // where that slot points is the guest's business and changes between runs. `Poke`
        // takes an address and would have planted nothing (or somewhere arbitrary).
        crate::axis::Axis::Write {
            target: target.to_owned(),
            slot,
            value: BASE,
        },
    ];
    if let Some(answer) = answer {
        // Targeted, because an empty target matches every import: the worker accepts a target
        // that is any substring of a label, and every label contains the empty string. That
        // would force the whole run to answer this value.
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
/// **Nothing is the common answer and it is not a failure.** A sweep that concluded
/// `Unmoved` measured every slot and found none of them; recording that as knowledge would
/// turn "we looked" into "we know", which is the distinction the whole `known_by` vocabulary
/// exists to hold.
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
        // **Recorded as an edge, not as a note.** A reimplementation that writes the
        // out-parameter and answers an error is one the guest ignores entirely, and that is
        // precisely "behaviour a reimplementation would otherwise get wrong".
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
            // **Everything the sweep did not establish.** A sweep measures which slot is
            // read and what is added to it; it measures nothing about what the other
            // arguments select, how large a region the guest intends to use, or what the
            // function is for. Left out, an entry would read as though it had (D291).
            assumptions: vec![
                format!(
                    "what the arguments other than arg{slot} select is not measured - the sweep varies one slot at a time and reads only where the fault lands"
                ),
                "how much space the guest intends to use is not measured - the offset is where it faulted, not the extent it asked for"
                    .to_owned(),
                "what this function is for is not measured; the name is a label on a hash and carries no observed behaviour"
                    .to_owned(),
            ],
            // The vocabulary's own definition of this value is "the guest proceeded when
            // answered this way, and stopped otherwise", which is a description of a sweep.
            known_by: Some(orbistoun_hle::knowledge::Oracle::GuestObserved),
            ..orbistoun_hle::knowledge::Record::default()
        },
    ))
}

/// What a function whose answer the guest dereferenced should hand back instead.
///
/// **Two hypotheses, both run.** Zero is what a caller may test; a region is what one may use.
/// The guest dereferencing an answer is consistent with either, so the loop tries both and
/// keeps whichever reaches further rather than following the rule it happened to have (D300).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Zero, which a caller is entitled to test for (D125).
    Zero,
    /// A mapped region, which a caller can use.
    Region {
        /// How large. Unmeasured - nothing observed says what the guest wanted.
        bytes: u64,
    },
}

/// How much memory a trial hands to a function that looks like it wants some.
///
/// Unmeasured, and one number rather than a sweep: what the guest *uses* is measurable with a
/// snapshot and that is not built, so this is a size big enough to tell "wants memory" from
/// "wants a value to test" and no more (D300).
const TRIAL_REGION: u64 = 0x10_000;

/// Where a trial region is put.
///
/// Far from anything the loader places, so a fault inside one is unmistakably about this
/// rather than about an overlap with the image, the stubs or the stack.
const TRIAL_REGION_BASE: u64 = 0x6000_0000;

/// Whether an address is one of this project's own placeholder codes.
///
/// **Read from the range `orbistoun-core` reserves, not re-stated here.** The codes deliberately
/// avoid the high bit so they can never be mistaken for a real firmware value (principle 3),
/// and a second copy of the boundary is a second thing to get wrong.
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
/// Watching `0x20` traps nothing and reports "never touched" - which is a *finding*, and a
/// false one. A threshold that refuses is better than a run that answers confidently.
const SMALLEST_POINTER: u64 = 0x1_0000;

/// The structure a faulting instruction was working on, from the fault's own registers.
///
/// **`rdi`, by the platform ABI.** System V puts the first argument there, and for a member
/// function that is the object - so at a fault inside one, `rdi` is the structure. Every
/// wall this has been pointed at so far has had one there, and the reads of `rdi+0x00`
/// immediately before `image+0xafc959` are what the first watchpoint run found (D276).
///
/// Aligned down to a word, because a watchpoint of eight bytes needs an eight-byte-aligned
/// address and the hardware refuses rather than rounding - see `orbistoun_worker::watchpoint`.
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
/// Evidence reads `just before: libkernel::0xabc(0x600000800d38) from 0x400001595d8b`, of
/// which only the bare symbol is addressable - the arguments and the return address are
/// there for a person, and the variable this feeds splits its value on `:` anyway.
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
/// The trace records the last few calls before a fault, and the one that produced the
/// placeholder is among them. Order is kept: the guest dereferenced something it was handed
/// recently, so the most recent is the likeliest and costs the same to try first.
fn unimplemented_calls(finding: &Finding) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in &finding.evidence {
        let Some(call) = line.strip_prefix(PRECEDED_BY) else {
            continue;
        };
        // **Qualified, and it used to be stripped here.** The bare name is what the diagnostic
        // needs - `ORBISTOUN_RETURN` splits its value on `:`, so a `library::symbol` cannot be
        // expressed in it - and stripping at this point threw the library away for everything
        // downstream too. A measurement with no library cannot say which knowledge file it
        // belongs in, so a turn that found a real contract wrote no proposal at all (D328,
        // D355). Stripped where the axis is built instead, which is the only place that needs
        // it stripped.
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
/// `libkernel::0xabc` becomes `0xabc`. Not cosmetic: the value is split on `:`, so the
/// qualified form silently addresses nothing.
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

    /// **Every kind of wall the report can name declines with a reason or acts.**
    ///
    /// The property this module exists to hold. A gap with no rule is not allowed to
    /// quietly produce nothing - it produces a [`Step::Person`] saying why, which is a
    /// different thing and can be read.
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

    /// **A qualified name is stripped, because the variable cannot carry one.**
    ///
    /// The expensive one. Passing `library::symbol` to a value that splits on `:` produced
    /// two hundred and seventy-six runs that planted nothing, and every one of them read
    /// as a clean negative.
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
    /// A sweep with an empty target runs, plants nothing, and reports negatives - the
    /// exact failure above, arrived at from the other direction.
    #[test]
    fn a_finding_with_nothing_to_work_on_declines() {
        assert!(
            !step(&finding(Gap::Unnamed, None)).is_automatic(),
            "an unnamed import with no hash produced an automatic step with nothing in it"
        );
    }

    /// **A fault is swept through the call that led in, never through its subject.**
    ///
    /// The one that had to be run against a real title to be found. A fault's subject is
    /// the *region* - `image` - so taking it as the call swept an argument list that does
    /// not exist: nothing was planted, across every slot, and only the distinction between
    /// "changed nothing" and "did nothing" stopped that reading as a clean negative.
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

    /// **Writing the implementation is not automated, and that is deliberate.**
    ///
    /// Pinned as a test rather than left as a comment, because it is a policy somebody
    /// could reasonably think was an oversight and 'fix'.
    #[test]
    fn implementing_a_function_stays_with_a_person() {
        assert!(!step(&finding(Gap::Unimplemented, Some("libkernel::sceFoo"))).is_automatic());
    }

    /// **The model appears in exactly one branch.**
    ///
    /// Guessing is safe where the hash checks the guess and nowhere else, so this is a
    /// structural claim worth failing on rather than a stylistic one.
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
    /// It ranks by how many calls each finding concerns, which is a fact about the run.
    /// Reordering here would be this module claiming to know better on no evidence.
    #[test]
    fn the_reports_ranking_survives() {
        let findings = vec![faulted(), finding(Gap::Unnamed, Some("libkernel::0xabc"))];
        let plan = plan(&findings, None);
        assert!(matches!(plan[0], Step::SweepArguments { .. }));
        assert!(matches!(plan[1], Step::NameAHash { .. }));
    }

    /// **The axes are asked of the address, so no finding produces them.**
    ///
    /// A fault finding names the call that led in; the axes ask about the address it died
    /// on. Different questions, and the second has no finding of its own - which is why it
    /// is appended rather than mapped, and why this test exists to notice if that ever
    /// silently stops happening.
    #[test]
    fn a_fault_adds_an_axis_sweep_that_no_finding_asked_for() {
        let findings = vec![faulted()];
        assert_eq!(
            plan(&findings, Some(0xfffe0)).last(),
            Some(&Step::SweepAxes { fault: 0xfffe0 })
        );
        // And a run that did not fault does not ask about an address it never had.
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
            // Slot 0, and only once the call is forced to succeed - the real wall's shape.
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

    /// A found out-parameter is **satisfied**, not merely reported.
    ///
    /// The step that changes what the loop is: everything before it measures the shape of a
    /// gap, and this one gives the guest what the measurement said was missing and asks
    /// whether that was enough. Nothing in it is chosen - the region, the slot and the forced
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
    /// **The negative half.** Reserving a region and planting a base after an inconclusive
    /// sweep would be an intervention nothing measured, and its result would read exactly
    /// like one that had been earned.
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
    /// Collapsing them would report the naming loop as a policy refusal, which is the
    /// opposite of true - it is the one branch this project deliberately gave a model
    /// (D289).
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
    /// **The oracle is that the fault stops being one of our own codes**, not that it moved.
    /// Forcing any answer changes the program, so a moved wall on its own is not a diagnosis
    /// (D224, D299).
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
        // The condition is an edge rather than a note: an implementation that writes the
        // slot and answers an error is one the guest ignores entirely.
        assert!(edges.contains("must answer 0x0"), "{edges}");
    }

    /// **What the sweep did not establish is recorded as not established.**
    ///
    /// The half that makes the entry admissible. A sweep varies one slot and reads where the
    /// fault lands; it measures nothing about what the other arguments select, how much space
    /// the guest wants, or what the function is *for*. An entry silent on those reads as
    /// though it had measured them, which is the convergence problem arriving through the
    /// loop instead of through a person (D291).
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
    /// "We looked and found none" is not knowledge, and recording it as such would turn a
    /// completed search into an established fact.
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
    /// The address is taken from the registers rather than from the faulting address: the
    /// question is who touched the *structure*, and the address the guest died on is the one
    /// place certain to hold nothing.
    #[test]
    fn a_fault_inside_an_object_asks_who_touched_the_object() {
        let mut fault = faulted();
        // As `Registers::lines()` writes them, four to a line, so this fails if that format
        // ever changes underneath it - which is the whole reason to parse a real one.
        fault
            .evidence
            .push("rsi=0x0 rdi=0x4000019e9ca6 rbp=0x600000800ee0 rsp=0x600000800e20".to_owned());
        assert_eq!(
            plan(&[fault], Some(0xfffe0)).last(),
            // Aligned **down** to a word: the hardware refuses an unaligned eight-byte
            // watchpoint outright rather than rounding it, so proposing one would produce a
            // step that cannot run.
            Some(&Step::WatchStructure {
                base: 0x4000_019e_9ca0,
                words: WATCHPOINTS,
            })
        );
    }

    /// A small `rdi` is a count, and watching it would answer confidently and wrongly.
    ///
    /// **The negative half, and the one that matters.** A watchpoint armed on `0x20` traps
    /// nothing and reports `never touched` - which does not read as a failure, it reads as a
    /// finding: *nobody uses this field*. Refusing to propose it is the only thing standing
    /// between that and a wrong conclusion (principle 3).
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
        assert_eq!(plan(&findings, None).len(), 1);
    }

    /// **The diagnostic that said something is the one that gets named.**
    ///
    /// The summary counted the silent ones - "asked 6 other diagnostics; 5 changed nothing" -
    /// and stopped, leaving the sixth unnamed. The whole reason to run a diagnostic is the
    /// answer that is not `Nothing`, so a report of the negative space is a report of the
    /// wrong thing (D331).
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

    /// **A diagnostic that applied zero times is reported as measuring nothing.**
    ///
    /// The failure D229 and D230 both record: a diagnostic that never reached the thing under
    /// test and one that reached it and changed nothing produce identical output, and only the
    /// second is a measurement.
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
}
