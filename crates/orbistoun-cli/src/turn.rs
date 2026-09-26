//! One learning turn: `turn`, its patches, scores and proposals.

use crate::common::{
    build_stamp, calls_by_function, knowledge_path, library_or, measurements, title_id,
};
use anyhow::{Context, Result};

/// `turn` - take every mechanical step one run's findings call for.
///
/// Every decision is made in `orbistoun-turn`; this resolves locations, spawns itself as the guest
/// runner, and prints (D034).
pub(crate) fn cmd_turn(
    path: &std::path::Path,
    record: bool,
    apply: bool,
    verify: Option<&std::path::Path>,
    symbols_db: Option<&std::path::Path>,
) -> Result<()> {
    use orbistoun_turn::{experiment::Finding, trial::GuestTrial, turn};

    // A verifying turn runs where nothing has been learned: an applied measurement removes the wall
    // it was measured at, so re-deriving it from this machine's state would find nothing (D297).
    let scratch = verify
        .is_some()
        .then(tempfile::tempdir)
        .transpose()
        .context("making a directory for a verifying run")?;
    let paths = scratch
        .as_ref()
        .map_or_else(orbistoun_paths::Paths::resolve, |dir| {
            orbistoun_paths::Paths::resolve_with(
                &orbistoun_paths::EnvSnapshot {
                    portable_flag: false,
                    data_dir: Some(dir.path().to_path_buf()),
                },
                None,
                None,
            )
        });
    let traces = paths.traces_dir();
    std::fs::create_dir_all(&traces).with_context(|| format!("creating {}", traces.display()))?;

    // Spawns this binary as the guest runner, as worker mode does, so the runner is the same build.
    let binary = std::env::current_exe().context("finding this executable")?;
    let mut trial = GuestTrial::new(&binary, path, &traces)
        .with_symbols(symbols_db.map(std::path::Path::to_path_buf));
    if let Some(dir) = &scratch {
        // The child reads its own learned file too, so the isolation reaches it.
        trial = trial.with_env(orbistoun_env::DATA_DIR.name, dir.path().to_string_lossy());
    }

    let baseline = trial
        .spawn(&[])
        .map_err(|e| anyhow::anyhow!("the first run could not be made: {e}"))?;
    let trace = orbistoun_report::trace::load_previous(&traces, path)
        .context("the run wrote no trace to read back")?;
    let found = orbistoun_report::diagnose::findings(&trace);

    let plan = turn::plan(&found, baseline.fault);
    println!(
        "{} finding(s), {} step(s), {} of them mechanical",
        found.len(),
        plan.len(),
        plan.iter().filter(|s| s.is_automatic()).count()
    );

    let taken = turn::turn(&mut trial, &plan)
        .map_err(|e| anyhow::anyhow!("a step could not be run: {e}"))?;

    for result in &taken {
        println!("  {}", result.say());
    }

    // Asked after the findings: a question is what a turn does once the run stops producing
    // mechanical steps, and it costs boots (D356).
    attempt_questions(&mut trial, &baseline)?;

    // Proposals are always written: a file nothing applies is inert. Only applying policy needs
    // `--apply` (D355).
    let proposed = write_proposals(path, &plan, &taken)?;
    if proposed > 0 {
        println!(
            "  {proposed} proposal(s) written to {}/ - nothing applied",
            orbistoun_submit::PATCHES_DIR
        );
    }

    if apply {
        apply_patches(&paths, path, &plan, &taken)?;
    }
    if let Some(submitted) = verify {
        verify_against(&paths, path, submitted, &plan, &taken)?;
    }
    if record {
        for (step, result) in plan.iter().zip(taken.iter()) {
            let (turn::Step::SweepArguments { target }, turn::Taken::Swept(finding)) =
                (step, result)
            else {
                continue;
            };
            let satisfied = taken
                .iter()
                .any(|t| matches!(t, turn::Taken::Confirmed { reached, was, .. } if reached > was));
            if let Some((library, learned)) = turn::promote(target, finding, satisfied) {
                print_learn_command(library.as_deref(), &learned);
            } else if !matches!(finding, Finding::OutParameter { .. }) {
                println!("  nothing to record: the sweep established no contract");
            }
        }
    }
    Ok(())
}

