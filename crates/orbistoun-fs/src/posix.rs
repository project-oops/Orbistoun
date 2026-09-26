//! The file calls that change a directory, under their POSIX names.
//!
//! These are the half of a file server that is not reading (`docs/PAYLOADS.md`). Each goes
//! through the same two gates as `create`: [`mount::resolve`], which refuses a path under no
//! mount or climbing out of one, and [`mount::is_writable`], which refuses anything outside
//! the storage the installation owns, so a guest cannot delete its own title (D250). A
//! missing path and a forbidden one both answer failure, as the interface would, without
//! telling a guest about files outside its storage. POSIX defines each call and its return
//! value; no `errno` is invented beside it.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

use crate::mount;

/// What a call here answers when it worked.
const OK: u64 = 0;

/// What it answers when it did not.
///
/// Negative one, as each of these documents. A caller tests `< 0`, and a large positive
/// placeholder would read as success.
const FAILED: u64 = -1_i64 as u64;

/// The host path a guest path names, if the guest may write to it.
///
/// Both gates in one place, since they are one question: may this call touch this path. The
/// top layer's copy, copied up from a lower layer first when that is the only one, so a
/// write never lands in the base tree or a staged title's library files (D722).
///
/// # Safety
///
/// `address` is a guest path, under the `orbistoun_mem::guest` contract.
unsafe fn writable_host_path(address: u64) -> Option<std::path::PathBuf> {
    // SAFETY: the caller's contract.
    let guest = unsafe { orbistoun_mem::guest::read_path(address) }?;
    mount::resolve_for_write(&guest)
}

/// The host path a guest may create, remove or rename a name at: the top layer's, and only
/// while no lower layer also holds the name, since removing it there needs a whiteout (D722).
///
/// # Safety
///
/// `address` is a guest path, under the `orbistoun_mem::guest` contract.
unsafe fn removable_host_path(address: u64) -> Option<std::path::PathBuf> {
    // SAFETY: the caller's contract.
    let guest = unsafe { orbistoun_mem::guest::read_path(address) }?;
    mount::resolve_for_removal(&guest)
}

/// Asking whether a path may be written, as a guest spells it.
///
/// Named because `orbistoun-libc` has a test comparing it against the harvested table, which
/// this crate cannot read.
pub const W_OK: u64 = 0x2;

/// Turns a host result into what the guest is told.
fn answered(worked: bool) -> u64 {
    if worked { OK } else { FAILED }
}

/// `mkdir(path, mode)`: creates one directory.
///
/// The mode is not applied: the host need not have POSIX permission bits, and a guest cannot
/// observe them from inside the emulator. One directory, not a path: a missing parent fails,
/// as the interface says.
///
/// Reference: POSIX.1-2008 `mkdir(2)`.
fn mkdir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(host) = (unsafe { removable_host_path(args[0]) }) else {
        return FAILED;
    };
    answered(std::fs::create_dir(host).is_ok())
}

/// `rmdir(path)`: removes one empty directory.
///
/// Empty only, as the interface says: a directory with anything in it fails.
///
/// Reference: POSIX.1-2008 `rmdir(2)`.
fn rmdir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(host) = (unsafe { removable_host_path(args[0]) }) else {
        return FAILED;
    };
    answered(std::fs::remove_dir(host).is_ok())
}

/// `unlink(path)`: removes one file.
///
/// Refuses a directory, checked here because host platforms disagree about it.
///
/// Reference: POSIX.1-2008 `unlink(2)`.
fn unlink(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(host) = (unsafe { removable_host_path(args[0]) }) else {
        return FAILED;
    };
    if host.is_dir() {
        return FAILED;
    }
    answered(std::fs::remove_file(host).is_ok())
}

