//! Ingest conformance records as observations on the functions they exercised.
//!
//! A run emits `try` naming the check's library and symbol, and `res` carrying the verdict,
//! a value and a note; joined on the check id, that is a hardware observation about a named
//! function. The value is not necessarily a return value: its meaning lives in the check,
//! so `sceKernelWrite` answering an error code and `sceKernelGetProcessTime` answering the
//! time share one field. An observation is therefore quoted rather than interpreted, and a
//! value that differs between runs is recorded as not constant. [`observation_of`] leaves
//! `known_by` unchanged.

use std::collections::{BTreeMap, BTreeSet};

use orbistoun_hle::knowledge::{KnowledgeFile, Record};

/// What one run's check reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// `pass`, `fail`, `partial` or `skip`.
    pub(crate) verdict: String,
    /// The value the check reported, exactly as written. May be empty.
    pub(crate) value: String,
    /// What the check said about it. May be empty.
    pub(crate) note: String,
    /// The capture files reporting this outcome, so the line can be found again.
    pub(crate) sources: Vec<String>,
}

/// What one check said about one named function, across every run that ran it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Observation {
    /// The check id, which is where the condition is written down.
    pub(crate) check: String,
    /// The function the check exercised.
    pub(crate) symbol: String,
    /// One outcome where the runs agreed; several where they did not.
    pub(crate) outcomes: Vec<Outcome>,
}

/// Reads the observations out of one capture, one outcome each.
///
/// Pseudo-symbols are skipped: a census or sweep check names its subject `(census)` or
/// `(symbol probe)`, and its whole-surface count is not a fact about one function.
pub(crate) fn observations_in(capture: &str, source: &str) -> Vec<Observation> {
    let mut subjects: BTreeMap<&str, &str> = BTreeMap::new();
    let mut out = Vec::new();
    for line in capture.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        match fields.as_slice() {
            ["OBS", "try", check, _library, symbol, ..] => {
                subjects.insert(check, symbol);
            }
            ["OBS", "res", check, verdict, value, note, ..] => {
                let Some(symbol) = subjects.get(check) else {
                    continue;
                };
                if symbol.starts_with('(') || (value.is_empty() && note.is_empty()) {
                    continue;
                }
                out.push(Observation {
                    check: (*check).to_owned(),
                    symbol: (*symbol).to_owned(),
                    outcomes: vec![Outcome {
                        verdict: (*verdict).to_owned(),
                        value: (*value).to_owned(),
                        note: (*note).to_owned(),
                        sources: vec![source.to_owned()],
                    }],
                });
            }
            _ => {}
        }
    }
    out
}

/// Gathers every run's report of the same check against the same function.
///
/// Where the runs agreed, one finding is recorded naming every run. Where they did not, the
/// disagreement is the finding: the value is not a platform constant.
#[must_use]
pub(crate) fn fold(observations: Vec<Observation>) -> Vec<Observation> {
    let mut out: Vec<Observation> = Vec::new();
    for observation in observations {
        let Some(existing) = out
            .iter_mut()
            .find(|kept| kept.check == observation.check && kept.symbol == observation.symbol)
        else {
            out.push(observation);
            continue;
        };
        for outcome in observation.outcomes {
            let same = existing.outcomes.iter_mut().find(|kept| {
                kept.verdict == outcome.verdict
                    && kept.value == outcome.value
                    && kept.note == outcome.note
            });
            match same {
                Some(kept) => {
                    for source in outcome.sources {
                        if !kept.sources.contains(&source) {
                            kept.sources.push(source);
                        }
                    }
                }
                None => existing.outcomes.push(outcome),
            }
        }
    }
    out
}

/// The capture files an outcome was seen in, as a citable phrase.
fn seen_in(sources: &[String]) -> String {
    sources
        .iter()
        .map(|s| format!("`data/hardware/{s}`"))
        .collect::<Vec<_>>()
        .join(" and ")
}

/// One run's report, as a phrase.
fn reported(outcome: &Outcome) -> String {
    let value = if outcome.value.is_empty() {
        String::new()
    } else {
        format!(", value {}", outcome.value)
    };
    let note = if outcome.note.is_empty() {
        String::new()
    } else {
        format!(" - {}", outcome.note)
    };
    format!("reported {}{value}{note}", outcome.verdict)
}