/// Writes what a turn measured into the learned policy.
///
/// Underneath what a person wrote: the worker folds the file in with `StubPolicy::absorb`, which
/// keeps every deliberate entry, so deleting the file is a complete undo (D296). A patch that
/// writes guest memory needs a conformance check covering it; a moved wall is not enough, because a
/// wrong write is invisible until something unrelated breaks.
fn apply_patches(
    paths: &orbistoun_paths::Paths,
    title: &std::path::Path,
    plan: &[orbistoun_turn::turn::Step],
    taken: &[orbistoun_turn::turn::Taken],
) -> Result<()> {
    let mut learned = orbistoun_hle::learned::Learned::load(&paths.learned_file())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut written = 0_usize;

    // What the probe said before anything was applied. A measurement that declares it needs a
    // conformance check is graded against this; one that does not is kept on reach alone. With no
    // probe there is no gate, and that is reported.
    let binary = std::env::current_exe().context("finding this executable")?;
    let probe = probe_module();
    let before = if let Some(path) = &probe {
        Some(probe_score(&binary, path, paths.data_root())?)
    } else {
        println!(
            "  no conformance probe ({PROBE_TITLE}) in the title library; grading on the corpus alone"
        );
        None
    };

    // And every guest this machine has: the corpus grades what a change does to guests nobody wrote
    // a check for (D303).
    let titles = corpus_titles();
    let corpus_before = corpus_score(&binary, &titles, paths.data_root());
    println!(
        "  grading against {} guest(s) on this machine",
        titles.len()
    );

    for measurement in measurements(title, plan, taken) {
        // Said, not assumed: announcing the evidence a measurement needs keeps "the wall moved"
        // from reading as "the behaviour is right".
        println!(
            "
  measured {}: needs {:?}",
            measurement.function, measurement.evidence
        );
        for assumption in &measurement.assumes {
            println!("    assumes: {assumption}");
        }
        // Applied to a scratch copy so a refused patch never touches the real file.
        let mut trial = learned.clone();
        trial.record(measurement.clone());
        let scratch = tempfile::tempdir().context("a directory to grade the patch in")?;
        std::fs::write(
            scratch.path().join(orbistoun_paths::LEARNED_FILE),
            trial.to_toml().map_err(|e| anyhow::anyhow!("{e}"))?,
        )
        .context("writing the patch to grade")?;

        // Every guest votes, and one regression refuses the change: nothing here can weigh one
        // guest's correctness against another's (D303).
        let corpus_after = corpus_score(&binary, &titles, scratch.path());
        let on_the_corpus = corpus_before.against(&corpus_after);
        println!("    corpus: {}", on_the_corpus.say());
        if !on_the_corpus.broken.is_empty() {
            println!("    not kept");
            continue;
        }

        // A patch that hands the guest memory cannot be judged on reach alone: a wrong write is
        // invisible until something unrelated breaks.
        let needs_a_spec =
            measurement.evidence == orbistoun_hle::learned::Evidence::ConformanceCheck;
        let kept = match (needs_a_spec, before.as_ref(), probe.as_ref()) {
            (false, ..) => true,
            (true, Some(graded), Some(path)) => {
                let verdict = graded.against(&probe_score(&binary, path, scratch.path())?);
                println!("    probe: {}", verdict.say());
                verdict.is_an_improvement()
            }
            // Falls back to the corpus rather than refusing: without a probe, refusing every
            // memory-handing patch would leave title mining unable to keep anything.
            (true, ..) => {
                println!("    no probe; kept on the corpus alone, which cannot say *correct*");
                !on_the_corpus.fixed.is_empty()
            }
        };
        if !kept {
            println!("    not kept");
            continue;
        }

        learned.record(measurement);
        written += 1;
    }

    if written == 0 {
        println!("  nothing measured that a policy could carry");
        return Ok(());
    }
    let path = paths.learned_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let text = learned.to_toml().map_err(|e| anyhow::anyhow!("{e}"))?;
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!("  wrote {written} measurement(s) to {}", path.display());
    Ok(())
}

