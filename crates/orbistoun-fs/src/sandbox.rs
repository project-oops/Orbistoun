//! A title's sandbox, established as one thing.
//!
//! # What this centralises, and why it is not just `filesystem::install`
//!
//! The overlay *engine* lives in [`crate::mount`] and [`crate::filesystem`]: a base tree
//! materialised from a knowledge file, and a per-title writable overlay stacked over it (D250,
//! D251). But assembling that for a *running title* is three steps that must happen in one order -
//! empty the overlay if this run is not to inherit the last one's files, install the base tree with
//! its writable device overlays, then layer the title's own files over `/app0` - and that order is
//! exactly what gets remembered wrong when it is spread across whoever happens to set a run up (the
//! textures-lost regression, D269).
//!
//! So it is one function, [`establish`], and every consumer calls it. A consumer supplies *where*
//! the bytes live and *how long* they last; nothing here reads the environment, so the fs crate
//! stays a mechanism its callers configure rather than one that configures itself (principle 5).

use std::path::Path;

/// Whether a title's sandbox keeps what it wrote between runs.
///
/// The console's own answer is presumably [`Ephemeral`](Self::Ephemeral); the default here is the
/// opposite on purpose, because a proof of concept wants the saves and the reports a run produced
/// to survive it. The choice is the consumer's, passed in - a run reads it from
/// `ORBISTOUN_SANDBOX`, a test states it outright - so the policy has one meaning and many callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Retention {
    /// Keep it: saves and a probe's reports persist past the run that wrote them.
    #[default]
    Retain,
    /// Empty it at the start of each run, closer to a console sandbox that carries no state.
    Ephemeral,
}

/// Establishes a title's sandboxed filesystem: the accountable base tree, its writable device
/// overlays, and the title's own files over `/app0` - the whole guest-visible namespace.
///
/// - `base` is where the console's base tree is materialised (read-only to the guest).
/// - `overlay` is the per-title directory a guest's writes land in.
/// - `title_module` is the guest that was loaded; its directory becomes `/app0`.
/// - `retention` decides whether `overlay` is emptied first.
///
/// The order is the one [`crate::mount`] requires and is not the caller's to get right: the base is
/// installed first, each writable entry's overlay is stacked over it, and the title is layered over
/// `/app0` last so its own files answer before anything the console provides.
pub fn establish(
    base: &Path,
    overlay: &Path,
    title_module: &Path,
    origin: &Origin,
    retention: Retention,
) {
    if retention == Retention::Ephemeral {
        // **At the start of a run, not at a teardown.** A process guest is jumped to and leaves by
        // calling exit, so nothing after the entry point reliably runs; "empty at the start" is the
        // only point that always executes, and it is the observable property anyway (D422).
        let _ = std::fs::remove_dir_all(overlay);
    }
    crate::filesystem::install(base, overlay);
    crate::mount::mount_title(title_module);
    if let Origin::Staged { id } = origin {
        crate::mount::stage_title(title_module, overlay, id);
    }
}

/// Where a title's files are stored, which is what decides whether its `/app0` is writable (D722).
///
/// Never anything the title ships: the console decides by the mount, not the package.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Origin {
    /// A library image, installed from a package: `/app0` is read-only (D250).
    #[default]
    Image,
    /// Staged on the writable user partition at `/data/homebrew/<id>`: `/app0` is that directory,
    /// and the title may write into it.
    Staged {
        /// The directory name it is staged under.
        id: String,
    },
}

/// Where installed titles are, from the guest's point of view: one directory per title id, each
/// holding what that title shipped (`eboot.bin`, `sce_sys/param.json`, `sce_sys/icon0.png`).
pub const LIBRARY_MOUNT: &str = "/user/app";

