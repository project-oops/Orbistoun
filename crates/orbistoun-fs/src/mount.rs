//! Where a guest path lands on the host.
//!
//! `/app0` maps to the directory the module was loaded from, and the table takes further
//! mounts without changing shape. Per-title behaviour belongs in the overrides layer, keyed
//! by a named setting, never by a title id here. A guest chooses these paths, and
//! `/app0/../../x` is one, so resolution walks components and refuses anything that climbs
//! out rather than resolving first and checking afterwards: a check after the fact is one
//! symbolic link away from being wrong.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Where a title's own files live, from the guest's point of view.
pub const APP_MOUNT: &str = "/app0";

/// Where a guest may write.
///
/// The platform gives an application writable storage separate from its read-only title; a
/// conformance probe creates its report there in its first few calls (D250).
pub const DATA_MOUNT: &str = "/data";

/// Where one engine's titles read their built asset cache from.
///
/// A title whose content is a read-only [`APP_MOUNT`] opens `/host//ap/rpf.cache`; the
/// doubled slash is the engine's own join and the literal path the guest passes. The
/// platform serves it from the title's own storage, so [`mount_title`] layers the title
/// directory here too and the shipped `rpf.cache` answers with no copied file (D709). It is
/// one engine's fixed cache path, not a title-id special case: a title that never opens it
/// resolves nothing here.
pub const RAGE_HOST_APP_MOUNT: &str = "/host//ap";

/// The mount table.
fn mounts() -> &'static Mutex<BTreeMap<String, Vec<PathBuf>>> {
    static MOUNTS: OnceLock<Mutex<BTreeMap<String, Vec<PathBuf>>>> = OnceLock::new();
    MOUNTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Points a guest prefix at a host directory.
///
/// Replaces any previous mapping for that prefix: a run configures its mounts once.
pub fn mount(guest_prefix: &str, host_root: PathBuf) {
    if let Ok(mut mounts) = mounts().lock() {
        mounts.insert(guest_prefix.to_owned(), vec![host_root]);
    }
}

/// Puts a host directory *over* whatever is already mounted at a prefix.
///
/// The base tree and a title's own files are one namespace to the guest. Merging them here
/// costs a list walk per resolve and keeps the base reproducible from its manifest (D251).
/// The overlay goes first, so a file a title has written shadows the base and every write
/// lands in the overlay.
pub fn layer(guest_prefix: &str, host_root: PathBuf) {
    if let Ok(mut mounts) = mounts().lock() {
        mounts
            .entry(guest_prefix.to_owned())
            .or_default()
            .insert(0, host_root);
    }
}

/// Mounts a title's own directory as `/app0`.
///
/// Takes the module path, the thing that was run, because every caller has it.
pub fn mount_title(module: &Path) {
    if let Some(directory) = module.parent() {
        // Layered, not mounted: `mount` replaces every root at a prefix and would discard the
        // base tree. The title goes over the base, so its own files answer first (D251).
        layer(APP_MOUNT, directory.to_path_buf());
        // The same directory under the engine's host-device cache path, so a title reading
        // `/host//ap/rpf.cache` finds the file it shipped (D709).
        layer(RAGE_HOST_APP_MOUNT, directory.to_path_buf());
    }
}

/// Where a staged title lives, from the guest's point of view: `/data/homebrew/<id>` (D722).
pub const STAGING_MOUNT: &str = "/data/homebrew";

/// Mounts a staged title: its directory at `/data/homebrew/<id>` and at `/app0`, both under
/// one writable top layer, `<overlay>/data/homebrew/<id>`.
///
/// On the hardware a title staged on the user partition runs with `/app0` being that
/// directory, and `/data` is read-write. Here the title's files are the library's, which are
/// never written: the shared top layer takes every write, and a file written through either
/// path is seen through the other (D722).
pub fn stage_title(module: &Path, overlay: &Path, id: &str) {
    let Some(directory) = module.parent() else {
        return;
    };
    let writable = overlay.join("data").join("homebrew").join(id);
    let _ = std::fs::create_dir_all(&writable);
    let staged = format!("{STAGING_MOUNT}/{id}");
    layer(&staged, directory.to_path_buf());
    layer(&staged, writable.clone());
    layer(APP_MOUNT, writable);
    allow_writes(APP_MOUNT);
}

/// Points `/data` at a host directory a guest may write into.
///
/// Created here rather than at first write: a missing host directory would read as a
/// missing file, which calls for a different response.
pub fn mount_data(host_root: PathBuf) {
    let _ = std::fs::create_dir_all(&host_root);
    mount(DATA_MOUNT, host_root);
}

