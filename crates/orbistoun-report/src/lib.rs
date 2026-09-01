//! Run reports: the machine-readable contract (D046).
//!
//! Logs are for humans. **This is what an agent reads.** The distinction matters: if
//! a consumer greps log prose, then rewording a message silently breaks it, and the
//! log becomes an unversioned API nobody knows they are maintaining.
//!
//! # Designed for a reader with no memory of the session that produced the code
//!
//! Every choice here follows from that:
//!
//! - **[`RunDiff`] against the previous run of the same title** is the most important
//!   output. One report says what happened; the delta says whether the last change
//!   helped. Without it, every session begins by re-deriving state it should have been
//!   handed.
//! - **First-touch as well as frequency.** The *first* unmet need is usually the
//!   cause; everything after it is cascade.
//! - **Inputs are embedded** ([`RunInputs`]) - title hash, policy hash, overrides in
//!   force, build identity. Otherwise a difference between runs cannot be attributed
//!   to the change rather than to config drift, and the loop chases ghosts.
//! - **Bounded to kilobytes.** A finite context cannot read a multi-gigabyte trace, so
//!   this is an *index*: [`TOP_N`] and [`TAIL_N`], with the trace queried on demand.
//!   A report of "everything that happened" stalls the loop on the one artifact it
//!   depends on.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use orbistoun_proto::{ImportRecord, Outcome, Phase, SurveySummary};
use serde::{Deserialize, Serialize};

pub mod diagnose;
pub mod retention;
pub mod store;
pub mod trace;

/// Schema version. Bump on any change a consumer could trip over.
pub const SCHEMA_VERSION: u32 = 1;

/// How many ranked entries a list carries.
///
/// The report is an index, not a transcript: enough to decide what to do next, small
/// enough to read in full.
pub const TOP_N: usize = 20;

/// How many trailing calls the failure tail carries.
pub const TAIL_N: usize = 64;

/// Identifier shared by a run's log, trace, and report.
///
/// Sorts chronologically as a string, so a directory listing is already in order and
/// "the previous run" needs no index.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RunId(String);

impl RunId {
    /// Mints an id from a millisecond timestamp and a disambiguating suffix.
    ///
    /// Time is passed in rather than read, so a test can produce a known id and the
    /// whole crate stays deterministic.
    pub fn new(unix_ms: u64, suffix: u32) -> Self {
        Self(format!("{unix_ms:013}-{suffix:04x}"))
    }

    /// Mints an id from the current clock.
    pub fn now(suffix: u32) -> Self {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        Self::new(ms, suffix)
    }

    /// Reconstructs an id from a filename stem.
    ///
    /// Deliberately not validating: an id read off disk is data, and refusing to list
    /// an unexpected filename would be less useful than listing it and letting the
    /// read fail with the path attached.
    pub fn from_raw(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// The id as text - also the filename stem for every artifact of this run.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What produced this run. Embedded so a difference can be attributed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunInputs {
    /// Content hash of the guest executable - the title's identity (D048).
    pub title_hash: String,
    /// Where it was loaded from. Diagnostic only; the hash is the identity.
    pub title_path: String,
    /// Hash of the stub policy in force, so a policy edit is visible as an input
    /// change rather than an unexplained behaviour change.
    pub policy_hash: String,
    /// Effective overrides, keyed by setting name, valued as `value (layer)`.
    /// Present so behaviour that came from an override is never invisible.
    pub overrides: BTreeMap<String, String>,
    /// Build identity.
    pub binary_version: String,
    /// Build commit.
    pub binary_commit: String,
}

/// One unresolved import and how often it was wanted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedCount {
    /// The hash the module imports by.
    pub nid: u64,
    /// Symbol name, where known.
    pub symbol: Option<String>,
    /// Library, where known.
    pub library: Option<String>,
    /// How many times it was needed.
    pub count: u64,
}

/// One recorded call near the end of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TailEntry {
    /// Global ordering position.
    pub seq: u64,
    /// The hash invoked.
    pub nid: u64,
    /// Symbol name, where known.
    pub symbol: Option<String>,
    /// Guest address the call returns to - the call site, not just the function.
    pub return_address: u64,
    /// Guest thread.
    pub thread_id: u32,
    /// What was returned to the guest.
    pub ret: u64,
    /// Whether a stub answered.
    pub stubbed: bool,
}

