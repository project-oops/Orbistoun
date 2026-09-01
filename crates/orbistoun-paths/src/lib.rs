//! Portable-first path resolution.
//!
//! One rule governs this crate: **orbistoun never writes outside its own resolved
//! root.** Every writable location - logs, traces, run reports, overrides, config -
//! comes from [`Paths`], which picks a root by explicit precedence and confines
//! everything beneath it.
//!
//! # Precedence
//!
//! 1. **Portable**, if any trigger fires. Root is `./.portable/` beside the binary.
//!    Nothing is written anywhere else, traces included - an exception is what makes
//!    "does not touch outside its own directory" false (D038).
//! 2. **[`ENV_DATA_DIR`]**, if set. An explicit relocation for anyone who wants one.
//! 3. **The collection's directory**, via `oops_paths` - `%APPDATA%\OOPS\` on Windows, and
//!    shared with every sibling rather than a directory of this project's own. That is what
//!    lets a save Prosperous pulls off real hardware be the tree a title's overlay mounts.
//!
//! Bulk that can be rebuilt - models, runtimes, compiled shaders, the base filesystem, traces -
//! goes to `%LOCALAPPDATA%\OOPS\` instead, so a roaming profile does not carry gigabytes it
//! could fetch again. See `cache_root`.
//!
//! Portable deliberately outranks the environment override: if an env var could
//! escape the portable root, the containment guarantee would be a suggestion.
//!
//! # Portable triggers
//!
//! Any of these, OR'd:
//!
//! - [`ENV_PORTABLE`] set to a truthy value
//! - the binary's filename stem contains `portable`, case-insensitively - this is what
//!   makes a single-file download work with no setup at all
//! - a `.portable` **directory** beside the binary
//!
//! # Why the sentinel is a directory and never a file
//!
//! The sentinel and the data root are the same path. A sibling project wrote a
//! `.portable` *file* as the marker, then failed on first run when `create_dir_all`
//! tried to make a directory over it. The directory's own existence is the sentinel -
//! `exists()` is true for a directory - so a portable install is sticky with no
//! sidecar. [`enable_portable_sentinel`] heals a stale file left by that scheme.
//!
//! # Testability
//!
//! [`Paths::resolve_with`] takes its inputs rather than reading the world, so
//! resolution is fully testable without touching real environment variables or the
//! real binary location (D016).

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Env var that forces portable mode when set to a truthy value.
///
/// Named by `orbistoun-env` rather than here, so the one list of what this project reads
/// from the environment cannot disagree with what it actually reads (D221).
pub const ENV_PORTABLE: &str = orbistoun_env::PORTABLE_MODE.name;
/// Env var that relocates the data root. Ignored in portable mode.
pub const ENV_DATA_DIR: &str = orbistoun_env::DATA_DIR.name;
/// Directory beside the binary that is both the portable root and its own sentinel.
pub const PORTABLE_DIR: &str = ".portable";
/// Explanatory note written inside the portable root so it is not a mystery folder.
pub const PORTABLE_NOTE: &str = "PORTABLE.txt";
/// Application name used for the OS-standard data directory.
pub const APP_NAME: &str = "orbistoun";

/// Subdirectory names under the data root. Named once so nothing hardcodes a string
/// twice (CLAUDE.md principle 12).
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
    /// The console's own filesystem, materialised from the manifest that describes it.
    ///
    /// Derived, never edited by hand: it can be deleted and rebuilt at any time, which is
    /// the test that it really is derived. Nothing a guest writes lands here (D251).
    pub const FILESYSTEM: &str = "filesystem";
    /// Everything one title owns, keyed by the title.
    ///
    /// Keyed by title first and by category second, so a title is one directory to back
    /// up, move or delete. A guest's writes land in its overlay under here and are merged
    /// over the base tree in process rather than on disk (D251).
    /// One directory per title, holding its guest filesystem and anything else known about
    /// it. Named as prosperous already named its own, because they are now the same directory.
    pub const TITLES: &str = "titles";
}