/// Prefixes a guest may write under.
fn writable() -> &'static Mutex<BTreeSet<String>> {
    static WRITABLE: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    WRITABLE.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Records that a guest may write under `guest_prefix`.
///
/// Set from the filesystem manifest rather than a constant here, so which directories are
/// writable is stated once, beside how it is known (D251).
pub fn allow_writes(guest_prefix: &str) {
    if let Ok(mut writable) = writable().lock() {
        writable.insert(guest_prefix.to_owned());
    }
}

/// Whether a guest path lies under a mount a guest may write to.
///
/// A guest writing into a title's own directory would be editing the material being run.
/// Asked by prefix; a path that climbs out of a mount is still refused by [`resolve`] (D250).
pub fn is_writable(guest_path: &str) -> bool {
    let path = guest_path.replace('\\', "/");
    let Ok(writable) = writable().lock() else {
        return false;
    };
    writable.iter().any(|prefix| {
        path == *prefix || (path.starts_with(prefix) && path[prefix.len()..].starts_with('/'))
    })
}

/// Forgets every mount.
pub fn clear() {
    if let Ok(mut writable) = writable().lock() {
        writable.clear();
    }
    if let Ok(mut mounts) = mounts().lock() {
        mounts.clear();
    }
}

/// Whether a guest path stays inside its mount.
///
/// Walks components rather than resolving and checking afterwards. `..` is refused outright
/// rather than cancelled against a preceding component, because cancelling is correct only
/// when nothing in the path is a symbolic link. Pure, so the rule is testable without a
/// filesystem.
pub fn is_contained(relative: &str) -> bool {
    !relative.is_empty()
        && Path::new(relative)
            .components()
            .all(|component| matches!(component, Component::Normal(part) if !part.is_empty()))
}

/// Whether the guest's path components (`rest`, under orbistoun's host `root`) exist on disk
/// case-sensitively, as on the platform's FreeBSD-derived filesystem.
///
/// `Path::exists` on a Windows host is case-insensitive and would report a wrong-case name
/// present that the platform answers `ENOENT` for. Only the guest-supplied components are
/// checked. On a case-sensitive host this is a plain `exists`.
#[cfg(not(windows))]
fn exists_case_sensitive(root: &Path, rest: &str) -> bool {
    root.join(rest).exists()
}

/// The Windows half: `canonicalize` returns the path's real on-disk case, so a wrong-case
/// request resolves to a different spelling, which is the not-found the platform gives.
/// Compared over the guest's trailing components only, so the host root's case and the
/// `\\?\` prefix are not part of the test.
#[cfg(windows)]
fn exists_case_sensitive(root: &Path, rest: &str) -> bool {
    let Ok(real) = std::fs::canonicalize(root.join(rest)) else {
        return false;
    };
    let want: Vec<&str> = rest.split('/').filter(|c| !c.is_empty()).collect();
    let real_components: Vec<&str> = real
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    real_components.len() >= want.len()
        && real_components[real_components.len() - want.len()..]
            .iter()
            .zip(&want)
            .all(|(on_disk, asked)| on_disk == asked)
}