/// `remove(path)`: the C spelling, which takes either.
///
/// Not an alias for `unlink`: C's `remove` removes a file or an empty directory.
///
/// Reference: ISO C `remove`; POSIX.1-2008 `remove(3)`.
fn remove(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(host) = (unsafe { removable_host_path(args[0]) }) else {
        return FAILED;
    };
    let worked = if host.is_dir() {
        std::fs::remove_dir(host).is_ok()
    } else {
        std::fs::remove_file(host).is_ok()
    };
    answered(worked)
}

/// `rename(from, to)`: moves a file within the guest's own storage.
///
/// Both ends go through the same gates, so a rename cannot write outside the writable mount.
///
/// Reference: POSIX.1-2008 `rename(2)`.
fn rename(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: both are paths the guest passed, whose call contract is a NUL-terminated string
    // in guest memory.
    let from = unsafe { removable_host_path(args[0]) };
    // SAFETY: as above.
    let to = unsafe { removable_host_path(args[1]) };
    let (Some(from), Some(to)) = (from, to) else {
        return FAILED;
    };
    answered(std::fs::rename(from, to).is_ok())
}

/// `access(path, mode)`: whether a path is there, and whether it may be written.
///
/// Read and execute are answered by existence, since everything reachable through a mount is
/// readable. Write is answered by the writable mount rather than host permissions, since
/// that is the rule this emulator enforces.
///
/// Reference: POSIX.1-2008 `access(2)`; `W_OK` from `sys/sys/unistd.h`.
fn access(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(guest) = (unsafe { orbistoun_mem::guest::read_path(args[0]) }) else {
        return FAILED;
    };
    let Some(host) = mount::resolve_existing(&guest) else {
        return FAILED;
    };
    if !host.exists() {
        return FAILED;
    }
    if args[1] & W_OK != 0 && !mount::is_writable(&guest) {
        return FAILED;
    }
    OK
}

/// `truncate(path, length)`: sets a file's size.
///
/// Reference: POSIX.1-2008 `truncate(2)`.
fn truncate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(host) = (unsafe { writable_host_path(args[0]) }) else {
        return FAILED;
    };
    let Ok(file) = std::fs::OpenOptions::new().write(true).open(host) else {
        return FAILED;
    };
    answered(file.set_len(args[1]).is_ok())
}

/// `ftruncate(fd, length)`: the same, by descriptor.
///
/// Reference: POSIX.1-2008 `ftruncate(2)`.
fn ftruncate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    answered(crate::descriptor::set_length(args[0], args[1]))
}

/// How wide one `struct iovec` is, and where its two fields sit.
///
/// `{ void *iov_base; size_t iov_len; }`: two machine words on this target. Reference:
/// POSIX.1-2008 `<sys/uio.h>`.
const IOVEC_BYTES: u64 = 16;

/// Reads one `iovec` from a guest's vector.
fn iovec(vector: u64, index: u64) -> Option<(u64, u64)> {
    if vector == 0 {
        return None;
    }
    let at = vector.checked_add(index.checked_mul(IOVEC_BYTES)?)?;
    let base = usize::try_from(at).ok()?;
    // SAFETY: a guest-supplied `iovec` array under the identity mapping, indexed within the
    // count the guest passed, the same contract the real call has.
    let iov_base = unsafe { std::ptr::read_unaligned(base as *const u64) };
    // SAFETY: the second word of the same entry, eight bytes after the first.
    let iov_len = unsafe { std::ptr::read_unaligned((base + 8) as *const u64) };
    Some((iov_base, iov_len))
}