/// Aggregate counters for the run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// Total guest calls recorded.
    pub calls: u64,
    /// How many hit a stub rather than a real implementation.
    pub stubbed: u64,
    /// Distinct unresolved imports encountered.
    pub distinct_unresolved: usize,
}

/// One run, as an agent reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    /// Schema version this document conforms to.
    pub schema_version: u32,
    /// Shared with this run's log and trace.
    pub run_id: RunId,
    /// When the run started.
    pub started_unix_ms: u64,
    /// What produced it.
    pub inputs: RunInputs,
    /// Furthest phase reached.
    pub reached: Phase,
    /// How it ended, if it ended.
    pub outcome: Option<Outcome>,
    /// Static survey, when one was taken.
    pub survey: Option<SurveySummary>,
    /// Unresolved imports ranked by how often they were wanted, capped at [`TOP_N`].
    pub unresolved_by_frequency: Vec<UnresolvedCount>,
    /// The first import the run needed and could not answer.
    ///
    /// Usually the actual cause; everything after it is cascade.
    pub first_unmet: Option<ImportRecord>,
    /// The last [`TAIL_N`] calls before termination.
    pub failure_tail: Vec<TailEntry>,
    /// Aggregate counters.
    pub counts: Counts,
}

impl RunReport {
    /// A report for a run that has just started.
    pub fn started(run_id: RunId, started_unix_ms: u64, inputs: RunInputs) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            run_id,
            started_unix_ms,
            inputs,
            reached: Phase::Start,
            outcome: None,
            survey: None,
            unresolved_by_frequency: Vec::new(),
            first_unmet: None,
            failure_tail: Vec::new(),
            counts: Counts::default(),
        }
    }

    /// Records that a phase completed, never regressing the high-water mark.
    pub fn reached(&mut self, phase: Phase) {
        if phase > self.reached {
            self.reached = phase;
        }
    }

    /// Attaches a survey, deriving the ranked and first-touch views from it.
    ///
    /// Ranking is by count descending, then NID, so equal counts order
    /// deterministically - a report is diffed, and unstable ordering reads as change.
    pub fn set_survey(&mut self, survey: SurveySummary) {
        let mut counts: BTreeMap<u64, UnresolvedCount> = BTreeMap::new();
        for import in survey.unresolved_imports() {
            counts
                .entry(import.nid)
                .and_modify(|c| c.count += 1)
                .or_insert_with(|| UnresolvedCount {
                    nid: import.nid,
                    symbol: import.symbol.clone(),
                    library: import.library.clone(),
                    count: 1,
                });
        }
        self.counts.distinct_unresolved = counts.len();

        let mut ranked: Vec<_> = counts.into_values().collect();
        ranked.sort_by(|a, b| b.count.cmp(&a.count).then(a.nid.cmp(&b.nid)));
        ranked.truncate(TOP_N);
        self.unresolved_by_frequency = ranked;

        self.first_unmet = survey.unresolved_imports().next().cloned();
        self.survey = Some(survey);
        self.reached(Phase::ImportsResolved);
    }

    /// Keeps only the last [`TAIL_N`] entries of the supplied tail.
    pub fn set_failure_tail(&mut self, mut tail: Vec<TailEntry>) {
        if tail.len() > TAIL_N {
            tail.drain(..tail.len() - TAIL_N);
        }
        self.failure_tail = tail;
    }

    /// Serialises to pretty JSON - read by machines, but also by people debugging why
    /// the machine did something.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parses a report.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

/// How the phase high-water mark moved between two runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseChange {
    /// Got further than last time.
    Advanced,
    /// Got less far - the clearest "that change made it worse" signal there is.
    Regressed,
    /// No change.
    Same,
}

/// The delta between two runs of the same title.
///
/// The single most valuable output of this crate: it turns "here is a report" into
/// "here is whether your last change helped".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunDiff {
    /// The run this describes.
    pub current: RunId,
    /// What it was compared against.
    pub previous: RunId,
    /// Whether the two runs even describe the same title.
    pub same_title: bool,
    /// Whether the inputs were otherwise identical - if not, a behaviour difference
    /// may be config drift rather than the change under test.
    pub same_inputs: bool,
    /// Phase movement.
    pub phase_change: PhaseChange,
    /// Symbols that were unresolved before and are not now.
    pub newly_resolved: Vec<String>,
    /// Symbols that are unresolved now and were not before.
    pub newly_unresolved: Vec<String>,
    /// Change in total recorded calls. Positive means the guest got further.
    pub calls_delta: i64,
}

