//! An orbistoun-aot build: a title's folder with a launcher beside its executable (D724).
//!
//! The launcher is orbistoun's own run path built as a program of its own. It runs the
//! `eboot.bin` in its folder the way `orbistoun-cli run` does, links it, and checks the plan
//! against the one stored for the title.

use std::path::{Path, PathBuf};

/// The file beside the launcher saying how to run the title it sits with.
pub const MANIFEST_FILE: &str = "orbistoun-aot.toml";

/// The executable a launcher runs, beside it.
pub const EXECUTABLE_FILE: &str = "eboot.bin";

/// The launcher as the build produces it, beside the CLI: the window, which plays the title it
/// finds a manifest beside.
pub const LAUNCHER_FILE: &str = if cfg!(windows) {
    "orbistoun-gui.exe"
} else {
    "orbistoun-gui"
};

/// The folder of the orbistoun-aot build this program is the launcher of, and its manifest.
#[must_use]
pub fn beside_this_program() -> Option<(PathBuf, Manifest)> {
    let folder = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let manifest = Manifest::read(&folder)?;
    Some((folder, manifest))
}

/// The one title an orbistoun-aot build in `folder` holds.
#[must_use]
pub fn title_entry(folder: &Path, manifest: &Manifest) -> crate::TitleEntry {
    crate::TitleEntry {
        name: manifest.title.clone(),
        module: folder.join(EXECUTABLE_FILE),
        metadata: crate::read_title_metadata(folder),
    }
}

/// How a launcher runs its title.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    /// The title's id, or its folder's name when it declares none.
    pub title: String,
    /// Whether the title came from the staging tree, so its `/app0` is writable (D722).
    #[serde(default)]
    pub staged: bool,
    /// The link plan the build was made with, for comparing against a run's.
    #[serde(default)]
    pub link_plan: String,
}

impl Manifest {
    /// The manifest for building the title whose executable is `executable`: its declared id, and
    /// staged when its folder lies directly under `staging` (D722).
    #[must_use]
    pub fn for_title(executable: &Path, staging: &Path, link_plan: String) -> Self {
        let source = executable.parent().unwrap_or(Path::new("."));
        let title = crate::read_title_metadata(source)
            .map(|meta| meta.title_id)
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| crate::linkplan::title_of(executable));
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        Self {
            title,
            staged: source.parent().map(canonical) == Some(canonical(staging)),
            link_plan,
        }
    }

    /// The manifest in `folder`, if one is there and readable.
    #[must_use]
    pub fn read(folder: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(folder.join(MANIFEST_FILE)).ok()?;
        toml::from_str(&text).ok()
    }

    /// Writes the manifest into `folder`.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn write(&self, folder: &Path) -> std::io::Result<()> {
        let text = toml::to_string(self).map_err(std::io::Error::other)?;
        std::fs::write(folder.join(MANIFEST_FILE), text)
    }
}

/// What a build wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// The launcher, named for the title.
    pub launcher: PathBuf,
    /// Files copied from the title's folder.
    pub files: usize,
    /// Bytes copied from it.
    pub bytes: u64,
}

/// Makes `out` an orbistoun-aot build of the title whose executable is `executable`: the title's
/// folder copied in, `launcher` beside it as `<name>.exe`, and the manifest. The build is portable:
/// everything its runs write stays beneath it, so it carries no state from the machine that made
/// it and leaves none on the one that plays it.
///
/// # Errors
///
/// When `out` lies inside the title's folder, or a file cannot be copied or written.
pub fn write_build(
    executable: &Path,
    out: &Path,
    launcher: &Path,
    name: &str,
    manifest: &Manifest,
) -> std::io::Result<Build> {
    let source = executable.parent().unwrap_or(Path::new("."));
    // Absolute rather than canonical: the build folder need not exist yet.
    let absolute = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    if absolute(out).starts_with(absolute(source)) {
        return Err(std::io::Error::other(format!(
            "{} lies inside the title's folder {}; a build goes beside it",
            out.display(),
            source.display()
        )));
    }
    std::fs::create_dir_all(out)?;
    let (files, bytes) = copy_tree(source, out)?;
    let named = out.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(launcher, &named)?;
    manifest.write(out)?;
    orbistoun_paths::enable_portable_sentinel(out)?;
    Ok(Build {
        launcher: named,
        files,
        bytes,
    })
}

/// Copies every file under `from` into `to`, keeping the layout; returns files and bytes copied.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<(usize, u64)> {
    let mut copied = (0, 0);
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            let (files, bytes) = copy_tree(&entry.path(), &target)?;
            copied = (copied.0 + files, copied.1 + bytes);
        } else {
            copied = (
                copied.0 + 1,
                copied.1 + std::fs::copy(entry.path(), &target)?,
            );
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::{Manifest, write_build};

    /// A title under the staging tree builds staged, and one elsewhere does not.
    #[test]
    fn a_staged_title_builds_staged() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("data").join("homebrew");
        let staged = staging.join("NVRB00001");
        let image = dir.path().join("PPSA00000-app0");
        for folder in [&staged, &image] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let built = Manifest::for_title(&staged.join("eboot.bin"), &staging, String::new());
        assert_eq!((built.title.as_str(), built.staged), ("NVRB00001", true));
        let built = Manifest::for_title(&image.join("eboot.bin"), &staging, String::new());
        assert_eq!(
            (built.title.as_str(), built.staged),
            ("PPSA00000-app0", false)
        );
    }

    /// A build holds the title's files, the launcher named for the title and the manifest, and a
    /// build inside the title's own folder is refused.
    #[test]
    fn a_build_is_the_title_folder_with_a_launcher_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let title = dir.path().join("NVRB00001");
        std::fs::create_dir_all(title.join("data")).unwrap();
        std::fs::write(title.join("eboot.bin"), b"elf").unwrap();
        std::fs::write(title.join("data").join("level.sol"), b"level").unwrap();
        let launcher = dir.path().join("launcher.exe");
        std::fs::write(&launcher, b"launcher").unwrap();
        let manifest = Manifest {
            title: "NVRB00001".to_owned(),
            staged: true,
            link_plan: "0123456789abcdef".to_owned(),
        };

        let out = dir.path().join("out");
        let build = write_build(
            &title.join("eboot.bin"),
            &out,
            &launcher,
            "neverball",
            &manifest,
        )
        .unwrap();
        assert_eq!((build.files, build.bytes), (2, 8));
        assert_eq!(std::fs::read(&build.launcher).unwrap(), b"launcher");
        assert_eq!(
            std::fs::read(out.join("data").join("level.sol")).unwrap(),
            b"level"
        );
        assert_eq!(Manifest::read(&out), Some(manifest.clone()));
        assert!(out.join(".portable").is_dir(), "a build keeps its data beside it");

        let inside = title.join("build");
        assert!(write_build(&title.join("eboot.bin"), &inside, &launcher, "x", &manifest).is_err());
        assert!(!inside.exists(), "a refused build leaves nothing behind");
    }
}
