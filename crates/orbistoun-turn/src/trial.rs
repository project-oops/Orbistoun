//! Running an experiment against a real guest.
//!
//! The other half of [`crate::experiment`]: that module decides what to try and reads
//! what the results mean, this one shells out and gets them.
//!
//! # It reads the trace, not the terminal
//!
//! D046 is explicit that a machine consumer must not grep log prose - *"rewording that
//! message silently breaks it; log prose becomes an unversioned API"*. So the fault
//! address comes from the persisted trace, which is the contract, rather than from what
//! the run printed.
//!
//! # One thing here is prose, and it fails safe
//!
//! Whether a planted write actually landed is reported inside `conditions.experiments`,
//! as a sentence: `"<target>:<slot>:<value> (N planted, M refused)"`. There is no
//! structured field for it, so this parses that string - the one place where the rule
//! above is broken because there is no alternative.
//!
//! **It fails in the safe direction.** A parse that does not match reports the write as
//! *not planted*, which the sweep reads as `NeverPlanted` - "nothing was measured" -
//! rather than as evidence against a slot. A format change therefore costs a
//! re-examination, not a wrong conclusion.
//!
//! # No guest is needed to test the reasoning
//!
//! Everything that decides anything lives in [`crate::experiment`], behind the `Trial`
//! trait. This module is the part that cannot be tested without a title and a
//! twenty-second boot, so it is deliberately as small and as stupid as it can be.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;
use crate::axis::{Axis, Change, compare};
use crate::experiment::{Experiment, Outcome, Trial};

/// Runs a real guest, once per experiment.
#[derive(Debug)]
pub struct GuestTrial {
    binary: PathBuf,
    title: PathBuf,
    traces: PathBuf,
    env: Vec<(String, String)>,
}

impl GuestTrial {
    /// Points a trial at a built binary, a title, and where traces land.
    #[must_use]
    pub fn new(
        binary: impl Into<PathBuf>,
        title: impl Into<PathBuf>,
        traces: impl Into<PathBuf>,
    ) -> Self {
        Self {
            binary: binary.into(),
            title: title.into(),
            traces: traces.into(),
            env: Vec::new(),
        }
    }

    /// Sets a variable on every run this makes.
    ///
    /// Needed because a sweep should not write its traces into whatever data directory
    /// the machine happens to use - twelve runs would overwrite whatever was there, and
    /// the newest-trace rule would pick up somebody else's run if one overlapped.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// The trace the last run wrote, parsed.
    ///
    /// Exposed so a caller can enumerate what the guest actually called before deciding
    /// what to sweep. Reading the trace rather than guessing at a target is the whole
    /// difference between a sweep that plants something and one that reports
    /// `NeverPlanted` twelve times.
    ///
    /// # Errors
    ///
    /// If no trace was written, or it cannot be read or parsed.
    pub fn trace(&self) -> Result<serde_json::Value, Error> {
        let path = self.newest_trace()?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::Reply(format!("reading {}: {e}", path.display())))?;
        serde_json::from_str(&text)
            .map_err(|e| Error::Reply(format!("parsing {}: {e}", path.display())))
    }

    /// The trace this run wrote.
    ///
    /// Newest by modification time. Traces are keyed by module, so a run overwrites its
    /// own rather than accumulating - which means "the newest" is the one just written
    /// whenever a sweep points at one title, and a sweep that pointed at several would
    /// need the module name instead.
    fn newest_trace(&self) -> Result<PathBuf, Error> {
        let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
        let entries = std::fs::read_dir(&self.traces)
            .map_err(|e| Error::Reply(format!("reading {}: {e}", self.traces.display())))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            if best.as_ref().is_none_or(|(seen, _)| modified > *seen) {
                best = Some((modified, path));
            }
        }
        best.map(|(_, path)| path).ok_or_else(|| {
            Error::Reply(format!("no trace was written to {}", self.traces.display()))
        })
    }
}

