//! What a guest is told about a file, and how it lists a directory.
//!
//! The FreeBSD checkout is newer than the target, and `struct stat` and `struct dirent`
//! changed shape in between (`st_dev`, `st_nlink` and `d_fileno` widened, fields reordered,
//! `d_off` added). Writing the modern layout for an older guest puts the size at the wrong
//! offset. Both layouts are in the checkout (`freebsd11_stat`, `freebsd11_dirent`), so which
//! one the target uses is `ORBISTOUN_STAT_LAYOUT`, defaulting to the older one (D374).
//!
//! Size, type and the three timestamps come from the host. Ownership, device, inode and
//! generation numbers are zero: they describe a filesystem this is not.

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
/// enforces.
const DEVICE_MODE: u32 = 0o444;

/// `DT_DIR`, from `sys/sys/dirent.h`.
pub const DT_DIR: u8 = 4;

/// `DT_REG`, from `sys/sys/dirent.h`.
pub const DT_REG: u8 = 8;

/// The permission bits reported for everything.
///
/// `0755` for a directory and `0644` for a file: what the mount model enforces at the only
/// granularity it has, everything readable and writable only under the storage the
/// installation owns.
const DIRECTORY_MODE: u32 = 0o755;

/// The permission bits reported for a file.
const FILE_MODE: u32 = 0o644;

/// Which generation of the platform's structures a guest expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The layout FreeBSD used through release 11, and the default.
    ///
    /// The target's user space predates the change; this is a hypothesis, which is why the
    /// layout is a setting.
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
/// Every field this cannot know is left zero: device and inode numbers describe a filesystem
/// this is not.
fn write_stat(at: u64, facts: Facts) -> bool {
    let layout = Layout::configured();
    let mut block = vec![0_u8; layout.stat_len()];
    let (seconds, nanos) = facts.modified;

    match layout {
        Layout::FreeBsd11 => {
            block[8..10].copy_from_slice(&(facts.mode as u16).to_le_bytes());
            // One link, since every path here names one file; zero would say the file is
            // unlinked.
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
    // Blocks allocated, derived from the size so a guest multiplying by the block size covers
    // the file.
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
    // SAFETY: a guest-supplied `struct stat *` under the identity mapping, written with
    // exactly the number of bytes the chosen layout says one has.
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

/// `stat(path, buffer)`: what a guest is told about a path.
///
/// Reference: POSIX.1-2008 `stat(2)`; the structure from `sys/sys/stat.h`, and which
/// generation of it is a setting (D374).
fn stat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(guest) = (unsafe { orbistoun_mem::guest::read_path(args[0]) }) else {
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

/// `sceKernelStat(path, buffer)`: the vendor-named form of [`stat`].
///
/// The two differ only on failure (D525): POSIX `stat` answers `-1`, a `sceKernel*` call a
/// vendor code in the `0x8002_00xx` family, which a caller tests as a negative 32-bit value.
/// The success path and the `struct stat` it writes are shared. `ENOENT` for a path nothing
/// answers. Registered in `lib.rs` under `libkernel_fs`, the library that declares the name.
pub(crate) fn kernel_stat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(guest) = (unsafe { orbistoun_mem::guest::read_path(args[0]) }) else {
        return u64::from(
            orbistoun_core::GuestError::vendor(orbistoun_core::errno::INVALID).as_raw(),
        );
    };
    let Some(facts) = facts_of(&guest) else {
        return u64::from(
            orbistoun_core::GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw(),
        );
    };
    if write_stat(args[1], facts) {
        OK
    } else {
        // The path resolved and the destination did not, so this is the caller's buffer.
        u64::from(orbistoun_core::GuestError::vendor(orbistoun_core::errno::INVALID).as_raw())
    }
}

/// What a guest is told about a path, host directory or mount point.
///
/// A mount point is a directory with no host behind it: `/` holds `app0` and `data` and no
/// host directory holds either. Its times and size are zero, since it is on no disk.
fn facts_of(guest: &str) -> Option<Facts> {
    if let Some(host) = crate::mount::resolve_existing(guest)
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
        // A character device: no size, no times. `S_IFCHR` rather than `S_IFREG`, since a
        // program told the kernel log is a regular file of size zero concludes it is empty
        // (D389).
        return Some(Facts {
            mode: S_IFCHR | DEVICE_MODE,
            size: 0,
            modified: (0, 0),
        });
    }
    // A path the guest asked about and this could not answer, recorded as evidence of what
    // the mount table is missing (D387).
    crate::wanted::note(guest);
    None
}

/// `lstat(path, buffer)`: the same, without following a symbolic link.
///
/// The same as `stat` here: nothing reachable through a mount is a symbolic link this
/// created, and following one out of a mount is refused by path resolution.
///
/// Reference: POSIX.1-2008 `lstat(2)`.
fn lstat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    stat(args)
}

/// What an open descriptor's host metadata says, in the [`Facts`] this module writes.
///
/// Shared by [`fstat`] and [`kernel_fstat`] so the `struct stat` a descriptor is described by
/// stays one decision (D525).
fn facts_from_descriptor(data: &std::fs::Metadata) -> Facts {
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
}

/// `fstat(fd, buffer)`: the same, by descriptor.
///
/// Reference: POSIX.1-2008 `fstat(2)`.
fn fstat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(facts) = crate::descriptor::facts(args[0])
        .as_ref()
        .map(facts_from_descriptor)
    else {
        return FAILED;
    };
    if write_stat(args[1], facts) {
        OK
    } else {
        FAILED
    }
}