/// The shared body of the scatter/gather calls.
///
/// Stops at the first short transfer, as the specification requires: `readv` and `writev`
/// answer the bytes actually moved.
fn gather(args: &[u64; GUEST_ARG_REGISTERS], offset: Option<u64>, writing: bool) -> u64 {
    let (fd, vector, count) = (args[0], args[1], args[2]);
    let mut moved = 0_u64;
    for index in 0..count {
        let Some((base, len)) = iovec(vector, index) else {
            return FAILED;
        };
        if len == 0 {
            continue;
        }
        let at = offset.map(|start| start + moved);
        let done = if writing {
            let Some(bytes) = guest_bytes(base, len) else {
                return FAILED;
            };
            match at {
                Some(start) => crate::descriptor::write_at(fd, bytes, start),
                None => crate::descriptor::write(fd, bytes),
            }
        } else {
            let Some(into) = guest_bytes_mut(base, len) else {
                return FAILED;
            };
            match at {
                Some(start) => crate::descriptor::read_at(fd, into, start),
                None => crate::descriptor::read(fd, into),
            }
        };
        let Some(done) = done else {
            return if moved == 0 { FAILED } else { moved };
        };
        moved += done as u64;
        if (done as u64) < len {
            break;
        }
    }
    moved
}

/// `readv(fd, iov, iovcnt)`: a read scattered across several buffers.
///
/// Reference: POSIX.1-2008 `readv(2)`.
fn readv(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    gather(args, None, false)
}

/// `writev(fd, iov, iovcnt)`: a write gathered from several buffers.
///
/// Reference: POSIX.1-2008 `writev(2)`.
fn writev(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    gather(args, None, true)
}

/// `preadv(fd, iov, iovcnt, offset)`: [`readv`] at an explicit offset, leaving the file
/// position alone.
///
/// Reference: POSIX.1-2008 `preadv(2)`.
fn preadv(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    gather(args, Some(args[3]), false)
}

/// `pwritev(fd, iov, iovcnt, offset)`: [`writev`] at an explicit offset.
///
/// Reference: POSIX.1-2008 `pwritev(2)`.
fn pwritev(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    gather(args, Some(args[3]), true)
}

/// `creat(path, mode)`: create a file for writing, truncating an existing one.
///
/// Reference: POSIX.1-2008 `creat(2)`, which defines it as exactly
/// `open(path, O_WRONLY | O_CREAT | O_TRUNC, mode)`. The mode is not honoured: this layer has
/// no permission model to apply it to.
fn creat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(path) = (unsafe { orbistoun_mem::guest::read_path(args[0]) }) else {
        return FAILED;
    };
    crate::descriptor::create(&path).unwrap_or(FAILED)
}

/// `fsync(fd)`: flush a file's contents to the storage behind it.
///
/// Reference: POSIX.1-2008 `fsync(2)`. A real flush, since the caller relies on its bytes
/// having landed.
fn fsync(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    answered(crate::descriptor::sync(args[0]))
}

/// `fdatasync(fd)`: as [`fsync`], without promising to flush metadata.
///
/// Reference: POSIX.1-2008 `fdatasync(2)`. Flushing metadata as well is more than the call
/// promises, and so conforming.
fn fdatasync(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    answered(crate::descriptor::sync(args[0]))
}

/// `getpagesize()`: the size of a page, in bytes.
///
/// Reference: POSIX.1-2001 `getpagesize(2)`. Answered from `GUEST_PAGE_SIZE`, the page size
/// the guest address space is built on, so a guest rounding an allocation gets the mapper's
/// number.
fn getpagesize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_core::GUEST_PAGE_SIZE
}

/// `madvise(addr, len, advice)`: tell the system how a range will be used.
///
/// Reference: POSIX.1-2008 `posix_madvise(2)`: the advice is not binding and an
/// implementation may ignore it, so answering success without acting conforms.
fn madvise(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
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

/// A guest buffer, as bytes this may read.
fn guest_bytes<'a>(address: u64, length: u64) -> Option<&'a [u8]> {
    if address == 0 {
        return None;
    }
    let at = usize::try_from(address).ok()?;
    let len = usize::try_from(length).ok()?;
    // SAFETY: as above, read rather than written.
    Some(unsafe { std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), len) })
}

