//! Where a guest path lands on the host.
//!
//! # One mount, and why that is enough for now
//!
//! A title asks for `/app0/game.bin` and `/app0/Textures/ui_assets.gnf`. Both are sitting
//! in the title's own directory - the files exist, there was simply nothing to hand them
//! over. So `/app0` maps to the directory the module was loaded from, and that single
//! mapping serves every path observed so far.
//!
//! More mounts will be needed (save data, downloads, the system's own paths) and the
//! table takes them without changing shape. What it will not take is a special case for a
//! particular title - per-title behaviour belongs in the overrides layer, keyed by a named
//! setting, never by an `if` on a title id.
//!
//! # Escaping the mount is refused, and that is not paranoia
//!
//! A guest chooses these paths, and `/app0/../../../windows/system32/config/sam` is a
//! path. Resolving it naively hands arbitrary host files to guest code we are deliberately
//! running without trusting. So resolution walks components and refuses anything that
//! climbs out, rather than resolving first and checking afterwards - a check after the
//! fact is one symlink away from being wrong (D165).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Where a title's own files live, from the guest's point of view.
pub const APP_MOUNT: &str = "/app0";

/// Where a guest may write.
///
/// The console gives an application writable storage separate from its read-only title,
/// and orbistoun had nothing there - so every `open` under it failed. The conformance
/// probe does that in its first few calls, to create its report, and then hands the
/// failure straight to `read` as though it were a descriptor (D250).
pub const DATA_MOUNT: &str = "/data";

/// The mount table.
fn mounts() -> &'static Mutex<BTreeMap<String, Vec<PathBuf>>> {
    static MOUNTS: OnceLock<Mutex<BTreeMap<String, Vec<PathBuf>>>> = OnceLock::new();
    MOUNTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Points a guest prefix at a host directory.
///
/// Replaces any previous mapping for that prefix, because a run configures its mounts
/// once and two mappings for one prefix has no meaning.
pub fn mount(guest_prefix: &str, host_root: PathBuf) {
    if let Ok(mut mounts) = mounts().lock() {
        mounts.insert(guest_prefix.to_owned(), vec![host_root]);
    }
}

/// Puts a host directory *over* whatever is already mounted at a prefix.
///
/// # Why a mount is a stack rather than a directory
///
/// The console's tree is one thing and a title's own files are another, and the guest must
/// see them as a single namespace. Merging them on disk would mean copying the base tree
/// per title and losing track of which files came from where; merging them here costs a
/// list walk per resolve and keeps the base reproducible - it can be deleted and rebuilt
/// from its manifest at any time, which is the test that it really is derived (D251).
///
/// The overlay goes first, so a file a title has written shadows the base, and every write
/// lands in the overlay by construction rather than by a rule somebody has to remember.
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
/// Takes the *module* path - the thing that was run - because that is what every caller
/// already has, and deriving the directory here means nobody has to remember to.
pub fn mount_title(module: &Path) {
    if let Some(directory) = module.parent() {
        // **Layered, not mounted.** `mount` replaces every root at a prefix, so this used
        // to discard the base tree installed underneath it - and installing the base
        // afterwards discarded the title instead, which cost one title its textures. The
        // title goes *over* the base: its own files answer first, and anything the console
        // provides is still there behind them (D269).
        layer(APP_MOUNT, directory.to_path_buf());
    }
}

/// Points `/data` at a host directory a guest may write into.
///
/// Created here rather than at first write: a guest asking for a file under a mount whose
/// host directory does not exist gets the same failure as one asking for a file that is
/// not there, and the two call for completely different responses.
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
/// Set from the filesystem manifest rather than from a constant here. Which directories a
/// guest may write to is a fact about the console, so it belongs beside the rest of what
/// this project claims about it, with the same account of how it is known - not spelled a
/// second time in code where the two can drift (D251).
pub fn allow_writes(guest_prefix: &str) {
    if let Ok(mut writable) = writable().lock() {
        writable.insert(guest_prefix.to_owned());
    }
}

/// Whether a guest path lies under a mount a guest may write to.
///
/// A title's own directory is the material being measured, and a guest able to write into
/// it would be editing its own evidence. Asked by prefix, so a path that climbs out of a
/// mount is still refused by [`resolve`] as it always was (D250).
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
/// Walks components rather than resolving and checking afterwards. `..` is refused
/// outright rather than cancelled against a preceding component, because cancelling is
/// only correct when nothing in the path is a symbolic link - and a guest supplies these.
///
/// Pure, and therefore testable without a filesystem, which is the shape principle 8 asks
/// for: the rule is the part worth protecting.
pub fn is_contained(relative: &str) -> bool {
    !relative.is_empty()
        && Path::new(relative)
            .components()
            .all(|component| matches!(component, Component::Normal(part) if !part.is_empty()))
}

/// The host path a guest path names, or `None`.
///
/// `None` covers a path under no mount and a path that tries to climb out of one. Both are
/// refusals rather than errors: a guest asking for something it cannot have gets the same
/// answer as a guest asking for something that is not there, which is what the interface
/// it thinks it is calling would tell it.
pub fn resolve(guest_path: &str) -> Option<PathBuf> {
    // Normalised so `\` from a guest that mixes conventions cannot slip a component past
    // the component walk below.
    let guest_path = guest_path.replace('\\', "/");
    let mounts = mounts().lock().ok()?;
    for (prefix, roots) in mounts.iter() {
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
        if !is_contained(rest) {
            return None;
        }
        let mut found = None;
        for (index, root) in roots.iter().enumerate() {
            let candidate = root.join(rest);
            // The first layer that actually has it. A title's own copy shadows the base,
            // which is the whole point of layering rather than merging.
            if candidate.exists() {
                return Some(candidate);
            }
            if index == 0 {
                found = Some(candidate);
            }
        }
        // Nowhere yet: answer the writable layer, so creating a file puts it where a
        // title's data belongs and reading one fails because it is genuinely not there.
        return found;
    }
    None
}

