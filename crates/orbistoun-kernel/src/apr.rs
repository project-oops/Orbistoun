//! Asynchronous file reads (`sceKernelApr*`), delivered through a reader installed from above.
//!
//! These functions are file I/O under the `libkernel` name. Reading a file belongs to
//! `orbistoun-fs`, a sibling crate, so the layer that depends on both installs the reader here
//! rather than one crate reaching across (D536).
//!
//! Delivery is a guess: the guest submits a command buffer whose storage is all zero, and this
//! delivers the file it last resolved. It runs only under `ORBISTOUN_APR_DELIVER`, which is
//! declared as intervening so a verdict under it carries the caveat (D227).

use std::sync::{Mutex, OnceLock};

/// Reads a guest path into guest memory, installed by a layer that can reach the filesystem.
///
/// `(guest_path, address, most)` to how many bytes arrived.
type Reader = fn(&str, u64, u64) -> Option<usize>;

/// The installed reader, if anything installed one.
static READER: OnceLock<Reader> = OnceLock::new();

/// Installs the reader the asynchronous file path uses.
///
/// Called once, before the guest runs, by the layer that owns both this crate and the
/// filesystem. A run where nothing installs one delivers nothing and says so.
pub fn on_file_read(reader: Reader) {
    let _ = READER.set(reader);
}

/// The paths the last resolve call was asked about.
///
/// Kept because the submit call does not carry a path; a submit is assumed to be about the last
/// resolved one.
fn resolved() -> &'static Mutex<Vec<String>> {
    static RESOLVED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    RESOLVED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Records what a resolve call named.
pub(crate) fn note_resolved(paths: Vec<String>) {
    if let Ok(mut held) = resolved().lock() {
        *held = paths;
    }
}

/// The path a submit is assumed to be about, or nothing.
pub(crate) fn last_resolved() -> Option<String> {
    resolved().lock().ok()?.first().cloned()
}

/// Reads `path` into `[address, address + most)`, answering how many bytes arrived.
///
/// [`None`] when nothing installed a reader, which is a different finding from a file that read
/// zero bytes.
pub(crate) fn deliver(path: &str, address: u64, most: u64) -> Option<usize> {
    READER.get().and_then(|read| read(path, address, most))
}

/// Looks a guest path up in the title's index, answering an identifier and a size.
///
/// Installed from above like [`on_file_read`], because the index is a file.
type Lookup = fn(&str) -> Option<(u64, u64)>;

/// The installed lookup, if anything installed one.
static LOOKUP: OnceLock<Lookup> = OnceLock::new();

/// Installs the index lookup the asynchronous file path resolves against.
pub fn on_index_lookup(lookup: Lookup) {
    let _ = LOOKUP.set(lookup);
}

/// What the index says about `path`, or nothing.
///
/// [`None`] when there is no index or the path is not in it; no size is invented.
pub(crate) fn look_up(path: &str) -> Option<(u64, u64)> {
    LOOKUP.get().and_then(|look| look(path))
}

/// Answers the size of an existing regular guest file, installed from above like [`on_file_read`].
type Sizer = fn(&str) -> Option<u64>;

/// The installed sizer, if anything installed one.
static SIZER: OnceLock<Sizer> = OnceLock::new();

/// Installs what the identifiers synthesised for files outside the index (D782) ask for a file's
/// size.
pub fn on_file_size(sizer: Sizer) {
    let _ = SIZER.set(sizer);
}

/// The first identifier synthesised for a file outside the title's index (D782): a range of its
/// own, so a synthesised identifier never meets one an index assigns.
pub(crate) const SYNTHESISED_FIRST: u64 = 0x4000_0000;

/// The paths given synthesised identifiers, in the order they were first asked for: the identifier
/// is [`SYNTHESISED_FIRST`] plus the position.
fn synthesised() -> &'static Mutex<Vec<String>> {
    static SYNTHESISED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    SYNTHESISED.get_or_init(|| Mutex::new(Vec::new()))
}

/// An identifier and size for `path` when the index has none (D782): an existing file under
/// `/app0/` gets one of its own, the same every time it is asked for, with its real size. Nothing
/// for a path outside `/app0/`, which the hardware refuses, or for a file that does not exist.
pub(crate) fn synthesise(path: &str) -> Option<(u64, u64)> {
    if !path.starts_with("/app0/") {
        return None;
    }
    let size = SIZER.get().and_then(|size_of| size_of(path))?;
    let mut held = synthesised().lock().ok()?;
    let position = if let Some(position) = held.iter().position(|known| known == path) {
        position
    } else {
        held.push(path.to_owned());
        held.len() - 1
    };
    Some((SYNTHESISED_FIRST + position as u64, size))
}

#[cfg(test)]
mod tests {
    /// A test filesystem: one existing file and one directory.
    fn size_of(path: &str) -> Option<u64> {
        (path == "/app0/Media/ScriptingAssemblies.json").then_some(0x1b0)
    }

    /// An existing `/app0` file the index does not name gets an identifier of its own in the
    /// synthesised range, with its real size, and the same one when asked again (D782); a missing
    /// file or a path outside `/app0` gets none.
    #[test]
    fn a_file_outside_the_index_gets_a_stable_synthesised_identifier() {
        super::on_file_size(size_of);
        let first = super::synthesise("/app0/Media/ScriptingAssemblies.json");
        let (id, size) = first.expect("an existing app0 file resolves");
        assert!(id >= super::SYNTHESISED_FIRST, "in the synthesised range");
        assert_eq!(size, 0x1b0);
        assert_eq!(
            super::synthesise("/app0/Media/ScriptingAssemblies.json"),
            first
        );
        assert_eq!(super::synthesise("/app0/Media/missing.json"), None);
        assert_eq!(super::synthesise("/data/ScriptingAssemblies.json"), None);
    }
}