/// A path under a mount with its `.` components (and the empty ones a doubled slash leaves)
/// dropped.
///
/// Only `.`, never `..`: dropping `.` cannot climb out of a mount or step through a symbolic
/// link, so [`is_contained`] still refuses `..`. `Path::components` keeps a leading `.`, so
/// `/app0/./data` would otherwise be refused where every POSIX lookup accepts it.
fn without_current_dir(rest: &str) -> String {
    rest.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Maps a guest path to its host path: the existing layer if it has one, else the writable
/// layer where a new file would go. Path mapping, not an existence check. `None` for a path
/// under no mount or one that climbs out, the same answer as for a missing file. Reads that
/// must fail on a missing path use [`resolve_existing`].
pub fn resolve(guest_path: &str) -> Option<PathBuf> {
    resolve_inner(guest_path, true)
}

/// The host path for a guest path that exists, case-sensitively; `None` when it is not
/// there, including when it exists only under a different case.
///
/// The read resolver. [`resolve`] answers where a file could be written even when nothing is
/// there, and on a case-insensitive host `File::open` would then open a wrong-case file.
pub fn resolve_existing(guest_path: &str) -> Option<PathBuf> {
    resolve_inner(guest_path, false)
}

/// Where a guest's write to `guest_path` goes: always the top layer of its mount.
///
/// `None` unless the path is under a writable prefix. A file that exists only in a lower
/// layer (the base tree, or a staged title's library copy under `/app0`) is copied up first,
/// so the lower layer is never touched (D722).
pub fn resolve_for_write(guest_path: &str) -> Option<PathBuf> {
    let (top, below) = locate_for_write(guest_path)?;
    if let Some(lower) = below {
        if let Some(parent) = top.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        if lower.is_dir() {
            std::fs::create_dir_all(&top).ok()?;
        } else {
            std::fs::copy(&lower, &top).ok()?;
        }
    }
    Some(top)
}

/// Where a guest's truncating create of `guest_path` goes: the top layer, with nothing copied
/// up, since the old contents are discarded. `None` unless the path is writable.
pub fn resolve_for_create(guest_path: &str) -> Option<PathBuf> {
    locate_for_write(guest_path).map(|(top, _)| top)
}

/// Where a guest's removal or rename of `guest_path` acts: the top layer's path, and only
/// when the name does not also exist in a lower layer.
///
/// Removing the top copy would leave the lower one visible, and removing the lower one would
/// write to the library. That needs a whiteout, which this overlay does not keep, so it is
/// refused (D722).
pub fn resolve_for_removal(guest_path: &str) -> Option<PathBuf> {
    let (top, _) = locate_for_write(guest_path)?;
    (!lower_layer_holds(guest_path)).then_some(top)
}

/// The top layer's path for a writable guest path, and the lower layer's copy when the name exists
/// only there.
fn locate_for_write(guest_path: &str) -> Option<(PathBuf, Option<PathBuf>)> {
    if !is_writable(guest_path) {
        return None;
    }
    let top = resolve_top(guest_path)?;
    let existing = resolve_existing(guest_path);
    let below = existing.filter(|found| *found != top);
    Some((top, below))
}

/// Whether some layer other than the top one holds `guest_path`.
fn lower_layer_holds(guest_path: &str) -> bool {
    let Some(top) = resolve_top(guest_path) else {
        return false;
    };
    let guest_path = guest_path.replace('\\', "/");
    let Ok(mounts) = mounts().lock() else {
        return false;
    };
    for (prefix, roots) in mounts.iter().rev() {
        let Some(rest) = guest_path.strip_prefix(prefix.as_str()) else {
            continue;
        };
        if !rest.is_empty() && !rest.starts_with('/') {
            continue;
        }
        let rest = without_current_dir(rest);
        // A lower root may be the same host directory as the top one (a writable entry's
        // overlay stacked twice), which is not a second copy.
        return !rest.is_empty()
            && is_contained(&rest)
            && roots
                .iter()
                .skip(1)
                .any(|root| root.join(&rest) != top && exists_case_sensitive(root, &rest));
    }
    false
}

/// The top layer's host path for a guest path, whether or not anything is there.
fn resolve_top(guest_path: &str) -> Option<PathBuf> {
    let guest_path = guest_path.replace('\\', "/");
    let mounts = mounts().lock().ok()?;
    for (prefix, roots) in mounts.iter().rev() {
        let Some(rest) = guest_path.strip_prefix(prefix.as_str()) else {
            continue;
        };
        if !rest.is_empty() && !rest.starts_with('/') {
            continue;
        }
        let rest = without_current_dir(rest);
        let root = roots.first()?;
        if rest.is_empty() {
            return Some(root.clone());
        }
        return is_contained(&rest).then(|| root.join(rest));
    }
    None
}

fn resolve_inner(guest_path: &str, writable_fallback: bool) -> Option<PathBuf> {
    // Normalised so `\` from a guest that mixes conventions cannot slip a component past the
    // component walk.
    let guest_path = guest_path.replace('\\', "/");
    let mounts = mounts().lock().ok()?;
    // The most specific mount answers first: a nested mount such as `/user/app/<id>` sorts
    // after its parent `/user/app`, so reverse key order reaches it first.
    for (prefix, roots) in mounts.iter().rev() {
        let Some(rest) = guest_path.strip_prefix(prefix.as_str()) else {
            continue;
        };
        // The mount itself, with no path under it.
        let rest = rest.trim_start_matches('/');
        if rest.is_empty() {
            return roots.first().cloned();
        }
        // A prefix must match a whole component: `/app0extra/x` is not inside `/app0`.
        if !guest_path[prefix.len()..].starts_with('/') {
            continue;
        }
        // `.` names the directory it sits in, so `/app0/./data` is `/app0/data`. Collapsed
        // after the whole-component check, so `/app0.` stays a different name from `/app0`.
        let rest = without_current_dir(rest);
        if rest.is_empty() {
            return roots.first().cloned();
        }
        if !is_contained(&rest) {
            return None;
        }
        let mut found = None;
        for (index, root) in roots.iter().enumerate() {
            let candidate = root.join(&rest);
            // The first layer that has it: a title's own copy shadows the base.
            if exists_case_sensitive(root, &rest) {
                return Some(candidate);
            }
            if index == 0 {
                found = Some(candidate);
            }
        }
        // Nowhere yet: a create gets the writable layer, where a title's data belongs; a read
        // gets `None`.
        return if writable_fallback { found } else { None };
    }
    None
}

/// Names that exist at `guest_path` only because a mount lies below it.
///
/// `/app0` and `/data` are directories a guest can enter that no host directory holds, so
/// without this `/` would list nothing and an FTP client's `CWD /` then `LIST` would fail.
/// For `/` it answers `app0` and `data`; for a mount at `/system_data/priv` it answers
/// `system_data` at the root and `priv` under that. Only the next component, as a directory
/// entry is. An empty list means no mount lies below the path.
#[must_use]
pub fn mounts_under(guest_path: &str) -> Vec<String> {
    let path = guest_path.replace('\\', "/");
    // An empty or relative path is not the root: `/` and `""` both trim to nothing, and
    // `stat("")` must fail as on a real system (D387). Nothing here has a working directory,
    // so a path not starting at the root names nothing.
    if !path.starts_with('/') {
        return Vec::new();
    }
    let here = path.trim_end_matches('/');
    let Ok(mounts) = mounts().lock() else {
        return Vec::new();
    };
    let mut names: BTreeSet<String> = BTreeSet::new();
    for prefix in mounts.keys() {
        let Some(rest) = prefix.strip_prefix(here) else {
            continue;
        };
        // A prefix must be below this path by a whole component: `/app0extra` is not under
        // `/app0`, and a path equal to the prefix is the mount itself.
        let Some(rest) = rest.strip_prefix('/') else {
            continue;
        };
        let next = rest.split('/').next().unwrap_or_default();
        if !next.is_empty() {
            names.insert(next.to_owned());
        }
    }
    names.into_iter().collect()
}

/// Whether a guest path names a directory, whether or not the host has one.
///
/// True for anything that resolves to a host directory, and for a path that exists only
/// because a mount sits below it, such as `/`.
#[must_use]
pub fn is_directory(guest_path: &str) -> bool {
    if !mounts_under(guest_path).is_empty() {
        return true;
    }
    resolve_existing(guest_path).is_some_and(|host| host.is_dir())
}

#[cfg(test)]
mod tests {
    /// The root lists its mounts, and an empty path lists nothing (D387).
    #[test]
    fn the_root_holds_its_mounts_and_an_empty_path_holds_nothing() {
        let _guard = with_app_mount("/titles/one");
        assert_eq!(super::mounts_under("/"), vec!["app0".to_owned()]);
        assert!(super::is_directory("/"));

        assert!(
            super::mounts_under("").is_empty(),
            "an empty path is not the root"
        );
        assert!(!super::is_directory(""));
        assert!(
            super::mounts_under("app0").is_empty(),
            "and a relative path is not either - nothing here has a working directory"
        );
        assert!(!super::is_directory("app0"));
    }

    /// An intermediate directory exists because a mount is below it, and holds only the
    /// next component.
    #[test]
    fn a_directory_above_a_mount_holds_the_next_component_only() {
        let _guard = crate::exclusively();
        clear();
        mount(
            "/system_data/priv/appmeta",
            std::path::PathBuf::from("/host/meta"),
        );
        assert_eq!(super::mounts_under("/"), vec!["system_data".to_owned()]);
        assert_eq!(super::mounts_under("/system_data"), vec!["priv".to_owned()]);
        assert_eq!(
            super::mounts_under("/system_data/priv"),
            vec!["appmeta".to_owned()]
        );
        assert!(
            super::mounts_under("/system_data/priv/appmeta").is_empty(),
            "the mount itself has nothing below it"
        );
        assert!(super::is_directory("/system_data/priv"));
        assert!(
            super::mounts_under("/system").is_empty(),
            "a prefix must match a whole component"
        );
    }

    use super::{APP_MOUNT, clear, is_contained, mount, mount_title, resolve};

    /// Sets up a lone `/app0` mount, holding the crate-wide lock for the caller.
    ///
    /// The guard is returned, not dropped: the mount table is process-global and these tests
    /// replace it, so another module's test could clear it mid-assertion.
    fn with_app_mount(root: &str) -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::exclusively();
        clear();
        mount(APP_MOUNT, std::path::PathBuf::from(root));
        guard
    }

    /// A title path lands in the title directory.
    #[test]
    fn a_title_path_lands_in_the_title_directory() {
        let _guard = with_app_mount("/titles/one");
        assert_eq!(
            resolve("/app0/game.bin"),
            Some(std::path::PathBuf::from("/titles/one").join("game.bin"))
        );
        assert_eq!(
            resolve("/app0/Textures/ui_assets.gnf"),
            Some(std::path::PathBuf::from("/titles/one").join("Textures/ui_assets.gnf"))
        );
    }

    /// A title reading its shipped cache at the engine's host path gets the file from its own
    /// directory, and `/app0` names the same file (D709).
    #[test]
    fn a_rage_title_reads_its_shipped_cache_from_the_host_app_path() {
        let _guard = crate::exclusively();
        clear();
        mount_title(std::path::Path::new("/titles/gtav/eboot.bin"));

        let shipped = std::path::PathBuf::from("/titles/gtav").join("rpf.cache");
        assert_eq!(
            resolve("/host//ap/rpf.cache"),
            Some(shipped.clone()),
            "the host cache path resolves into the title's own directory, not an ENOENT"
        );
        assert_eq!(
            resolve("/app0/rpf.cache"),
            Some(shipped),
            "and it is the same shipped file /app0 names - one file, no copy"
        );
    }

    /// Climbing out of a mount is refused.
    #[test]
    fn climbing_out_of_a_mount_is_refused() {
        // Resolving it would hand arbitrary host files to guest code.
        let _guard = with_app_mount("/titles/one");
        assert_eq!(resolve("/app0/../../etc/passwd"), None);
        assert_eq!(resolve("/app0/a/../../b"), None);
        assert_eq!(
            resolve("/app0/..%2f.."),
            Some(std::path::PathBuf::from("/titles/one").join("..%2f..")),
            "only real components climb; an unescaped literal is just a filename"
        );
    }

    /// `.` names the directory it is in; `..` is still refused.
    #[test]
    fn a_current_directory_component_names_the_directory_it_is_in() {
        let _guard = with_app_mount("/titles/one");
        let root = std::path::PathBuf::from("/titles/one");
        assert_eq!(
            resolve("/app0/./data/ttf/font.ttf"),
            Some(root.join("data/ttf/font.ttf"))
        );
        assert_eq!(resolve("/app0/a/./b"), Some(root.join("a/b")));
        assert_eq!(resolve("/app0/."), Some(root.clone()));
        assert_eq!(
            resolve("/app0/./../secret"),
            None,
            "a `.` does not unlock `..`"
        );
        assert_eq!(
            resolve("/app0."),
            None,
            "`/app0.` is its own name, not the mount"
        );
    }

    /// A backslash cannot slip a component past the containment check.
    #[test]
    fn a_backslash_cannot_smuggle_a_component_past_the_check() {
        let _guard = with_app_mount("/titles/one");
        assert_eq!(resolve(r"/app0\..\..\secret"), None);
    }

    /// A prefix must match a whole component.
    #[test]
    fn a_prefix_must_match_a_whole_component() {
        let _guard = with_app_mount("/titles/one");
        assert_eq!(resolve("/app0extra/game.bin"), None);
    }

    /// A path under no mount is refused.
    #[test]
    fn a_path_under_no_mount_is_refused_rather_than_guessed_at() {
        let _guard = with_app_mount("/titles/one");
        assert_eq!(resolve("/savedata/slot0"), None);
        assert_eq!(resolve("relative/path"), None);
    }

    /// The mount itself resolves to its root.
    #[test]
    fn the_mount_itself_resolves_to_its_root() {
        let _guard = with_app_mount("/titles/one");
        assert_eq!(
            resolve("/app0"),
            Some(std::path::PathBuf::from("/titles/one"))
        );
        assert_eq!(
            resolve("/app0/"),
            Some(std::path::PathBuf::from("/titles/one"))
        );
    }

    /// Containment is decided on components, not on spelling.
    #[test]
    fn containment_is_decided_on_components_not_on_spelling() {
        assert!(is_contained("a/b/c.bin"));
        assert!(!is_contained(".."));
        assert!(!is_contained("a/../b"));
        assert!(!is_contained("/absolute"));
        assert!(!is_contained(""));
    }
}