/// `pread(fd, buffer, count, offset)`: a read at an offset, leaving the position alone.
///
/// The position is the point: `pread` lets two threads read one file at once, which a seek,
/// read and seek back cannot do race-free.
///
/// Reference: POSIX.1-2008 `pread(2)`.
fn pread(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(into) = guest_bytes_mut(args[1], args[2]) else {
        return FAILED;
    };
    crate::descriptor::read_at(args[0], into, args[3]).map_or(FAILED, |n| n as u64)
}

/// `pwrite(fd, buffer, count, offset)`: the same, writing.
///
/// Reference: POSIX.1-2008 `pwrite(2)`.
fn pwrite(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(bytes) = guest_bytes(args[1], args[2]) else {
        return FAILED;
    };
    crate::descriptor::write_at(args[0], bytes, args[3]).map_or(FAILED, |n| n as u64)
}

/// `dup2(from, to)`: makes one descriptor refer to what another refers to.
///
/// Answers the new descriptor, `to`, as the interface does.
///
/// Reference: POSIX.1-2008 `dup2(2)`.
fn dup2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if crate::descriptor::duplicate_into(args[0], args[1]) {
        args[1]
    } else {
        FAILED
    }
}

/// `chmod(path, mode)`: accepted, and applied to nothing.
///
/// A file server sets a mode on a file it has created, and a failure stops the transfer, but
/// the host need not have POSIX permission bits. So the path is still checked (a `chmod` on
/// the title directory is refused as a write would be) and a mode on a file the guest owns
/// succeeds without effect.
///
/// Reference: POSIX.1-2008 `chmod(2)`.
fn chmod(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(host) = (unsafe { writable_host_path(args[0]) }) else {
        return FAILED;
    };
    // Existence still decides: `chmod` on a missing path fails.
    answered(host.exists())
}

/// `fchmod(fd, mode)`: the same, by descriptor.
///
/// Reference: POSIX.1-2008 `fchmod(2)`.
fn fchmod(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    answered(crate::descriptor::exists(args[0]))
}