/// How every guest on this machine fared, for grading a change against a corpus.
///
/// The oracle when nobody wrote a check: every other title is an independent guest with its own
/// expectations of the same function, so the corpus is the suite (D303).
fn corpus_score(
    binary: &std::path::Path,
    titles: &[std::path::PathBuf],
    data_dir: &std::path::Path,
) -> orbistoun_turn::conformance::Corpus {
    use orbistoun_turn::conformance::{Corpus, Reach};

    let mut corpus = Corpus::default();
    // Where the child writes its traces: the worker resolves its trace directory from
    // `ORBISTOUN_DATA_DIR`, so reading anywhere else finds no traces.
    let traces = orbistoun_turn::trial::traces_in(data_dir);
    std::fs::create_dir_all(&traces).ok();
    for title in titles {
        let mut trial = orbistoun_turn::trial::GuestTrial::new(binary, title, &traces)
            .with_env(orbistoun_env::DATA_DIR.name, data_dir.to_string_lossy());
        let outcome = match orbistoun_turn::experiment::Trial::spawn_axes(&mut trial, &[]) {
            Ok(outcome) => outcome,
            Err(e) => {
                // Said, not skipped: a guest that could not be run says nothing about the change,
                // and a sweep must not grade fewer guests than it claims.
                println!("    {} could not be run: {e}", title.display());
                continue;
            }
        };
        corpus.saw(
            &title_id(title).unwrap_or_else(|| "unknown".to_owned()),
            Reach {
                reached: outcome.reached,
                touched: outcome.touched,
                faulted: outcome.fault.is_some(),
            },
        );
    }
    corpus
}

/// Every guest this machine can run, the probe included.
fn corpus_titles() -> Vec<std::path::PathBuf> {
    let paths = orbistoun_paths::Paths::resolve();
    let library = orbistoun_service::FileConfig::load(&paths.config_file())
        .map(|config| config.library.root.clone())
        .unwrap_or_default();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&library) {
        for entry in entries.flatten() {
            let module = entry.path().join("eboot.bin");
            if module.exists() {
                out.push(module);
            }
        }
    }
    out.sort();
    out
}

/// Where the conformance probe lives, if it has been put there.
///
/// A title like any other, so it needs no special path handling; a machine without it cannot grade
/// what asks to be graded, and says so.
const PROBE_TITLE: &str = "PPSA99980";

/// The probe, if this machine's shared title library has it.
fn probe_module() -> Option<std::path::PathBuf> {
    let path = library_or(None)
        .join(PROBE_TITLE)
        .join(orbistoun_service::TITLE_ENTRY_FILE);
    path.exists().then_some(path)
}

/// Runs the conformance probe and reads what it graded.
///
/// `FURTHER` says only that the guest got past something; the probe grades checks against a
/// specification, by name (D303).
fn probe_score(
    binary: &std::path::Path,
    probe: &std::path::Path,
    data_dir: &std::path::Path,
) -> Result<orbistoun_turn::conformance::Score> {
    let output = std::process::Command::new(binary)
        .arg("run")
        .arg(probe)
        .arg("--calls")
        .arg(PROBE_CALL_BUDGET.to_string())
        .env(orbistoun_env::DATA_DIR.name, data_dir)
        .output()
        .with_context(|| format!("running the probe at {}", probe.display()))?;
    // The probe writes its records to the error stream, which the worker inherits.
    let transcript = String::from_utf8_lossy(&output.stderr);
    Ok(orbistoun_turn::conformance::Score::read(&transcript))
}

