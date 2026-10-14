//! A baseline build to measure against: the command line with the Tracy client, built from one
//! revision of the repository, so a change is compared with the commit before it rather than with
//! a binary that happened to be lying around.
//!
//! The revision is exported whole (`git archive`) into a directory of its own under `dir`, its
//! timestamps set to the export's so cargo builds what changed rather than trusting the commit's,
//! and its sibling path dependencies pointed at the repository's own siblings. Every revision
//! builds in one target directory, so a second baseline rebuilds only what differs. A revision built
//! once is reused.

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
    let built = dir.join(&commit).join(CLI);
    if built.is_file() {
        return Ok(built);
    }
    let source = dir.join(&commit).join("source");
    let _ = std::fs::remove_dir_all(&source);
    std::fs::create_dir_all(&source).map_err(|e| format!("creating {}: {e}", source.display()))?;
    let archive = dir.join(&commit).join("source.tar");
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
}
