//! Open files, and the handles a guest holds them by.
//!
//! `fopen` returns a `FILE *` that the guest dereferences without checking, so the handle is
//! the address of a real, zeroed, never-freed block, as for thread and lock handles (D151).
//! A guest reading a field gets zero rather than a fault, and a handle kept past a close reads
//! as zeroes rather than freed memory. Nothing is written into the block: the layout of the
//! structure the guest expects has no lawful source.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// How the guest refers to an open file.
pub type FileHandle = u64;

/// Handle meaning "not open", and what a caller tests for.
pub const NO_FILE: FileHandle = 0;

/// How much zeroed memory sits behind a handle.
///
/// Matched to the other subsystems' control blocks, and generous next to any field a caller
/// reads at a small offset.
const CONTROL_BLOCK_WORDS: usize = 32;

/// One open file.
#[derive(Debug)]
struct Open {
    /// The host file.
    file: std::fs::File,
    /// The guest path, kept for traces.
    path: String,
    /// Whether a read has hit the end.
    ///
    /// Tracked rather than derived, because `feof` is asked after the read that ended the file.
    at_end: bool,
}

/// Streams that are a descriptor rather than a file of their own.
///
/// A `FILE` is a buffered descriptor; a server `fdopen`s an accepted connection and
/// `fprintf`s its replies into it. A separate table from [`Open`] because an `Open` owns a
/// host file and this owns nothing: the descriptor table does, and a stream closing a
/// descriptor it did not own would close it out from under the guest.
fn wrapped() -> &'static Mutex<BTreeMap<FileHandle, u64>> {
    static WRAPPED: OnceLock<Mutex<BTreeMap<FileHandle, u64>>> = OnceLock::new();
    WRAPPED.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Answers a stream handle that stands for an already-open descriptor.
///
/// The descriptor stays the descriptor table's: closing the stream forgets the wrapper and
/// leaves the descriptor open. That differs from the platform's `fclose`, and the knowledge
/// file states it.
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
/// Completeness rather than content: counting whether the guest got as many bytes as it
/// asked for costs a counter, where verifying the bytes would double every read. A title
/// that receives a truncated asset faults later in its own parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadStats {
    /// Reads attempted.
    pub reads: u64,
    /// Reads that delivered fewer bytes than asked for.
    ///
    /// Reading to the end of a file is a short read by definition. A short read before the
    /// end is the defect; the two are told apart by whether the file was already at its end.
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
/// The same source, so two tables cannot collide: a directory handle and a stream handle are
/// both addresses a guest dereferences, and closing one must never close the other.
pub fn fresh_handle() -> FileHandle {
    next_handle()
}

/// Hands out handles: the address of a fresh zeroed block, never freed.
///
/// From the one region every guest-visible handle comes from, so a `FILE *` is the same
/// address in every run (D584).
fn next_handle() -> FileHandle {
    orbistoun_mem::blocks::block(CONTROL_BLOCK_WORDS)
}

