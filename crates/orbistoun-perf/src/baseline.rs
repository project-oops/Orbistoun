//! A baseline build to measure against: the command line with the Tracy client, built from one
//! revision of the repository, so a change is compared with the commit before it rather than with
//! a binary that happened to be lying around.
//!
//! The revision is exported whole (`git archive`) into a directory of its own under `dir`, its
//! timestamps set to the export's so cargo builds what changed rather than trusting the commit's,
//! and its sibling path dependencies pointed at the repository's own siblings. Every revision
//! builds in one target directory, so a second baseline rebuilds only what differs. A revision built
//! once is reused.
//!
//! A candidate can be built the same way with some of the working tree's files laid over the
//! export ([`build_with`]): what is measured is then the revision and those files, not whatever
//! else the working tree holds - another session's unfinished work in a shared checkout included.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The command line's executable name on this host.
const CLI: &str = if cfg!(windows) {
    "orbistoun-cli.exe"
} else {
    "orbistoun-cli"
};

/// The executable built from `revision` of the repository `within` lies in, built under `dir`
/// unless it was before. The build takes its jobs from the environment (`CARGO_BUILD_JOBS`).
///
/// # Errors
///
/// When the revision does not name a commit, or exporting or building it fails.
pub fn build_at(within: &Path, revision: &str, dir: &Path) -> Result<PathBuf, String> {
    build(within, revision, &[], dir)
}

/// [`build_at`] with each of `files` - paths in the working tree, absolute or from `within` - laid
/// over the export as the working tree holds it now. Built again whenever their contents differ
/// from the build kept, which is kept apart from the revision's own.
///
/// # Errors
///
/// As [`build_at`], and when a file cannot be read or lies outside the repository.
pub fn build_with(
    within: &Path,
    revision: &str,
    files: &[PathBuf],
    dir: &Path,
) -> Result<PathBuf, String> {
    build(within, revision, files, dir)
}

fn build(within: &Path, revision: &str, files: &[PathBuf], dir: &Path) -> Result<PathBuf, String> {
    let repo = PathBuf::from(
        run(Command::new("git")
            .arg("-C")
            .arg(within)
            .args(["rev-parse", "--show-toplevel"]))?
        .trim(),
    );
    let repo = repo.as_path();
    let commit = run(Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify"])
        .arg(format!("{revision}^{{commit}}")))?;
    let commit = commit.trim().to_owned();
    let overlay = overlay(repo, within, files)?;
    let name = if overlay.is_empty() {
        commit.clone()
    } else {
        format!("{commit}-with-{:016x}", overlay_hash(&overlay))
    };
    let built = dir.join(&name).join(CLI);
    if built.is_file() {
        return Ok(built);
    }
    let source = dir.join(&name).join("source");
    let _ = std::fs::remove_dir_all(&source);
    std::fs::create_dir_all(&source).map_err(|e| format!("creating {}: {e}", source.display()))?;
    let archive = dir.join(&name).join("source.tar");
    run(Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["archive", "--format=tar", "-o"])
        .arg(&archive)
        .arg(&commit))?;
    // `-m`: the export's own time, not the commit's, or a target directory shared with a later
    // revision would take its sources for older than what it built from.
    run(Command::new("tar")
        .arg("-xmf")
        .arg(&archive)
        .arg("-C")
        .arg(&source))?;
    let _ = std::fs::remove_file(&archive);
    for (relative, bytes) in &overlay {
        let to = source.join(relative);
        std::fs::write(&to, bytes).map_err(|e| format!("writing {}: {e}", to.display()))?;
    }
    let manifest = source.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| format!("reading {}: {e}", manifest.display()))?;
    let siblings = repo
        .parent()
        .ok_or_else(|| format!("{} has no parent for its siblings", repo.display()))?;
    std::fs::write(&manifest, with_siblings_at(&text, siblings))
        .map_err(|e| format!("writing {}: {e}", manifest.display()))?;
    let target = dir.join("target");
    let mut cargo = Command::new("cargo");
    cargo
        .current_dir(&source)
        .args([
            "build",
            "--release",
            "-p",
            "orbistoun-cli",
            "--features",
            "tracy",
        ])
        .env("CARGO_TARGET_DIR", &target)
        // The build says which commit it is, not that it is dirty.
        .env("OOPS_COMMIT", &commit);
    run(&mut cargo)?;
    std::fs::copy(target.join("release").join(CLI), &built)
        .map_err(|e| format!("keeping {}: {e}", built.display()))?;
    let _ = std::fs::remove_dir_all(&source);
    Ok(built)
}