/// `mlock(address, length)`: accepted, and pins nothing.
///
/// Guest memory is host memory this process reserved and holds for the whole run, so the
/// guarantee the call asks for already holds.
///
/// Reference: POSIX.1-2008 `mlock(2)`.
fn mlock(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `munlock(address, length)`: the same, unpinning nothing.
///
/// Reference: POSIX.1-2008 `munlock(2)`.
fn munlock(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `fdopen(fd, mode)`: a stream over a descriptor that is already open.
///
/// An FTP server accepts a connection, wraps the descriptor, and writes replies with
/// `fprintf`. The mode is not honoured: the descriptor decides what it can do.
///
/// Reference: POSIX.1-2008 `fdopen(3)`.
fn fdopen(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !crate::descriptor::exists(args[0]) {
        // Null rather than an error code: a caller reads this as a pointer (D125).
        return 0;
    }
    crate::open::wrap_descriptor(args[0]).unwrap_or(0)
}

/// `fileno(stream)`: the descriptor behind a stream.
///
/// Answers only for a stream that is a descriptor. A stream from `fopen` owns a host file,
/// so it has no number to give.
///
/// Reference: POSIX.1-2008 `fileno(3)`.
fn fileno(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    crate::open::wrapped_descriptor(args[0]).unwrap_or(FAILED)
}

/// `sendfile(fd, s, offset, nbytes, hdtr, sbytes, flags)`: a file straight into a socket.
///
/// How a file server sends a file; refusing it fails the transfer rather than falling back to
/// `read` and `write`. Copied in bounded chunks, since a file may be larger than this
/// process should hold at once. The header and trailer are not honoured: `hdtr` points at a
/// vendor structure whose layout is not derivable here, so a non-null one is refused.
///
/// Reference: FreeBSD `sendfile(2)`.
fn sendfile(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// Bytes moved per round.
    const CHUNK: usize = 64 * 1024;

    let (from, to, offset, count, headers) = (args[0], args[1], args[2], args[3], args[4]);
    if headers != 0 {
        return FAILED;
    }
    // Zero means "to the end of the file", as the interface says.
    let wanted = if count == 0 { u64::MAX } else { count };

    let mut buffer = vec![0_u8; CHUNK];
    let mut at = offset;
    let mut sent = 0_u64;
    while sent < wanted {
        let room = usize::try_from(wanted - sent).unwrap_or(CHUNK).min(CHUNK);
        let Some(read) = crate::descriptor::read_at(from, &mut buffer[..room], at) else {
            return FAILED;
        };
        if read == 0 {
            break;
        }
        let Some(written) = crate::descriptor::write(to, &buffer[..read]) else {
            return FAILED;
        };
        sent += written as u64;
        at += written as u64;
        if written < read {
            // A short write is the socket refusing more: the caller reads how much went and
            // sends the rest.
            break;
        }
    }

    // How many bytes went, written where the caller asked; a null destination is allowed.
    if let Ok(destination) = usize::try_from(args[5])
        && destination != 0
    {
        // SAFETY: a guest-supplied `off_t *` under the identity mapping, written unaligned
        // because nothing promises the guest aligned it.
        unsafe {
            std::ptr::write_unaligned(
                std::ptr::with_exposed_provenance_mut::<u64>(destination),
                sent,
            );
        }
    }
    OK
}

/// Implementations this module provides, by symbol name.
///
/// Declared in `libc`, where FreeBSD puts them, and implemented here beside the mount model
/// that decides whether a guest may touch a path (D367).
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("creat", creat),
        ("readv", readv),
        ("writev", writev),
        ("preadv", preadv),
        ("pwritev", pwritev),
        ("fsync", fsync),
        ("fdatasync", fdatasync),
        ("getpagesize", getpagesize),
        ("madvise", madvise),
        ("mkdir", mkdir),
        ("rmdir", rmdir),
        ("unlink", unlink),
        ("remove", remove),
        ("rename", rename),
        ("access", access),
        ("truncate", truncate),
        ("ftruncate", ftruncate),
        ("pread", pread),
        ("pwrite", pwrite),
        ("dup2", dup2),
        ("chmod", chmod),
        ("fchmod", fchmod),
        ("mlock", mlock),
        ("munlock", munlock),
        ("fdopen", fdopen),
        ("fileno", fileno),
        ("sendfile", sendfile),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    use crate::exclusively;

    /// A writable `/data` and a read-only `/app0`, which is the arrangement a guest gets.
    pub(super) fn an_installation(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("orbistoun-posix-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data")).expect("data");
        std::fs::create_dir_all(root.join("app0")).expect("app0");
        std::fs::write(root.join("app0/eboot.bin"), b"title").expect("a title file");
        crate::mount::clear();
        crate::mount::mount(crate::mount::APP_MOUNT, root.join("app0"));
        crate::mount::mount_data(root.join("data"));
        // Writability comes from the filesystem manifest, not from mounting (D251).
        crate::mount::allow_writes(crate::mount::DATA_MOUNT);
        root
    }

    /// Calls one implementation with raw arguments.
    fn call_raw(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .expect("declared");
        function(&args)
    }

    /// Calls one implementation with guest strings for its path arguments.
    fn call(name: &str, paths: &[&str], extra: u64) -> u64 {
        let held: Vec<std::ffi::CString> = paths
            .iter()
            .map(|p| std::ffi::CString::new(*p).expect("a path"))
            .collect();
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        for (slot, text) in args.iter_mut().zip(&held) {
            *slot = text.as_ptr() as usize as u64;
        }
        args[held.len()] = extra;
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .expect("declared");
        function(&args)
    }

    /// A directory can be made and removed under the writable mount.
    #[test]
    fn a_directory_can_be_made_and_removed_under_the_writable_mount() {
        let _guard = exclusively();
        let root = an_installation("mkdir");
        assert_eq!(call("mkdir", &["/data/saves"], 0o755), 0);
        assert!(root.join("data/saves").is_dir());
        assert_eq!(call("rmdir", &["/data/saves"], 0), 0);
        assert!(!root.join("data/saves").exists());
    }

    /// Nothing here can touch the title directory (D250).
    #[test]
    fn nothing_here_can_touch_the_title_directory() {
        let _guard = exclusively();
        let root = an_installation("readonly");
        assert_ne!(call("mkdir", &["/app0/newdir"], 0o755), 0);
        assert_ne!(call("unlink", &["/app0/eboot.bin"], 0), 0);
        assert_ne!(call("remove", &["/app0/eboot.bin"], 0), 0);
        assert!(
            root.join("app0/eboot.bin").is_file(),
            "the title survives every one of them"
        );
    }

    /// A path climbing out of its mount is refused before anything reaches the host.
    #[test]
    fn a_path_that_climbs_out_of_its_mount_is_refused() {
        let _guard = exclusively();
        an_installation("escape");
        assert_ne!(call("mkdir", &["/data/../escaped"], 0o755), 0);
        assert_ne!(call("unlink", &["/data/../../secret"], 0), 0);
    }

    /// `unlink` removes a file; a directory needs `rmdir`.
    #[test]
    fn a_file_can_be_removed_but_a_directory_needs_the_call_that_takes_one() {
        let _guard = exclusively();
        let root = an_installation("unlink");
        std::fs::write(root.join("data/save.bin"), b"x").expect("a save");
        std::fs::create_dir(root.join("data/folder")).expect("a folder");

        assert_ne!(
            call("unlink", &["/data/folder"], 0),
            0,
            "unlink refuses a directory"
        );
        assert_eq!(
            call("remove", &["/data/folder"], 0),
            0,
            "remove takes either"
        );
        assert_eq!(call("unlink", &["/data/save.bin"], 0), 0);
        assert!(!root.join("data/save.bin").exists());
    }

    /// Renaming moves a file, and both ends are checked.
    #[test]
    fn renaming_moves_a_file_and_both_ends_are_checked() {
        let _guard = exclusively();
        let root = an_installation("rename");
        std::fs::write(root.join("data/before"), b"contents").expect("a file");

        assert_ne!(
            call("rename", &["/data/before", "/app0/after"], 0),
            0,
            "a destination outside the writable mount is refused"
        );
        assert_eq!(call("rename", &["/data/before", "/data/after"], 0), 0);
        assert_eq!(
            std::fs::read(root.join("data/after")).expect("moved"),
            b"contents"
        );
    }

    /// `access` answers the rule this emulator enforces, not the host's permissions.
    #[test]
    fn access_reports_the_writable_mount_rather_than_the_hosts_permissions() {
        /// Existence only, which is what a zero mode asks about.
        const F_OK: u64 = 0;

        let _guard = exclusively();
        an_installation("access");
        assert_eq!(call("access", &["/app0/eboot.bin"], F_OK), 0, "it is there");
        assert_ne!(
            call("access", &["/app0/eboot.bin"], super::W_OK),
            0,
            "and it may not be written"
        );
        assert_ne!(call("access", &["/data/missing"], F_OK), 0);
    }

    /// A positioned read does not move the descriptor's own position.
    #[test]
    fn a_positioned_read_does_not_move_the_descriptors_own_position() {
        let _guard = exclusively();
        let root = an_installation("pread");
        std::fs::write(root.join("data/log"), b"0123456789").expect("a file");
        let fd = crate::descriptor::open("/data/log").expect("opens");

        let mut first = [0_u8; 4];
        assert_eq!(crate::descriptor::read(fd, &mut first), Some(4));
        assert_eq!(&first, b"0123");

        // A positioned read from the start, which leaves the position above alone.
        let mut middle = [0_u8; 3];
        assert_eq!(
            crate::descriptor::read_at(fd, &mut middle, 0),
            Some(3),
            "reads where it was told"
        );
        assert_eq!(&middle, b"012");

        let mut next = [0_u8; 3];
        assert_eq!(crate::descriptor::read(fd, &mut next), Some(3));
        assert_eq!(&next, b"456", "and the ordinary position carried on");
        assert!(crate::descriptor::close(fd));
    }

    /// A descriptor can be duplicated onto a number the guest chooses.
    #[test]
    fn a_descriptor_can_be_duplicated_onto_a_number_the_guest_chooses() {
        let _guard = exclusively();
        let root = an_installation("dup2");
        std::fs::write(root.join("data/log"), b"abcd").expect("a file");
        let fd = crate::descriptor::open("/data/log").expect("opens");

        let chosen = 40;
        assert!(crate::descriptor::duplicate_into(fd, chosen));
        let mut buffer = [0_u8; 4];
        assert_eq!(crate::descriptor::read(chosen, &mut buffer), Some(4));
        assert_eq!(&buffer, b"abcd");

        // The standard streams are refused: they are the host's, and the worker's protocol is
        // on the other side of them (D170).
        assert!(!crate::descriptor::duplicate_into(fd, 1));

        assert!(crate::descriptor::close(chosen));
        assert!(crate::descriptor::close(fd));
    }

    /// Duplicating a descriptor onto itself succeeds and closes nothing.
    #[test]
    fn duplicating_a_descriptor_onto_itself_succeeds_and_closes_nothing() {
        let _guard = exclusively();
        let root = an_installation("dupself");
        std::fs::write(root.join("data/log"), b"xy").expect("a file");
        let fd = crate::descriptor::open("/data/log").expect("opens");
        assert!(crate::descriptor::duplicate_into(fd, fd));
        let mut buffer = [0_u8; 2];
        assert_eq!(crate::descriptor::read(fd, &mut buffer), Some(2));
        assert!(crate::descriptor::close(fd));
    }

    /// A stream over a descriptor writes to that descriptor.
    #[test]
    fn a_stream_over_a_descriptor_writes_to_that_descriptor() {
        let _guard = exclusively();
        let root = an_installation("fdopen");
        std::fs::write(root.join("data/out.txt"), b"").expect("a file");
        let fd = crate::descriptor::create("/data/out.txt").expect("creates");

        let stream = call_raw("fdopen", [fd, 0, 0, 0, 0, 0]);
        assert_ne!(stream, 0, "a handle the guest dereferences");
        assert_eq!(
            call_raw("fileno", [stream, 0, 0, 0, 0, 0]),
            fd,
            "and it can say which descriptor it is"
        );

        assert_eq!(crate::descriptor::write(fd, b"220 ready"), Some(9));
        assert!(crate::open::close(stream), "closing the wrapper");
        assert!(
            crate::descriptor::exists(fd),
            "and the descriptor it wrapped is still open, because the stream never owned it"
        );
        assert!(crate::descriptor::close(fd));
        assert_eq!(
            std::fs::read(root.join("data/out.txt")).expect("read"),
            b"220 ready"
        );
    }

    /// A stream from `fopen` owns a file rather than a descriptor, so it has no number.
    #[test]
    fn a_stream_that_is_not_a_descriptor_has_no_descriptor_number() {
        assert_ne!(call_raw("fileno", [0xDEAD, 0, 0, 0, 0, 0]), 0);
    }

    /// A file goes into a socket, in chunks, and the count comes back.
    #[test]
    fn a_file_can_be_sent_straight_into_another_descriptor() {
        let _guard = exclusively();
        let root = an_installation("sendfile");
        std::fs::write(root.join("data/asset.bin"), b"abcdefghij").expect("a file");
        std::fs::write(root.join("data/sent.bin"), b"").expect("a destination");
        let from = crate::descriptor::open("/data/asset.bin").expect("opens");
        let to = crate::descriptor::create("/data/sent.bin").expect("creates");

        let mut moved = 0_u64;
        assert_eq!(
            call_raw(
                "sendfile",
                [from, to, 2, 5, 0, std::ptr::addr_of_mut!(moved) as u64]
            ),
            0
        );
        assert_eq!(moved, 5, "how many bytes went, where the caller asked");
        assert!(crate::descriptor::close(from));
        assert!(crate::descriptor::close(to));
        assert_eq!(
            std::fs::read(root.join("data/sent.bin")).expect("read"),
            b"cdefg",
            "from the offset it was given"
        );
    }

    /// A header structure this cannot read is refused rather than half-performed.
    #[test]
    fn a_sendfile_with_headers_is_refused() {
        let _guard = exclusively();
        an_installation("sendfilehdr");
        assert_ne!(call_raw("sendfile", [3, 4, 0, 0, 0x1000, 0]), 0);
    }

    /// A file can be truncated to a length.
    #[test]
    fn a_file_can_be_truncated_to_a_length() {
        let _guard = exclusively();
        let root = an_installation("truncate");
        std::fs::write(root.join("data/log"), b"0123456789").expect("a file");
        assert_eq!(call("truncate", &["/data/log"], 4), 0);
        assert_eq!(std::fs::read(root.join("data/log")).expect("read"), b"0123");
    }
}

