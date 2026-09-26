//! Portable-first path resolution (D038).
//!
//! Orbistoun never writes outside its resolved root: every writable location comes from [`Paths`].
//! The root is, in order: `./.portable/` beside the binary when portable mode is triggered
//! ([`ENV_PORTABLE`] truthy, a binary stem containing `portable`, or a `.portable` directory beside
//! the binary); [`ENV_DATA_DIR`] if set; otherwise the collection's shared directory from
//! `oops_paths`. Portable outranks the override so containment holds. Rebuildable bulk goes to the
//! cache root instead (see `cache_root`). The sentinel is a directory because it is also the root.
//! [`Paths::resolve_with`] takes its inputs, so resolution is testable.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Env var that forces portable mode when set to a truthy value.
///
/// Named by `orbistoun-env`, the one list of what this project reads from the environment (D221).
pub const ENV_PORTABLE: &str = orbistoun_env::PORTABLE_MODE.name;
/// Env var that relocates the data root. Ignored in portable mode.
pub const ENV_DATA_DIR: &str = orbistoun_env::DATA_DIR.name;
/// Directory beside the binary that is both the portable root and its own sentinel.
pub const PORTABLE_DIR: &str = ".portable";
/// Explanatory note written inside the portable root so it is not a mystery folder.
///
/// `oops-paths` owns the filename; the body is orbistoun's (`PORTABLE_NOTE_BODY`) because it names
/// this tool.
pub use oops_paths::PORTABLE_NOTE;
/// Application name used for the OS-standard data directory.
pub const APP_NAME: &str = "orbistoun";

/// Subdirectory names under the data root, named once.
pub mod dirs {
    /// Developer logs.
    pub const LOGS: &str = "logs";
    /// Binary guest-call traces.
    pub const TRACES: &str = "traces";
    /// Machine-readable run reports.
    pub const REPORTS: &str = "reports";
    /// User-supplied per-title overrides.
    pub const OVERRIDES: &str = "overrides";
    /// Window captures taken from the toolbar.
    pub const SCREENSHOTS: &str = "screenshots";
    /// The platform's own filesystem, materialised from the manifest that describes it.
    ///
    /// Derived and never edited by hand, so it can be deleted and rebuilt. Nothing a guest writes
    /// lands here (D251).
    pub const FILESYSTEM: &str = "filesystem";
    /// One directory per title, holding its guest filesystem and anything else known about it.
    ///
    /// A guest's writes land in its overlay under here and are merged over the base tree in process
    /// (D251). Prosperous uses the same directory.
    pub const TITLES: &str = "titles";
    /// Raw executables run directly, rather than installed titles.
    ///
    /// Separate from [`TITLES`]: a title is a directory with a `param.json`, an `eboot.bin` and its
    /// own filesystem, and a payload is one ELF (D661).
    pub const PAYLOADS: &str = "payloads";
    /// The platform's own writable storage as a system application sees it: one tree shared by
    /// everything run with the system filesystem view, rather than one sandbox per title.
    pub const CONSOLE: &str = "console";
    /// Installable packages, before anything installs them. An installed package becomes a
    /// directory under [`TITLES`].
    pub const PACKAGES: &str = "packages";
}

/// Filename of the instance-wide settings file under the data root.
pub const CONFIG_FILE: &str = "config.toml";
/// Policy the loop worked out for itself, kept apart from what a person configured.
///
/// Deleting it is a complete undo, a diff separates the loop's guesses from a person's decisions,
/// and an entry in `config.toml` wins (D296).
pub const LEARNED_FILE: &str = "learned.toml";

/// What the emulated machine is set to, as a person set it.
///
/// Apart from `config.toml`, which configures the emulator, so the machine settings can be carried
/// between installations on their own.
pub const SHELL_FILE: &str = "shell.toml";

