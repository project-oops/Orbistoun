//! What a guest is told about a file, and how it lists a directory.
//!
//! # The problem this module has that the others do not
//!
//! Everything else here is read out of the FreeBSD checkout and used. These two cannot be,
//! because **the checkout is a newer FreeBSD than the target**, and these are exactly the two
//! structures that changed shape in between:
//!
//! ```text
//! struct stat     st_dev went 32-bit to 64-bit, st_nlink 16-bit to 64-bit, fields reordered
//! struct dirent   d_fileno went 32-bit to 64-bit, and d_off was added
//! ```
//!
//! Writing the modern layout for an older guest puts the file size at the wrong offset. That
//! is not a call that fails - it is a file server reporting the wrong size for every file, and
//! nothing anywhere saying so.
//!
//! # Both layouts are in the same header, so the choice is a setting
//!
//! `sys/sys/stat.h` carries `struct freebsd11_stat` beside `struct stat`, and
//! `sys/sys/dirent.h` carries `struct freebsd11_dirent` beside `struct dirent`. Both are
//! citable from one checkout, so neither is a guess about what a structure *is* - the only
//! open question is **which one this target uses**, and that is a hypothesis the guest is the
//! only oracle for (principle 5, D374).
//!
//! So it is `ORBISTOUN_STAT_LAYOUT`, defaulting to the older one because the target's user
//! space predates the change, and a run can try the other without a rebuild.
//!
//! # What is filled in, and what is honestly zero
//!
//! Size, type and the three timestamps come from the host and are true. Ownership, device
//! numbers, inode numbers and generation counts are **zero**: they describe a filesystem this
//! is not, and a plausible number there is a fact about nothing that a guest might print or
//! compare.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Answered by a call that worked.
const OK: u64 = 0;

/// Answered by a call that did not.
const FAILED: u64 = -1_i64 as u64;

/// `S_IFDIR`, from `sys/sys/stat.h`.
pub const S_IFDIR: u32 = 0o040_000;

/// `S_IFREG`, from `sys/sys/stat.h`.
pub const S_IFREG: u32 = 0o100_000;

/// `S_IFCHR`, from `sys/sys/stat.h`.
pub const S_IFCHR: u32 = 0o020_000;

/// The permission bits reported for a device.
///
/// `0444`: readable by anyone and writable by nobody, which is what [`crate::device`]
/// actually enforces rather than what a console would say.
const DEVICE_MODE: u32 = 0o444;

/// `DT_DIR`, from `sys/sys/dirent.h`.
pub const DT_DIR: u8 = 4;

/// `DT_REG`, from `sys/sys/dirent.h`.
pub const DT_REG: u8 = 8;

/// The permission bits reported for everything.
///
/// **`0755` for a directory and `0644` for a file**, which is what the mount model actually
/// enforces at the only granularity it has: everything readable, and writable only under the
/// storage the installation owns. `chmod` records that a mode cannot be honoured; this is the
/// same fact from the reading side.
const DIRECTORY_MODE: u32 = 0o755;

/// The permission bits reported for a file.
const FILE_MODE: u32 = 0o644;

/// Which generation of the platform's structures a guest expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The layout FreeBSD used through release 11, and the default.
    ///
    /// Chosen because the target's user space predates the change, which is a **hypothesis**
    /// rather than a measurement - and the reason this is a setting at all.
    FreeBsd11,
    /// The layout in the checkout the constants are harvested from.
    Current,
}

impl Layout {
    /// What this run is using.
    pub fn configured() -> Self {
        static CHOSEN: OnceLock<Layout> = OnceLock::new();
        *CHOSEN.get_or_init(|| match orbistoun_env::STAT_LAYOUT.get().as_deref() {
            Some("current") => Self::Current,
            _ => Self::FreeBsd11,
        })
    }

    /// Bytes of a `struct stat`.
    const fn stat_len(self) -> usize {
        match self {
            Self::FreeBsd11 => 120,
            Self::Current => 224,
        }
    }

    /// Bytes of a `struct dirent` before the name.
    const fn dirent_name_at(self) -> usize {
        match self {
            Self::FreeBsd11 => 8,
            Self::Current => 24,
        }
    }

    /// Bytes of a whole `struct dirent`, name included.
    const fn dirent_len(self) -> usize {
        self.dirent_name_at() + 256
    }
}

