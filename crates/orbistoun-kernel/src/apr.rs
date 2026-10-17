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

use orbistoun_mem::guest;

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

/// Where an AMPR command-buffer object counts the bytes its commands take, a 32-bit word
/// (REQ-aw01).
pub const USED_AT: u64 = 4;
/// Where it counts the commands appended, a 32-bit word (REQ-aw01).
pub const COUNT_AT: u64 = 8;
/// Where `sceAmprCommandBufferSetBuffer` writes the bound buffer's size, a 32-bit word.
pub const SIZE_AT: u64 = 12;
/// Where it writes the bound buffer's address.
pub const BUFFER_AT: u64 = 16;
/// The write-address command's opcode, the header word's low byte (REQ-aw01, REQ-aw02).
pub const WRITE_ADDRESS: u32 = 0x05;

/// The write-address command for `value` written to `address`, as a console appended it (REQ-aw01,
/// REQ-aw02, `report-1791833977.txt` 658-693): a header - the opcode, a flags byte, the address's bits
/// 47:32 - then the address's low word, then the value shifted down two where that is not zero. The
/// flags carry the value's low two bits at 5:4 and the words after the header at 3:0, with bit 6 set
/// where the value word is there.
#[must_use]
pub fn write_address_command(address: u64, value: u32) -> Vec<u32> {
    let shifted = value >> 2;
    let words = if shifted == 0 { 1 } else { 2 };
    let flags = (u32::from(shifted != 0) << 6) | ((value & 0x3) << 4) | words;
    let high = ((address >> 32) & 0xffff) as u32;
    let mut command = vec![WRITE_ADDRESS | (flags << 8) | (high << 16), address as u32];
    if shifted != 0 {
        command.push(shifted);
    }
    command
}

/// One write-address command: where, and what.
pub type WriteAddress = (u64, u32);

/// What a walk of a command buffer's words found: the write-addresses before a command that is
/// not one, and that command's word index.
pub type Stopped = (Vec<WriteAddress>, usize);

/// The write-address commands in a command buffer's words, in order, as `(address, value)`: the
/// inverse of [`write_address_command`]. A command that is not one, or is cut short, ends the walk:
/// [`Err`] carries the commands before it and the index of its header word.
pub fn write_addresses(words: &[u32]) -> Result<Vec<WriteAddress>, Stopped> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < words.len() {
        let header = words[at];
        let flags = (header >> 8) & 0xff;
        let after = (flags & 0xf) as usize;
        if header & 0xff != WRITE_ADDRESS || !(1..=2).contains(&after) || at + after >= words.len()
        {
            return Err((out, at));
        }
        let address = (u64::from(header >> 16) << 32) | u64::from(words[at + 1]);
        let shifted = if after == 2 { words[at + 2] } else { 0 };
        out.push((address, (shifted << 2) | ((flags >> 4) & 0x3)));
        at += 1 + after;
    }
    Ok(out)
}

/// Carries out the write-address commands in the buffer the AMPR command-buffer object at `object`
/// has bound, in order: each writes its value, zero-extended, as the eight-byte word at its address
/// (D834, assumed). Answers how many were carried out; a command that is not a write-address is
/// reported, and it and everything after it are not.
pub(crate) fn run_bound_commands(object: u64) -> usize {
    // SAFETY: the guest's own command-buffer object, at its measured fields.
    let used = unsafe { guest::read_u32(object + USED_AT) };
    // SAFETY: as above.
    let buffer = unsafe { guest::read_u64(object + BUFFER_AT) };
    let (Some(used), Some(buffer)) = (used, buffer) else {
        return 0;
    };
    if used == 0 || buffer == 0 {
        return 0;
    }
    let mut bytes = vec![0_u8; used as usize];
    // SAFETY: the bound buffer, of which the commands take its first `used` bytes.
    if !unsafe { guest::read_into(buffer, &mut bytes) } {
        return 0;
    }
    let words: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    let writes = match write_addresses(&words) {
        Ok(writes) => writes,
        Err((writes, at)) => {
            tracing::warn!(
                "command buffer {object:#x}: command {:#010x} at word {at} is not carried out, nor anything after it",
                words[at]
            );
            writes
        }
    };
    let mut done = 0;
    for (address, value) in writes {
        // SAFETY: an address the guest's own command named, written through the checked accessor.
        if unsafe { guest::write_u64(address, u64::from(value)) } {
            done += 1;
        }
    }
    done
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

/// Reads queued, carried out, the bytes those brought, and reads that arrived short, this run.
static READS: [std::sync::atomic::AtomicU64; 4] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];
/// Indices into [`READS`].
const QUEUED: usize = 0;
const CARRIED: usize = 1;
const BYTES: usize = 2;
const SHORT: usize = 3;

