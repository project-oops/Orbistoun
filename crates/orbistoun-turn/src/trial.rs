//! Running an experiment against a real guest.
//!
//! The other half of [`crate::experiment`], which decides what to try and reads what the results
//! mean; this module runs the guest and gets them. The fault address comes from the persisted
//! trace, never from log prose (D046). The one exception is whether a planted write landed,
//! reported only as a sentence in `conditions.experiments`; a parse that does not match reads as
//! not planted, which the sweep reports as nothing measured rather than as evidence against a slot.
//! Everything that decides anything lives behind the `Trial` trait, so this module is as small as
//! it can be.

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
    /// The symbol database every run is given, if the caller has one.
    ///
    /// The database decides which imports have names, and an unnamed import gets a stub, so a run
    /// without it is a different program from the one the loop measures.
    symbols: Option<PathBuf>,
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
            symbols: None,
        }
    }

    /// Gives every run the symbol database the caller was given.
    ///
    /// Without it a sweep measures a guest whose unnamed imports all landed on stubs.
    #[must_use]
    pub fn with_symbols(mut self, symbols: Option<PathBuf>) -> Self {
        self.symbols = symbols;
        self
    }

    /// Sets a variable on every run this makes.
    ///
    /// A sweep writes its traces into its own data directory, so it neither overwrites the
    /// machine's nor picks up another run's trace as the newest.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// The trace the last run wrote, parsed.
    ///
    /// Exposed so a caller can enumerate what the guest called before deciding what to sweep.
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
    /// Newest by modification time. Traces are keyed by module, so a run overwrites its own and the
    /// newest is the one just written while a sweep points at one title.
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
    /// A slice, because a two-condition dependency needs both applied at once: a plant can need the
    /// call to answer success before the guest reads what was planted (D286).
    ///
    /// # Errors
    ///
    /// If the run could not be made, or wrote no trace.
    pub fn spawn(&self, axes: &[Axis]) -> Result<Outcome, Error> {
        let mut command = Command::new(&self.binary);
        // Before the subcommand, because it is a global option.
        if let Some(symbols) = &self.symbols {
            command.arg("--symbols-db").arg(symbols);
        }
        command.arg("run").arg(&self.title);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        // Every diagnostic variable is cleared first, so a run inherits nothing from another
        // experiment or from the environment the sweep was launched from.
        for name in Axis::every_variable() {
            command.env_remove(name);
        }
        // Set after the clearing loop, so nothing an experiment asks for is removed.
        for axis in axes {
            let (name, value) = axis.env();
            command.env(name, value);
        }

        // An axis that reports by writing has the child's error stream let through, since
        // `output()` would capture and drop it. Only for those axes: a sweep is hundreds of boots,
        // and these produce no verdict of their own, so what they print is the finding.
        let reports_by_writing = axes
            .iter()
            .any(|a| matches!(a, Axis::Read { .. } | Axis::Watch { .. }));
        if reports_by_writing {
            command.stderr(std::process::Stdio::inherit());
        }
        // A faulting guest is the normal outcome and often a non-zero exit, so the status is not
        // checked; whether a trace was written is, below.
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
            // Only a planted write reports a count. A run carrying a write is applied only if the
            // write landed; one carrying any other axis is applied by having set its variable; a
            // baseline applied nothing.
            planted: match axes.iter().find(|a| matches!(a, Axis::Write { .. })) {
                Some(_) => planted > 0,
                None => !axes.is_empty(),
            },
            refused: axes.iter().any(|a| matches!(a, Axis::Write { .. })) && refused > 0,
            // The second signal, read from the trace, because "the fault moved" and "the guest was
            // broken earlier" look identical from the address alone.
            reached: usize::try_from(
                trace
                    .get("distinct")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default(),
            )
            .unwrap_or(usize::MAX),
            // The third. An illegal instruction, a breakpoint or a stack overflow carries no
            // address parameters, so the reporter fills the field with the instruction pointer.
            // Classified by the list `orbistoun-report` publishes, so there is one definition.
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
/// Planted means the value reached guest memory. Refused means the address in that argument is not
/// writable, so the argument is not a pointer. When neither appears the write was never attempted.
/// Anything that does not match reads as zeroes: nothing was measured, rather than evidence against
/// a slot.
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
    // Split on the label rather than on position, so a reordering reads correctly and a sentence
    // with neither label reads as zero.
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
/// Matches `orbistoun-paths`' own constant without depending on it, keeping this crate clear of the
/// workspace's path policy; a caller with `orbistoun-paths` passes its answer instead.
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

    /// Anything unrecognised reads as zero, so the sweep reports nothing measured rather than a
    /// slot ruled out.
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
    /// A stale sentence left in a trace by an earlier run must not make a baseline look planted.
    #[test]
    fn a_baseline_is_never_reported_as_planted() {
        // The behaviour is in `Trial::run`, which needs a guest; what is checked here is the half
        // that decides it: `experiment.is_some_and(..)` is false for a baseline.
        let stale = "0x6abac2f3dc6f8cee:0:0x11000000 (3 planted, 0 refused)";
        assert!(counts(stale).0 > 0, "the fixture must look planted");
        let baseline: Option<&super::Experiment> = None;
        assert!(
            baseline.is_none_or(|_| counts(stale).0 == 0),
            "a baseline reported a planted write from a stale condition"
        );
    }
}