/// Filename of the instance-wide settings file under the data root.
pub const CONFIG_FILE: &str = "config.toml";
/// Policy the loop worked out for itself, kept apart from what a person configured.
///
/// **A separate file for three reasons, each of which is the reason.** Deleting it is a
/// complete undo; a diff shows the loop's guesses and a person's decisions separately; and an
/// entry in `config.toml` wins, so nothing written here can quietly override a deliberate
/// choice (D296).
pub const LEARNED_FILE: &str = "learned.toml";

/// What the emulated console is set to, as a person set it.
///
/// Apart from `config.toml` because that holds how the *emulator* is configured and this
/// holds what the machine it presents is set to. Different decisions, made at different
/// times by different reasoning - a call budget is a debugging choice and a language is
/// not - so this file can be carried between installations on its own.
pub const SHELL_FILE: &str = "shell.toml";

/// The environment as resolution sees it.
///
/// Captured rather than read at each decision point, so a test can supply one
/// directly.
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
/// Deliberately narrow and case-insensitive. An unrecognised value is *not* truthy,
/// because silently treating `ORBISTOUN_PORTABLE_MODE=no` as on would be exactly the
/// kind of surprise portable mode must not have.
fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// A resolved, confined set of writable locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    data_root: PathBuf,
    /// The collection's answer, kept so the shared locations are its to define rather than
    /// this crate's to reconstruct from a root and a convention.
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
        // **The rules are shared; the variable names and the directories are not.**
        //
        // What stays here: this project declares every environment variable it reads in
        // `orbistoun-env`, so the names are `ORBISTOUN_PORTABLE_MODE` and `ORBISTOUN_DATA_DIR`
        // rather than the `<APP>_PORTABLE` / `<APP>_DATA_DIR` the shared crate derives. That
        // registry is a feature of this project and is not being given up to save a struct
        // literal - so the reading happens here and the *answers* are handed over.
        //
        // What goes: sentinel detection, the name check, the precedence between them, and the
        // platform root. Four rules that were written twice in this collection and have to
        // agree, because a portable build that disagrees with itself about where it is writing
        // is a bug nobody sees until a stick is unplugged.
        // Start from what the shared crate reads about this machine - the home directory and
        // the platform's own data directory - and override only the three things this project
        // answers for itself. Building the whole value here instead would mean taking a
        // dependency on `dirs` again just to fill one field, and re-deriving it every time the
        // shared crate learns about another location.
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
            // The same layout as every sibling, because they share a root and cannot disagree
            // about where it is. That is the platform's own directory - `%APPDATA%\OOPS` here -
            // which is where a person, a backup tool and a roaming profile all already look.
            oops_paths::Layout::default(),
            &process,
        );
        // The flag is ignored on purpose: this crate has always answered, falling back to a
        // visible directory rather than panicking, and a dozen callers below expect a root.
        // `resolve_found` exists so that ignoring it costs no `expect`.
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
    /// `%LOCALAPPDATA%\OOPS` beside the roaming `data_root`, and the *same* directory in a
    /// portable run. The test for which side something belongs on is **can you get it back
    /// without the console?** Models and runtimes download, shaders compile, the base
    /// filesystem is materialised from a manifest, and a trace is one re-run away - all cache.
    /// A report measured against real hardware is not, and neither is an override somebody
    /// typed, so those stay with the data.
    #[must_use]
    pub fn cache_root(&self) -> &Path {
        self.shared.cache_root()
    }

    /// Where downloaded model weights go. Four gigabytes of them, hence the cache root.
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
        // A trace is one run's record and the next run rewrites it. Regenerable by re-running,
        // which is the test.
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
    /// Here rather than beside the binary or in a pictures folder, because everything
    /// orbistoun writes resolves through this type - that is what makes portable mode move
    /// all of it at once, and what lets `orbistoun-cli paths` answer "where did it go?"
    /// without anybody guessing.
    pub fn screenshots_dir(&self) -> PathBuf {
        self.data_root.join(dirs::SCREENSHOTS)
    }

    /// The base filesystem, as materialised from the manifest.
    pub fn filesystem_dir(&self) -> PathBuf {
        // Materialised from the manifest that describes it, and its own documentation says it
        // can be deleted and rebuilt at any time. That is the definition of cache.
        self.cache_root().join(dirs::FILESYSTEM)
    }

    /// The root every title's own data lives under.
    pub fn titles_dir(&self) -> PathBuf {
        self.data_root.join(dirs::TITLES)
    }

    /// One title's overlay, merged over the base tree while it runs.
    ///
    /// Under the data root rather than beside the module: a title's own directory is the
    /// material being measured, and a guest able to write into it would be editing its own
    /// evidence (D250, D251).
    pub fn title_overlay_dir(&self, title: &str) -> PathBuf {
        // **This is the point of a shared directory.**
        //
        // The overlay is keyed by the *guest's* path - a file the title writes to
        // `/user/home/<user>/savedata_prospero/<id>/x` lands at that path inside this tree.
        // Prosperous reads the console at exactly that path, measured. So a save pulled off real
        // hardware is this directory, with no translation: the guest's path is the format both
        // sides already speak.
        self.shared.title_dir(title).join("fs")
    }

    /// Where one title's save states are kept.
    pub fn title_savestates_dir(&self, title: &str) -> PathBuf {
        // Beside the guest filesystem, under the same title. A savestate is a snapshot of this
        // emulator's own memory and means nothing to a console - but everything known about one
        // title belongs in one directory, and "which of these can go back to hardware" is a
        // question about the file, not about where it was filed.
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
    /// Its own file rather than a section of [`Self::config_file`], for the same reason
    /// [`Self::learned_file`] is: that one holds how the *emulator* is configured, and this
    /// holds what the emulated console is set to. They are edited by different people at
    /// different times - a run limit is a debugging decision, a language is not - and
    /// keeping them apart means a shell settings file can be copied between installations
    /// without carrying somebody's call budget with it.
    pub fn shell_file(&self) -> PathBuf {
        self.data_root.join(SHELL_FILE)
    }

    /// Every directory this crate hands out, with the name it goes by.
    ///
    /// **The only list.** [`Self::all_dirs`] and the `paths` command both read it, and
    /// that is deliberate: they used to be two hand-written lists, so a new location could
    /// be containment-tested and still be missing from the report a person actually looks
    /// at when asking where something went. Two copies of one list is how they come to
    /// disagree (D215).
    pub fn named_dirs(&self) -> Vec<(&'static str, PathBuf)> {
        vec![
            (dirs::LOGS, self.logs_dir()),
            (dirs::TRACES, self.traces_dir()),
            (dirs::REPORTS, self.reports_dir()),
            (dirs::OVERRIDES, self.overrides_dir()),
            (dirs::SCREENSHOTS, self.screenshots_dir()),
            (dirs::FILESYSTEM, self.filesystem_dir()),
            (dirs::TITLES, self.titles_dir()),
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
/// Materialises the `.portable` **directory** with an explanatory note inside. If a
/// stale `.portable` *file* is present from an older scheme it is removed first -
/// otherwise `create_dir_all` fails over it, which is the exact first-run crash this
/// design exists to avoid.
pub fn enable_portable_sentinel(binary_dir: &Path) -> io::Result<()> {
    let sentinel = binary_dir.join(PORTABLE_DIR);
    if sentinel.is_file() {
        fs::remove_file(&sentinel)?;
    }
    fs::create_dir_all(&sentinel)?;
    fs::write(
        sentinel.join(PORTABLE_NOTE),
        concat!(
            "This directory makes orbistoun run in portable mode.

",
            "Everything orbistoun writes - logs, traces, run reports, settings, per-title
",
            "overrides - stays beneath this directory. Nothing is written anywhere else.

",
            "Delete this directory to return to the OS-standard data location.
"
        ),
    )
}

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

    #[test]
    fn truthy_values_are_narrow_and_case_insensitive() {
        for v in ["1", "true", "TRUE", "Yes", " on "] {
            assert!(is_truthy(v), "{v:?} should be truthy");
        }
        // The important half: anything unrecognised is OFF. Treating `no` as on would
        // be exactly the surprise portable mode must not have.
        for v in ["0", "false", "no", "off", "", "maybe", "portable"] {
            assert!(!is_truthy(v), "{v:?} should not be truthy");
        }
    }

    #[test]
    fn env_var_triggers_portable() {
        let p = Paths::resolve_with(
            &env_portable(),
            Some(Path::new("/opt/app")),
            Some("orbistoun"),
        );
        assert!(p.is_portable());
        assert_eq!(p.data_root(), Path::new("/opt/app").join(PORTABLE_DIR));
        // The sentinel directory *is* the root - one directory for every tool on the stick,
        // exactly as an installed set shares one.
        assert_eq!(p.data_root(), Path::new("/opt/app").join(PORTABLE_DIR));
    }

    #[test]
    fn filename_containing_portable_triggers_it_with_no_sentinel_or_env() {
        // This is what makes a single-file download work with zero instructions.
        let p = Paths::resolve_with(
            &EnvSnapshot::default(),
            Some(Path::new("/downloads")),
            Some("orbistoun-portable-gui"),
        );
        assert!(p.is_portable());
    }

    #[test]
    fn filename_match_is_case_insensitive() {
        let p = Paths::resolve_with(
            &EnvSnapshot::default(),
            Some(Path::new("/downloads")),
            Some("Orbistoun-PORTABLE-GUI"),
        );
        assert!(p.is_portable());
    }

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

    #[test]
    fn portable_outranks_the_data_dir_override() {
        // If an env var could escape the portable root, containment would be a
        // suggestion rather than a guarantee.
        let env = EnvSnapshot {
            portable_flag: true,
            data_dir: Some(PathBuf::from("/somewhere/else")),
        };
        let p = Paths::resolve_with(&env, Some(Path::new("/opt/app")), Some("orbistoun-cli"));
        assert!(p.is_portable());
        assert!(p.data_root().starts_with("/opt/app"));
    }

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

    #[test]
    fn unknown_binary_location_still_honours_the_env_flag() {
        let p = Paths::resolve_with(&env_portable(), None, None);
        assert!(p.is_portable());
        assert_eq!(p.data_root(), Path::new(".").join(PORTABLE_DIR));
    }

    #[test]
    fn enabling_the_sentinel_heals_a_stale_file_from_the_old_scheme() {
        // The regression this guards: sentinel and data root are the same path, so a
        // `.portable` FILE makes create_dir_all fail on first run.
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join(PORTABLE_DIR), b"stale marker").expect("write stale file");

        enable_portable_sentinel(tmp.path()).expect("should heal, not fail");

        let sentinel = tmp.path().join(PORTABLE_DIR);
        assert!(sentinel.is_dir(), "sentinel must end up a directory");
        assert!(
            sentinel.join(PORTABLE_NOTE).is_file(),
            "note explains itself"
        );

        // And the whole point: creating the tree now succeeds.
        let p = Paths::resolve_with(
            &EnvSnapshot::default(),
            Some(tmp.path()),
            Some("orbistoun-cli"),
        );
        p.ensure_dirs()
            .expect("first run must not fail over the sentinel");
    }

    /// The containment guarantee, asserted rather than assumed (D038).
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

    #[test]
    fn every_writable_location_is_listed_in_all_dirs() {
        // all_dirs drives the containment test, so a new location that forgets to
        // register here would be silently unverified. This is the reminder.
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
        ] {
            assert!(all.contains(&d), "{d:?} missing from all_dirs()");
        }
        assert_eq!(
            all.len(),
            7,
            "a location was added without updating the test"
        );

        // **A title's data is deliberately in two places now, and D251 said it should be in
        // one.** That rule bought "one title is one directory to move or delete", and it has
        // been given up on purpose: the guest filesystem is the tree Prosperous fills from real
        // hardware, so it has to be somewhere a sibling can reach.
        //
        // **Everything about one title is one directory**, which is what D251 asked for and
        // what the shared root finally delivers: the guest filesystem prosperous fills from
        // hardware and the savestates this emulator writes sit under the same identifier.
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

    #[test]
    fn env_snapshot_reads_the_process_without_panicking() {
        // Smoke test for the real-world path; value depends on the ambient env.
        let snap = EnvSnapshot::from_process();
        let _ = snap.portable_flag;
        assert_eq!(ENV_PORTABLE, "ORBISTOUN_PORTABLE_MODE");
    }
}