/// The environment as resolution sees it, captured so a test can supply one directly.
#[derive(Debug, Clone, Default)]
pub struct EnvSnapshot {
    /// Whether [`ENV_PORTABLE`] was set truthy.
    pub portable_flag: bool,
    /// Value of [`ENV_DATA_DIR`], if set.
    pub data_dir: Option<PathBuf>,
}

impl EnvSnapshot {
    /// Reads the real process environment.
    pub fn from_process() -> Self {
        Self {
            portable_flag: env::var(ENV_PORTABLE).is_ok_and(|v| is_truthy(&v)),
            data_dir: env::var_os(ENV_DATA_DIR)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
        }
    }
}

/// Whether an environment value counts as "on".
///
/// Narrow and case-insensitive: an unrecognised value is not truthy, so
/// `ORBISTOUN_PORTABLE_MODE=no` does not turn portable mode on.
fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Where homebrew is staged, as path components: `/data/homebrew` (D722). Inside the titles library
/// it is the same components, so a staged title's host path mirrors its guest one.
pub const STAGING: [&str; 2] = ["data", "homebrew"];

/// The staging root under a titles library: [`STAGING`] joined on. What
/// [`Paths::staged_titles_dir`] answers, for a caller holding only a library root.
#[must_use]
pub fn staged_under(titles: &Path) -> PathBuf {
    STAGING
        .iter()
        .fold(titles.to_path_buf(), |path, part| path.join(part))
}

/// A resolved, confined set of writable locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    data_root: PathBuf,
    /// The collection's answer, so the shared locations are its to define.
    shared: oops_paths::Paths,
    portable: bool,
}

impl Paths {
    /// Resolves for the current process, reading the environment and the filesystem
    /// beside the binary.
    pub fn resolve() -> Self {
        let exe = env::current_exe().ok();
        let binary_dir = exe.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        let binary_name = exe
            .as_deref()
            .and_then(Path::file_stem)
            .and_then(|s| s.to_str())
            .map(str::to_owned);
        Self::resolve_with(
            &EnvSnapshot::from_process(),
            binary_dir.as_deref(),
            binary_name.as_deref(),
        )
    }

    /// Resolution core, parameterised on its inputs.
    ///
    /// `binary_dir` is where the executable lives, used for the portable sentinel and
    /// root; `None` where it cannot be determined, in which case the sentinel check is
    /// skipped and portable mode can still be forced by env, rooted at the current
    /// directory. `binary_name` is the executable's file stem.
    pub fn resolve_with(
        env: &EnvSnapshot,
        binary_dir: Option<&Path>,
        binary_name: Option<&str>,
    ) -> Self {
        // The shared crate supplies sentinel detection, the name check, precedence and the platform
        // root. Start from what it reads about this machine and override only what this project
        // answers itself.
        let mut process = oops_paths::Process::read(APP_NAME);
        // This project declares every environment variable it reads, so the names are
        // `ORBISTOUN_PORTABLE_MODE` and `ORBISTOUN_DATA_DIR` rather than the `<APP>_PORTABLE` /
        // `<APP>_DATA_DIR` the shared crate derives. Its answers replace them.
        process.env = oops_paths::EnvSnapshot {
            portable_flag: env.portable_flag,
            data_dir: env.data_dir.clone(),
        };
        process.binary_dir = binary_dir.map(Path::to_path_buf);
        process.binary_name = binary_name.map(str::to_owned);

        let (shared, _) = oops_paths::Paths::resolve_found(
            APP_NAME,
            // The same layout as every sibling, because they share a root: the platform's own data
            // directory.
            oops_paths::Layout::default(),
            &process,
        );
        // The flag is ignored: this crate always answers with a root, falling back to a visible
        // directory rather than panicking.
        Self {
            data_root: shared.data_root().to_path_buf(),
            portable: shared.is_portable(),
            shared,
        }
    }

    /// Whether this run is confined beside its binary.
    pub const fn is_portable(&self) -> bool {
        self.portable
    }