/// How far the probe is allowed to run when it is being used as a gate.
///
/// Generous: a budget that stops it early removes checks from the report, and the verdict counts
/// missing checks as a regression.
const PROBE_CALL_BUDGET: u64 = 2_000_000;

/// `turn --verify` - re-derive a submitted file locally and report where it disagrees.
///
/// A measurement is checked by measuring again, so the claim is falsifiable by a command (D297).
/// "Not measured here" is reported and is not a refutation: it usually means the title is absent or
/// the run never reached the call.
fn verify_against(
    paths: &orbistoun_paths::Paths,
    title: &std::path::Path,
    submitted: &std::path::Path,
    plan: &[orbistoun_turn::turn::Step],
    taken: &[orbistoun_turn::turn::Taken],
) -> Result<()> {
    use orbistoun_hle::learned::{Disagreement, Learned};

    let _ = paths;
    let theirs = Learned::load(submitted).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut ours = Learned::default();
    for measurement in measurements(title, plan, taken) {
        ours.record(measurement);
    }

    let disagreements = ours.disagreements(&theirs);
    println!(
        "
  verified {} submitted measurement(s) against {} of our own",
        theirs.measurements.len(),
        ours.measurements.len()
    );
    if disagreements.is_empty() {
        println!("  every one agrees with what this machine measured");
        return Ok(());
    }
    for disagreement in &disagreements {
        match disagreement {
            Disagreement::NotMeasuredHere { function } => println!(
                "  ?  {function}: not measured here - the title may be absent, or the run never reached it"
            ),
            Disagreement::Differs {
                function,
                here,
                there,
            } => println!("  !  {function}: here {here}, submitted {there}"),
        }
    }
    Ok(())
}

/// Prints what a turn earned as the command that would record it.
///
/// Printed, not run: changing a tracked file stays a deliberate act reviewed as a diff (D291).
fn print_learn_command(library: Option<&str>, learned: &orbistoun_hle::knowledge::Record) {
    use std::fmt::Write as _;

    /// Where a library was not carried by the label, since `learn` insists on one.
    const UNATTRIBUTED: &str = "libkernel";

    // A quoted argument a shell hands over whole. Inner double quotes become single ones: these are
    // sentences this code wrote, and copy-paste safety matters more than the character.
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "'"));

    // One line per argument, continued with a shell backslash, not a Rust string continuation,
    // which `cargo fmt` collapses while baking the source indentation into the text.
    let mut out = format!(
        "\n  orbistoun-cli learn {} --library {} --known guest-observed",
        learned.function,
        library.unwrap_or(UNATTRIBUTED)
    );
    for edge in &learned.edge_cases {
        let _ = write!(out, " \\\n    --edge {}", quote(edge));
    }
    for assumption in &learned.assumptions {
        let _ = write!(out, " \\\n    --assumes {}", quote(assumption));
    }
    println!("{out}");
}

