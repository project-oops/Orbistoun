//! Reading a directory through its descriptor: the `getdirentries` family.
//!
//! A guest that lists a directory without the C library - a launcher scanning `/user/app` for
//! installed titles does - opens the directory, then issues the system call on the descriptor and
//! walks the records it gets back itself. So the records are the kernel's own layout, exactly:
//!
//! - `getdirentries` (554) writes the current `struct dirent`: `d_fileno` 64-bit at 0, `d_off`
//!   at 8, `d_reclen` at 16, `d_type` at 18, `d_namlen` 16-bit at 20, the name at 24, each
//!   record rounded to 8 bytes (FreeBSD `sys/sys/dirent.h`, `_GENERIC_DIRSIZ`);
//! - `freebsd11_getdirentries` (196) and `freebsd11_getdents` (272) write the older one:
//!   `d_fileno` 32-bit at 0, `d_reclen` at 4, `d_type` at 6, `d_namlen` 8-bit at 7, the name
//!   at 8, rounded to 4 (FreeBSD 11 `sys/sys/dirent.h`).
//!
//! The entries are the listing `metadata::listing` builds for `opendir`, taken when the
//! directory is opened and walked from there, so the two ways of reading a directory agree.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Answered for a call that could not be carried out.
const FAILED: u64 = -1_i64 as u64;

/// `d_type` for a directory and a regular file (`sys/sys/dirent.h`).
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;

/// A directory descriptor's entries and how far a guest has read them.
struct Listing {
    entries: Vec<(String, bool)>,
    next: usize,
}

/// Open directory descriptors' listings, by descriptor.
fn listings() -> &'static Mutex<BTreeMap<u64, Listing>> {
    static LISTINGS: OnceLock<Mutex<BTreeMap<u64, Listing>>> = OnceLock::new();
    LISTINGS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Takes `guest_path`'s listing for descriptor `fd`, just opened on it.
pub(crate) fn note_directory(fd: u64, guest_path: &str) {
    if let Some(entries) = crate::metadata::listing(guest_path)
        && let Ok(mut open) = listings().lock()
    {
        open.insert(fd, Listing { entries, next: 0 });
    }
}

/// Forgets a descriptor's listing as it is closed.
pub(crate) fn forget(fd: u64) {
    if let Ok(mut open) = listings().lock() {
        open.remove(&fd);
    }
}

/// Which `struct dirent` a call writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Generation {
    /// FreeBSD 12's, with a 64-bit inode: records rounded to 8.
    Current,
    /// FreeBSD 11's: records rounded to 4.
    FreeBsd11,
}

impl Generation {
    /// Where the name starts in a record.
    const fn name_at(self) -> usize {
        match self {
            Self::Current => 24,
            Self::FreeBsd11 => 8,
        }
    }

    /// A record's length for a name of `len` bytes: header, name, terminator, rounded up.
    const fn record(self, len: usize) -> usize {
        let unrounded = self.name_at() + len + 1;
        match self {
            Self::Current => (unrounded + 7) & !7,
            Self::FreeBsd11 => (unrounded + 3) & !3,
        }
    }
}

/// Writes records for `entries[from..]` into `buffer` while they fit, answering the bytes written
/// and the index of the first entry left unwritten.
fn encode(
    entries: &[(String, bool)],
    from: usize,
    buffer: &mut [u8],
    generation: Generation,
) -> (usize, usize) {
    let mut at = 0;
    let mut index = from;
    while let Some((name, is_directory)) = entries.get(index) {
        let name = &name.as_bytes()[..name.len().min(255)];
        let record = generation.record(name.len());
        let Some(slot) = buffer.get_mut(at..at + record) else {
            break;
        };
        slot.fill(0);
        // A non-zero inode, since a reader such as the C library skips zero-inode records.
        let inode = index as u64 + 1;
        let kind = if *is_directory { DT_DIR } else { DT_REG };
        let reclen = u16::try_from(record).unwrap_or(u16::MAX);
        match generation {
            Generation::Current => {
                slot[0..8].copy_from_slice(&inode.to_le_bytes());
                slot[8..16].copy_from_slice(&(index as u64 + 1).to_le_bytes());
                slot[16..18].copy_from_slice(&reclen.to_le_bytes());
                slot[18] = kind;
                slot[20..22]
                    .copy_from_slice(&u16::try_from(name.len()).unwrap_or(255).to_le_bytes());
            }
            Generation::FreeBsd11 => {
                slot[0..4].copy_from_slice(&u32::try_from(inode).unwrap_or(u32::MAX).to_le_bytes());
                slot[4..6].copy_from_slice(&reclen.to_le_bytes());
                slot[6] = kind;
                slot[7] = u8::try_from(name.len()).unwrap_or(255);
            }
        }
        let name_at = generation.name_at();
        slot[name_at..name_at + name.len()].copy_from_slice(name);
        at += record;
        index += 1;
    }
    (at, index)
}