/// What a guest is told about one file.
#[derive(Debug, Clone, Copy)]
struct Facts {
    /// Its mode, type bits and all.
    mode: u32,
    /// Its size in bytes.
    size: u64,
    /// Seconds and nanoseconds of its last modification.
    modified: (u64, u32),
}

/// What the host says about a path, or nothing.
fn facts_about(host: &std::path::Path) -> Option<Facts> {
    let data = std::fs::metadata(host).ok()?;
    let modified = data
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or((0, 0), |d| (d.as_secs(), d.subsec_nanos()));
    Some(Facts {
        mode: if data.is_dir() {
            S_IFDIR | DIRECTORY_MODE
        } else {
            S_IFREG | FILE_MODE
        },
        size: data.len(),
        modified,
    })
}

/// Writes a `struct stat` where a guest asked for one.
///
/// Every field this cannot know is left zero, which is the honest content: device and inode
/// numbers describe a filesystem this is not.
fn write_stat(at: u64, facts: Facts) -> bool {
    let layout = Layout::configured();
    let mut block = vec![0_u8; layout.stat_len()];
    let (seconds, nanos) = facts.modified;

    match layout {
        Layout::FreeBsd11 => {
            block[8..10].copy_from_slice(&(facts.mode as u16).to_le_bytes());
            // One link, because every path here names one file. Zero would say the file is
            // unlinked and waiting to be reclaimed, which is a different thing.
            block[10..12].copy_from_slice(&1_u16.to_le_bytes());
            put_timespec(&mut block, 24, seconds, nanos);
            put_timespec(&mut block, 40, seconds, nanos);
            put_timespec(&mut block, 56, seconds, nanos);
            block[72..80].copy_from_slice(&facts.size.to_le_bytes());
            block[88..92].copy_from_slice(&BLOCK_SIZE.to_le_bytes());
            put_timespec(&mut block, 104, seconds, nanos);
        }
        Layout::Current => {
            block[16..24].copy_from_slice(&1_u64.to_le_bytes());
            block[24..26].copy_from_slice(&(facts.mode as u16).to_le_bytes());
            put_timespec(&mut block, 48, seconds, nanos);
            put_timespec(&mut block, 64, seconds, nanos);
            put_timespec(&mut block, 80, seconds, nanos);
            put_timespec(&mut block, 96, seconds, nanos);
            block[112..120].copy_from_slice(&facts.size.to_le_bytes());
            block[128..132].copy_from_slice(&BLOCK_SIZE.to_le_bytes());
        }
    }
    // Blocks allocated, derived from the size rather than asked of the host: what a host
    // really allocated is about its filesystem, and a guest that multiplies this by the
    // block size expects it to cover the file.
    let blocks = facts.size.div_ceil(u64::from(BLOCK_SIZE));
    let blocks_at = match layout {
        Layout::FreeBsd11 => 80,
        Layout::Current => 120,
    };
    block[blocks_at..blocks_at + 8].copy_from_slice(&blocks.to_le_bytes());

    let Ok(destination) = usize::try_from(at) else {
        return false;
    };
    if destination == 0 {
        return false;
    }
    // SAFETY: a guest-supplied `struct stat *` under the identity mapping (D014), written
    // with exactly the number of bytes the chosen layout says one has.
    unsafe {
        std::ptr::copy_nonoverlapping(
            block.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(destination),
            block.len(),
        );
    }
    true
}

/// The block size reported, and the unit the block count is in.
const BLOCK_SIZE: u32 = 512;

/// Writes a `timespec` into `block` at `at`.
fn put_timespec(block: &mut [u8], at: usize, seconds: u64, nanos: u32) {
    block[at..at + 8].copy_from_slice(&seconds.to_le_bytes());
    block[at + 8..at + 16].copy_from_slice(&u64::from(nanos).to_le_bytes());
}

/// `stat(path, buffer)` - what a guest is told about a path.
///
/// Reference: POSIX.1-2008 `stat(2)`; the structure from `sys/sys/stat.h`, and which
/// generation of it is a setting (D374).
fn stat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(guest) = crate::read_guest_path(args[0]) else {
        return FAILED;
    };
    let Some(facts) = facts_of(&guest) else {
        return FAILED;
    };
    if write_stat(args[1], facts) {
        OK
    } else {
        FAILED
    }
}