/// `sceKernelFstat(fd, buffer)`: the vendor-named form of [`fstat`].
///
/// Related to `fstat` as [`kernel_stat`] is to `stat` (D525): the success path is shared
/// through [`facts_from_descriptor`], and failure is a vendor `0x8002_00xx` code: `EBADF` for
/// a descriptor with no open file, `EFAULT` for a buffer that cannot be written.
pub(crate) fn kernel_fstat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(facts) = crate::descriptor::facts(args[0])
        .as_ref()
        .map(facts_from_descriptor)
    else {
        return u64::from(
            orbistoun_core::GuestError::vendor(orbistoun_core::errno::BAD_DESCRIPTOR).as_raw(),
        );
    };
    if write_stat(args[1], facts) {
        OK
    } else {
        u64::from(orbistoun_core::GuestError::vendor(orbistoun_core::errno::FAULT).as_raw())
    }
}

/// One open directory, and the entry the guest is currently looking at.
struct Directory {
    /// What is left to hand out.
    remaining: std::vec::IntoIter<(String, bool)>,
    /// The buffer `readdir` answers a pointer to.
    ///
    /// One buffer per directory, reused: `readdir` answers a pointer valid until the next call
    /// on the same directory, so a fresh allocation per entry would leak.
    entry: Vec<u8>,
}

/// Open directories, by the handle a guest holds them with.
fn directories() -> &'static Mutex<BTreeMap<u64, Directory>> {
    static OPEN: OnceLock<Mutex<BTreeMap<u64, Directory>>> = OnceLock::new();
    OPEN.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Empties the open-directory table between tests; see [`crate::descriptor::clear`].
#[cfg(test)]
pub(crate) fn clear() {
    if let Ok(mut open) = directories().lock() {
        open.clear();
    }
}

/// `opendir(path)`: answers a handle a guest walks with `readdir`.
///
/// The handle is an address, since a guest dereferences it (D151). The listing is taken once
/// at open: a directory stream is a snapshot a program iterates.
///
/// Reference: POSIX.1-2008 `opendir(3)`.
fn opendir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(guest) = (unsafe { orbistoun_mem::guest::read_path(args[0]) }) else {
        return 0;
    };
    let Some(entries) = listing(&guest) else {
        crate::wanted::note(&guest);
        return 0;
    };
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

