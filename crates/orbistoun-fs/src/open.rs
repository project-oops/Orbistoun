//! Open files, and the handles a guest holds them by.
//!
//! # A handle is an address, for the third time
//!
//! `fopen` returns a `FILE *`, and the guest **dereferences it without checking**. That is
//! measured, not assumed: with the call unimplemented and answering an error code, the
//! guest carried that code through four more calls; answering null instead, it read offset
//! four of the null and faulted immediately.
//!
//! So the same rule as thread handles and lock handles (D151): the value handed back is the
//! address of a real, zeroed, never-freed block. A guest reading a field out of it gets a
//! zero rather than a fault, and a handle kept past a close reads as zeroes rather than as
//! a use-after-free.
//!
//! Nothing is written into the block, because the layout of the structure the guest thinks
//! it has is not known from any lawful source. Zero is the honest content.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// How the guest refers to an open file.
pub type FileHandle = u64;

/// Handle meaning "not open", and what a caller tests for.
pub const NO_FILE: FileHandle = 0;

/// How much zeroed memory sits behind a handle.
///
/// Matched to the other subsystems' control blocks, and generous next to any field a
/// caller reads at a small offset.
const CONTROL_BLOCK_WORDS: usize = 32;

/// One open file.
#[derive(Debug)]
struct Open {
    /// The host file.
    file: std::fs::File,
    /// The guest path, kept for traces - "which file" is the first thing anybody asks.
    path: String,
    /// Whether a read has hit the end.
    ///
    /// Tracked rather than derived, because `feof` is asked *after* the read that ended
    /// the file and the answer has to survive until then.
    at_end: bool,
}

/// Streams that are a descriptor rather than a file of their own.
///
/// # Why an FTP server needs this and nothing else did
///
/// A `FILE` is a buffered descriptor. Nothing here needed that until a server wanted to
/// `fdopen` an accepted connection and then `fprintf` its replies into it - which is how
/// every one of these servers writes a protocol.
///
/// Kept as a separate table rather than a field on [`Open`], because the two are genuinely
/// different things: an `Open` owns a host file, and this owns nothing - the descriptor table
/// does. A stream that closed a descriptor it did not own would close it out from under the
/// guest.
fn wrapped() -> &'static Mutex<BTreeMap<FileHandle, u64>> {
    static WRAPPED: OnceLock<Mutex<BTreeMap<FileHandle, u64>>> = OnceLock::new();
    WRAPPED.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Answers a stream handle that stands for an already-open descriptor.
///
/// The descriptor stays the descriptor table's: closing the stream forgets the wrapper and
/// leaves the descriptor open, which is not what `fclose` does on the platform and **is** what
/// this can promise. Stated in the knowledge file rather than papered over.
pub fn wrap_descriptor(fd: u64) -> Option<FileHandle> {
    let handle = next_handle();
    wrapped().lock().ok()?.insert(handle, fd);
    Some(handle)
}

/// The descriptor a stream wraps, if it wraps one.
#[must_use]
pub fn wrapped_descriptor(handle: FileHandle) -> Option<u64> {
    wrapped().lock().ok()?.get(&handle).copied()
}

/// Forgets a wrapper, leaving the descriptor it named alone.
pub fn unwrap_descriptor(handle: FileHandle) -> bool {
    wrapped()
        .lock()
        .is_ok_and(|mut w| w.remove(&handle).is_some())
}

/// How reads have gone, across the whole run.
///
/// **Completeness rather than content.** Verifying that delivered bytes match the file
/// would double every read; verifying that the guest got *as many bytes as it asked for*
/// costs a counter and catches the failure that matters. A title that silently receives a
/// truncated asset then faults somewhere in its own parser, which is exactly the shape of
/// wall that is hard to attribute (D175).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadStats {
    /// Reads attempted.
    pub reads: u64,
    /// Reads that delivered fewer bytes than asked for.
    ///
    /// Not automatically a fault: reading to the end of a file is a short read by
    /// definition, and it is how the end announces itself. A short read *before* the end
    /// is the defect, and the two are told apart by whether the file was already at its
    /// end.
    pub short: u64,
    /// Total bytes delivered.
    pub bytes: u64,
}