/// What a guest is told about a path, host directory or mount point.
///
/// **A mount point is a directory with no host behind it.** `/` is the case that matters: it
/// holds `app0` and `data` and no host directory holds either, so asking the host about it
/// answers nothing and a guest is told the root does not exist (D385).
///
/// Its times are zero and its size is zero, which is the honest content - it is not a
/// directory on any disk, so there is no modification time to report and inventing one would
/// be a fact about nothing.
fn facts_of(guest: &str) -> Option<Facts> {
    if let Some(host) = crate::mount::resolve(guest)
        && let Some(facts) = facts_about(&host)
    {
        return Some(facts);
    }
    if crate::mount::is_directory(guest) || crate::device::is_directory(guest) {
        return Some(Facts {
            mode: S_IFDIR | DIRECTORY_MODE,
            size: 0,
            modified: (0, 0),
        });
    }
    if crate::device::named(guest).is_some() {
        // A character device: no size, no times. `S_IFCHR` rather than `S_IFREG`, because a
        // caller that stats it before reading is asking exactly this question - a program
        // told the kernel log is a regular file of size zero concludes there is nothing in
        // it (D389).
        return Some(Facts {
            mode: S_IFCHR | DEVICE_MODE,
            size: 0,
            modified: (0, 0),
        });
    }
    // A path the guest asked about and this could not answer. Recorded rather than only
    // refused: it is a path something real wanted, spelled by the thing that wanted it, which
    // is the only kind of evidence there is about what the mount table is missing (D387).
    crate::wanted::note(guest);
    None
}

/// `lstat(path, buffer)` - the same, without following a symbolic link.
///
/// **The same as `stat` here, and that is stated.** Nothing a guest can reach through a mount
/// is a symbolic link this created, and following one out of a mount is already refused by
/// path resolution - so the distinction the call exists to make has nothing to act on.
///
/// Reference: POSIX.1-2008 `lstat(2)`.
fn lstat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    stat(args)
}

/// `fstat(fd, buffer)` - the same, by descriptor.
///
/// Reference: POSIX.1-2008 `fstat(2)`.
fn fstat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(facts) = crate::descriptor::facts(args[0]).map(|data| {
        let modified = data
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or((0, 0), |d| (d.as_secs(), d.subsec_nanos()));
        Facts {
            mode: if data.is_dir() {
                S_IFDIR | DIRECTORY_MODE
            } else {
                S_IFREG | FILE_MODE
            },
            size: data.len(),
            modified,
        }
    }) else {
        return FAILED;
    };
    if write_stat(args[1], facts) {
        OK
    } else {
        FAILED
    }
}

/// One open directory, and the entry the guest is currently looking at.
struct Directory {
    /// What is left to hand out.
    remaining: std::vec::IntoIter<(String, bool)>,
    /// The buffer `readdir` answers a pointer to.
    ///
    /// **One buffer per directory, reused.** `readdir` answers a pointer the caller reads
    /// and does not free, valid until the next call on the same directory - so a fresh
    /// allocation per entry would leak one per file in the listing.
    entry: Vec<u8>,
}

/// Open directories, by the handle a guest holds them with.
fn directories() -> &'static Mutex<BTreeMap<u64, Directory>> {
    static OPEN: OnceLock<Mutex<BTreeMap<u64, Directory>>> = OnceLock::new();
    OPEN.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Empties the open-directory table between tests - see [`crate::descriptor::clear`].
#[cfg(test)]
pub(crate) fn clear() {
    if let Ok(mut open) = directories().lock() {
        open.clear();
    }
}