/// Names that exist at `guest_path` only because a mount lies below it.
///
/// # Why the mount table is the only thing that knows
///
/// `/app0` and `/data` are directories a guest can enter, and **no host directory holds
/// them**: they are prefixes this project maps onto host roots that live somewhere else
/// entirely. So `resolve("/")` answers nothing, `opendir("/")` finds nothing, and the first
/// thing an FTP client does - `CWD /`, then `LIST` - has nowhere to go. `zftpd` logged a
/// client in and then answered `550 Not a directory.` (D385).
///
/// This synthesises the missing half. For `/` it answers `app0` and `data`; for a mount at
/// `/system_data/priv` it answers `system_data` at the root and `priv` under that, so an
/// intermediate directory nobody mounted still exists as far as a guest walking down to the
/// mount can tell.
///
/// **Only the next component**, never a whole prefix: a listing of `/` holds `system_data`,
/// not `system_data/priv`, because that is what a directory entry is.
///
/// Answers an empty list for a path with no mount below it, which is how a caller tells
/// "a directory this synthesises" from "not a directory at all".
#[must_use]
pub fn mounts_under(guest_path: &str) -> Vec<String> {
    let path = guest_path.replace('\\', "/");
    // **An empty path is not the root, and a relative one is not either.**
    //
    // `/` and `""` both trim to nothing, so trimming first made them the same path - and
    // `stat("")` answered *a directory with two entries in it*. `zftpd` stats every name in
    // a listing and passes an empty one for each, which on a real system fails and sends it
    // to `d_type` instead; here it succeeded, and every file came back as `drwxr-xr-x` of
    // size zero (D387).
    //
    // Nothing here has a working directory, which `getcwd` already reports, so a path that
    // does not start at the root names nothing this can find.
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
        // `/app0`, and a path equal to the prefix is the mount itself rather than under it.
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
/// because a mount sits below it - which is what makes `/` a directory.
#[must_use]
pub fn is_directory(guest_path: &str) -> bool {
    if !mounts_under(guest_path).is_empty() {
        return true;
    }
    resolve(guest_path).is_some_and(|host| host.is_dir())
}

#[cfg(test)]
mod tests {
    /// **The root lists its mounts, and an empty path lists nothing.**
    ///
    /// The pair matters: `/` and `""` both trim to nothing, so a rule written by trimming
    /// first makes them one path - and `stat("")` then answers a directory (D387).
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

    use super::{APP_MOUNT, clear, is_contained, mount, resolve};

    /// Mounts are process-global, so the tests that touch them set their own up and the
    /// assertions never depend on what another test left behind.
    /// Sets up a lone `/app0` mount, holding the crate-wide lock for the caller.
    ///
    /// **The guard has to be returned, not dropped here.** The mount table is
    /// process-global and these tests replace it wholesale; without holding the lock for
    /// the length of the test, another module's test clears it mid-assertion. That is the
    /// same race that made the descriptor tests fail two runs in five (D241), in the one
    /// module whose tests had never taken the lock.
    fn with_app_mount(root: &str) -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::exclusively();
        clear();
        mount(APP_MOUNT, std::path::PathBuf::from(root));
        guard
    }

    #[test]
    fn a_title_path_lands_in_the_title_directory() {
        // The whole point: the files a guest asks for are already on disk, in the
        // directory the module came from.
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

    #[test]
    fn climbing_out_of_a_mount_is_refused() {
        // A guest chooses these strings, and this one is a path like any other. Resolving
        // it would hand arbitrary host files to code we are running precisely because we
        // do not trust it.
        let _guard = with_app_mount("/titles/one");
        assert_eq!(resolve("/app0/../../etc/passwd"), None);
        assert_eq!(resolve("/app0/a/../../b"), None);
        assert_eq!(
            resolve("/app0/..%2f.."),
            Some(std::path::PathBuf::from("/titles/one").join("..%2f..")),
            "only real components climb; an unescaped literal is just a filename"
        );
    }

    #[test]
    fn a_backslash_cannot_smuggle_a_component_past_the_check() {
        // A guest that mixes conventions must not get a different answer than one that
        // does not, or the check is decoration.
        let _guard = with_app_mount("/titles/one");
        assert_eq!(resolve(r"/app0\..\..\secret"), None);
    }

    #[test]
    fn a_prefix_must_match_a_whole_component() {
        // `/app0extra` is not inside `/app0`, and matching on the string alone would say
        // it was.
        let _guard = with_app_mount("/titles/one");
        assert_eq!(resolve("/app0extra/game.bin"), None);
    }

    #[test]
    fn a_path_under_no_mount_is_refused_rather_than_guessed_at() {
        let _guard = with_app_mount("/titles/one");
        assert_eq!(resolve("/savedata/slot0"), None);
        assert_eq!(resolve("relative/path"), None);
    }

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

    #[test]
    fn containment_is_decided_on_components_not_on_spelling() {
        assert!(is_contained("a/b/c.bin"));
        assert!(!is_contained(".."));
        assert!(!is_contained("a/../b"));
        assert!(!is_contained("/absolute"));
        assert!(!is_contained(""));
    }
}