/// Read statistics for the run.
fn stats() -> &'static Mutex<ReadStats> {
    static STATS: OnceLock<Mutex<ReadStats>> = OnceLock::new();
    STATS.get_or_init(|| Mutex::new(ReadStats::default()))
}

/// How reads have gone so far.
pub fn read_stats() -> ReadStats {
    stats().lock().map(|s| *s).unwrap_or_default()
}

/// Every open file, by handle.
fn table() -> &'static Mutex<BTreeMap<FileHandle, Open>> {
    static TABLE: OnceLock<Mutex<BTreeMap<FileHandle, Open>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Hands out a handle nothing else will hand out, for another subsystem's own table.
///
/// **The same source, so two tables cannot collide.** A directory handle and a stream handle
/// are both addresses a guest holds and dereferences, and two independent sources of them
/// would eventually issue one twice - at which point closing one would close the other.
pub fn fresh_handle() -> FileHandle {
    next_handle()
}

/// Hands out handles: the address of a fresh zeroed block, never freed.
fn next_handle() -> FileHandle {
    let block: Box<[u64; CONTROL_BLOCK_WORDS]> = Box::new([0; CONTROL_BLOCK_WORDS]);
    std::ptr::from_mut(Box::leak(block)) as usize as u64
}

/// Opens a guest path for reading.
///
/// Read-only, and deliberately: nothing observed writes, and a guest that could write
/// through this would be writing into the user's own title directory. Adding write access
/// is a decision with consequences, not an oversight to be corrected silently.
///
/// `None` when the path is under no mount, tries to climb out of one, or does not exist.
pub fn open(guest_path: &str) -> Option<FileHandle> {
    let host = crate::mount::resolve(guest_path)?;
    let file = std::fs::File::open(host).ok()?;
    let handle = next_handle();
    table().lock().ok()?.insert(
        handle,
        Open {
            file,
            path: guest_path.to_owned(),
            at_end: false,
        },
    );
    Some(handle)
}

/// Opens a guest path for writing, creating it if it is not there.
///
/// # Why this exists now and did not before
///
/// [`open`] is read-only, and its comment said adding write access was *"a decision with
/// consequences, not an oversight"*. The consequence it named was a guest writing into the
/// user's own title directory - which is the material being measured, so a guest able to
/// change it would be editing its own evidence.
///
/// That objection is answered by *where*, not by *whether*: `/data` is storage the
/// installation owns and the guest is meant to have. `/app0` stays read-only, and
/// [`crate::mount::is_writable`] is what separates them (D250).
///
/// `None` when the path is under no mount, climbs out of one, or is not writable.
pub fn create(guest_path: &str) -> Option<FileHandle> {
    if !crate::mount::is_writable(guest_path) {
        return None;
    }
    let host = crate::mount::resolve(guest_path)?;
    if let Some(parent) = host.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(host)
        .ok()?;
    let handle = next_handle();
    table().lock().ok()?.insert(
        handle,
        Open {
            file,
            path: guest_path.to_owned(),
            at_end: false,
        },
    );
    Some(handle)
}

/// Runs `f` against an open file.
fn with<R>(handle: FileHandle, f: impl FnOnce(&mut Open) -> R) -> Option<R> {
    table().lock().ok()?.get_mut(&handle).map(f)
}

/// Reads up to `len` bytes into `into`, answering how many arrived.
///
/// `None` when the handle names nothing.
pub fn read(handle: FileHandle, into: &mut [u8]) -> Option<usize> {
    use std::io::Read as _;
    let outcome = with(handle, |open| {
        // Whether the end had *already* been reached, which is what separates "this file
        // is finished" from "this read was cut short".
        let was_at_end = open.at_end;
        let read = open.file.read(into).unwrap_or(0);
        // A short read is how the end announces itself, and `feof` is asked afterwards.
        if read < into.len() {
            open.at_end = true;
        }
        (read, was_at_end)
    })?;
    let (read, was_at_end) = outcome;
    if let Ok(mut stats) = stats().lock() {
        stats.reads += 1;
        stats.bytes += read as u64;
        // Counted only when the file was not already finished - reading to the end is a
        // short read by definition and is not a defect.
        if read < into.len() && !was_at_end {
            stats.short += 1;
        }
    }
    Some(read)
}