/// Adds `amount` to the [`READS`] counter at `index`.
fn count(index: usize, amount: u64) {
    READS[index].fetch_add(amount, std::sync::atomic::Ordering::Relaxed);
}

/// How a run's asynchronous file reads fared.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadSummary {
    /// Reads command buffers were given.
    pub queued: u64,
    /// Reads a submit carried out.
    pub carried: u64,
    /// Bytes those brought.
    pub bytes: u64,
    /// Reads that arrived short, ending their buffer's submit.
    pub short: u64,
    /// Reads still held by command buffers no submit has carried out.
    pub left: u64,
    /// How many command buffers hold them.
    pub left_buffers: u64,
}

impl ReadSummary {
    /// The report's line, where the run queued any read.
    #[must_use]
    pub fn describe(&self) -> Option<String> {
        (self.queued > 0).then(|| {
            format!(
                "asynchronous file reads: {} queued, {} carried out ({} bytes), {} arrived short, {} left in {} command buffer(s) never submitted",
                self.queued, self.carried, self.bytes, self.short, self.left, self.left_buffers
            )
        })
    }
}

/// How the run's asynchronous file reads have fared so far.
#[must_use]
pub fn read_summary() -> ReadSummary {
    let load = |index: usize| READS[index].load(std::sync::atomic::Ordering::Relaxed);
    let (left, left_buffers) = queued().lock().map_or((0, 0), |held| {
        (
            held.values().map(|reads| reads.len() as u64).sum(),
            held.values().filter(|reads| !reads.is_empty()).count() as u64,
        )
    });
    ReadSummary {
        queued: load(QUEUED),
        carried: load(CARRIED),
        bytes: load(BYTES),
        short: load(SHORT),
        left,
        left_buffers,
    }
}

/// Adds a read to the command buffer at `buffer`; `false`, and nothing added, for an identifier
/// no resolve gave.
pub fn queue_read(buffer: u64, read: Read) -> bool {
    let Some(path) = path_of(read.id) else {
        return false;
    };
    count(QUEUED, 1);
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

/// Reads carried out, by the address each landed at: where it ends, the file's identifier and the
/// offset in it.
type Landings = std::collections::BTreeMap<u64, (u64, u64, u64)>;

/// Each read carried out this run, by the address it landed at. A later read landing at the same
/// address replaces the earlier.
fn landed() -> &'static Mutex<Landings> {
    static LANDED: OnceLock<Mutex<Landings>> = OnceLock::new();
    LANDED.get_or_init(|| Mutex::new(Landings::new()))
}