/// Opens a guest path for reading.
///
/// Read-only: a guest writing through this would write into the title directory being run.
///
/// `None` when the path is under no mount, tries to climb out of one, or does not exist.
pub fn open(guest_path: &str) -> Option<FileHandle> {
    let host = crate::mount::resolve_existing(guest_path)?;
    let file = std::fs::File::open(host).ok()?;
    crate::opened::note(guest_path);
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
/// `/data` is storage the installation owns and the guest is meant to have; `/app0` stays
/// read-only, and [`crate::mount::is_writable`] separates them (D250).
///
/// `None` when the path is under no mount, climbs out of one, or is not writable.
pub fn create(guest_path: &str) -> Option<FileHandle> {
    let host = crate::mount::resolve_for_create(guest_path)?;
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
        // Whether the end had already been reached, which separates "this file is finished"
        // from "this read was cut short".
        let was_at_end = open.at_end;
        let read = open.file.read(into).unwrap_or(0);
        // A short read is how the end announces itself, and `feof` is asked afterwards.
        if read < into.len() {
            open.at_end = true;
        }
        (read, was_at_end)
    })?;
    let (read, was_at_end) = outcome;
    // Which file, and how much it asked for, for the opens record.
    if let Some(path) = path_of(handle) {
        crate::opened::note_read(&path, into.len(), read);
    }
    if let Ok(mut stats) = stats().lock() {
        stats.reads += 1;
        stats.bytes += read as u64;
        // Counted only when the file was not already finished: reading to the end is not a
        // defect.
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
    /// How it is written in a record a person reads.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Current => "here",
            Self::End => "end",
        }
    }

    /// The POSIX `SEEK_*` value, which is what a guest passes.
    ///
    /// Published values, the same on every System V platform: set 0, current 1, end 2.
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
        // Recorded, because seeking to the end is how a guest learns a file's size.
        crate::opened::note_seek(open.path.as_str(), from.label(), offset, at);
        // Seeking clears the end marker, so read-to-end, rewind, read-again works.
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
/// The control block is not freed: a guest holding a stale handle reads zeroes, and the count
/// is bounded by how many files a title opens.
pub fn close(handle: FileHandle) -> bool {
    // A stream that wraps a descriptor owns nothing: forgetting the wrapper is the whole of
    // closing it, and the guest still holds the descriptor.
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
    /// Mounts are process-global and the harness runs tests in parallel.
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

    /// A handle is a zeroed block the guest can read through.
    #[test]
    fn a_handle_is_memory_the_guest_can_read_through() {
        let _guard = exclusively();
        // The guest dereferences what `fopen` returns without checking it.
        a_title_with("deref", b"hello");
        let h = open("/app0/game.bin").expect("opens");
        assert_ne!(h, super::NO_FILE);
        assert_eq!(h % 8, 0, "aligned, so a word read is a word read");

        // SAFETY: the address of a leaked, zeroed, aligned block this module owns and never
        // frees, so a word read from it is valid.
        let first = unsafe { std::ptr::read(h as usize as *const u64) };
        assert_eq!(first, 0, "unknown fields read as zero, not as garbage");
    }

    /// A short read at the end of a file is not counted as a defect.
    #[test]
    fn a_short_read_at_the_end_of_a_file_is_not_counted_as_a_defect() {
        // Counting it would bury a read cut short before the end: a truncated asset.
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

    /// Reading gives back what is in the file.
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

    /// Seeking to the end reports the file's size.
    #[test]
    fn seeking_to_the_end_is_how_a_guest_learns_the_size() {
        let _guard = exclusively();
        // Seek to the end, ask where that is, allocate that much, rewind, read.
        a_title_with("size", &[7_u8; 1234]);
        let h = open("/app0/game.bin").expect("opens");
        assert_eq!(seek(h, From::End, 0), Some(1234));
        assert_eq!(tell(h), Some(1234));
        assert_eq!(seek(h, From::Start, 0), Some(0));

        let mut buf = [0_u8; 1234];
        assert_eq!(read(h, &mut buf), Some(1234));
        assert!(buf.iter().all(|b| *b == 7));
    }

    /// The end is reported after the read that reached it, and a seek clears it.
    #[test]
    fn the_end_is_reported_after_the_read_that_reached_it() {
        let _guard = exclusively();
        // `feof` is asked after the short read, so the answer has to persist.
        a_title_with("eof", b"ab");
        let h = open("/app0/game.bin").expect("opens");
        let mut buf = [0_u8; 8];
        assert_eq!(read(h, &mut buf), Some(2));
        assert_eq!(at_end(h), Some(true));
        // Seeking back clears it, so rewind-and-read-again works.
        seek(h, From::Start, 0);
        assert_eq!(at_end(h), Some(false));
    }

    /// A missing file is refused rather than given a handle.
    #[test]
    fn a_file_that_is_not_there_is_refused_rather_than_handled() {
        let _guard = exclusively();
        // "No such file" must differ from an empty file.
        a_title_with("missing", b"x");
        assert_eq!(open("/app0/not-here.bin"), None);
    }

    /// A path climbing out of its mount never opens.
    #[test]
    fn a_path_climbing_out_of_the_mount_never_opens() {
        let _guard = exclusively();
        // The containment rule is tested pure elsewhere; this checks it is consulted on the
        // path that touches the disk.
        a_title_with("escape", b"x");
        assert_eq!(open("/app0/../../../etc/passwd"), None);
    }

    /// A closed handle stops answering.
    #[test]
    fn a_closed_handle_stops_answering() {
        let _guard = exclusively();
        a_title_with("closed", b"x");
        let h = open("/app0/game.bin").expect("opens");
        assert!(close(h));
        assert_eq!(read(h, &mut [0_u8; 1]), None, "a stale handle is a miss");
        assert!(!close(h), "and closing twice reports the truth");
    }

    /// A handle remembers the guest path it was opened with.
    #[test]
    fn a_handle_remembers_which_file_it_is() {
        let _guard = exclusively();
        a_title_with("named", b"x");
        let h = open("/app0/game.bin").expect("opens");
        assert_eq!(path_of(h).as_deref(), Some("/app0/game.bin"));
    }
}