/// The sentence an observation becomes.
///
/// Quoted, never interpreted: the value reads as "the check reported this", not "the
/// function returns this".
pub(crate) fn edge_case(observation: &Observation) -> String {
    let head = format!("Measured on hardware: obSCEne `{}` ", observation.check);
    match observation.outcomes.as_slice() {
        [only] => format!(
            concat!(
                "{}{}. The check names the condition; see {} in the sibling ",
                "conformance-probe repository."
            ),
            head,
            reported(only),
            seen_in(&only.sources)
        ),
        many => {
            let parts: Vec<String> = many
                .iter()
                .map(|o| format!("{} in {}", reported(o), seen_in(&o.sources)))
                .collect();
            // The disagreement comes first, before any value a reader might copy.
            format!(
                concat!(
                    "{}did not report the same thing twice, so the value is not a constant of ",
                    "the platform: {}. The check names the condition; both are in the sibling ",
                    "conformance-probe repository."
                ),
                head,
                parts.join("; ")
            )
        }
    }
}

/// Turns an observation into a record that adds one edge case and changes nothing else.
///
/// `known_by` is left as it was (D180): it says how the whole recorded behaviour was
/// established, and one observation about one edge does not establish a function's
/// contract. Promoting it would inflate the tier counts; the observation is itemised
/// instead.
pub(crate) fn observation_of(observation: &Observation) -> Record {
    Record {
        function: observation.symbol.clone(),
        edge_cases: vec![edge_case(observation)],
        ..Record::default()
    }
}

/// Joins the parts of a measurement id, using a character no field can contain.
///
/// A unit separator, because check ids contain slashes and conditions contain hyphens.
const JOIN: char = '\u{1f}';

/// Reads the `measure` records out of one capture.
///
/// These records carry their own condition, so unlike a `res` value they can be asserted.
/// They share the [`Observation`] shape, so [`fold`] decides agreement for both.
pub(crate) fn measurements_in(capture: &str, source: &str) -> Vec<Observation> {
    let mut out = Vec::new();
    for line in capture.lines() {
        // Read through `orbistoun-probe`, which owns the record format, rather than a second
        // copy of the protocol here.
        let Ok(orbistoun_probe::Line::Record(orbistoun_probe::Record::Measure {
            section,
            subject,
            field,
            value,
            unit,
        })) = orbistoun_probe::parse_line(line)
        else {
            continue;
        };
        // The export census is not an observation of a condition: its subject is a hash and
        // its value an address, so it goes to the name search instead.
        if section == orbistoun_probe::KEXPORT_SECTION {
            continue;
        }
        out.push(Observation {
            // The condition rides in the check field so `fold` groups by the full triple, and
            // two conditions of one check stay separate measurements.
            check: format!("{section}{JOIN}{field}{JOIN}{unit}"),
            symbol: subject,
            outcomes: vec![Outcome {
                verdict: String::new(),
                value,
                note: String::new(),
                sources: vec![source.to_owned()],
            }],
        });
    }
    out
}

