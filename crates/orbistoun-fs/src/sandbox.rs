//! A title's sandbox, established as one thing.
//!
//! The overlay mechanism is in [`crate::mount`] and [`crate::filesystem`] (D251). Assembling
//! it for a running title is three steps in a fixed order: empty the overlay if this run does
//! not keep the last one's files, install the base tree with its writable device overlays,
//! then layer the title's own files over `/app0`. [`establish`] does all three, and every
//! consumer calls it. A consumer supplies where the bytes live and how long they last;
//! nothing here reads the environment.

use std::path::Path;

/// Whether a title's sandbox keeps what it wrote between runs.
///
/// The default keeps them, so saves and reports a run produced survive it; the hardware's
/// sandbox is presumably [`Ephemeral`](Self::Ephemeral). The consumer chooses: a run reads
/// `ORBISTOUN_SANDBOX`, a test states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Retention {
    /// Keep it: saves and a probe's reports persist past the run that wrote them.
    #[default]
    Retain,
    /// Empty it at the start of each run, closer to a hardware sandbox that carries no state.
    Ephemeral,
}

/// Establishes a title's sandboxed filesystem: the accountable base tree, its writable device
/// overlays, and the title's own files over `/app0`.
///
/// `base` is where the base tree is materialised (read-only to the guest); `overlay` is the
/// per-title directory a guest's writes land in; `title_module` is the loaded guest, whose
/// directory becomes `/app0`; `retention` decides whether `overlay` is emptied first. The base
/// is installed first, each writable entry's overlay stacked over it, and the title layered
/// over `/app0` last so its own files answer first.
pub fn establish(
    base: &Path,
    overlay: &Path,
    title_module: &Path,
    origin: &Origin,
    retention: Retention,
) {
    if retention == Retention::Ephemeral {
        // At the start of a run, not at teardown: a guest leaves by calling exit, so nothing
        // after the entry point reliably runs (D422).
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
/// Never anything the title ships: the platform decides by the mount, not the package.
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

/// Shows installed titles to the guest under [`LIBRARY_MOUNT`], one directory per title id,
/// read-only - the system view a launcher has, where a sandboxed title sees only its own `/app0`.
///
/// By id rather than by the library's folder names: the platform names `/user/app/<id>` by
/// the id the title declares, and a launcher skips names that are not ids. Read-only because
/// nothing in the manifest marks the prefix writable.
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

    /// Retain keeps what a previous run wrote; ephemeral does not.
    ///
    /// Asserted on a file placed in the overlay, which is what a guest's write becomes.
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

    /// After establishing, a device path such as `/mnt/usb0` is a writable overlay like
    /// `/data` (D422).
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

    /// A library image's `/app0` refuses writes; a staged title's accepts them (D722).
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
        // One directory under two names, as on the hardware.
        assert!(crate::metadata::listing("/data/homebrew/NVRB00001/Replays").is_some());
        assert!(crate::metadata::listing("/data/homebrew/NVRB00001/data").is_some());

        crate::mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Writing a shipped file copies it up, and the library keeps its bytes.
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
        // Removing a name the library also holds would need a whiteout: refused.
        assert!(crate::mount::resolve_for_removal("/app0/data/shipped.txt").is_none());
        assert!(crate::mount::resolve_for_removal("/app0/data/new.txt").is_some());

        crate::mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The system view lists the library at `/user/app` and refuses writes into it.
    #[test]
    fn the_system_view_lists_the_library_read_only() {
        let _guard = crate::exclusively();
        crate::mount::clear();

        let root = scratch("library");
        let library = root.join("titles");
        let launcher = library.join("SCSH00001");
        // Unpacked under a folder name that is not its id.
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