/// A guest directory's entries, `(name, is a directory)`, `.` and `..` first, or `None` when
/// the guest path is no directory here. Both `opendir` and `getdirentries` read it, so a
/// directory lists the same through either.
pub(crate) fn listing(guest: &str) -> Option<Vec<(String, bool)>> {
    // The mount points first, then the host's own entries. A path can be both: a mount at
    // `/system_data/priv` makes `/system_data` a directory, and anything also mounted there
    // belongs in the same listing.
    let mut below = crate::mount::mounts_under(guest);
    if crate::device::is_directory(guest) {
        below.extend(crate::device::in_directory());
    } else if guest.replace('\\', "/").trim_end_matches('/') == "/" {
        // `/dev` exists because a device is in it, which nothing else knows.
        below.push(crate::device::DIRECTORY.trim_start_matches('/').to_owned());
    }
    let host = crate::mount::resolve_existing(guest).filter(|path| path.is_dir());
    if below.is_empty() && host.is_none() {
        return None;
    }

    let mut entries: Vec<(String, bool)> = Vec::new();
    // `.` and `..` first, as a program counting or skipping them expects.
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
            // A mount point already listed wins: it is what the guest can enter.
            if entries.iter().any(|(held, _)| *held == name) {
                continue;
            }
            let directory = found.file_type().is_ok_and(|t| t.is_dir());
            entries.push((name, directory));
        }
    }
    Some(entries)
}

/// `readdir(handle)`: the next entry, or null at the end.
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
        // Null at the end terminates the guest's loop.
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

/// `closedir(handle)`: gives the listing back.
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

    /// `sceKernelFstat` answers a vendor code, not POSIX -1, for a bad descriptor (D525).
    #[test]
    fn kernel_fstat_answers_a_vendor_code_on_a_bad_descriptor() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = u64::MAX; // no open file behind this descriptor
        let answer = super::kernel_fstat(&args);
        assert_ne!(
            answer,
            super::FAILED,
            "not the POSIX -1 a sceKernel* caller never matches"
        );
        assert_eq!(
            answer,
            u64::from(
                orbistoun_core::GuestError::vendor(orbistoun_core::errno::BAD_DESCRIPTOR).as_raw()
            ),
            "EBADF in the vendor 0x8002_00xx family"
        );
    }

    /// The vendor stat fails with a vendor code, never -1 (D525).
    ///
    /// Pins the family and the sign, which a caller branches on. The specific errno the
    /// hardware answers for a missing path is unmeasured; `ENOENT` is what the failure is here.
    #[test]
    fn the_vendor_stat_refuses_with_a_vendor_code_and_never_minus_one() {
        let _guard = exclusively();
        an_installation("kernel-stat");
        let mut buffer = [0_u8; 256];
        let at = buffer.as_mut_ptr() as usize as u64;

        // Called directly: the body lives here and the name is registered by
        // `libkernel_fs`.
        let good = path("/data/save.bin");
        assert_eq!(
            super::kernel_stat(&[good.as_ptr() as usize as u64, at, 0, 0, 0, 0]),
            0,
            "a path that exists is answered like the POSIX form"
        );

        let missing = path("/data/nothing-here.bin");
        let refused = super::kernel_stat(&[missing.as_ptr() as usize as u64, at, 0, 0, 0, 0]);
        assert_ne!(refused, 0, "a missing path is a failure");
        assert_ne!(
            refused,
            u64::MAX,
            concat!(
                "and NOT the POSIX -1 - that is the whole reason this is not the POSIX ",
                "function under a second name"
            )
        );
        assert_eq!(
            refused & 0xffff_0000,
            0x8002_0000,
            "it is in the vendor family a caller tests against: {refused:#x}"
        );
    }

    /// A file's size lands at the offset the chosen layout gives it.
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

    /// A directory opens, walks with `.` and `..` included, and closes.
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

    /// The two layouts put fields in different places.
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
