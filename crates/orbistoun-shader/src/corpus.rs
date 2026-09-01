//! Capturing every shader the guest hands over.
//!
//! # A corpus that builds itself
//!
//! Every shader a title uploads is a test case. Stored by content hash, the set
//! accumulates as titles are run and never needs anyone to write it - and once it
//! exists, changing the translator means re-running the whole corpus and diffing, so
//! a regression is visible immediately rather than the next time someone happens to
//! load the affected scene.
//!
//! That is the same trick as the run report: the artefact captured for one purpose is
//! the test data for another.
//!
//! # Identity is content, not order
//!
//! Shaders are named by the hash of their bytes. Two runs of the same title produce
//! the same names in the same corpus, so re-running adds nothing and a diff between
//! two titles shows exactly what they share. Naming by capture order would make every
//! run look entirely new.
//!
//! # The pure part is separate
//!
//! [`shader_id`] is a pure function of bytes and is where the identity rule lives;
//! [`ShaderCorpus`] is the thin layer that touches the filesystem. The D016 pattern -
//! the rule is testable without a directory existing.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::ShaderError;

/// File extension for a shader **dumped from a title**.
///
/// Console-derived, so it must never be tracked - the provenance guard bans this
/// extension outright and that ban is correct.
pub const SHADER_EXTENSION: &str = "bin";

/// File extension for a shader **generated here**, from source in `tools/`.
///
/// The opposite obligation: these are committed on purpose, so the differential test
/// runs on a machine with no LLVM. They were `.bin` too until the provenance guard
/// rejected them, which was the guard being right - one extension for material that must
/// never be tracked and material that must be is a distinction waiting to be lost.
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
/// Full SHA-1 is 40 and unwieldy in a report; 16 is 64 bits, which will not collide
/// across any corpus this will ever hold. Truncating is safe here precisely because
/// identity is used for deduplication and naming rather than for security.
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
        // Writing into a String cannot fail; the result is discarded rather than
        // unwrapped so a formatting change can never introduce a panic here.
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
    /// The common case by a wide margin - a title re-uploads the same shaders
    /// constantly - and the reason capture is cheap enough to leave switched on.
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
    /// Reading the existing set up front is what makes re-running a title nearly free:
    /// the second run recognises every shader without writing anything.
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
    /// Returns its identity and whether it was new. Never overwrites: a hash
    /// collision on identical content is not a collision, and on different content
    /// would be a far bigger problem than a lost write.
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

    #[test]
    fn identity_is_a_function_of_content_alone() {
        // Two captures of the same shader from different runs must land on the same
        // name, or a corpus grows a duplicate every time a title is launched.
        assert_eq!(shader_id(b"same bytes"), shader_id(b"same bytes"));
        assert_ne!(shader_id(b"one"), shader_id(b"two"));
        assert_eq!(shader_id(b"anything").len(), ID_LENGTH);
    }

    #[test]
    fn an_empty_shader_still_has_an_identity() {
        // A zero-length capture is a finding worth storing, not a crash.
        assert_eq!(shader_id(b"").len(), ID_LENGTH);
    }

    #[test]
    fn a_repeated_shader_is_recognised_rather_than_rewritten() {
        // Titles re-upload the same shaders constantly. If every upload wrote a file,
        // capture would be far too expensive to leave switched on.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut corpus = ShaderCorpus::open(dir.path()).expect("open");

        let (first_id, first) = corpus.capture(b"shader body").expect("capture");
        let (second_id, second) = corpus.capture(b"shader body").expect("capture");

        assert_eq!(first, Capture::Added);
        assert_eq!(second, Capture::AlreadyHeld);
        assert_eq!(first_id, second_id);
        assert_eq!(corpus.len(), 1);
    }

    #[test]
    fn a_corpus_reopened_recognises_what_it_already_holds() {
        // The property that makes a second run of a title nearly free.
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

    #[test]
    fn stored_bytes_come_back_unchanged() {
        // The corpus is the regression suite; a shader that does not round-trip would
        // silently change what every future comparison is comparing against.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut corpus = ShaderCorpus::open(dir.path()).expect("open");
        let body: Vec<u8> = (0..=255u8).collect();
        let (id, _) = corpus.capture(&body).expect("capture");
        assert_eq!(corpus.load(&id).expect("load"), body);
    }

    #[test]
    fn ids_are_listed_in_a_stable_order() {
        // Reports get diffed. Directory order would make every listing differ from the
        // last for no reason.
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

    #[test]
    fn opening_a_missing_directory_creates_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("does").join("not").join("exist");
        let corpus = ShaderCorpus::open(&nested).expect("should create");
        assert!(corpus.is_empty());
        assert!(nested.exists());
    }
}
