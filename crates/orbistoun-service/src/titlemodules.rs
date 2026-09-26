//! Finding the modules a title ships with itself.
//!
//! An executable's vendor tables name the libraries it imports from with bare names and no paths
//! (D482). The platform's own modules are resident and are not loaded from here. A title's own
//! modules sit in its tree, where the filename is the library name and the directory is a
//! convention of whatever built the title, so the loader searches the tree for the name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Extensions a title's own module is shipped with. Title modules use `.prx` and the platform's
/// `.sprx`, but nothing guarantees a title follows that, so both are searched.
const MODULE_EXTENSIONS: [&str; 2] = ["prx", "sprx"];

/// How deep to look before giving up.
///
/// Deeper than any layout in the corpus (`Media/Modules/` is two), and bounded so a large data tree
/// does not turn a load into a filesystem walk. A module deeper than this is reported as not found.
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
/// Matched on the file's stem against the library name, case-insensitively, preferring an
/// exact-case match. The case-insensitivity is for the host filesystem; titles spell the filename
/// as they spell the import (D482). A name nothing answers is absent from the result, which is
/// normal for the platform's own libraries.
#[must_use]
pub fn find(root: &Path, wanted: &[String]) -> Vec<TitleModule> {
    let candidates = shipped_modules(root);
    let mut out = Vec::new();
    for library in wanted {
        let lowered = library.to_lowercase();
        let Some(matches) = candidates.get(&lowered) else {
            continue;
        };
        // Exact case wins where the filesystem offered several spellings; otherwise the first,
        // which is stable because the walk sorts.
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
/// Collected in one walk: a title imports from dozens of libraries, almost all the platform's, so a
/// walk per name would search the tree dozens of times for nothing.
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

    /// A module is found by the name that imports it, wherever the title put it; `Media/Modules/`
    /// is a build convention, not a platform path.
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

    /// A name the title does not ship is absent, not an error.
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

    /// The case a title uses is the case it gets back, such as `Il2cppUserAssemblies` with a
    /// lower-case `c` in both import and filename.
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

    /// A file whose case differs from the import is still found, and the exact one is preferred.
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