/// Each of `files` as the working tree holds it, by its path within `repo`.
fn overlay(
    repo: &Path,
    within: &Path,
    files: &[PathBuf],
) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let root = std::fs::canonicalize(repo).map_err(|e| format!("{}: {e}", repo.display()))?;
    files
        .iter()
        .map(|file| {
            let at = if file.is_absolute() {
                file.clone()
            } else {
                within.join(file)
            };
            let at = std::fs::canonicalize(&at).map_err(|e| format!("{}: {e}", at.display()))?;
            let relative = at
                .strip_prefix(&root)
                .map_err(|_| format!("{} is not in {}", at.display(), root.display()))?
                .to_path_buf();
            let bytes = std::fs::read(&at).map_err(|e| format!("reading {}: {e}", at.display()))?;
            Ok((relative, bytes))
        })
        .collect()
}

/// A hash of what an overlay lays down, naming the build made with it.
fn overlay_hash(overlay: &[(PathBuf, Vec<u8>)]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    overlay.hash(&mut hasher);
    hasher.finish()
}

/// A workspace manifest's `path = "../..."` dependencies pointed at `siblings`, the directory the
/// repository's own siblings lie in, since the export does not lie beside them.
#[must_use]
pub fn with_siblings_at(manifest: &str, siblings: &Path) -> String {
    let at = siblings.display().to_string().replace('\\', "/");
    manifest.replace("path = \"../", &format!("path = \"{at}/"))
}

/// Runs `command`, answering what it printed, or why it failed.
fn run(command: &mut Command) -> Result<String, String> {
    let out = command
        .output()
        .map_err(|e| format!("running {command:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{command:?} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::with_siblings_at;

    /// Only the sibling paths move: a dependency inside the workspace keeps its own path.
    #[test]
    fn sibling_paths_point_at_the_repository_s_siblings() {
        let manifest = concat!(
            "oops-log = { path = \"../oops-libs/crates/oops-log\" }\n",
            "orbistoun-core = { path = \"crates/orbistoun-core\" }\n",
        );
        let moved = with_siblings_at(manifest, std::path::Path::new("/work/OOPS"));
        assert!(moved.contains("path = \"/work/OOPS/oops-libs/crates/oops-log\""));
        assert!(moved.contains("path = \"crates/orbistoun-core\""));
    }

    /// A file laid over an export is found by its path within the repository, named from the
    /// working directory or absolutely, with what the working tree holds; one outside the repository
    /// is refused, and different contents name a different build.
    #[test]
    fn an_overlay_is_the_working_tree_s_files_by_their_place_in_the_repository() {
        let root = std::env::temp_dir().join(format!("orbistoun-overlay-{}", std::process::id()));
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join("crates")).expect("repo");
        std::fs::write(repo.join("crates").join("lib.rs"), b"one").expect("file");
        std::fs::write(root.join("outside.rs"), b"two").expect("outside");

        let laid = super::overlay(&repo, &repo, &[std::path::PathBuf::from("crates/lib.rs")])
            .expect("an overlay");
        assert_eq!(
            laid,
            vec![(
                std::path::Path::new("crates").join("lib.rs"),
                b"one".to_vec()
            )]
        );
        let absolute = super::overlay(&repo, &root, &[repo.join("crates").join("lib.rs")])
            .expect("an absolute path");
        assert_eq!(absolute, laid);
        assert!(super::overlay(&repo, &repo, &[root.join("outside.rs")]).is_err());

        std::fs::write(repo.join("crates").join("lib.rs"), b"changed").expect("rewrite");
        let changed = super::overlay(&repo, &repo, &[std::path::PathBuf::from("crates/lib.rs")])
            .expect("an overlay");
        assert_ne!(super::overlay_hash(&changed), super::overlay_hash(&laid));
        let _ = std::fs::remove_dir_all(&root);
    }
}