impl RunDiff {
    /// Compares `current` against `previous`.
    pub fn between(previous: &RunReport, current: &RunReport) -> Self {
        let names = |r: &RunReport| -> std::collections::BTreeSet<String> {
            r.unresolved_by_frequency
                .iter()
                .map(|u| {
                    u.symbol
                        .clone()
                        .unwrap_or_else(|| format!("nid:{:#018x}", u.nid))
                })
                .collect()
        };
        let before = names(previous);
        let after = names(current);

        Self {
            current: current.run_id.clone(),
            previous: previous.run_id.clone(),
            same_title: previous.inputs.title_hash == current.inputs.title_hash,
            same_inputs: previous.inputs == current.inputs,
            phase_change: match current.reached.cmp(&previous.reached) {
                std::cmp::Ordering::Greater => PhaseChange::Advanced,
                std::cmp::Ordering::Less => PhaseChange::Regressed,
                std::cmp::Ordering::Equal => PhaseChange::Same,
            },
            newly_resolved: before.difference(&after).cloned().collect(),
            newly_unresolved: after.difference(&before).cloned().collect(),
            calls_delta: i64::try_from(current.counts.calls).unwrap_or(i64::MAX)
                - i64::try_from(previous.counts.calls).unwrap_or(i64::MAX),
        }
    }

    /// Whether this diff shows any change worth reading.
    pub fn is_noteworthy(&self) -> bool {
        self.phase_change != PhaseChange::Same
            || !self.newly_resolved.is_empty()
            || !self.newly_unresolved.is_empty()
            || self.calls_delta != 0
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Counts, PhaseChange, RunDiff, RunId, RunInputs, RunReport, SCHEMA_VERSION, TAIL_N, TOP_N,
        TailEntry,
    };
    use orbistoun_proto::{ImportRecord, Phase, SurveySummary};

    fn import(nid: u64, symbol: Option<&str>, known: bool) -> ImportRecord {
        ImportRecord {
            nid,
            library: Some("libTest".to_owned()),
            symbol: symbol.map(str::to_owned),
            known,
            kind: orbistoun_proto::ImportKind::Function,
        }
    }

    fn report(id: &str) -> RunReport {
        RunReport::started(
            RunId::new(1_700_000_000_000, u32::from_str_radix(id, 16).unwrap_or(0)),
            1_700_000_000_000,
            RunInputs {
                title_hash: "abc123".to_owned(),
                ..RunInputs::default()
            },
        )
    }

    #[test]
    fn run_ids_sort_chronologically_as_strings() {
        // A directory listing is then already in order, so "the previous run" needs
        // no index and no parsing.
        let a = RunId::new(1_700_000_000_000, 1);
        let b = RunId::new(1_700_000_000_001, 0);
        assert!(a < b, "{a} should sort before {b}");
    }

    #[test]
    fn phase_high_water_mark_never_regresses_within_a_run() {
        let mut r = report("1");
        r.reached(Phase::Mapped);
        r.reached(Phase::ContainerParsed);
        assert_eq!(
            r.reached,
            Phase::Mapped,
            "a later earlier-phase event must not lower the mark"
        );
    }

    #[test]
    fn survey_produces_frequency_ranking_and_first_touch() {
        let mut r = report("1");
        r.set_survey(SurveySummary {
            entry: 0x1000,
            imports: vec![
                import(1, Some("first_unmet"), false),
                import(2, Some("popular"), false),
                import(2, Some("popular"), false),
                import(3, Some("resolved"), true),
            ],
        });

        // Frequency ranking puts the most-wanted first.
        assert_eq!(
            r.unresolved_by_frequency[0].symbol.as_deref(),
            Some("popular")
        );
        assert_eq!(r.unresolved_by_frequency[0].count, 2);
        // First-touch is a different question and often the more useful one.
        assert_eq!(
            r.first_unmet.expect("some").symbol.as_deref(),
            Some("first_unmet")
        );
        assert_eq!(r.counts.distinct_unresolved, 2, "resolved imports excluded");
        assert_eq!(r.reached, Phase::ImportsResolved);
    }