/// Fills a guest buffer from descriptor `fd`'s listing, storing the position it started at in
/// `*basep` when one is given. Answers the bytes written - zero at the end - or [`FAILED`] for a
/// descriptor that is no open directory, or a buffer too small for the next record.
fn read_entries(fd: u64, (buffer, length): (u64, u64), basep: u64, generation: Generation) -> u64 {
    let Ok(mut open) = listings().lock() else {
        return FAILED;
    };
    let Some(listing) = open.get_mut(&fd) else {
        return FAILED;
    };
    let Some(bytes) = guest_bytes_mut(buffer, length) else {
        return FAILED;
    };
    let start = listing.next;
    let (written, next) = encode(&listing.entries, start, bytes, generation);
    if written == 0 && next < listing.entries.len() {
        // The next record does not fit at all: `EINVAL` on the hardware.
        return FAILED;
    }
    listing.next = next;
    if let Some(base) = guest_bytes_mut(basep, 8) {
        base.copy_from_slice(&(start as u64).to_le_bytes());
    }
    written as u64
}

/// A guest buffer, as bytes this may write into.
fn guest_bytes_mut<'a>(address: u64, length: u64) -> Option<&'a mut [u8]> {
    if address == 0 {
        return None;
    }
    let at = usize::try_from(address).ok()?;
    let len = usize::try_from(length).ok()?;
    // SAFETY: a guest-supplied buffer under the identity mapping, with the length the guest
    // passed, the same contract the real call has.
    Some(unsafe {
        std::slice::from_raw_parts_mut(std::ptr::with_exposed_provenance_mut::<u8>(at), len)
    })
}

/// `getdirentries(fd, buf, nbytes, basep)`: the current record layout.
///
/// Reference: FreeBSD `getdirentries(2)`.
fn getdirentries(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    read_entries(args[0], (args[1], args[2]), args[3], Generation::Current)
}

/// `sceKernelGetdirentries(fd, buf, nbytes, basep)`: the FreeBSD 11 record layout, and what the
/// old system call 196 (`freebsd11_getdirentries`) is served by.
fn get_dir_entries(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    read_entries(args[0], (args[1], args[2]), args[3], Generation::FreeBsd11)
}

/// `sceKernelGetdents(fd, buf, nbytes)`: the FreeBSD 11 layout with no position out, and what the
/// old system call 272 (`freebsd11_getdents`) is served by.
fn get_dents(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    read_entries(args[0], (args[1], args[2]), 0, Generation::FreeBsd11)
}

/// Implementations this module provides, by name. The system-call table binds `getdirentries` to
/// 554 by its own name, and 196 and 272 to the two vendor names (`SPELT_DIFFERENTLY`).
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("getdirentries", getdirentries),
        ("sceKernelGetdirentries", get_dir_entries),
        ("sceKernelGetdents", get_dents),
    ]
}

#[cfg(test)]
mod tests {
    use super::{Generation, encode};

    fn entries() -> Vec<(String, bool)> {
        vec![
            (".".to_owned(), true),
            ("GLCB00001".to_owned(), true),
            ("eboot.bin".to_owned(), false),
        ]
    }

    /// The current layout: 64-bit inode, 8-byte records, the name at 24 (FreeBSD 12
    /// `sys/sys/dirent.h`).
    #[test]
    fn the_current_layout_puts_each_field_where_the_header_does() {
        let mut buffer = [0u8; 256];
        let (written, next) = encode(&entries(), 0, &mut buffer, Generation::Current);
        assert_eq!(next, 3);
        // ".": 24 + 1 + 1 = 26, rounded to 32. "GLCB00001": 24 + 9 + 1 = 34, rounded to 40.
        assert_eq!(u16::from_le_bytes([buffer[16], buffer[17]]), 32);
        let second = 32;
        assert_eq!(
            u16::from_le_bytes([buffer[second + 16], buffer[second + 17]]),
            40
        );
        assert_eq!(buffer[second + 18], 4, "a directory");
        assert_eq!(
            u16::from_le_bytes([buffer[second + 20], buffer[second + 21]]),
            9
        );
        assert_eq!(&buffer[second + 24..second + 33], b"GLCB00001");
        assert_ne!(
            u64::from_le_bytes(buffer[second..second + 8].try_into().unwrap()),
            0
        );
        assert_eq!(written, 32 + 40 + 40);
    }

    /// The FreeBSD 11 layout: 32-bit inode, 4-byte records, the name at 8.
    #[test]
    fn the_freebsd11_layout_puts_each_field_where_its_header_does() {
        let mut buffer = [0u8; 256];
        let (_, next) = encode(&entries(), 1, &mut buffer, Generation::FreeBsd11);
        assert_eq!(next, 3);
        // "GLCB00001": 8 + 9 + 1 = 18, rounded to 20.
        assert_eq!(u16::from_le_bytes([buffer[4], buffer[5]]), 20);
        assert_eq!(buffer[6], 4);
        assert_eq!(buffer[7], 9);
        assert_eq!(&buffer[8..17], b"GLCB00001");
        assert_eq!(buffer[20 + 6], 8, "a regular file");
    }

    /// A short buffer stops at the last whole record, and the next call resumes after it.
    #[test]
    fn a_short_buffer_stops_at_a_whole_record() {
        let mut buffer = [0u8; 40];
        let (written, next) = encode(&entries(), 0, &mut buffer, Generation::Current);
        assert_eq!(
            (written, next),
            (32, 1),
            "the second record (40) does not fit after the first"
        );
    }
}