    /// Where material that can be rebuilt goes.
    ///
    /// The local (non-roaming) collection directory beside `data_root`, and the same directory in a
    /// portable run. Anything recoverable without the hardware (models, runtimes, compiled shaders,
    /// the materialised filesystem, traces) is cache; a report measured on hardware or an override
    /// somebody typed stays with the data.
    #[must_use]
    pub fn cache_root(&self) -> &Path {
        self.shared.cache_root()
    }

    /// Where downloaded model weights go.
    #[must_use]
    pub fn models_dir(&self) -> PathBuf {
        self.cache_root().join("models")
    }

    /// Where downloaded runtimes go.
    #[must_use]
    pub fn runtime_dir(&self) -> PathBuf {
        self.cache_root().join("runtime")
    }

    /// Where compiled shader material is cached.
    #[must_use]
    pub fn shaders_dir(&self) -> PathBuf {
        self.cache_root().join("shaders")
    }

    /// The root everything else hangs off.
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    /// Developer logs.
    pub fn logs_dir(&self) -> PathBuf {
        self.cache_root().join(dirs::LOGS)
    }

    /// Binary guest-call traces.
    pub fn traces_dir(&self) -> PathBuf {
        // A trace is one run's record, rewritten by the next run, so it is cache.
        self.cache_root().join(dirs::TRACES)
    }

    /// Machine-readable run reports.
    pub fn reports_dir(&self) -> PathBuf {
        self.data_root.join(dirs::REPORTS)
    }

    /// User-supplied per-title overrides.
    pub fn overrides_dir(&self) -> PathBuf {
        self.data_root.join(dirs::OVERRIDES)
    }

    /// Window captures taken from the toolbar.
    ///
    /// Resolved here like everything else orbistoun writes, so portable mode moves it too.
    pub fn screenshots_dir(&self) -> PathBuf {
        self.data_root.join(dirs::SCREENSHOTS)
    }

    /// The base filesystem, as materialised from the manifest.
    pub fn filesystem_dir(&self) -> PathBuf {
        // Materialised from the manifest and rebuildable, so it is cache.
        self.cache_root().join(dirs::FILESYSTEM)
    }

    /// The root every title's own data lives under.
    pub fn titles_dir(&self) -> PathBuf {
        self.data_root.join(dirs::TITLES)
    }

    /// Where homebrew is staged inside the titles library: `/data/homebrew/<id>`, one directory per
    /// title (D722). A title launched from here has a writable `/app0`.
    pub fn staged_titles_dir(&self) -> PathBuf {
        staged_under(&self.titles_dir())
    }

    /// Where raw executables live, run directly rather than installed.
    pub fn payloads_dir(&self) -> PathBuf {
        self.data_root.join(dirs::PAYLOADS)
    }

    /// Where installable packages wait, before anything installs them.
    pub fn packages_dir(&self) -> PathBuf {
        self.data_root.join(dirs::PACKAGES)
    }

    /// One title's overlay, merged over the base tree while it runs.
    ///
    /// Under the data root rather than beside the module, so a guest cannot write into the material
    /// being measured (D250).
    pub fn title_overlay_dir(&self, title: &str) -> PathBuf {
        // The overlay is keyed by the guest's path, so a file the title writes to
        // `/user/home/<user>/savedata_prospero/<id>/x` lands at that path here. Prosperous reads
        // the hardware at that same path, so a save pulled off hardware is this directory
        // unchanged.
        self.shared.title_dir(title).join("fs")
    }

    /// The overlay a title run with the system filesystem view writes into.
    ///
    /// One for the whole machine, as a system application has: a launcher's settings under `/data`
    /// belong to the device, not to a per-title sandbox.
    pub fn console_overlay_dir(&self) -> PathBuf {
        self.data_root.join(dirs::CONSOLE)
    }