/// Attempts the highest-ranked open question that names its own experiment.
///
/// Run reports cover what crashed; the open questions, ranked by how often a guest calls the
/// function, are the other half of what is unknown. One question per turn, because each costs boots
/// (D356). The result is reported, not concluded: deciding a shape is right needs the guest to
/// accept it and get further.
fn attempt_questions(
    trial: &mut orbistoun_turn::trial::GuestTrial,
    baseline: &orbistoun_turn::experiment::Outcome,
) -> Result<()> {
    use orbistoun_turn::experiment::Trial as _;
    use orbistoun_turn::question::{Answers, Question, attemptable};

    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let called = calls_by_function();

    let mut questions = Vec::new();
    for f in knowledge.functions() {
        let asked = f.open_questions_asked();
        if asked.is_empty() || f.answerable_by.is_empty() {
            continue;
        }
        let (calls, _) = called.get(&f.name).copied().unwrap_or((0, 0));
        let open = asked.len();
        for label in &f.answerable_by {
            // A label nothing recognises is an error: a knowledge file naming an experiment this
            // build lacks is a claim nobody can act on.
            let Some(answers) = Answers::named(label) else {
                println!(
                    "  ! {} names an experiment nothing here has: {label}",
                    f.name
                );
                continue;
            };
            questions.push(Question {
                function: f.name.clone(),
                asked: format!("{open} open question(s)"),
                calls,
                answers: Some(answers),
            });
        }
    }

    let ranked = attemptable(&questions);
    let Some(question) = ranked.first() else {
        return Ok(());
    };

    println!();
    println!(
        "  asking the top open question - {} ({} calls in the corpus)",
        question.function, question.calls
    );
    // Every open question on the function, because the experiment belongs to the function; pairing
    // it with one question could pair it with the wrong one.
    println!(
        "    {}; the axes below say what each run asks",
        question.asked
    );

    let Some(answers) = question.answers else {
        return Ok(());
    };
    for axes in answers.axes() {
        let asked = axes
            .first()
            .map_or_else(String::new, orbistoun_turn::axis::Axis::question);
        let outcome = trial
            .spawn_axes(&axes)
            .map_err(|e| anyhow::anyhow!("a question could not be asked: {e}"))?;
        // The same vocabulary the diagnostic axes report in, with the same distinction between a
        // fault that moved and a guest broken earlier.
        let change = orbistoun_turn::axis::compare(baseline, &outcome, outcome.planted);
        println!("    {asked}");
        println!("      {}", describe_change(&change));

        // And what the run was for: which boundary the guest feeds back, computed from the offsets
        // it queried against the map it was shown, both in the trace (D357).
        if matches!(answers, Answers::MapShape) {
            if let Ok(trace) = trial.trace() {
                let map = queried_map(&trace);
                let queried = queried_offsets(&trace, &question.function);
                match orbistoun_turn::question::walked_by(&map, &queried) {
                    orbistoun_turn::question::Reading::WalksBy(walk) => {
                        println!("      *** it walks by {walk:?} - the question is answered");
                        // Written down as a proposal, not only printed (D357).
                        match write_answer(&question.function, walk, &map) {
                            Ok(true) => println!("      recorded as a proposal in patches/"),
                            Ok(false) => {}
                            Err(e) => println!("      could not write the answer: {e:#}"),
                        }
                    }
                    orbistoun_turn::question::Reading::Undecided(why) => {
                        println!("      undecided: {why}");
                    }
                }
            }
        }
    }
    Ok(())
}