/// `opendir(path)` - answers a handle a guest walks with `readdir`.
///
/// **A handle is an address**, for the same reason `fopen`'s is (D165): a guest dereferences
/// what it gets, so an error code there is a wild pointer. The listing is taken once, at open,
/// which is what a directory stream is - a snapshot a program iterates.
///
/// Reference: POSIX.1-2008 `opendir(3)`.
fn opendir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(guest) = crate::read_guest_path(args[0]) else {
        return 0;
    };
    // **The mount points first, then the host's own entries.** A path can be both: a mount
    // at `/system_data/priv` makes `/system_data` a directory that no host holds, and if
    // something is *also* mounted at `/system_data` its files belong in the same listing.
    let mut below = crate::mount::mounts_under(&guest);
    if crate::device::is_directory(&guest) {
        below.extend(crate::device::in_directory());
    } else if guest.replace('\\', "/").trim_end_matches('/') == "/" {
        // `/dev` exists because a device is in it, which nothing else knows.
        below.push(crate::device::DIRECTORY.trim_start_matches('/').to_owned());
    }
    let host = crate::mount::resolve(&guest).filter(|path| path.is_dir());
    if below.is_empty() && host.is_none() {
        crate::wanted::note(&guest);
        return 0;
    }

    let mut entries: Vec<(String, bool)> = Vec::new();
    // `.` and `..` first, because a listing has them and a program counting entries or
    // skipping them expects to see them.
    entries.push((".".to_owned(), true));
    entries.push(("..".to_owned(), true));
    for name in below {
        entries.push((name, true));
    }
    if let Some(host) = host
        && let Ok(reading) = std::fs::read_dir(&host)
    {
        for found in reading.flatten() {
            let name = found.file_name().to_string_lossy().into_owned();
            // A mount point already listed wins: it is what the guest can actually enter.
            if entries.iter().any(|(held, _)| *held == name) {
                continue;
            }
            let directory = found.file_type().is_ok_and(|t| t.is_dir());
            entries.push((name, directory));
        }
    }

    let handle = crate::open::fresh_handle();
    let Ok(mut open) = directories().lock() else {
        return 0;
    };
    open.insert(
        handle,
        Directory {
            remaining: entries.into_iter(),
            entry: vec![0_u8; Layout::configured().dirent_len()],
        },
    );
    handle
}

/// `readdir(handle)` - the next entry, or null at the end.
///
/// Reference: POSIX.1-2008 `readdir(3)`; the structure from `sys/sys/dirent.h`, and which
/// generation of it is the same setting `stat` uses (D374).
fn readdir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let layout = Layout::configured();
    let Ok(mut open) = directories().lock() else {
        return 0;
    };
    let Some(directory) = open.get_mut(&args[0]) else {
        return 0;
    };
    let Some((name, is_directory)) = directory.remaining.next() else {
        // Null at the end, which is how the loop a guest wrote terminates.
        return 0;
    };

    directory.entry.fill(0);
    let bytes = name.as_bytes();
    let len = bytes.len().min(255);
    let name_at = layout.dirent_name_at();
    let kind = if is_directory { DT_DIR } else { DT_REG };
    let record = (name_at + len + 1) as u16;

    match layout {
        Layout::FreeBsd11 => {
            directory.entry[4..6].copy_from_slice(&record.to_le_bytes());
            directory.entry[6] = kind;
            directory.entry[7] = len as u8;
        }
        Layout::Current => {
            directory.entry[16..18].copy_from_slice(&record.to_le_bytes());
            directory.entry[18] = kind;
            directory.entry[20..22].copy_from_slice(&(len as u16).to_le_bytes());
        }
    }
    directory.entry[name_at..name_at + len].copy_from_slice(&bytes[..len]);
    directory.entry.as_ptr() as u64
}

/// `closedir(handle)` - gives the listing back.
///
/// Reference: POSIX.1-2008 `closedir(3)`.
fn closedir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(mut open) = directories().lock() else {
        return FAILED;
    };
    if open.remove(&args[0]).is_some() {
        OK
    } else {
        FAILED
    }
}