/// What filled `address`, where a read this run carried out landed over it: the file and the
/// offset in it it was read from.
#[must_use]
pub fn read_landed_at(address: u64) -> Option<String> {
    let (start, (end, id, offset)) = landed()
        .lock()
        .ok()?
        .range(..=address)
        .next_back()
        .map(|(&start, &landing)| (start, landing))?;
    if address >= end {
        return None;
    }
    let path = path_of(id)?;
    Some(format!(
        "read from {path} at {:#x}",
        offset + (address - start)
    ))
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
        count(BYTES, got as u64);
        if (got as u64) < read.length {
            count(SHORT, 1);
            tracing::warn!(
                "{path}: {got} of {} bytes from {:#x} arrived",
                read.length,
                read.offset
            );
            return None;
        }
        count(CARRIED, 1);
        if let Ok(mut landed) = landed().lock() {
            landed.insert(read.into, (read.into + read.length, read.id, read.offset));
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

/// The most unresolved paths remembered: a title probing a tree it does not have can ask forever.
const MOST_UNRESOLVED: usize = 256;

/// A path a resolve could not answer, with the import that asked, as an index into the thunk table.
type Unresolved = (String, Option<u32>);

/// Each path a resolve could not answer, once, with the import that asked: what the run's report
/// lists beside the paths an open or a stat could not answer.
fn unresolved_paths() -> &'static Mutex<Vec<Unresolved>> {
    static UNRESOLVED: OnceLock<Mutex<Vec<Unresolved>>> = OnceLock::new();
    UNRESOLVED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Remembers `path`, which a resolve could not answer, with the import `by` that asked.
pub(crate) fn note_unresolved(path: &str, by: Option<u32>) {
    let Ok(mut held) = unresolved_paths().lock() else {
        return;
    };
    if held.len() < MOST_UNRESOLVED && !held.iter().any(|(known, _)| known == path) {
        held.push((path.to_owned(), by));
    }
}

/// Every path a resolve could not answer this run, in the order asked, with the import that asked
/// first, as an index into the thunk table.
#[must_use]
pub fn unresolved() -> Vec<Unresolved> {
    unresolved_paths()
        .lock()
        .map(|held| held.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    /// A path a resolve could not answer is remembered once, in the order asked.
    #[test]
    fn an_unresolved_path_is_remembered_once() {
        super::note_unresolved("/app0/nowhere/a.rpf", None);
        super::note_unresolved("/app0/nowhere/b.rpf", Some(3));
        super::note_unresolved("/app0/nowhere/a.rpf", Some(4));
        let held = super::unresolved();
        let ours: Vec<_> = held
            .iter()
            .filter(|(path, _)| path.starts_with("/app0/nowhere/"))
            .collect();
        assert_eq!(
            ours,
            [
                &("/app0/nowhere/a.rpf".to_owned(), None),
                &("/app0/nowhere/b.rpf".to_owned(), Some(3))
            ]
        );
    }

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
        // Where it landed says what was read there.
        assert_eq!(
            super::read_landed_at(into.as_ptr() as u64 + 2).as_deref(),
            Some("read from /app0/ps5/audio/audio_rel.rpf at 0x12"),
            "two bytes into a read from 0x10"
        );
        assert_eq!(
            super::read_landed_at(into.as_ptr() as u64 + 4),
            None,
            "past its end"
        );
        assert!(!super::queue_read(
            1,
            super::Read {
                id: 0x0fff_0000,
                ..read
            }
        ));
    }

    /// A run says how its asynchronous file reads fared: how many were queued, carried out and
    /// how many bytes they brought, how many arrived short, and how many are still held by command
    /// buffers never submitted - reads a title waits on that never land.
    #[test]
    fn a_run_says_how_its_file_reads_fared() {
        let summary = super::ReadSummary {
            queued: 5,
            carried: 3,
            bytes: 0x300,
            short: 1,
            left: 1,
            left_buffers: 1,
        };
        assert_eq!(
            summary.describe().as_deref(),
            Some(concat!(
                "asynchronous file reads: 5 queued, 3 carried out (768 bytes), 1 arrived short, ",
                "1 left in 1 command buffer(s) never submitted"
            ))
        );
        assert_eq!(super::ReadSummary::default().describe(), None);
        // A read queued and never submitted is counted as left.
        super::remember(0xfa, "/app0/ps5/audio/audio_rel.rpf");
        let before = super::read_summary();
        assert!(super::queue_read(
            0x7400_0020_7000,
            super::Read {
                id: 0xfa,
                into: 0,
                length: 4,
                offset: 0,
            }
        ));
        let after = super::read_summary();
        assert!(after.queued > before.queued && after.left >= 1);
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

#[cfg(test)]
mod command_tests {
    use super::{WRITE_ADDRESS, write_address_command, write_addresses};

    /// A write-address command decodes back to the address and value it was built from, whether or
    /// not its value needs the third word (D834).
    #[test]
    fn a_write_address_decodes_to_what_it_was_built_from() {
        for (address, value) in [
            (0x7400_001f_1430, 0),
            (0x7400_001f_1430, 3),
            (0x7400_001f_1438, 0x1234_5678),
        ] {
            let words = write_address_command(address, value);
            assert_eq!(write_addresses(&words), Ok(vec![(address, value)]));
        }
        let two: Vec<u32> = [
            write_address_command(0x1000, 1),
            write_address_command(0x2000, 0),
        ]
        .concat();
        assert_eq!(
            write_addresses(&two),
            Ok(vec![(0x1000, 1), (0x2000, 0)]),
            "in order"
        );
    }

    /// A command that is not a write-address stops the walk at its word, so nothing after it is
    /// carried out on a guess.
    #[test]
    fn an_unknown_command_stops_the_walk() {
        let mut words = write_address_command(0x1000, 1);
        words.push(0x0000_0007);
        assert_eq!(write_addresses(&words), Err((vec![(0x1000, 1)], 2)));
        assert_ne!(0x07, WRITE_ADDRESS);
    }
}