    /// Where one title's save states are kept.
    pub fn title_savestates_dir(&self, title: &str) -> PathBuf {
        // Beside the guest filesystem, under the same title, so everything known about one title is
        // one directory.
        self.shared.title_dir(title).join("savestates")
    }

    /// Where the loop writes policy it worked out for itself.
    ///
    /// Kept apart from [`Self::config_file`] so deleting it is a complete undo and a diff
    /// shows the loop's guesses separately from a person's decisions (D296).
    pub fn learned_file(&self) -> PathBuf {
        self.data_root.join(LEARNED_FILE)
    }

    /// Instance-wide settings file.
    pub fn config_file(&self) -> PathBuf {
        self.data_root.join(CONFIG_FILE)
    }

    /// What the machine is set to, as a person set it.
    ///
    /// Its own file rather than a section of [`Self::config_file`], which configures the emulator,
    /// so shell settings can be copied between installations without a call budget.
    pub fn shell_file(&self) -> PathBuf {
        self.data_root.join(SHELL_FILE)
    }

    /// Every directory this crate hands out, with the name it goes by.
    ///
    /// The only list: [`Self::all_dirs`] and the `paths` command both read it.
    pub fn named_dirs(&self) -> Vec<(&'static str, PathBuf)> {
        vec![
            (dirs::LOGS, self.logs_dir()),
            (dirs::TRACES, self.traces_dir()),
            (dirs::REPORTS, self.reports_dir()),
            (dirs::OVERRIDES, self.overrides_dir()),
            (dirs::SCREENSHOTS, self.screenshots_dir()),
            (dirs::FILESYSTEM, self.filesystem_dir()),
            (dirs::TITLES, self.titles_dir()),
            (dirs::PAYLOADS, self.payloads_dir()),
            (dirs::PACKAGES, self.packages_dir()),
            (dirs::CONSOLE, self.console_overlay_dir()),
        ]
    }

    /// Every directory this crate hands out, for iteration and for the containment
    /// test. Adding a new writable location means adding it to [`Self::named_dirs`].
    pub fn all_dirs(&self) -> Vec<PathBuf> {
        self.named_dirs().into_iter().map(|(_, dir)| dir).collect()
    }