/// Implementations this module provides, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("stat", stat),
        ("lstat", lstat),
        ("fstat", fstat),
        ("opendir", opendir),
        ("readdir", readdir),
        ("closedir", closedir),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    use super::Layout;
    use crate::exclusively;

    fn an_installation(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("orbistoun-meta-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data/sub")).expect("data");
        std::fs::write(root.join("data/save.bin"), b"0123456789").expect("a save");
        crate::mount::clear();
        crate::mount::mount_data(root.join("data"));
        crate::mount::allow_writes(crate::mount::DATA_MOUNT);
        root
    }

    fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .expect("declared");
        function(&args)
    }

    fn path(text: &str) -> std::ffi::CString {
        std::ffi::CString::new(text).expect("a path")
    }

    /// **The size is at the offset the chosen layout says**, which is the whole risk here.
    #[test]
    fn a_files_size_lands_where_the_layout_puts_it() {
        let _guard = exclusively();
        an_installation("stat");
        let name = path("/data/save.bin");
        let mut buffer = [0_u8; 256];
        assert_eq!(
            call(
                "stat",
                [name.as_ptr() as u64, buffer.as_mut_ptr() as u64, 0, 0, 0, 0]
            ),
            0
        );

        let size_at = match Layout::configured() {
            Layout::FreeBsd11 => 72,
            Layout::Current => 112,
        };
        let size = u64::from_le_bytes(buffer[size_at..size_at + 8].try_into().expect("eight"));
        assert_eq!(size, 10, "the ten bytes the file actually has");

        let mode_at = match Layout::configured() {
            Layout::FreeBsd11 => 8,
            Layout::Current => 24,
        };
        let mode = u16::from_le_bytes(buffer[mode_at..mode_at + 2].try_into().expect("two"));
        assert_eq!(
            u32::from(mode) & super::S_IFREG,
            super::S_IFREG,
            "and it is a regular file"
        );
    }

    /// A directory is reported as one, which is what a listing branches on.
    #[test]
    fn a_directory_is_reported_as_a_directory() {
        let _guard = exclusively();
        an_installation("statdir");
        let name = path("/data/sub");
        let mut buffer = [0_u8; 256];
        assert_eq!(
            call(
                "stat",
                [name.as_ptr() as u64, buffer.as_mut_ptr() as u64, 0, 0, 0, 0]
            ),
            0
        );
        let mode_at = match Layout::configured() {
            Layout::FreeBsd11 => 8,
            Layout::Current => 24,
        };
        let mode = u32::from(u16::from_le_bytes(
            buffer[mode_at..mode_at + 2].try_into().expect("two"),
        ));
        assert_eq!(mode & super::S_IFDIR, super::S_IFDIR);
    }

    /// A path that is not there fails rather than answering zeroes.
    #[test]
    fn a_path_that_is_not_there_fails() {
        let _guard = exclusively();
        an_installation("statmissing");
        let name = path("/data/nothing");
        let mut buffer = [0xAA_u8; 256];
        assert_ne!(
            call(
                "stat",
                [name.as_ptr() as u64, buffer.as_mut_ptr() as u64, 0, 0, 0, 0]
            ),
            0
        );
        assert_eq!(buffer[0], 0xAA, "and nothing is written");
    }

    /// **The listing a file server walks**, including the two entries every directory has.
    #[test]
    fn a_directory_can_be_opened_walked_and_closed() {
        let _guard = exclusively();
        an_installation("dir");
        let name = path("/data");
        let handle = call("opendir", [name.as_ptr() as u64, 0, 0, 0, 0, 0]);
        assert_ne!(handle, 0, "a handle, which the guest dereferences");

        let layout = Layout::configured();
        let name_at = match layout {
            Layout::FreeBsd11 => 8,
            Layout::Current => 24,
        };
        let mut seen = Vec::new();
        loop {
            let entry = call("readdir", [handle, 0, 0, 0, 0, 0]);
            if entry == 0 {
                break;
            }
            // SAFETY: `readdir` answered a pointer into the directory's own buffer, which
            // lives until the next call on it.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    std::ptr::with_exposed_provenance::<u8>(entry as usize),
                    name_at + 256,
                )
            };
            let end = bytes[name_at..]
                .iter()
                .position(|b| *b == 0)
                .expect("terminated");
            seen.push(String::from_utf8_lossy(&bytes[name_at..name_at + end]).into_owned());
            assert!(seen.len() < 16, "the listing terminates");
        }

        assert!(seen.contains(&".".to_owned()));
        assert!(seen.contains(&"..".to_owned()));
        assert!(seen.contains(&"save.bin".to_owned()));
        assert!(seen.contains(&"sub".to_owned()));
        assert_eq!(call("closedir", [handle, 0, 0, 0, 0, 0]), 0);
        assert_ne!(
            call("closedir", [handle, 0, 0, 0, 0, 0]),
            0,
            "and closing it twice is refused"
        );
    }

    /// A directory nobody opened answers null rather than a wild pointer.
    #[test]
    fn reading_a_directory_nobody_opened_answers_nothing() {
        assert_eq!(call("readdir", [0xDEAD, 0, 0, 0, 0, 0]), 0);
    }

    /// The two layouts really do differ, which is why the setting exists.
    #[test]
    fn the_two_layouts_put_things_in_different_places() {
        assert_ne!(
            Layout::FreeBsd11.stat_len(),
            Layout::Current.stat_len(),
            "if these agreed there would be nothing to choose between"
        );
        assert_ne!(
            Layout::FreeBsd11.dirent_name_at(),
            Layout::Current.dirent_name_at()
        );
    }
}