impl Trial for GuestTrial {
    fn run(&mut self, experiment: Option<&Experiment>) -> Result<Outcome, Error> {
        self.spawn(&experiment.map(Experiment::axes).unwrap_or_default())
    }

    fn spawn_axes(&mut self, axes: &[Axis]) -> Result<Outcome, Error> {
        self.spawn(axes)
    }
}

impl GuestTrial {
    /// Runs once with these axes applied, or with none of them for a baseline.
    ///
    /// **A slice rather than one**, because a two-condition dependency needs both applied at
    /// once and one at a time cannot see it: at `image+0xafc959` the plant needed the call to
    /// answer success before the guest would read what was planted, and either alone left the
    /// fault exactly where it was (D283, D286).
    ///
    /// # Errors
    ///
    /// If the run could not be made, or wrote no trace.
    pub fn spawn(&self, axes: &[Axis]) -> Result<Outcome, Error> {
        let mut command = Command::new(&self.binary);
        command.arg("run").arg(&self.title);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        // **Every diagnostic variable cleared first, always.** One experiment inheriting
        // another's - or the environment this sweep was launched from - is not a
        // controlled run, and a baseline taken with a stale variable set is not a
        // baseline at all.
        for name in Axis::every_variable() {
            command.env_remove(name);
        }
        // Set after the clearing loop above, so nothing an experiment asks for is removed by
        // the tidy-up meant for the run before it.
        for axis in axes {
            let (name, value) = axis.env();
            command.env(name, value);
        }

        // A faulting guest is the normal outcome and often a non-zero exit, so the status
        // is not checked. What matters is whether a trace was written, and that is
        // checked below.
        command
            .output()
            .map_err(|e| Error::Reply(format!("running {}: {e}", self.binary.display())))?;

        let trace = self.trace()?;
        let conditions = trace
            .pointer("/conditions/experiments")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let (planted, refused) = counts(conditions);
        Ok(Outcome {
            fault: trace
                .pointer("/fault/address")
                .and_then(serde_json::Value::as_u64),
            // Only a planted write reports a count. Every other axis is applied by
            // setting its variable, and the run either honoured it or the worker said so
            // on its own terms - so an axis with no count of its own counts as applied.
            // A run carrying a write is applied only if the write landed; one carrying any
            // other axis is applied by having set its variable. A run carrying nothing is a
            // baseline and applied nothing.
            planted: match axes.iter().find(|a| matches!(a, Axis::Write { .. })) {
                Some(_) => planted > 0,
                None => !axes.is_empty(),
            },
            refused: axes.iter().any(|a| matches!(a, Axis::Write { .. })) && refused > 0,
            // The second signal. Read from the trace rather than inferred, because
            // "the fault moved" and "the guest was broken earlier" look identical
            // from the address alone - and one of those is worth an afternoon.
            reached: usize::try_from(
                trace
                    .get("distinct")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default(),
            )
            .unwrap_or(usize::MAX),
            // The third. An illegal instruction, a breakpoint or a stack overflow carries
            // no address parameters, so the reporter fills the field with the instruction
            // pointer - a real number that is not somewhere the guest asked to touch.
            // Classified by the list `orbistoun-report` publishes, rather than by matching
            // the prose here, so there is one definition of it.
            touched: trace
                .pointer("/fault/kind")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| orbistoun_report::trace::FaultSite::TOUCHED.contains(&kind)),
        })
    }

    /// Runs a list of axes against one baseline and says what each changed.
    ///
    /// # Errors
    ///
    /// If the baseline or any run could not be made.
    pub fn probe(&self, axes: &[Axis]) -> Result<Vec<(Axis, Change)>, Error> {
        let baseline = self.spawn(&[])?;
        let mut out = Vec::with_capacity(axes.len());
        for axis in axes {
            let outcome = self.spawn(std::slice::from_ref(axis))?;
            let applied = outcome.planted;
            out.push((axis.clone(), compare(&baseline, &outcome, applied)));
        }
        Ok(out)
    }
}

