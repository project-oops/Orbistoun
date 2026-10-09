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
/// `(guest_path, offset, address, most)` to how many bytes arrived: `most` bytes from `offset`
/// into the file, fewer where the file ends first.
type Reader = fn(&str, u64, u64, u64) -> Option<usize>;

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

/// Reads `path` from `offset` into `[address, address + most)`, answering how many bytes arrived.
///
/// [`None`] when nothing installed a reader, which is a different finding from a file that read
/// zero bytes.
pub(crate) fn deliver(path: &str, offset: u64, address: u64, most: u64) -> Option<usize> {
    READER
        .get()
        .and_then(|read| read(path, offset, address, most))
}

/// One read a command buffer holds, as `sceAmprAprCommandBufferReadFile` was asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Read {
    /// The identifier a resolve gave the file.
    pub id: u64,
    /// Where in guest memory the bytes go.
    pub into: u64,
    /// How many bytes.
    pub length: u64,
    /// Where in the file they start.
    pub offset: u64,
}

/// The reads each command buffer holds, by the buffer object's address, until it is submitted.
///
/// Held here rather than encoded into the guest's buffer: the encoding the platform uses is
/// unmeasured, and the guest only hands the object back to be submitted (D782).
fn queued() -> &'static Mutex<std::collections::BTreeMap<u64, Vec<Read>>> {
    static QUEUED: OnceLock<Mutex<std::collections::BTreeMap<u64, Vec<Read>>>> = OnceLock::new();
    QUEUED.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// Whether any submit has carried its reads out.