/// Where to seek from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum From {
    /// From the beginning.
    Start,
    /// From where it is.
    Current,
    /// From the end.
    End,
}

impl From {
    /// The POSIX `SEEK_*` value, which is what a guest passes.
    ///
    /// Published values, and the same on every System V platform: set 0, current 1, end 2.
    pub const fn from_whence(whence: u64) -> Option<Self> {
        match whence {
            0 => Some(Self::Start),
            1 => Some(Self::Current),
            2 => Some(Self::End),
            _ => None,
        }
    }
}

/// Moves the read position, answering where it ended up.
pub fn seek(handle: FileHandle, from: From, offset: i64) -> Option<u64> {
    use std::io::Seek as _;
    with(handle, |open| {
        let to = match from {
            From::Start => std::io::SeekFrom::Start(offset.max(0) as u64),
            From::Current => std::io::SeekFrom::Current(offset),
            From::End => std::io::SeekFrom::End(offset),
        };
        let at = open.file.seek(to).unwrap_or(0);
        // Seeking clears the end marker, which is what makes the read-to-end then
        // rewind then read-again pattern work.
        open.at_end = false;
        at
    })
}

/// Where the read position is.
pub fn tell(handle: FileHandle) -> Option<u64> {
    use std::io::Seek as _;
    with(handle, |open| open.file.stream_position().unwrap_or(0))
}

/// Whether a read has hit the end.
pub fn at_end(handle: FileHandle) -> Option<bool> {
    with(handle, |open| open.at_end)
}

/// The guest path a handle was opened with.
pub fn path_of(handle: FileHandle) -> Option<String> {
    with(handle, |open| open.path.clone())
}

/// Closes a file. Answers whether there was one.
///
/// The control block is deliberately **not** freed - a guest holding a stale handle then
/// reads zeroes rather than freed memory, and the count is bounded by how many files a
/// title opens.
pub fn close(handle: FileHandle) -> bool {
    // A stream that wraps a descriptor owns nothing: forgetting the wrapper is the whole of
    // closing it, and closing the descriptor as well would take it out from under a guest
    // that still holds the number.
    if unwrap_descriptor(handle) {
        return true;
    }
    table()
        .lock()
        .is_ok_and(|mut table| table.remove(&handle).is_some())
}

