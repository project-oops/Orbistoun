//! Retention: artifacts are a dev cycle, not an archive.
//!
//! Two guards, because either alone fails:
//!
//! - **Age.** [`DEFAULT_MAX_AGE_HOURS`] hours. Enough to compare against yesterday,
//!   not enough to accumulate.
//! - **Size.** An agent doing hundreds of runs with traces enabled can breach a disk
//!   budget well inside the age window, so the byte cap is the one that actually
//!   fires in the case that matters.
//!
//! Purging is oldest-first, which relies on [`crate::RunId`] sorting chronologically
//! as a string - so a filename listing is already in age order.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Age after which an artifact is purged.
pub const DEFAULT_MAX_AGE_HOURS: u64 = 72;

/// Byte budget across all artifacts in one directory.
///
/// Conservative on purpose: it is a guard against a runaway loop filling a disk, not
/// a tuned figure. Adjust once real traces exist and their size is known rather than
/// guessed.
pub const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// What a purge did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// Files removed.
    pub removed: usize,
    /// Bytes reclaimed.
    pub bytes_freed: u64,
    /// Files left behind.
    pub retained: usize,
    /// Bytes still on disk.
    pub bytes_retained: u64,
}

/// Limits for one purge.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Maximum age.
    pub max_age: Duration,
    /// Maximum total bytes after purging.
    pub max_bytes: u64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_age: Duration::from_secs(DEFAULT_MAX_AGE_HOURS * 60 * 60),
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// One file considered for purging.
#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

/// Removes artifacts older than the policy allows, then oldest-first until the byte
/// budget is met.
///
/// A missing directory is success with nothing done, not an error: purging runs on
/// startup, and a first run has nothing to purge.
pub fn purge(dir: &Path, policy: Policy, now: SystemTime) -> io::Result<PurgeReport> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(PurgeReport::default()),
        Err(e) => return Err(e),
    };

    let mut candidates: Vec<Candidate> = Vec::new();
    for entry in entries {
        let entry = entry?;
        let meta = entry.metadata()?;
        if !meta.is_file() {
            continue;
        }
        candidates.push(Candidate {
            path: entry.path(),
            bytes: meta.len(),
            modified: meta.modified().unwrap_or(now),
        });
    }
    // Oldest first, so both passes below remove the least useful thing next.
    candidates.sort_by_key(|c| c.modified);

    let mut report = PurgeReport::default();
    let mut keep: Vec<Candidate> = Vec::new();

    for c in candidates {
        let too_old = now
            .duration_since(c.modified)
            .is_ok_and(|age| age > policy.max_age);
        if too_old {
            fs::remove_file(&c.path)?;
            report.removed += 1;
            report.bytes_freed += c.bytes;
        } else {
            keep.push(c);
        }
    }

    let mut total: u64 = keep.iter().map(|c| c.bytes).sum();
    let mut index = 0;
    while total > policy.max_bytes && index < keep.len() {
        let c = &keep[index];
        fs::remove_file(&c.path)?;
        report.removed += 1;
        report.bytes_freed += c.bytes;
        total -= c.bytes;
        index += 1;
    }

    report.retained = keep.len() - index;
    report.bytes_retained = total;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::{Policy, PurgeReport, purge};
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    fn write(dir: &Path, name: &str, bytes: usize) {
        fs::write(dir.join(name), vec![b'x'; bytes]).expect("write");
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        // Purging runs on startup and a first run has nothing to purge.
        let r = purge(
            Path::new("definitely/not/here"),
            Policy::default(),
            SystemTime::now(),
        )
        .expect("missing dir is fine");
        assert_eq!(r, PurgeReport::default());
    }

    #[test]
    fn recent_files_survive() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), "recent.json", 10);
        let r = purge(tmp.path(), Policy::default(), SystemTime::now()).expect("purge");
        assert_eq!(r.removed, 0);
        assert_eq!(r.retained, 1);
    }

    #[test]
    fn old_files_are_removed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), "old.json", 10);
        // Ask from far enough in the future that the file is past the age limit.
        let later = SystemTime::now() + Duration::from_secs(100 * 60 * 60);
        let r = purge(tmp.path(), Policy::default(), later).expect("purge");
        assert_eq!(r.removed, 1);
        assert_eq!(r.bytes_freed, 10);
        assert!(!tmp.path().join("old.json").exists());
    }

    #[test]
    fn the_byte_budget_fires_even_when_nothing_is_old() {
        // The case that actually matters: an agent doing hundreds of runs breaches a
        // disk budget well inside the age window.
        let tmp = tempfile::tempdir().expect("tempdir");
        for i in 0..5 {
            write(tmp.path(), &format!("run-{i}.json"), 100);
        }
        let policy = Policy {
            max_age: Duration::from_secs(u64::from(u32::MAX)),
            max_bytes: 250,
        };
        let r = purge(tmp.path(), policy, SystemTime::now()).expect("purge");
        assert!(r.bytes_retained <= 250, "budget must be met");
        assert!(r.removed >= 3, "removed {} files", r.removed);
    }

    #[test]
    fn directories_are_left_alone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(tmp.path().join("subdir")).expect("mkdir");
        let later = SystemTime::now() + Duration::from_secs(100 * 60 * 60);
        purge(tmp.path(), Policy::default(), later).expect("purge");
        assert!(
            tmp.path().join("subdir").is_dir(),
            "purging must not recurse into or remove directories"
        );
    }

    #[test]
    fn nothing_is_removed_when_both_limits_are_satisfied() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), "a.json", 10);
        write(tmp.path(), "b.json", 10);
        let r = purge(tmp.path(), Policy::default(), SystemTime::now()).expect("purge");
        assert_eq!(r.removed, 0);
        assert_eq!(r.retained, 2);
        assert_eq!(r.bytes_retained, 20);
    }
}