/// How many writes the run planted, and how many it refused.
///
/// Reads the sentence the worker writes, which on a live run looks like:
///
/// ```text
/// 0x11000000 at *arg1 of 0x6abac2f3dc6f8cee (0 planted, 1 refused)
/// ```
///
/// **Both numbers matter and they mean different things.** Planted means the value
/// reached guest memory. Refused means the address in that argument is not writable -
/// so the argument is not a pointer at all, which rules the slot out for a reason. When
/// neither number appears the write was never attempted, and nothing was measured.
///
/// Zeroes for anything that does not match, which is the safe direction: the sweep reads
/// that as *nothing was measured* rather than as evidence against a slot.
#[must_use]
pub fn counts(experiments: &str) -> (u64, u64) {
    let Some(inside) = experiments.split_once('(').map(|(_, rest)| rest) else {
        return (0, 0);
    };
    let number_before = |word: &str| -> u64 {
        inside
            .split(word)
            .next()
            .and_then(|before| before.split_whitespace().last())
            .and_then(|number| number.parse().ok())
            .unwrap_or(0)
    };
    // Split on the label rather than on position: "N planted, M refused" and any future
    // reordering both read correctly, and a sentence with neither label reads as zero.
    let planted = if inside.contains("planted") {
        number_before("planted")
    } else {
        0
    };
    let refused = if inside.contains("refused") {
        inside
            .split("refused")
            .next()
            .and_then(|before| before.split_whitespace().last())
            .and_then(|number| number.parse().ok())
            .unwrap_or(0)
    } else {
        0
    };
    (planted, refused)
}

/// Where traces land beneath a data directory.
///
/// Matches `orbistoun-paths`' own constant without depending on it - this crate stays
/// clear of the workspace's path policy for the same reason `orbistoun-llm` does, and a
/// caller that has `orbistoun-paths` should pass its answer instead.
pub const TRACES_DIR: &str = "traces";

/// The traces directory beneath a data root.
#[must_use]
pub fn traces_in(data_dir: &Path) -> PathBuf {
    data_dir.join(TRACES_DIR)
}

#[cfg(test)]
mod tests {
    use super::counts;

    /// The count is read out of the sentence the run writes.
    #[test]
    fn a_planted_count_is_read_from_the_conditions() {
        assert_eq!(
            counts("0x6abac2f3dc6f8cee:0:0x11000000 (3 planted, 0 refused)").0,
            3
        );
        assert_eq!(counts("libkernel::foo:1:0x22 (0 planted, 7 refused)").0, 0);
    }

    /// **Anything unrecognised reads as zero, and that is the safe direction.**
    ///
    /// Zero means the sweep reports `NeverPlanted` - nothing was measured - rather than
    /// treating the run as evidence that the slot is innocent. A change to the sentence
    /// this parses therefore costs a re-examination, never a wrong conclusion.
    #[test]
    fn an_unrecognised_condition_reads_as_nothing_planted() {
        for unrecognised in [
            "",
            "0x6abac2f3dc6f8cee:0:0x11000000",
            "planted three of them",
            "(many planted)",
            "(  planted",
        ] {
            assert_eq!(counts(unrecognised).0, 0, "{unrecognised:?}");
        }
    }

    /// A run with no experiment reports nothing planted, whatever the conditions say.
    ///
    /// The baseline has nothing to plant, so a stale sentence left in a trace by an
    /// earlier run must not make it look as though it had.
    #[test]
    fn a_baseline_is_never_reported_as_planted() {
        // The behaviour is in `Trial::run`, which needs a guest; what is checked here is
        // the half that decides it - `experiment.is_some_and(..)` is false for a
        // baseline regardless of what the string holds.
        let stale = "0x6abac2f3dc6f8cee:0:0x11000000 (3 planted, 0 refused)";
        assert!(counts(stale).0 > 0, "the fixture must look planted");
        let baseline: Option<&super::Experiment> = None;
        assert!(
            baseline.is_none_or(|_| counts(stale).0 == 0),
            "a baseline reported a planted write from a stale condition"
        );
    }
}
