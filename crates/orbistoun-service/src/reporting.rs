//! Turning a survey into a persisted, diffed run report.
//!
//! The report is the machine-readable contract (D046). This module is where a survey
//! becomes one: it stamps the inputs that make a run reproducible, writes it beside
//! previous runs, and produces the diff that says whether anything changed.

use std::path::Path;

use orbistoun_report::store::ReportStore;
use orbistoun_report::{RunDiff, RunId, RunInputs, RunReport};
use sha1::{Digest, Sha1};

use crate::{Service, ServiceError};

/// What one reporting run produced.
#[derive(Debug, Clone)]
pub struct RunOutput {
    /// The report for this run.
    pub report: RunReport,
    /// The delta against the previous run of the same title, if there was one.
    ///
    /// `None` on a title's first run - which is information, not a failure: there is
    /// simply nothing to compare against yet.
    pub diff: Option<RunDiff>,
    /// Where the report was written, if reporting is enabled.
    pub written_to: Option<std::path::PathBuf>,
}

/// Content hash identifying a title.
///
/// Hashes the **executable**, never the directory: a title directory can be tens of
/// gigabytes, and the executable is both small enough and the semantically right thing -
/// it is what changes when a title is patched (D048).
pub fn content_hash(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    h.finalize().iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Builds, diffs, and persists a report for one survey.
pub(crate) fn emit(
    service: &Service,
    path: &Path,
    bytes: &[u8],
    survey: orbistoun_proto::SurveySummary,
    now_unix_ms: u64,
) -> Result<RunOutput, ServiceError> {
    let policy_toml = service.default_policy_toml()?;
    let inputs = RunInputs {
        title_hash: content_hash(bytes),
        title_path: path.display().to_string(),
        policy_hash: content_hash(policy_toml.as_bytes()),
        overrides: std::collections::BTreeMap::new(),
        binary_version: env!("CARGO_PKG_VERSION").to_owned(),
        // `option_env!` is a macro and needs a literal, so this is the one name in the
        // tree that cannot be taken from `orbistoun-env`. Declared there anyway, so the
        // listing and the typo check both know about it (D221).
        binary_commit: option_env!("ORBISTOUN_COMMIT")
            .unwrap_or("unknown")
            .to_owned(),
    };

    // The run id's suffix disambiguates two runs landing in the same millisecond.
    // Derived from the title hash so it is stable and needs no randomness, which
    // would make reports irreproducible.
    let suffix = u32::from_str_radix(&inputs.title_hash[..4], 16).unwrap_or(0);
    let mut report = RunReport::started(RunId::new(now_unix_ms, suffix), now_unix_ms, inputs);
    report.reached(orbistoun_proto::Phase::ContainerParsed);
    report.set_survey(survey);

    let Some(paths) = service.paths.as_ref() else {
        return Ok(RunOutput {
            report,
            diff: None,
            written_to: None,
        });
    };

    let store = ReportStore::new(paths.reports_dir());
    let previous = store
        .previous_for_title(&report)
        .map_err(|e| ServiceError::Serialise(e.to_string()))?;
    let diff = previous.as_ref().map(|p| RunDiff::between(p, &report));
    let written_to = store
        .write(&report)
        .map_err(|e| ServiceError::Serialise(e.to_string()))?;

    Ok(RunOutput {
        report,
        diff,
        written_to: Some(written_to),
    })
}

#[cfg(test)]
mod tests {
    use super::content_hash;

    #[test]
    fn the_content_hash_is_stable_and_distinguishes_content() {
        assert_eq!(content_hash(b"abc"), content_hash(b"abc"));
        assert_ne!(content_hash(b"abc"), content_hash(b"abd"));
        assert_eq!(content_hash(b"abc").len(), 40, "hex sha1");
    }

    #[test]
    fn an_empty_input_still_hashes() {
        // A zero-byte file is a legitimate thing to be handed, and hashing it must not
        // produce an empty identity that collides with "no title".
        assert_eq!(content_hash(b"").len(), 40);
        assert_ne!(content_hash(b""), content_hash(b"x"));
    }
}