    /// Creates every directory, including the root.
    pub fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(&self.data_root)?;
        for d in self.all_dirs() {
            fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

/// Makes portable mode sticky for the binary in `binary_dir`.
///
/// Delegates to `oops-paths`, which materialises the `.portable` directory and replaces a stale
/// `.portable` file, over which `create_dir_all` would fail. The note body is orbistoun's and is
/// passed in; `None` writes no note.
pub fn enable_portable_sentinel(binary_dir: &Path) -> io::Result<()> {
    oops_paths::enable_portable_sentinel(binary_dir, Some(PORTABLE_NOTE_BODY))
}

/// What the note beside the sentinel says. Here because it names orbistoun.
const PORTABLE_NOTE_BODY: &str = concat!(
    "This directory makes orbistoun run in portable mode.\n\n",
    "Everything orbistoun writes - logs, traces, run reports, settings, per-title\n",
    "overrides - stays beneath this directory. Nothing is written anywhere else.\n\n",
    "Delete this directory to return to the OS-standard data location.\n"
);

#[cfg(test)]
mod tests {
    use super::{
        ENV_PORTABLE, EnvSnapshot, PORTABLE_DIR, PORTABLE_NOTE, Paths, enable_portable_sentinel,
        is_truthy,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    fn env_portable() -> EnvSnapshot {
        EnvSnapshot {
            portable_flag: true,
            data_dir: None,
        }
    }

    /// Only the recognised values count as on, in any case.
    #[test]
    fn truthy_values_are_narrow_and_case_insensitive() {
        for v in ["1", "true", "TRUE", "Yes", " on "] {
            assert!(is_truthy(v), "{v:?} should be truthy");
        }
        // Anything unrecognised is off.
        for v in ["0", "false", "no", "off", "", "maybe", "portable"] {
            assert!(!is_truthy(v), "{v:?} should not be truthy");
        }
    }

    /// The environment flag triggers portable mode.
    #[test]
    fn env_var_triggers_portable() {
        let p = Paths::resolve_with(
            &env_portable(),
            Some(Path::new("/opt/app")),
            Some("orbistoun"),
        );
        assert!(p.is_portable());
        assert_eq!(p.data_root(), Path::new("/opt/app").join(PORTABLE_DIR));
        // The sentinel directory is the root.
        assert_eq!(p.data_root(), Path::new("/opt/app").join(PORTABLE_DIR));
    }

    /// A binary stem containing `portable` triggers it alone.
    #[test]
    fn filename_containing_portable_triggers_it_with_no_sentinel_or_env() {
        // A single-file download needs no setup.
        let p = Paths::resolve_with(
            &EnvSnapshot::default(),
            Some(Path::new("/downloads")),
            Some("orbistoun-portable-gui"),
        );
        assert!(p.is_portable());
    }

    /// The stem match ignores case.
    #[test]
    fn filename_match_is_case_insensitive() {
        let p = Paths::resolve_with(
            &EnvSnapshot::default(),
            Some(Path::new("/downloads")),
            Some("Orbistoun-PORTABLE-GUI"),
        );
        assert!(p.is_portable());
    }

    /// A plain binary is not portable.
    #[test]
    fn plain_binary_is_not_portable() {
        let p = Paths::resolve_with(
            &EnvSnapshot::default(),
            Some(Path::new("/usr/bin")),
            Some("orbistoun-cli"),
        );
        assert!(
            !p.is_portable(),
            "portable must be opt-in, never the default"
        );
    }

    /// A sentinel directory beside the binary triggers portable mode.
    #[test]
    fn sentinel_directory_beside_the_binary_triggers_portable() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(tmp.path().join(PORTABLE_DIR)).expect("create sentinel");

        let p = Paths::resolve_with(
            &EnvSnapshot::default(),
            Some(tmp.path()),
            Some("orbistoun-cli"),
        );
        assert!(
            p.is_portable(),
            "the .portable directory is itself the sentinel"
        );
    }

    /// Portable mode outranks the data-dir override.
    #[test]
    fn portable_outranks_the_data_dir_override() {
        // An env var must not escape the portable root.
        let env = EnvSnapshot {
            portable_flag: true,
            data_dir: Some(PathBuf::from("/somewhere/else")),
        };
        let p = Paths::resolve_with(&env, Some(Path::new("/opt/app")), Some("orbistoun-cli"));
        assert!(p.is_portable());
        assert!(p.data_root().starts_with("/opt/app"));
    }

    /// The data-dir override applies when not portable.
    #[test]
    fn data_dir_override_applies_when_not_portable() {
        let env = EnvSnapshot {
            portable_flag: false,
            data_dir: Some(PathBuf::from("/var/lib/orbistoun")),
        };
        let p = Paths::resolve_with(&env, Some(Path::new("/usr/bin")), Some("orbistoun-cli"));
        assert!(!p.is_portable());
        assert_eq!(p.data_root(), Path::new("/var/lib/orbistoun"));
    }

    /// The environment flag works with no known binary location.
    #[test]
    fn unknown_binary_location_still_honours_the_env_flag() {
        let p = Paths::resolve_with(&env_portable(), None, None);
        assert!(p.is_portable());
        assert_eq!(p.data_root(), Path::new(".").join(PORTABLE_DIR));
    }

    /// Enabling the sentinel replaces a stale `.portable` file.
    #[test]
    fn enabling_the_sentinel_heals_a_stale_file_from_the_old_scheme() {
        // Sentinel and data root are the same path, so a `.portable` file makes `create_dir_all`
        // fail.
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join(PORTABLE_DIR), b"stale marker").expect("write stale file");

        enable_portable_sentinel(tmp.path()).expect("should heal, not fail");

        let sentinel = tmp.path().join(PORTABLE_DIR);
        assert!(sentinel.is_dir(), "sentinel must end up a directory");
        assert!(
            sentinel.join(PORTABLE_NOTE).is_file(),
            "note explains itself"
        );

        // Creating the tree succeeds.
        let p = Paths::resolve_with(
            &EnvSnapshot::default(),
            Some(tmp.path()),
            Some("orbistoun-cli"),
        );
        p.ensure_dirs()
            .expect("first run must not fail over the sentinel");
    }

    /// Portable mode writes nothing outside its root (D038).
    #[test]
    fn portable_mode_writes_nothing_outside_its_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // A marker file beside the root, to prove the walk below can see siblings.
        fs::write(tmp.path().join("sibling.txt"), b"untouched").expect("write sibling");

        let p = Paths::resolve_with(&env_portable(), Some(tmp.path()), Some("orbistoun-cli"));
        p.ensure_dirs().expect("ensure_dirs");

        // Write through every location the API hands out.
        for d in p.all_dirs() {
            fs::write(d.join("probe"), b"x").expect("write probe");
        }
        fs::write(p.config_file(), b"x").expect("write config");

        let root = tmp.path().join(PORTABLE_DIR);
        let mut outside = Vec::new();
        for entry in fs::read_dir(tmp.path()).expect("read tmp") {
            let path = entry.expect("entry").path();
            if path != root && path != tmp.path().join("sibling.txt") {
                outside.push(path);
            }
        }
        assert!(
            outside.is_empty(),
            "portable mode wrote outside its root: {outside:?}"
        );

        // And everything it did write is genuinely beneath the root.
        for d in p.all_dirs() {
            assert!(d.starts_with(&root), "{d:?} escaped the portable root");
        }
        assert!(p.config_file().starts_with(&root));
    }

