//! Persisting reports, and finding the one to compare against.
//!
//! The store exists for a single question: **what did the previous run of this title
//! do?** Everything else here serves that. A [`crate::RunDiff`] needs a previous
//! report, and locating it must not require an index, a database, or parsing every
//! file on disk.
//!
//! Layout is one file per run, named by [`crate::RunId`], which sorts chronologically
//! as a string. So "most recent" is a directory listing sorted descending, and the
//! only files that need parsing are candidates, not the whole history.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{RunId, RunReport};

/// Extension used for report files.
pub const REPORT_EXT: &str = "json";

/// Why a store operation failed.
#[derive(Debug)]
pub enum StoreError {
    /// Filesystem trouble.
    Io(io::Error),
    /// A file was present but not a report we understand.
    Malformed {
        /// Which file.
        path: PathBuf,
        /// What went wrong.
        detail: String,
    },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Malformed { path, detail } => {
                write!(f, "malformed report {}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A directory of run reports.
#[derive(Debug, Clone)]
pub struct ReportStore {
    dir: PathBuf,
}

impl ReportStore {
    /// Opens a store rooted at `dir`. Nothing is created until a write.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Where reports live.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Path a given run's report occupies.
    pub fn path_for(&self, id: &RunId) -> PathBuf {
        self.dir.join(format!("{id}.{REPORT_EXT}"))
    }

    /// Writes a report, creating the directory if needed.
    pub fn write(&self, report: &RunReport) -> Result<PathBuf, StoreError> {
        fs::create_dir_all(&self.dir)?;
        let path = self.path_for(&report.run_id);
        let json = report.to_json().map_err(|e| StoreError::Malformed {
            path: path.clone(),
            detail: e.to_string(),
        })?;
        fs::write(&path, json)?;
        Ok(path)
    }

    /// Reads one report.
    pub fn read(&self, id: &RunId) -> Result<RunReport, StoreError> {
        let path = self.path_for(id);
        let text = fs::read_to_string(&path)?;
        RunReport::from_json(&text).map_err(|e| StoreError::Malformed {
            path,
            detail: e.to_string(),
        })
    }

    /// Every run id present, newest last.
    ///
    /// A missing directory lists as empty rather than failing - the first run has no
    /// history and that is not an error.
    pub fn ids(&self) -> Result<Vec<RunId>, StoreError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut ids: Vec<RunId> = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some(REPORT_EXT) {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                ids.push(RunId::from_raw(stem));
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// The most recent report for the same title as `current`, excluding `current`.
    ///
    /// Walks newest-first and stops at the first match, so a long history costs one
    /// parse rather than all of them. Title identity is the content hash (D048), not
    /// the path - a title moved on disk is still the same title.
    pub fn previous_for_title(&self, current: &RunReport) -> Result<Option<RunReport>, StoreError> {
        for id in self.ids()?.into_iter().rev() {
            if id == current.run_id {
                continue;
            }
            let candidate = self.read(&id)?;
            if candidate.inputs.title_hash == current.inputs.title_hash {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::ReportStore;
    use crate::{RunId, RunInputs, RunReport};
    use orbistoun_proto::Phase;

    fn report(ms: u64, title: &str) -> RunReport {
        RunReport::started(
            RunId::new(ms, 0),
            ms,
            RunInputs {
                title_hash: title.to_owned(),
                ..RunInputs::default()
            },
        )
    }

    #[test]
    fn an_empty_store_lists_nothing_rather_than_failing() {
        // A first run has no history; that is not an error condition.
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ReportStore::new(tmp.path().join("never-created"));
        assert!(store.ids().expect("empty is fine").is_empty());
    }

    #[test]
    fn a_report_round_trips_through_disk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ReportStore::new(tmp.path());
        let mut r = report(1_700_000_000_000, "abc");
        r.reached(Phase::Mapped);

        store.write(&r).expect("write");
        assert_eq!(store.read(&r.run_id).expect("read"), r);
    }

    #[test]
    fn ids_come_back_in_chronological_order() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ReportStore::new(tmp.path());
        for ms in [1_700_000_000_002, 1_700_000_000_000, 1_700_000_000_001] {
            store.write(&report(ms, "abc")).expect("write");
        }
        let ids = store.ids().expect("ids");
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "listing must already be ordered");
    }

    #[test]
    fn previous_run_is_found_by_title_hash_not_by_path() {
        // A title moved on disk is still the same title; a different title in the
        // same place is not.
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ReportStore::new(tmp.path());

        let mut old = report(1_700_000_000_000, "abc");
        old.inputs.title_path = "/somewhere/old".to_owned();
        store.write(&old).expect("write");
        store
            .write(&report(1_700_000_000_001, "other"))
            .expect("write");

        let mut current = report(1_700_000_000_002, "abc");
        current.inputs.title_path = "/somewhere/new".to_owned();
        store.write(&current).expect("write");

        let found = store
            .previous_for_title(&current)
            .expect("lookup")
            .expect("a previous run for this title exists");
        assert_eq!(found.run_id, old.run_id);
    }

    #[test]
    fn the_current_run_is_never_its_own_predecessor() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ReportStore::new(tmp.path());
        let current = report(1_700_000_000_000, "abc");
        store.write(&current).expect("write");
        assert!(
            store
                .previous_for_title(&current)
                .expect("lookup")
                .is_none(),
            "comparing a run against itself would report no change, forever"
        );
    }

    #[test]
    fn a_title_with_no_history_has_no_previous() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ReportStore::new(tmp.path());
        store
            .write(&report(1_700_000_000_000, "abc"))
            .expect("write");
        let fresh = report(1_700_000_000_001, "never-seen");
        assert!(store.previous_for_title(&fresh).expect("lookup").is_none());
    }

    #[test]
    fn the_most_recent_match_wins_not_the_oldest() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ReportStore::new(tmp.path());
        store
            .write(&report(1_700_000_000_000, "abc"))
            .expect("write");
        let middle = report(1_700_000_000_001, "abc");
        store.write(&middle).expect("write");

        let current = report(1_700_000_000_002, "abc");
        let found = store
            .previous_for_title(&current)
            .expect("lookup")
            .expect("some");
        assert_eq!(found.run_id, middle.run_id, "compare against the last run");
    }

    #[test]
    fn non_report_files_are_ignored() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("notes.txt"), b"hello").expect("write");
        let store = ReportStore::new(tmp.path());
        assert!(store.ids().expect("ids").is_empty());
    }
}