    #[test]
    fn ranking_is_deterministic_when_counts_tie() {
        // Reports are diffed; unstable ordering on ties would read as change.
        let mut r = report("1");
        r.set_survey(SurveySummary {
            entry: 0,
            imports: vec![
                import(9, Some("nine"), false),
                import(3, Some("three"), false),
                import(7, Some("seven"), false),
            ],
        });
        let nids: Vec<_> = r.unresolved_by_frequency.iter().map(|u| u.nid).collect();
        assert_eq!(nids, [3, 7, 9], "equal counts must fall back to NID order");
    }

    #[test]
    fn the_report_stays_bounded() {
        // A finite context cannot read an unbounded document; that is the whole
        // reason for the caps.
        let mut r = report("1");
        let imports = (0..TOP_N as u64 * 5)
            .map(|n| import(n, Some("x"), false))
            .collect();
        r.set_survey(SurveySummary { entry: 0, imports });
        assert_eq!(r.unresolved_by_frequency.len(), TOP_N);
        assert!(
            r.counts.distinct_unresolved > TOP_N,
            "the true count is still reported even though the list is capped"
        );

        let tail = (0..TAIL_N as u64 * 3)
            .map(|seq| TailEntry {
                seq,
                nid: 1,
                symbol: None,
                return_address: 0,
                thread_id: 0,
                ret: 0,
                stubbed: true,
            })
            .collect();
        r.set_failure_tail(tail);
        assert_eq!(r.failure_tail.len(), TAIL_N);
        assert_eq!(
            r.failure_tail[0].seq,
            (TAIL_N as u64 * 3) - TAIL_N as u64,
            "the tail must keep the END of the run, not the start"
        );
    }

    #[test]
    fn a_report_round_trips_as_json() {
        let mut r = report("1");
        r.counts = Counts {
            calls: 42,
            stubbed: 7,
            distinct_unresolved: 3,
        };
        let json = r.to_json().expect("serialise");
        assert_eq!(RunReport::from_json(&json).expect("parse"), r);
        assert!(json.contains("\"schema_version\""), "version is explicit");
    }

    #[test]
    fn schema_version_is_recorded_so_a_consumer_can_refuse_an_unknown_shape() {
        assert_eq!(report("1").schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn diff_detects_progress() {
        let mut before = report("1");
        before.reached(Phase::ContainerParsed);
        before.counts.calls = 100;
        let mut after = report("2");
        after.reached(Phase::Entered);
        after.counts.calls = 500;

        let d = RunDiff::between(&before, &after);
        assert_eq!(d.phase_change, PhaseChange::Advanced);
        assert_eq!(d.calls_delta, 400);
        assert!(d.same_title);
        assert!(d.is_noteworthy());
    }

    #[test]
    fn diff_detects_regression() {
        // The clearest "your change made it worse" signal the report can carry.
        let mut before = report("1");
        before.reached(Phase::Entered);
        let mut after = report("2");
        after.reached(Phase::Mapped);
        assert_eq!(
            RunDiff::between(&before, &after).phase_change,
            PhaseChange::Regressed
        );
    }

    #[test]
    fn diff_names_what_changed_in_both_directions() {
        let mut before = report("1");
        before.set_survey(SurveySummary {
            entry: 0,
            imports: vec![
                import(1, Some("was_missing"), false),
                import(2, Some("still"), false),
            ],
        });
        let mut after = report("2");
        after.set_survey(SurveySummary {
            entry: 0,
            imports: vec![
                import(2, Some("still"), false),
                import(3, Some("now_missing"), false),
            ],
        });

        let d = RunDiff::between(&before, &after);
        assert_eq!(d.newly_resolved, ["was_missing"]);
        assert_eq!(d.newly_unresolved, ["now_missing"]);
    }

    #[test]
    fn diff_flags_input_drift_so_a_difference_is_not_misattributed() {
        // Without this, an agent credits its own change for a difference caused by a
        // policy edit, and chases ghosts.
        let before = report("1");
        let mut after = report("2");
        after.inputs.policy_hash = "changed".to_owned();

        let d = RunDiff::between(&before, &after);
        assert!(d.same_title, "same title");
        assert!(!d.same_inputs, "but the inputs moved");
    }

    #[test]
    fn diff_notices_a_different_title_entirely() {
        let before = report("1");
        let mut after = report("2");
        after.inputs.title_hash = "different".to_owned();
        assert!(!RunDiff::between(&before, &after).same_title);
    }

    #[test]
    fn an_unchanged_run_is_not_noteworthy() {
        let a = report("1");
        let b = report("2");
        assert!(!RunDiff::between(&a, &b).is_noteworthy());
    }
}