/// Shows installed titles to the guest under [`LIBRARY_MOUNT`], one directory per **title id**,
/// read-only - the system view a launcher has, where a sandboxed title sees only its own `/app0`.
///
/// By id rather than by the library's folder names: a console names `/user/app/<id>` by the id
/// the title declares, and a library folder is whatever it was unpacked as (`PPSA02664-app0`).
/// Mounting the library folder whole showed a launcher those names, and it skipped every one that
/// was not an id (worklog 842). Read-only because nothing in the manifest marks the prefix
/// writable.
///
/// `base` is the materialised base tree [`establish`] was given: `/user/app` itself is an empty
/// directory there, so a guest that opens it to walk it by descriptor gets a descriptor, and the
/// titles are the mount points listed under it.
pub fn expose_library(base: &Path, titles: &[(String, std::path::PathBuf)]) {
    let root = base.join(LIBRARY_MOUNT.trim_start_matches('/'));
    let _ = std::fs::create_dir_all(&root);
    crate::mount::layer(LIBRARY_MOUNT, root);
    for (id, directory) in titles {
        crate::mount::layer(&format!("{LIBRARY_MOUNT}/{id}"), directory.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::{Origin, Retention, establish};

    /// A scratch root unique to this test, cleaned at both ends.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orbistoun-sandbox-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// **Retain keeps what a previous run wrote; ephemeral does not.**
    ///
    /// This is the whole reason the policy is a setting: a save must survive the run that made it,
    /// and a run that must start clean must be able to say so. Asserted on a file placed in the
    /// overlay, because that is what a guest's write becomes.
    #[test]
    fn retention_keeps_or_empties_the_overlay_as_asked() {
        let _guard = crate::exclusively();
        crate::mount::clear();

        let root = scratch("retain");
        let base = root.join("base");
        let overlay = root.join("overlay");
        let title = root.join("title");
        std::fs::create_dir_all(&title).unwrap();
        std::fs::write(title.join("eboot.bin"), b"guest").unwrap();

        // A file a "previous run" left in the writable /data overlay.
        let left = overlay.join("data").join("save.bin");
        std::fs::create_dir_all(left.parent().unwrap()).unwrap();
        std::fs::write(&left, b"a save").unwrap();

        establish(
            &base,
            &overlay,
            &title.join("eboot.bin"),
            &Origin::Image,
            Retention::Retain,
        );
        assert!(left.exists(), "retain keeps a previous run's file");

        establish(
            &base,
            &overlay,
            &title.join("eboot.bin"),
            &Origin::Image,
            Retention::Ephemeral,
        );
        assert!(
            !left.exists(),
            "ephemeral empties the overlay before the run"
        );

        crate::mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **After establishing, a console device path is mounted and writable.**
    ///
    /// The point of the manifest entries: `/mnt/usb0` is not a special case in code, it is a
    /// writable overlay like `/data`, so a guest's `mkdir` and write land there rather than
    /// faulting. If this regresses, obSCEne's report sink is back to crashing (D422).
    #[test]
    fn a_device_path_is_writable_after_establishing() {
        let _guard = crate::exclusively();
        crate::mount::clear();

        let root = scratch("device");
        let base = root.join("base");
        let overlay = root.join("overlay");
        let title = root.join("title");
        std::fs::create_dir_all(&title).unwrap();
        std::fs::write(title.join("eboot.bin"), b"guest").unwrap();

        establish(
            &base,
            &overlay,
            &title.join("eboot.bin"),
            &Origin::Image,
            Retention::Retain,
        );
        assert!(
            crate::mount::is_writable("/mnt/usb0/obscene/report.txt"),
            "a USB device path is a writable sandbox after establishing"
        );

        crate::mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A title directory holding `eboot.bin` and one shipped file, with a base and overlay beside it.
    fn staged_fixture(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = scratch(tag);
        let title = root.join("NVRB00001");
        std::fs::create_dir_all(title.join("data")).unwrap();
        std::fs::write(title.join("eboot.bin"), b"guest").unwrap();
        std::fs::write(title.join("data").join("shipped.txt"), b"library").unwrap();
        (root, title)
    }

    /// **A library image's `/app0` refuses writes; a staged title's accepts them (D722).**
    ///
    /// Both halves in one test, because the decision is the difference between them: the same
    /// directory, established with each origin, answers each way.
    #[test]
    fn only_a_staged_title_may_write_its_app0() {
        let _guard = crate::exclusively();
        crate::mount::clear();
        let (root, title) = staged_fixture("origin");

        establish(
            &root.join("base"),
            &root.join("overlay"),
            &title.join("eboot.bin"),
            &Origin::Image,
            Retention::Retain,
        );
        assert!(!crate::mount::is_writable("/app0/Replays/Last.nbr"));
        assert!(crate::descriptor::create("/app0/Replays/Last.nbr").is_none());

        crate::mount::clear();
        establish(
            &root.join("base"),
            &root.join("overlay"),
            &title.join("eboot.bin"),
            &Origin::Staged {
                id: "NVRB00001".to_owned(),
            },
            Retention::Retain,
        );
        let fd = crate::descriptor::create("/app0/Replays/Last.nbr")
            .expect("a staged /app0 is writable");
        crate::descriptor::close(fd);
        let landed = root
            .join("overlay")
            .join("data/homebrew/NVRB00001/Replays/Last.nbr");
        assert!(landed.exists(), "the write lands in the title's overlay");
        assert!(!title.join("Replays").exists(), "and never in the library");
        // One directory under two names, as on the console.
        assert!(crate::metadata::listing("/data/homebrew/NVRB00001/Replays").is_some());
        assert!(crate::metadata::listing("/data/homebrew/NVRB00001/data").is_some());

        crate::mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **Writing a shipped file copies it up; the library keeps its bytes.**
    ///
    /// Without the copy-up a write to an existing file resolved to the first layer holding it -
    /// the library - and a staged title rewrote its own shipped files on the host.
    #[test]
    fn a_staged_write_to_a_shipped_file_never_reaches_the_library() {
        let _guard = crate::exclusively();
        crate::mount::clear();
        let (root, title) = staged_fixture("copyup");
        establish(
            &root.join("base"),
            &root.join("overlay"),
            &title.join("eboot.bin"),
            &Origin::Staged {
                id: "NVRB00001".to_owned(),
            },
            Retention::Retain,
        );

        let host = crate::mount::resolve_for_write("/app0/data/shipped.txt").expect("writable");
        assert_eq!(
            std::fs::read(&host).unwrap(),
            b"library",
            "copied up with its bytes"
        );
        std::fs::write(&host, b"changed").unwrap();
        assert_eq!(
            std::fs::read(title.join("data").join("shipped.txt")).unwrap(),
            b"library"
        );
        let seen = crate::mount::resolve_existing("/app0/data/shipped.txt").unwrap();
        assert_eq!(
            std::fs::read(seen).unwrap(),
            b"changed",
            "the guest sees its own copy"
        );
        // Removing a name the library also holds would need a whiteout: refused, not faked.
        assert!(crate::mount::resolve_for_removal("/app0/data/shipped.txt").is_none());
        assert!(crate::mount::resolve_for_removal("/app0/data/new.txt").is_some());

        crate::mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **The system view lists the library at `/user/app`, and refuses writes into it.**
    ///
    /// What a launcher scans for installed titles; a sandboxed run, which never calls
    /// `expose_library`, has no such directory.
    #[test]
    fn the_system_view_lists_the_library_read_only() {
        let _guard = crate::exclusively();
        crate::mount::clear();

        let root = scratch("library");
        let library = root.join("titles");
        let launcher = library.join("SCSH00001");
        // Unpacked under a folder name that is not its id, as retail dumps are.
        let cube = library.join("GLCB00001-app0");
        std::fs::create_dir_all(cube.join("sce_sys")).unwrap();
        std::fs::create_dir_all(&launcher).unwrap();
        std::fs::write(launcher.join("eboot.bin"), b"guest").unwrap();

        establish(
            &root.join("base"),
            &root.join("console"),
            &launcher.join("eboot.bin"),
            &Origin::Image,
            Retention::Retain,
        );
        assert!(
            crate::metadata::listing("/user/app").is_none(),
            "a sandbox has no library"
        );

        super::expose_library(&root.join("base"), &[("GLCB00001".to_owned(), cube)]);
        // A launcher opens the directory and walks it through the descriptor.
        let fd = crate::descriptor::open("/user/app").expect("the library opens as a directory");
        crate::descriptor::close(fd);
        let listed = crate::metadata::listing("/user/app").expect("the library lists");
        assert!(listed.iter().any(|(n, dir)| n == "GLCB00001" && *dir));
        assert!(
            !listed.iter().any(|(n, _)| n.ends_with("-app0")),
            "listed by id, never by folder name"
        );
        assert!(crate::metadata::listing("/user/app/GLCB00001/sce_sys").is_some());
        assert!(!crate::mount::is_writable("/user/app/GLCB00001/x"));

        crate::mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }
}
