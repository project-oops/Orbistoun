//! Delivering the bytes the asynchronous file path was asked for.
//!
//! # Why this is a hook and not a call
//!
//! The three `sceKernelApr*` functions are file I/O under the `libkernel` name, and this crate
//! declares `libkernel`. Reading a file is `orbistoun-fs`, which is a **sibling subsystem** -
//! the same relation `orbistoun-libc` has, and the reason the clocks moved down to
//! `orbistoun-hle` rather than one crate reaching across (D536).
//!
//! Splitting `libkernel` across two crates would be the alternative and is a bigger change than
//! this earns: nothing else does it, and a library with two owners is a new concept rather than
//! a new function. So the reader is installed from above, by the layer that already depends on
//! both - the same inversion `on_guest_stop` uses, for the same reason (D160).
//!
//! # It is an experiment, and it says so
//!
//! Nothing here is established. The guest submits a command buffer whose header claims one
//! command of twenty bytes and whose storage is entirely zero, having never called the library
//! function that would put a command there (D587). Delivering the file it resolved *anyway* is a
//! guess about what the buffer means, and the guest is the only thing that can grade it.
//!
//! So it is off unless asked for, and `ORBISTOUN_APR_DELIVER` is declared as intervening: a
//! verdict under it carries the caveat the run report prints, because a wall that moves under an
//! intervention is not a diagnosis (D224, D226, D227).

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
/// **Remembered because the submit call does not carry them.** The guest resolves a path, builds
/// a buffer, and submits it - and what the buffer says about *which* file is exactly the part
/// that is not established. Using the last resolved path is the guess this whole module is.
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
/// zero bytes - and the caller says which.
pub(crate) fn deliver(path: &str, address: u64, most: u64) -> Option<usize> {
    READER.get().and_then(|read| read(path, address, most))
}

/// Looks a guest path up in the title's index, answering an identifier and a size.
///
/// Installed from above like [`on_file_read`], and for the same reason: the index is a file.
type Lookup = fn(&str) -> Option<(u64, u64)>;

/// The installed lookup, if anything installed one.
static LOOKUP: OnceLock<Lookup> = OnceLock::new();

/// Installs the index lookup the asynchronous file path resolves against.
pub fn on_index_lookup(lookup: Lookup) {
    let _ = LOOKUP.set(lookup);
}

/// What the index says about `path`, or nothing.
///
/// [`None`] when there is no index or the path is not in it - both of which are answers a guest
/// can be told honestly, and neither of which is a size invented to fill the gap.
pub(crate) fn look_up(path: &str) -> Option<(u64, u64)> {
    LOOKUP.get().and_then(|look| look(path))
}
