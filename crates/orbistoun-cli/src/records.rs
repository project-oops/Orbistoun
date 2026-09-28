//! Where the compatibility records live: a directory per title, `compat/<title>/`, holding its
//! `report.toml` and, beside it, what reproduces and shows the run it records (D736).
//!
//! One place owns the layout, so `compat`, `status` and `submit` cannot read the tree differently.

use anyhow::{Context, Result};

/// The record itself: what Orbistoun sets for the title and what its runs reached.
pub(crate) const REPORT: &str = "report.toml";

/// The directory holding one title's record and everything beside it.
pub(crate) fn title_dir(dir: &std::path::Path, title: &str) -> std::path::PathBuf {
    dir.join(title)
}

/// Where one title's record lives.
pub(crate) fn report_path(dir: &std::path::Path, title: &str) -> std::path::PathBuf {
    title_dir(dir, title).join(REPORT)
}

/// Every title's record under `dir`, sorted by title, or [`None`] when `dir` does not exist.
///
/// A directory without a `report.toml` is not a title: it is skipped, never guessed at.
///
/// # Errors
///
/// When `dir` or a record cannot be read, or a record does not parse.
pub(crate) fn read_all(
    dir: &std::path::Path,
) -> Result<Option<Vec<(String, orbistoun_overrides::OverrideFile)>>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut records = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path().join(REPORT);
        if !path.is_file() {
            continue;
        }
        let title = entry.file_name().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let file = orbistoun_overrides::OverrideFile::from_toml(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        records.push((title, file));
    }
    records.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Some(records))
}

#[cfg(test)]
mod tests {
    /// A title is a directory holding a `report.toml`; anything else in `compat/` is skipped.
    #[test]
    fn a_title_is_a_directory_with_a_report() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let dir = root.path();
        assert!(
            super::read_all(&dir.join("absent"))
                .expect("reads")
                .is_none()
        );
        for (title, reach) in [("B0001", "entered"), ("A0001", "flipped")] {
            std::fs::create_dir_all(super::title_dir(dir, title)).expect("creates");
            std::fs::write(
                super::report_path(dir, title),
                format!("[status]\nreach = \"{reach}\"\noutcome = \"x\"\nimports = 1\n"),
            )
            .expect("writes");
        }
        std::fs::create_dir_all(dir.join("no-report")).expect("creates");
        std::fs::write(dir.join("README.md"), "not a title").expect("writes");
        let titles: Vec<String> = super::read_all(dir)
            .expect("reads")
            .expect("exists")
            .into_iter()
            .map(|(title, _)| title)
            .collect();
        assert_eq!(titles, ["A0001", "B0001"], "sorted, and only the two");
    }
}