#[cfg(test)]
mod scatter_gather {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    use super::tests::an_installation;
    use crate::exclusively;

    /// An `iovec` array this test owns, plus the buffers it points at.
    struct Vector {
        _buffers: Vec<Box<[u8]>>,
        entries: Box<[u64]>,
    }

    impl Vector {
        fn of(pieces: &[&[u8]]) -> Self {
            let mut buffers = Vec::new();
            let mut entries = Vec::new();
            for piece in pieces {
                let owned: Box<[u8]> = piece.to_vec().into_boxed_slice();
                entries.push(owned.as_ptr() as u64);
                entries.push(owned.len() as u64);
                buffers.push(owned);
            }
            Self {
                _buffers: buffers,
                entries: entries.into_boxed_slice(),
            }
        }

        fn at(&self) -> u64 {
            self.entries.as_ptr() as u64
        }
    }

    fn call(f: fn(&[u64; GUEST_ARG_REGISTERS]) -> u64, args: [u64; 4]) -> u64 {
        let mut regs = [0_u64; GUEST_ARG_REGISTERS];
        regs[..4].copy_from_slice(&args);
        f(&regs)
    }

    /// Several buffers become one transfer, and the answer is the total bytes moved.
    #[test]
    fn writev_gathers_every_buffer_and_answers_the_total() {
        let _guard = exclusively();
        let _root = an_installation("writev");
        let fd = crate::descriptor::create("/data/gathered").expect("creates");
        let vector = Vector::of(&[b"one", b"two", b"three"]);
        let written = call(super::writev, [fd, vector.at(), 3, 0]);
        assert_eq!(written, 11, "3 + 3 + 5 bytes, not 3 buffers");
        crate::descriptor::close(fd);

        let fd = crate::descriptor::open("/data/gathered").expect("opens");
        let mut back = [0_u8; 11];
        crate::descriptor::read(fd, &mut back);
        assert_eq!(&back, b"onetwothree", "and in order");
        crate::descriptor::close(fd);
    }

    /// A zero-length entry is skipped rather than treated as the end.
    #[test]
    fn an_empty_entry_does_not_end_the_transfer() {
        let _guard = exclusively();
        let _root = an_installation("empties");
        let fd = crate::descriptor::create("/data/empties").expect("creates");
        let vector = Vector::of(&[b"a", b"", b"b"]);
        assert_eq!(call(super::writev, [fd, vector.at(), 3, 0]), 2);
        crate::descriptor::close(fd);
    }

    /// `getpagesize` answers the guest page size, not the host's.
    #[test]
    fn getpagesize_answers_the_guest_page_size() {
        let regs = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::getpagesize(&regs), orbistoun_core::GUEST_PAGE_SIZE);
    }
}