/// Turns a committed table back into observations, so a regeneration adds rather than replaces.
///
/// Report directories are overwritten, so the committed table is folded in beside the
/// captures, as `write_symbol_db` accumulates names. Otherwise a measurement marked
/// `constant = false` because two batches disagreed would turn constant once one batch was
/// gone. The `disagreed` field is parsed back from the `value in source and source` shape
/// [`table`] writes.
pub(crate) fn observations_in_table(
    table: &orbistoun_hle::hardware::Measurements,
) -> Vec<Observation> {
    let mut out = Vec::new();
    for m in &table.measurements {
        let mut outcomes = vec![Outcome {
            verdict: String::new(),
            value: m.observation.clone(),
            note: String::new(),
            sources: m.sources.clone(),
        }];
        for rendered in &m.disagreed {
            // `value in a.txt and b.txt`. A line that does not split that way is kept whole as
            // the value with no source, so the disagreement is never dropped.
            let (value, sources) = rendered
                .split_once(" in ")
                .map_or((rendered.as_str(), Vec::new()), |(v, s)| {
                    (v, s.split(" and ").map(str::to_owned).collect())
                });
            outcomes.push(Outcome {
                verdict: String::new(),
                value: value.to_owned(),
                note: String::new(),
                sources,
            });
        }
        out.push(Observation {
            check: format!("{}{JOIN}{}{JOIN}{}", m.check, m.condition, m.kind),
            symbol: m.subject.clone(),
            outcomes,
        });
    }
    out
}
/// Turns folded observations into the committed table.
///
/// A measurement is constant when every run that took it agreed, decided by the runs
/// rather than by its kind. Disagreeing measurements are kept.
pub(crate) fn table(folded: &[Observation]) -> orbistoun_hle::hardware::Measurements {
    let mut out = Vec::new();
    for observation in folded {
        let mut parts = observation.check.split(JOIN);
        let (check, condition, kind) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        let first = &observation.outcomes[0];
        out.push(orbistoun_hle::hardware::Measurement {
            id: format!("{check}:{}:{condition}", observation.symbol),
            check: check.to_owned(),
            subject: observation.symbol.clone(),
            condition: condition.to_owned(),
            kind: kind.to_owned(),
            observation: first.value.clone(),
            sources: first.sources.clone(),
            constant: observation.outcomes.len() == 1,
            disagreed: observation.outcomes[1..]
                .iter()
                .map(|o| format!("{} in {}", o.value, o.sources.join(" and ")))
                .collect(),
        });
    }
    orbistoun_hle::hardware::Measurements { measurements: out }
}

/// What an ingest decided, before anything is written.
#[derive(Debug, Default)]
pub(crate) struct Ingested {
    /// The files to write, by library name.
    pub(crate) files: BTreeMap<String, KnowledgeFile>,
    /// How many observations were attached.
    pub(crate) attached: usize,
    /// How many of those record a disagreement between runs.
    pub(crate) varying: usize,
    /// Observed functions with no knowledge entry, which are reported rather than invented.
    pub(crate) unknown: BTreeSet<String>,
    /// Provenance faults. Any fault means nothing is written.
    pub(crate) faults: Vec<String>,
}