static CARRIED_OUT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a submit has carried reads out, so a wait has nothing outstanding to wait for.
pub(crate) fn any_carried_out() -> bool {
    CARRIED_OUT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Adds a read to the command buffer at `buffer`; `false`, and nothing added, for an identifier
/// no resolve gave.
pub fn queue_read(buffer: u64, read: Read) -> bool {
    let Some(path) = path_of(read.id) else {
        return false;
    };
    tracing::debug!(
        "{buffer:#x} queues {:#x} byte(s) from {:#x} of {path} into {:#x}",
        read.length,
        read.offset,
        read.into
    );
    queued()
        .lock()
        .map(|mut held| held.entry(buffer).or_default().push(read))
        .is_ok()
}

/// Carries out every read the command buffer at `buffer` holds, in the order they were added, and
/// empties it: how many it carried out, or [`None`] when it held none or one could not be read in
/// full.
pub(crate) fn carry_out(buffer: u64) -> Option<usize> {
    let reads = queued().lock().ok()?.remove(&buffer)?;
    CARRIED_OUT.store(true, std::sync::atomic::Ordering::Relaxed);
    for read in &reads {
        let path = path_of(read.id)?;
        let got = deliver(&path, read.offset, read.into, read.length)?;
        if (got as u64) < read.length {
            tracing::warn!(
                "{path}: {got} of {} bytes from {:#x} arrived",
                read.length,
                read.offset
            );
            return None;
        }
    }
    Some(reads.len())
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

/// Writes a guest file's `struct stat` at a guest address, installed from above like
/// [`on_file_read`]; `false` for a path that names nothing or an address that cannot be written.
type StatWriter = fn(&str, u64) -> bool;

/// The installed stat writer, if anything installed one.
static STAT_WRITER: OnceLock<StatWriter> = OnceLock::new();

/// Installs what a stat by identifier writes with.
pub fn on_file_stat(writer: StatWriter) {
    let _ = STAT_WRITER.set(writer);
}

/// The path behind every identifier a resolve has given out, an index's or a synthesised one.
fn identified() -> &'static Mutex<std::collections::BTreeMap<u64, String>> {
    static IDENTIFIED: OnceLock<Mutex<std::collections::BTreeMap<u64, String>>> = OnceLock::new();
    IDENTIFIED.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// Records that a resolve gave `path` the identifier `id`.
pub(crate) fn remember(id: u64, path: &str) {
    if let Ok(mut held) = identified().lock() {
        held.insert(id, path.to_owned());
    }
}

/// The path a resolve gave `id`, or nothing for an identifier no resolve gave.
pub(crate) fn path_of(id: u64) -> Option<String> {
    identified().lock().ok()?.get(&id).cloned()
}

/// Writes the `struct stat` of the file a resolve gave `id` at `at`, as `sceKernelStat` writes it.
pub(crate) fn stat_by_id(id: u64, at: u64) -> Option<bool> {
    let path = path_of(id)?;
    STAT_WRITER.get().map(|write| write(&path, at))
}

/// `sceKernelAprGetFileStat(id, stat)`: the `struct stat` of the file a resolve gave `id`, written
/// as `sceKernelStat` writes it, answering 0 (D782, assumed: on firmware 12.40 no probe can hold a
/// real identifier). An identifier no resolve gave, or a stat that cannot be written, is refused
/// with the project's placeholder, since the hardware's code for it is unmeasured.
pub(crate) fn get_file_stat(args: &[u64; orbistoun_core::GUEST_ARG_REGISTERS]) -> u64 {
    match stat_by_id(args[0], args[1]) {
        Some(true) => 0,
        _ => u64::from(orbistoun_core::GuestError::InvalidArgument.as_raw()),
    }
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

    /// The path a stat by identifier names: whatever a resolve gave that identifier, an index's or
    /// a synthesised one, and nothing for an identifier no resolve gave (D782).
    #[test]
    fn an_identifier_names_the_path_it_was_resolved_from() {
        super::remember(130, "/app0/Media/level0");
        assert_eq!(super::path_of(130).as_deref(), Some("/app0/Media/level0"));
        assert_eq!(super::path_of(0x7777_7777), None);
    }

    /// A test file of 0x40 bytes, each its own offset, read as the installed reader reads.
    fn counting_file(path: &str, offset: u64, address: u64, most: u64) -> Option<usize> {
        if path != "/app0/ps5/audio/audio_rel.rpf" {
            return None;
        }
        let bytes: Vec<u8> = (offset..0x40.min(offset + most)).map(|b| b as u8).collect();
        // SAFETY: the test's own buffer, `most` bytes at least.
        unsafe { orbistoun_mem::guest::write_bytes(address, &bytes) }.then_some(bytes.len())
    }

    /// A read added to a command buffer lands, from its offset, only when the buffer is carried
    /// out, which empties it; an identifier no resolve gave is not added (D782).
    #[test]
    fn a_queued_read_lands_when_its_buffer_is_carried_out() {
        super::on_file_read(counting_file);
        super::remember(0xf9, "/app0/ps5/audio/audio_rel.rpf");
        let mut into = [0xa5_u8; 8];
        let read = super::Read {
            id: 0xf9,
            into: into.as_mut_ptr() as u64,
            length: 4,
            offset: 0x10,
        };
        assert!(super::queue_read(0x7400_0020_62b0, read));
        assert_eq!(into, [0xa5; 8], "nothing read before the submit");
        assert_eq!(super::carry_out(0x7400_0020_62b0), Some(1));
        assert_eq!(into, [0x10, 0x11, 0x12, 0x13, 0xa5, 0xa5, 0xa5, 0xa5]);
        assert_eq!(super::carry_out(0x7400_0020_62b0), None, "emptied");
        assert!(!super::queue_read(
            1,
            super::Read {
                id: 0x0fff_0000,
                ..read
            }
        ));
    }

    /// Writes one marker word where a stat was asked for, for the test below.
    fn marker_stat(path: &str, at: u64) -> bool {
        if path != "/app0/Media/sharedassets0.assets" {
            return false;
        }
        // SAFETY: the test's own buffer, eight bytes at least.
        unsafe { orbistoun_mem::guest::write_u64(at, 0x5354_4154) }
    }

    /// `sceKernelAprGetFileStat(id, stat)` writes the stat of the file a resolve gave `id` and
    /// answers 0; an identifier no resolve gave is refused and nothing is written (D782).
    #[test]
    fn a_stat_by_identifier_writes_the_resolved_file_s_stat() {
        super::on_file_stat(marker_stat);
        super::remember(0x4000_0100, "/app0/Media/sharedassets0.assets");
        let mut stat = [0_u64; 16];
        let mut args = [0_u64; orbistoun_core::GUEST_ARG_REGISTERS];
        args[0] = 0x4000_0100;
        args[1] = stat.as_mut_ptr() as u64;
        assert_eq!(super::get_file_stat(&args), 0);
        assert_eq!(stat[0], 0x5354_4154);
        stat[0] = 0;
        args[0] = 0x4000_0fff;
        assert_ne!(super::get_file_stat(&args), 0);
        assert_eq!(stat[0], 0, "nothing written");
    }
}
