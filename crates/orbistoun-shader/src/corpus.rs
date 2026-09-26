//! Capturing every shader the guest hands over.
//!
//! Every shader a title uploads is stored by content hash, so the corpus accumulates as
//! titles run and a translator change is checked by re-running it and diffing. Naming by
//! hash rather than capture order means re-running a title adds nothing and two titles'
//! corpora diff to what they share. [`shader_id`] is the pure identity rule;
//! [`ShaderCorpus`] is the thin filesystem layer over it (D016).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::ShaderError;

/// File extension for a shader dumped from a title.
///
/// Guest-derived material is never tracked; the provenance guard bans this extension.
pub const SHADER_EXTENSION: &str = "bin";

/// File extension for a shader generated here, from source in `tools/`.
///
/// These are committed so the differential test runs on a machine with no LLVM; a
/// separate extension keeps them distinct from guest dumps, which must never be tracked.
pub const GENERATED_EXTENSION: &str = "gcn";

/// Whether a path holds a shader this crate will read.
///
/// Both kinds decode identically; they differ only in where they came from and whether
/// they may be committed.
pub fn is_shader(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some(SHADER_EXTENSION | GENERATED_EXTENSION)
    )
}

/// How many hex characters of the hash name a shader.
///
/// 16 hex characters are 64 bits, enough to avoid collisions in any corpus this holds;
/// the identity is for deduplication and naming, not security.
pub const ID_LENGTH: usize = 16;

/// The identity of a shader: a truncated hash of its bytes.
///
/// Pure, so the naming rule is testable with no filesystem involved.
pub fn shader_id(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    use sha1::{Digest, Sha1};
    let digest = Sha1::digest(bytes);
    let mut out = String::with_capacity(ID_LENGTH);
    for byte in &digest {
        if out.len() >= ID_LENGTH {
            break;
        }
        // Writing into a String cannot fail; discarding the result keeps this panic-free.
        let _ = write!(out, "{byte:02x}");
    }
    out.truncate(ID_LENGTH);
    out
}

/// What happened when a shader was offered to the corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    /// Not seen before; written.
    Added,
    /// Already held, byte for byte. Nothing written.
    ///
    /// The common case: titles re-upload the same shaders constantly, so capture is
    /// cheap enough to leave on.
    AlreadyHeld,
}

/// A directory of captured shaders.
#[derive(Debug, Clone)]
pub struct ShaderCorpus {
    root: PathBuf,
    held: BTreeSet<String>,
}

impl ShaderCorpus {
    /// Opens a corpus at `root`, creating it if absent, and indexes what is already
    /// there.
    ///
    /// Indexing up front lets a re-run recognise every shader without writing anything.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ShaderError> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)
            .map_err(|e| ShaderError::Corpus(format!("{}: {e}", root.display())))?;

        let mut held = BTreeSet::new();
        let entries = std::fs::read_dir(&root)
            .map_err(|e| ShaderError::Corpus(format!("{}: {e}", root.display())))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if is_shader(&path) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    held.insert(stem.to_owned());
                }
            }
        }
        Ok(Self { root, held })
    }

    /// Offers a shader to the corpus.
    ///
    /// Returns its identity and whether it was new. Never overwrites an existing file.
    pub fn capture(&mut self, bytes: &[u8]) -> Result<(String, Capture), ShaderError> {
        let id = shader_id(bytes);
        if self.held.contains(&id) {
            return Ok((id, Capture::AlreadyHeld));
        }
        let path = self.path_for(&id);
        std::fs::write(&path, bytes)
            .map_err(|e| ShaderError::Corpus(format!("{}: {e}", path.display())))?;
        self.held.insert(id.clone());
        Ok((id, Capture::Added))
    }

    /// Reads a stored shader back.
    pub fn load(&self, id: &str) -> Result<Vec<u8>, ShaderError> {
        let path = self.path_for(id);
        std::fs::read(&path).map_err(|e| ShaderError::Corpus(format!("{}: {e}", path.display())))
    }

    /// Every shader held, in a stable order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.held.iter().map(String::as_str)
    }

    /// How many shaders are held.
    pub fn len(&self) -> usize {
        self.held.len()
    }

    /// Whether the corpus holds nothing.
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// Where the corpus lives.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.{SHADER_EXTENSION}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{Capture, ID_LENGTH, ShaderCorpus, shader_id};

    /// The same bytes always get the same name, so a corpus never holds duplicates.
    #[test]
    fn identity_is_a_function_of_content_alone() {
        assert_eq!(shader_id(b"same bytes"), shader_id(b"same bytes"));
        assert_ne!(shader_id(b"one"), shader_id(b"two"));
        assert_eq!(shader_id(b"anything").len(), ID_LENGTH);
    }

    /// A zero-length capture is named and stored, not a crash.
    #[test]
    fn an_empty_shader_still_has_an_identity() {
        assert_eq!(shader_id(b"").len(), ID_LENGTH);
    }

    /// A second capture of the same bytes writes nothing.
    #[test]
    fn a_repeated_shader_is_recognised_rather_than_rewritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut corpus = ShaderCorpus::open(dir.path()).expect("open");

        let (first_id, first) = corpus.capture(b"shader body").expect("capture");
        let (second_id, second) = corpus.capture(b"shader body").expect("capture");

        assert_eq!(first, Capture::Added);
        assert_eq!(second, Capture::AlreadyHeld);
        assert_eq!(first_id, second_id);
        assert_eq!(corpus.len(), 1);
    }

    /// Reopening a corpus indexes the shaders already on disk.
    #[test]
    fn a_corpus_reopened_recognises_what_it_already_holds() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut corpus = ShaderCorpus::open(dir.path()).expect("open");
            corpus.capture(b"from an earlier run").expect("capture");
        }
        let mut reopened = ShaderCorpus::open(dir.path()).expect("reopen");
        assert_eq!(reopened.len(), 1, "existing files must be indexed");
        let (_, capture) = reopened.capture(b"from an earlier run").expect("capture");
        assert_eq!(capture, Capture::AlreadyHeld);
    }

    /// A stored shader round-trips byte for byte.
    #[test]
    fn stored_bytes_come_back_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut corpus = ShaderCorpus::open(dir.path()).expect("open");
        let body: Vec<u8> = (0..=255u8).collect();
        let (id, _) = corpus.capture(&body).expect("capture");
        assert_eq!(corpus.load(&id).expect("load"), body);
    }

    /// Identities list in sorted order, so reports diff cleanly.
    #[test]
    fn ids_are_listed_in_a_stable_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut corpus = ShaderCorpus::open(dir.path()).expect("open");
        for body in [b"a".as_slice(), b"b", b"c", b"d"] {
            corpus.capture(body).expect("capture");
        }
        let once: Vec<String> = corpus.ids().map(str::to_owned).collect();
        let twice: Vec<String> = corpus.ids().map(str::to_owned).collect();
        assert_eq!(once, twice);
        let mut sorted = once.clone();
        sorted.sort();
        assert_eq!(once, sorted);
    }

    /// Opening a corpus at a missing path creates the directory.
    #[test]
    fn opening_a_missing_directory_creates_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("does").join("not").join("exist");
        let corpus = ShaderCorpus::open(&nested).expect("should create");
        assert!(corpus.is_empty());
        assert!(nested.exists());
    }
}