/// Attaches every observation to the entry for the function it names.
///
/// An observation about a function with no entry is reported, not created: the vendor
/// library the check used does not say which knowledge file models it.
pub(crate) fn ingest(
    observations: &[Observation],
    existing: &BTreeMap<String, KnowledgeFile>,
    today: &str,
) -> Ingested {
    let mut where_recorded: BTreeMap<&str, &str> = BTreeMap::new();
    for (library, file) in existing {
        for entry in &file.functions {
            where_recorded.entry(entry.name.as_str()).or_insert(library);
        }
    }
    let mut out = Ingested {
        files: existing.clone(),
        ..Ingested::default()
    };
    for observation in observations {
        let Some(library) = where_recorded.get(observation.symbol.as_str()) else {
            out.unknown.insert(observation.symbol.clone());
            continue;
        };
        let Some(file) = out.files.get_mut(*library) else {
            continue;
        };
        out.faults
            .extend(file.merge(&observation_of(observation), today));
        out.attached += 1;
        if observation.outcomes.len() > 1 {
            out.varying += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{edge_case, fold, ingest, observation_of, observations_in};

    /// A committed table survives a regeneration that cannot see the captures it came from.
    ///
    /// Otherwise a measurement marked non-constant because two batches disagreed would turn
    /// constant once one batch was gone.
    #[test]
    fn a_committed_table_is_carried_through_a_regeneration_that_cannot_see_its_captures() {
        use super::{observations_in_table, table};
        use orbistoun_hle::hardware::{Measurement, Measurements};

        let committed = Measurements {
            measurements: vec![
                Measurement {
                    id: "015-sync/x:sceKernelFoo:only".to_owned(),
                    check: "015-sync/x".to_owned(),
                    subject: "sceKernelFoo".to_owned(),
                    condition: "only".to_owned(),
                    kind: "code".to_owned(),
                    observation: "0x0".to_owned(),
                    sources: vec!["gone.txt".to_owned()],
                    constant: true,
                    disagreed: Vec::new(),
                },
                Measurement {
                    id: "015-sync/y:sceKernelBar:both".to_owned(),
                    check: "015-sync/y".to_owned(),
                    subject: "sceKernelBar".to_owned(),
                    condition: "both".to_owned(),
                    kind: "code".to_owned(),
                    observation: "0x1".to_owned(),
                    sources: vec!["one.txt".to_owned()],
                    constant: false,
                    disagreed: vec!["0x2 in two.txt and three.txt".to_owned()],
                },
            ],
        };

        // Read back and written out again with no captures, as a regeneration that saw
        // nothing new does.
        let again = table(&fold(observations_in_table(&committed)));
        assert_eq!(again.measurements.len(), 2, "nothing may be dropped");

        let kept = again
            .get("015-sync/y:sceKernelBar:both")
            .expect("still there");
        assert!(
            !kept.constant,
            "a disagreement must survive: losing it is how a deleted capture makes a claim stronger"
        );
        assert_eq!(
            kept.disagreed,
            vec!["0x2 in two.txt and three.txt".to_owned()]
        );
        assert_eq!(
            kept.values(),
            vec![1, 2],
            "and both values are still readable"
        );

        let alone = again
            .get("015-sync/x:sceKernelFoo:only")
            .expect("still there");
        assert!(alone.constant);
        assert_eq!(
            alone.sources,
            vec!["gone.txt".to_owned()],
            "including where it came from"
        );
    }

    const CAPTURE: &str = concat!(
        "OBS|try|015-sync/mutex-unlock-unheld|libkernel|scePthreadMutexUnlock\n",
        "OBS|res|015-sync/mutex-unlock-unheld|pass|0xffffffff80020001||derived\n",
        "OBS|try|900-surface/census|libkernel|(census)\n",
        "OBS|res|900-surface/census|pass|0x2f1||derived\n",
        "OBS|try|010-kernel/is-stack|libkernel|sceKernelIsStack\n",
        "OBS|res|010-kernel/is-stack|fail|0x0|a stack address was reported static|derived\n",
        "OBS|try|050-time/clock|libkernel|sceKernelGetProcessTime\n",
        "OBS|res|050-time/clock|pass|||derived\n"
    );

    /// A census row names no function, so it is not attached to one.
    ///
    /// A whole-surface count is not a fact about one function.
    #[test]
    fn a_census_row_is_not_an_observation_about_a_function() {
        let found = observations_in(CAPTURE, "ps5-full.txt");
        let names: Vec<&str> = found.iter().map(|o| o.symbol.as_str()).collect();
        assert_eq!(names, ["scePthreadMutexUnlock", "sceKernelIsStack"]);
    }

    /// A row carrying neither a value nor a note says nothing, so nothing is recorded.
    #[test]
    fn a_row_with_nothing_in_it_records_nothing() {
        let found = observations_in(CAPTURE, "ps5-full.txt");
        assert!(
            !found.iter().any(|o| o.symbol == "sceKernelGetProcessTime"),
            "a bare pass with no value and no note is not a finding"
        );
    }

    /// The value is quoted as a report, never as what the function returns; the wording is
    /// the guarantee, so the test asserts on it.
    #[test]
    fn an_observation_is_quoted_rather_than_interpreted() {
        let found = observations_in(CAPTURE, "ps5-full.txt");
        let text = edge_case(&found[0]);
        assert!(
            text.starts_with(concat!(
                "Measured on hardware: obSCEne `015-sync/mutex-unlock-unheld` reported pass, ",
                "value 0xffffffff80020001"
            )),
            "{text}"
        );
        assert!(
            text.contains("The check names the condition"),
            "and it says where the condition is: {text}"
        );
        assert!(
            !text.contains("returns"),
            "it must not claim a return value: {text}"
        );
    }

    /// The note travels with the verdict, because on a failure it is the whole finding.
    #[test]
    fn a_failure_carries_the_note_that_explains_it() {
        let found = observations_in(CAPTURE, "ps5-full.txt");
        let text = edge_case(&found[1]);
        assert!(text.contains("reported fail"), "{text}");
        assert!(
            text.contains("a stack address was reported static"),
            "{text}"
        );
    }

    /// The same finding in two runs is recorded once, naming both.
    #[test]
    fn an_outcome_seen_in_two_runs_is_one_finding() {
        let mut both = observations_in(CAPTURE, "ps5-full.txt");
        both.extend(observations_in(CAPTURE, "ps5-imports.txt"));
        assert_eq!(both.len(), 4, "two per capture before folding");

        let folded = fold(both);
        assert_eq!(folded.len(), 2, "and one per finding after");
        let text = edge_case(&folded[0]);
        assert!(
            text.contains("`data/hardware/ps5-full.txt` and `data/hardware/ps5-imports.txt`"),
            "both runs are named: {text}"
        );
    }

    /// A value that differs between runs is recorded as not constant, neither merged nor
    /// dropped.
    #[test]
    fn a_value_that_differs_between_runs_says_so() {
        let a = concat!(
            "OBS|try|010-kernel/clock|libkernel|sceKernelGetProcessTime\n",
            "OBS|res|010-kernel/clock|pass|0xc3||derived\n"
        );
        let b = concat!(
            "OBS|try|010-kernel/clock|libkernel|sceKernelGetProcessTime\n",
            "OBS|res|010-kernel/clock|pass|0x83||derived\n"
        );
        let mut both = observations_in(a, "ps5-full.txt");
        both.extend(observations_in(b, "ps5-imports.txt"));

        let folded = fold(both);
        assert_eq!(folded.len(), 1, "one check, one function, one finding");
        assert_eq!(folded[0].outcomes.len(), 2, "carrying both outcomes");

        let text = edge_case(&folded[0]);
        assert!(
            text.contains("not a constant of the platform"),
            "the disagreement is stated: {text}"
        );
        assert!(text.contains("0xc3") && text.contains("0x83"), "{text}");
    }

    /// `known_by` is not touched: one observation does not establish a function's whole
    /// recorded behaviour.
    #[test]
    fn an_observation_does_not_promote_the_entry_to_measured() {
        let found = observations_in(CAPTURE, "ps5-full.txt");
        let record = observation_of(&found[0]);
        assert_eq!(record.known_by, None, "the tier is the entry's, not ours");
        assert_eq!(
            record.cites, None,
            "the citation travels inside the edge case"
        );
        assert_eq!(record.purpose, None);
        assert_eq!(record.arity, None);
        assert_eq!(record.edge_cases.len(), 1);
    }

    /// A function nothing has recorded is reported rather than invented.
    #[test]
    fn an_unrecorded_function_is_reported_not_created() {
        let found = observations_in(CAPTURE, "ps5-full.txt");
        let existing = std::collections::BTreeMap::new();
        let out = ingest(&found, &existing, "2026-09-02");
        assert_eq!(out.attached, 0);
        assert_eq!(out.unknown.len(), 2, "both are named, neither is created");
        assert!(out.files.is_empty(), "and no file is conjured for them");
    }

    /// Ingesting the same capture twice does not record it twice.
    #[test]
    fn ingesting_the_same_capture_twice_is_idempotent() {
        use orbistoun_hle::knowledge::{FunctionKnowledge, KnowledgeFile};

        let mut existing = std::collections::BTreeMap::new();
        existing.insert(
            "libkernel".to_owned(),
            KnowledgeFile {
                library: "libkernel".to_owned(),
                functions: vec![FunctionKnowledge {
                    name: "scePthreadMutexUnlock".to_owned(),
                    ..FunctionKnowledge::default()
                }],
            },
        );
        let found = observations_in(CAPTURE, "ps5-full.txt");
        let once = ingest(&found, &existing, "2026-09-02");
        let twice = ingest(&found, &once.files, "2026-09-02");
        let edges = |i: &super::Ingested| i.files["libkernel"].functions[0].edge_cases.len();
        assert_eq!(edges(&once), 1);
        assert_eq!(edges(&twice), 1, "the merge appends without duplicating");
    }
}