/// Closes every open file.
pub fn close_all() {
    if let Ok(mut table) = table().lock() {
        table.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{From, at_end, close, open, path_of, read, seek, tell};

    /// Serialises the tests that touch the mount table.
    ///
    /// Mounts are process-global - they describe one guest - and the harness runs tests in
    /// parallel, so without this a test can have the mount cleared out from under it.
    use crate::exclusively;

    /// A title directory with one known file in it.
    fn a_title_with(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("orbistoun-fs-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        std::fs::write(root.join("game.bin"), contents).expect("file");
        crate::mount::clear();
        crate::mount::mount(crate::mount::APP_MOUNT, root.clone());
        root
    }

    #[test]
    fn a_handle_is_memory_the_guest_can_read_through() {
        let _guard = exclusively();
        // Measured, not assumed: the guest dereferences what `fopen` returns without
        // checking it. Answering an error code made it carry that code through four more
        // calls; answering null made it read offset four of the null and fault (D165).
        a_title_with("deref", b"hello");
        let h = open("/app0/game.bin").expect("opens");
        assert_ne!(h, super::NO_FILE);
        assert_eq!(h % 8, 0, "aligned, so a word read is a word read");

        // SAFETY: the address of a leaked, zeroed, aligned block this module owns and
        // never frees, so a word read from it is always valid.
        let first = unsafe { std::ptr::read(h as usize as *const u64) };
        assert_eq!(first, 0, "unknown fields read as zero, not as garbage");
    }

    #[test]
    fn a_short_read_at_the_end_of_a_file_is_not_counted_as_a_defect() {
        // Reading to the end is a short read by definition. Counting it would bury the
        // case that matters - a read cut short *before* the end, which is a truncated
        // asset the guest will then try to parse (D175).
        let _guard = exclusively();
        a_title_with("shortcount", b"ab");
        let before = super::read_stats().short;
        let h = open("/app0/game.bin").expect("opens");
        let mut buf = [0_u8; 8];
        assert_eq!(read(h, &mut buf), Some(2), "two bytes, then the end");
        assert_eq!(read(h, &mut buf), Some(0), "and nothing after it");
        assert_eq!(
            super::read_stats().short,
            before + 1,
            "only the first counts - the second was already at the end"
        );
        close(h);
    }

    #[test]
    fn reading_gives_back_what_is_in_the_file() {
        let _guard = exclusively();
        a_title_with("read", b"abcdefgh");
        let h = open("/app0/game.bin").expect("opens");
        let mut buf = [0_u8; 4];
        assert_eq!(read(h, &mut buf), Some(4));
        assert_eq!(&buf, b"abcd");
        assert_eq!(tell(h), Some(4));
        assert!(close(h));
    }

    #[test]
    fn seeking_to_the_end_is_how_a_guest_learns_the_size() {
        let _guard = exclusively();
        // The pattern this whole subsystem exists to serve: seek to the end, ask where
        // that is, allocate that much, rewind, read. A wrong answer here is how a title
        // ended up asking for a two gigabyte buffer.
        a_title_with("size", &[7_u8; 1234]);
        let h = open("/app0/game.bin").expect("opens");
        assert_eq!(seek(h, From::End, 0), Some(1234));
        assert_eq!(tell(h), Some(1234));
        assert_eq!(seek(h, From::Start, 0), Some(0));

        let mut buf = [0_u8; 1234];
        assert_eq!(read(h, &mut buf), Some(1234));
        assert!(buf.iter().all(|b| *b == 7));
    }

    #[test]
    fn the_end_is_reported_after_the_read_that_reached_it() {
        let _guard = exclusively();
        // `feof` is asked *after* the short read, so the answer has to survive until
        // then rather than being derived at the moment it is asked.
        a_title_with("eof", b"ab");
        let h = open("/app0/game.bin").expect("opens");
        let mut buf = [0_u8; 8];
        assert_eq!(read(h, &mut buf), Some(2));
        assert_eq!(at_end(h), Some(true));
        // And seeking back clears it, which is what makes rewind-and-read-again work.
        seek(h, From::Start, 0);
        assert_eq!(at_end(h), Some(false));
    }

    #[test]
    fn a_file_that_is_not_there_is_refused_rather_than_handled() {
        let _guard = exclusively();
        // The guest must be able to tell "no such file" from "here is an empty one".
        a_title_with("missing", b"x");
        assert_eq!(open("/app0/not-here.bin"), None);
    }

    #[test]
    fn a_path_climbing_out_of_the_mount_never_opens() {
        let _guard = exclusively();
        // The containment rule is tested pure elsewhere; this is the check that it is
        // actually consulted on the path that touches the disk.
        a_title_with("escape", b"x");
        assert_eq!(open("/app0/../../../etc/passwd"), None);
    }

    #[test]
    fn a_closed_handle_stops_answering() {
        let _guard = exclusively();
        a_title_with("closed", b"x");
        let h = open("/app0/game.bin").expect("opens");
        assert!(close(h));
        assert_eq!(read(h, &mut [0_u8; 1]), None, "a stale handle is a miss");
        assert!(!close(h), "and closing twice reports the truth");
    }

    #[test]
    fn a_handle_remembers_which_file_it_is() {
        let _guard = exclusively();
        // "which file" is the first thing anybody asks of a trace.
        a_title_with("named", b"x");
        let h = open("/app0/game.bin").expect("opens");
        assert_eq!(path_of(h).as_deref(), Some("/app0/game.bin"));
    }
}
