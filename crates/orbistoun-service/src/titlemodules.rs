//! Finding the modules a title ships with itself.
//!
//! # Why this is a search rather than a lookup
//!
//! An executable's vendor tables name the libraries it imports from and say **nothing about
//! where they live** - bare names, no paths, in both the library table and the module table
//! (D482). So a loader given `Il2CppUserAssemblies` has to go and find it.
//!
//! The platform's own modules are in three fixed directories and are not this: they are
//! resident, and a request for one is refused rather than loaded. What is left is the title's
//! own tree, where the corpus is consistent - **the filename is the library name** - and where
//! the directory is a convention of whatever built the title rather than anything the platform
//! guarantees. Hence: search the tree for the name, do not walk to a path.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Extensions a title's own module is shipped with.
///
/// Both are seen in the corpus; a title's own modules use `.prx` and the platform's own
/// `.sprx`, but nothing guarantees a title follows that so both are searched.
const MODULE_EXTENSIONS: [&str; 2] = ["prx", "sprx"];

/// How deep to look before giving up.
///
/// Deep enough for every layout in the corpus - the deepest is `Media/Modules/` at two - with
/// room to spare, and bounded so a title with a large data tree does not turn a load into a
/// filesystem walk. A module that is deeper than this is reported as not found, which is the
/// truth, rather than the search running on.
const MAX_DEPTH: usize = 6;

/// A module the title ships, and the library name that asked for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleModule {
    /// The library name an import carries, exactly as the executable spells it.
    pub library: String,
    /// Where the file was found.
    pub path: PathBuf,
}

/// Finds the file for each library name, under the title's own root.
///
/// **Matched on the file's stem against the library name**, case-insensitively, with an
/// exact-case match preferred where several files answer. The case-insensitive part is for the
/// host filesystem rather than for the guest: every title in the corpus spells its filename
/// exactly as it spells the import, including one that uses a lower-case `c` in both (D482).
///
/// A name nothing answers is simply absent from the result. That is not an error here - plenty
/// of the names an executable imports from are the platform's, and those are supposed to be
/// missing from the title's tree.
#[must_use]
pub fn find(root: &Path, wanted: &[String]) -> Vec<TitleModule> {
    let candidates = shipped_modules(root);
    let mut out = Vec::new();
    for library in wanted {
        let lowered = library.to_lowercase();
        let Some(matches) = candidates.get(&lowered) else {
            continue;
        };
        // Exact case wins where the filesystem offered more than one spelling; otherwise the
        // first, which is stable because the walk sorts.
        let chosen = matches
            .iter()
            .find(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s == library)
            })
            .or_else(|| matches.first());
        if let Some(path) = chosen {
            out.push(TitleModule {
                library: library.clone(),
                path: path.clone(),
            });
        }
    }
    out
}

/// Every module-shaped file under the title root, by lower-cased stem.
///
/// Collected in one walk rather than one per name: a title imports from dozens of libraries
/// and almost all of them are the platform's, so searching the tree per name would walk it
/// dozens of times to find nothing.
fn shipped_modules(root: &Path) -> BTreeMap<String, Vec<PathBuf>> {
    let mut found: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut queue = vec![(root.to_path_buf(), 0_usize)];
    while let Some((dir, depth)) = queue.pop() {
        if depth > MAX_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        // Sorted, so a title with two spellings of one name resolves the same way twice.
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                queue.push((path, depth + 1));
                continue;
            }
            let is_module = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| MODULE_EXTENSIONS.contains(&e.to_lowercase().as_str()));
            if !is_module {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                found.entry(stem.to_lowercase()).or_default().push(path);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{TitleModule, find};

    /// Builds a title tree: each entry is a path relative to the root.
    fn title(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for f in files {
            let path = dir.path().join(f);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("the parent directory");
            }
            std::fs::write(&path, b"not really a module").expect("the file");
        }
        dir
    }

    /// A module is found by the name that imports it, wherever the title put it.
    ///
    /// The corpus puts them under `Media/Modules/`, which is a convention of whatever built
    /// the title and not a platform path - so the search must not depend on it.
    #[test]
    fn a_module_is_found_by_name_wherever_it_sits() {
        let dir = title(&[
            "Media/Modules/Il2CppUserAssemblies.prx",
            "sce_module/libc.prx",
        ]);
        let found = find(
            dir.path(),
            &["Il2CppUserAssemblies".to_owned(), "libc".to_owned()],
        );
        let names: Vec<&str> = found.iter().map(|m| m.library.as_str()).collect();
        assert_eq!(names, ["Il2CppUserAssemblies", "libc"]);
        assert!(found[0].path.ends_with("Il2CppUserAssemblies.prx"));
    }

    /// **A name the title does not ship is absent, not an error.**
    ///
    /// Most of what an executable imports from is the platform's, and those are supposed to
    /// be missing from the title's own tree. Reporting them would make every load look broken.
    #[test]
    fn a_platform_library_is_simply_not_there() {
        let dir = title(&["Media/Modules/Il2CppUserAssemblies.prx"]);
        let found = find(
            dir.path(),
            &["libkernel".to_owned(), "Il2CppUserAssemblies".to_owned()],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].library, "Il2CppUserAssemblies");
    }

    /// The case a title actually uses is the case it gets back.
    ///
    /// One title in the corpus spells the library `Il2cppUserAssemblies` - lower-case `c` - in
    /// its imports *and* in its filename. A match that normalised the answer would hand the
    /// loader a name the executable never asked for.
    #[test]
    fn the_library_name_comes_back_as_the_executable_spells_it() {
        let dir = title(&["Media/Modules/Il2cppUserAssemblies.prx"]);
        let found = find(dir.path(), &["Il2cppUserAssemblies".to_owned()]);
        assert_eq!(
            found,
            vec![TitleModule {
                library: "Il2cppUserAssemblies".to_owned(),
                path: dir.path().join("Media/Modules/Il2cppUserAssemblies.prx"),
            }]
        );
    }

    /// A file whose case differs from the import is still found, and the exact one preferred.
    ///
    /// The case-insensitive half is for the host filesystem, not the guest: a tree that
    /// preserved case differently would otherwise turn a working title into a missing module.
    #[test]
    fn an_exact_spelling_wins_where_the_filesystem_offers_two() {
        let dir = title(&["a/Il2CppUserAssemblies.prx", "b/il2cppuserassemblies.prx"]);
        let found = find(dir.path(), &["Il2CppUserAssemblies".to_owned()]);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].path.ends_with("Il2CppUserAssemblies.prx"),
            "the exactly-spelled file is the one meant: {:?}",
            found[0].path
        );
    }

    /// Something that is not a module is not a module, whatever it is called.
    #[test]
    fn a_file_that_is_not_a_module_is_not_matched() {
        let dir = title(&["Media/Modules/Il2CppUserAssemblies.dat"]);
        assert!(find(dir.path(), &["Il2CppUserAssemblies".to_owned()]).is_empty());
    }
}