    /// Every writable location is listed in `all_dirs`.
    #[test]
    fn every_writable_location_is_listed_in_all_dirs() {
        // `all_dirs` drives the containment test, so a location missing from it would be
        // unverified.
        let p = Paths::resolve_with(&env_portable(), Some(Path::new("/opt/app")), Some("x"));
        let all = p.all_dirs();
        for d in [
            p.logs_dir(),
            p.traces_dir(),
            p.reports_dir(),
            p.overrides_dir(),
            p.screenshots_dir(),
            p.filesystem_dir(),
            p.titles_dir(),
            p.payloads_dir(),
            p.packages_dir(),
            p.console_overlay_dir(),
        ] {
            assert!(all.contains(&d), "{d:?} missing from all_dirs()");
        }
        assert_eq!(
            all.len(),
            10,
            "a location was added without updating the test"
        );
        // The three library roots are distinct siblings (D661).
        for (a, b) in [
            (p.titles_dir(), p.payloads_dir()),
            (p.titles_dir(), p.packages_dir()),
            (p.payloads_dir(), p.packages_dir()),
        ] {
            assert_ne!(a, b, "the library roots must be distinct");
            assert!(
                !a.starts_with(&b) && !b.starts_with(&a),
                "{a:?} and {b:?} must be siblings"
            );
        }

        // Everything about one title sits under one directory: the guest filesystem and the
        // savestates.
        for under in [
            p.title_overlay_dir("PPSA00000"),
            p.title_savestates_dir("PPSA00000"),
        ] {
            assert!(
                under.starts_with(p.titles_dir().join("PPSA00000")),
                "{under:?} is not under its own title"
            );
        }
    }

    /// Reading the process environment does not panic.
    #[test]
    fn env_snapshot_reads_the_process_without_panicking() {
        // Smoke test for the real-world path; the value depends on the ambient environment.
        let snap = EnvSnapshot::from_process();
        let _ = snap.portable_flag;
        assert_eq!(ENV_PORTABLE, "ORBISTOUN_PORTABLE_MODE");
    }
}
