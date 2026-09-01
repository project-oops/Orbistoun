//! The console's filesystem: a base tree this project can account for, and a title's own
//! files layered over it.
//!
//! # Why the base is generated rather than committed
//!
//! Every path in it is a claim about the platform, and a claim here says how it is known.
//! A committed directory tree cannot carry that - and git does not track empty directories
//! anyway, so the tree would have to be faked with placeholder files to survive a clone.
//!
//! So the source of truth is [`MANIFEST`], a knowledge file in the same shape as the
//! function knowledge base, and the directories are materialised from it. They can be
//! deleted at any time and rebuilt, which is the test that they really are derived (D251).

use std::path::Path;

/// The manifest, compiled in so a fresh install has it before it has anything else.
pub const MANIFEST: &str = include_str!("../data/filesystem.toml");

/// One directory the console is known to have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Where it appears in the guest's namespace.
    pub path: String,
    /// Whether a guest may write there.
    pub writable: bool,
    /// How its existence is known, in the knowledge base's vocabulary.
    pub known_by: String,
}

/// Reads the manifest.
///
/// Deliberately a small hand-rolled reader rather than a TOML dependency: this crate has
/// none, the file is a flat list, and a parse that silently produced an empty tree would
/// give every guest a filesystem with nothing in it and no error to explain it.
pub fn entries(manifest: &str) -> Vec<Entry> {
    let mut found = Vec::new();
    let mut current: Option<Entry> = None;
    for line in manifest.lines().map(str::trim) {
        if line == "[[entry]]" {
            if let Some(entry) = current.take() {
                found.push(entry);
            }
            current = Some(Entry {
                path: String::new(),
                writable: false,
                known_by: String::new(),
            });
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "path" => value.clone_into(&mut entry.path),
            "writable" => entry.writable = value == "true",
            "known_by" => value.clone_into(&mut entry.known_by),
            _ => {}
        }
    }
    if let Some(entry) = current {
        found.push(entry);
    }
    found.retain(|e| !e.path.is_empty());
    found
}

/// Creates the base tree and mounts it, with `overlay` on top for the running title.
///
/// Idempotent, and it never deletes: a directory that is already there is left alone, and
/// one a person added by hand survives. The base is rebuilt only in the sense that missing
/// entries reappear.
pub fn install(base: &Path, overlay: &Path) {
    for entry in entries(MANIFEST) {
        // `/data` becomes `<base>/data`. The leading slash is the guest's, not the host's.
        let under = entry.path.trim_start_matches('/');
        if under.is_empty() {
            continue;
        }
        let _ = std::fs::create_dir_all(base.join(under));
        crate::mount::mount(&entry.path, base.join(under));
        if entry.writable {
            let host = overlay.join(under);
            let _ = std::fs::create_dir_all(&host);
            // Over the base, so a file this title wrote shadows one that shipped, and every
            // write lands here by construction rather than by a rule to remember.
            crate::mount::layer(&entry.path, host);
            crate::mount::allow_writes(&entry.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MANIFEST, entries};

    /// The shipped manifest parses, and says what it claims about each path.
    ///
    /// A reader that silently produced nothing would hand every guest an empty filesystem
    /// with no error to explain it, which is the failure this whole project keeps writing
    /// decisions about (D251).
    #[test]
    fn the_shipped_manifest_describes_the_paths_it_claims() {
        let found = entries(MANIFEST);
        assert!(!found.is_empty(), "the manifest parsed to nothing");

        let app0 = found.iter().find(|e| e.path == "/app0").expect("/app0");
        assert!(!app0.writable, "the title's own content is not writable");

        let data = found.iter().find(|e| e.path == "/data").expect("/data");
        assert!(data.writable, "an application's storage is writable");

        // Every entry accounts for itself. An unaccounted path is a platform fact nobody
        // can check, and the tree is meant to grow by evidence (D242, D251).
        for entry in &found {
            assert!(
                !entry.known_by.is_empty(),
                "{} does not say how it is known",
                entry.path
            );
        }
    }

    /// Only what the manifest holds, and nothing that merely sounds plausible.
    ///
    /// A guest resolving a path orbistoun invented gets a fabricated platform fact back,
    /// which is worse than the failure it replaces - the failure is information.
    #[test]
    fn nothing_is_present_that_nothing_asked_for() {
        let found = entries(MANIFEST);
        for guessed in ["/system", "/hostapp", "/savedata", "/dev", "/mnt"] {
            assert!(
                !found.iter().any(|e| e.path == guessed),
                "{guessed} is in the manifest with nothing having asked for it"
            );
        }
    }
}