/// Writes an answered question into `patches/`, as a change to the entry that asked it.
///
/// A proposal is inert, undone by deleting it, and promoted by a person reading it (D322).
/// `known_by = "measured"`: the guest was put in a situation constructed to separate two readings
/// and its answer was arithmetic. Returns whether anything was written; nothing is when the entry
/// already records the answer.
fn write_answer(
    function: &str,
    walk: orbistoun_turn::question::Walk,
    map: &[(u64, u64, bool)],
) -> Result<bool> {
    let bare = function.rsplit("::").next().unwrap_or(function);
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let Some(library) = knowledge.library_of(bare) else {
        return Ok(false);
    };
    let path = knowledge_path(library);
    let existing =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    let says = match walk {
        orbistoun_turn::question::Walk::End => {
            "the guest walks the map by feeding back each region's END"
        }
        orbistoun_turn::question::Walk::NextStart => {
            "the guest walks the map by feeding back the NEXT REGION'S START"
        }
    };
    // Already settled. Checked on the sentence rather than the function, because an entry can carry
    // several answers.
    if existing.contains(says) {
        return Ok(false);
    }

    let hole = map
        .windows(2)
        .find(|p| p[1].0 > p[0].1)
        .map_or_else(String::new, |p| {
            format!(
                " A hole from {:#x} to {:#x} separated them.",
                p[0].1, p[1].0
            )
        });
    // One sentence, built once: it is both what is written and what is checked for. Named
    // arguments, because `concat!` is a macro call, not a literal, and cannot capture implicitly.
    let says = format!(
        concat!(
            "MEASURED: {says}, established by running the title against a map with a gap in it ",
            "and reading which side of the hole it queried next.{hole} Answers the open ",
            "question about the second field that D218 left, which no contiguous map could ",
            "settle."
        ),
        says = says,
        hole = hole
    );
    let quoted = format!("\"{}\"", says.replace('\\', "\\\\").replace('"', "\\\""));

    let display = path.display().to_string().replace('\\', "/");
    // Joined where the key exists, added where it does not, so the table never gets a duplicate
    // `edge_cases` key.
    let diff = match orbistoun_turn::patch::key_line_of(&existing, bare, "edge_cases") {
        Some(at) => {
            let line = existing.lines().nth(at).unwrap_or_default();
            let Some(rest) = line.strip_prefix("edge_cases = [") else {
                return Ok(false);
            };
            orbistoun_turn::patch::replacing_diff(
                &display,
                &existing,
                at,
                // Trimmed: a multi-line array leaves `rest` empty, and the line would end in a
                // space.
                format!("edge_cases = [{quoted}, {rest}").trim_end(),
            )
        }
        None => orbistoun_turn::patch::inserting_diff(
            &display,
            &existing,
            &format!("name = \"{bare}\""),
            &format!("edge_cases = [{quoted}]\n"),
        ),
    };
    let Some(diff) = diff else {
        return Ok(false);
    };

    let into = std::path::Path::new(orbistoun_submit::PATCHES_DIR);
    std::fs::create_dir_all(into).with_context(|| format!("creating {}", into.display()))?;
    let file = format!("{bare}-answer.patch");
    std::fs::write(into.join(&file), diff)
        .with_context(|| format!("writing the answer for {bare}"))?;

    let mut held = match std::fs::read_to_string(into.join(orbistoun_submit::PROPOSALS_FILE)) {
        Ok(text) => orbistoun_submit::Proposals::parse(&text)
            .map_err(|e| anyhow::anyhow!("reading the proposals already there: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            orbistoun_submit::Proposals::default()
        }
        Err(e) => return Err(e).context("reading the proposals already there"),
    };
    held.proposal.retain(|p| p.file != file);
    held.proposal.push(orbistoun_submit::Proposal {
        file,
        what: format!("record what {bare} does with the map it is shown"),
        proposed_by: build_stamp(),
        known: orbistoun_hle::knowledge::Oracle::Measured,
        evidence: "ran against a map with a gap and read which side of it the guest queried"
            .to_owned(),
        assumes: vec![
            concat!(
                "the reading holds for maps with one hole; nothing establishes what a guest does ",
                "with several"
            )
            .to_owned(),
        ],
    });
    let text = held
        .to_toml()
        .map_err(|e| anyhow::anyhow!("rendering the proposals: {e}"))?;
    std::fs::write(into.join(orbistoun_submit::PROPOSALS_FILE), text)
        .context("writing the proposals")?;
    Ok(true)
}

/// The physical memory map a run recorded presenting.
///
/// Read from the trace, not recomputed from the requested shape: a shape whose regions did not fit
/// falls back, and the guest saw the fallback (D357).
fn queried_map(trace: &serde_json::Value) -> Vec<(u64, u64, bool)> {
    trace
        .pointer("/conditions/memory_map")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let r = r.as_array()?;
                    Some((
                        r.first()?.as_u64()?,
                        r.get(1)?.as_u64()?,
                        r.get(2)?.as_bool()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every first argument the guest passed to one import, in call order.
///
/// In order with duplicates, because a walk is a sequence. From `tail`, the ordered record that
/// carries arguments; `calls` is one summary row per import with no arguments.
fn queried_offsets(trace: &serde_json::Value, function: &str) -> Vec<u64> {
    let bare = function.rsplit("::").next().unwrap_or(function);
    trace
        .get("tail")
        .and_then(serde_json::Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .filter(|c| {
                    c.get("label")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|l| l.ends_with(bare))
                })
                .filter_map(|c| c.get("arg0").and_then(serde_json::Value::as_u64))
                .collect()
        })
        .unwrap_or_default()
}

/// One line for what a question's run did, in the dispatcher's own words.
fn describe_change(change: &orbistoun_turn::axis::Change) -> String {
    use orbistoun_turn::axis::Change;

    match change {
        Change::Nothing => "no different from the baseline".to_owned(),
        Change::MovedTo { address } => format!("the guest died at {address:#x} instead"),
        Change::BrokeEarlier {
            address,
            reached,
            was,
        } => format!("broke it earlier: {address:#x}, reaching {reached} against {was}"),
        Change::NoLongerFaulted => {
            "it stopped faulting - reach has saturated, so the probe is what settles it".to_owned()
        }
        Change::NotApplied => "**applied zero times** - this measured nothing".to_owned(),
    }
}

/// Writes what a turn measured as inert proposals, and says how many.
///
/// A proposal changes no behaviour, is undone by deleting it, and `patches/` is untracked, so it
/// needs no flag (D355). Skipped when the turn measured nothing, since an empty directory would
/// claim a turn ran and found nothing.
fn write_proposals(
    title: &std::path::Path,
    plan: &[orbistoun_turn::turn::Step],
    taken: &[orbistoun_turn::turn::Taken],
) -> Result<usize> {
    let measured = measurements(title, plan, taken);
    if measured.is_empty() {
        return Ok(0);
    }

    let into = std::path::Path::new(orbistoun_submit::PATCHES_DIR);
    std::fs::create_dir_all(into).with_context(|| format!("creating {}", into.display()))?;

    let mut held = match std::fs::read_to_string(into.join(orbistoun_submit::PROPOSALS_FILE)) {
        Ok(text) => orbistoun_submit::Proposals::parse(&text)
            .map_err(|e| anyhow::anyhow!("reading the proposals already there: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            orbistoun_submit::Proposals::default()
        }
        Err(e) => return Err(e).context("reading the proposals already there"),
    };

    let mut written = 0;
    for measurement in &measured {
        // No library, no file to aim a diff at: a measurement recorded without one does not say
        // which knowledge file it belongs in.
        if measurement.library.is_empty() {
            continue;
        }
        let path = knowledge_path(&measurement.library);
        let Ok(existing) = std::fs::read_to_string(&path) else {
            continue;
        };
        // One claim per function: an existing entry means this was already promoted.
        if existing.contains(&format!("name = \"{}\"", measurement.function)) {
            continue;
        }

        let entry = orbistoun_turn::patch::knowledge_entry(measurement);
        let display = path.display().to_string().replace('\\', "/");
        let file = format!("{}.patch", measurement.function);
        std::fs::write(
            into.join(&file),
            orbistoun_turn::patch::appending_diff(&display, &existing, &entry),
        )
        .with_context(|| format!("writing the patch for {}", measurement.function))?;

        // Replaced rather than appended, as in `Learned::record`: two entries for one function
        // cannot be ordered, and the newer turn measured the newer emulator.
        held.proposal.retain(|p| p.file != file);
        held.proposal.push(orbistoun_submit::Proposal {
            file,
            what: format!("promote {} into {display}", measurement.function),
            proposed_by: build_stamp(),
            known: measurement.known,
            evidence: format!("measured against {}", measurement.measured),
            assumes: measurement.assumes.clone(),
        });
        written += 1;
    }

    if written > 0 {
        let text = held
            .to_toml()
            .map_err(|e| anyhow::anyhow!("rendering the proposals: {e}"))?;
        let path = into.join(orbistoun_submit::PROPOSALS_FILE);
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(written)
}
