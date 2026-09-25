//! The file calls that change a directory, under their POSIX names.
//!
//! # Why these, and why now
//!
//! Measured against the open-toolchain payload set: `mkdir` is imported by seventeen of the
//! twenty-five, `unlink` by fifteen, `rmdir` and `rename` by eleven each. They are Stage 3 of
//! `docs/PAYLOADS.md` - the half of a file server that is not reading - and they are the part
//! `pros backup` would exercise, because copying a save directory out means making
//! directories on the way in.
//!
//! # None of this widens what a guest may touch
//!
//! Every one of these goes through the same two gates `create` already goes through:
//! [`mount::resolve`], which refuses a path under no mount and refuses one that climbs out of
//! its mount, and [`mount::is_writable`], which refuses anything outside the storage the
//! installation owns. **A guest cannot delete its own title** - `/app0` is the user's own
//! files and is not writable, and that separation is the whole decision (D250).
//!
//! So a refusal here has two quite different causes and one answer, deliberately: a path that
//! does not exist and a path the guest may not have both report failure, because that is what
//! the interface a guest thinks it is calling would tell it, and distinguishing them would
//! tell a guest about files outside its own storage.
//!
//! # There is no oracle problem
//!
//! POSIX says what each of these does and what it answers. The one judgement is what to do
//! about `errno`, and the answer is the same as everywhere else here: the return value is
//! what a caller branches on, and this does not invent a number to put beside it.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

use crate::mount;

/// What a call here answers when it worked.
const OK: u64 = 0;

/// What it answers when it did not.
///
/// Negative one, which is what every one of these documents. Deliberately not one of this
/// project's placeholder codes: a caller tests `< 0`, and a large positive placeholder would
/// read as success.
const FAILED: u64 = -1_i64 as u64;

/// The host path a guest path names, if the guest may write to it.
///
/// Both gates in one place, because they are one question - *may this call touch this path* -
/// and splitting them is how one caller comes to check only the first.
///
/// The top layer's copy, copied up from a lower layer first when that is the only one, so a write
/// never lands in the base tree or a staged title's library files (D722).
fn writable_host_path(address: u64) -> Option<std::path::PathBuf> {
    let guest = crate::read_guest_path(address)?;
    mount::resolve_for_write(&guest)
}

/// The host path a guest may create, remove or rename a name at: the top layer's, and only while no
/// lower layer also holds the name, because removing it there would need a whiteout (D722).
fn removable_host_path(address: u64) -> Option<std::path::PathBuf> {
    let guest = crate::read_guest_path(address)?;
    mount::resolve_for_removal(&guest)
}

/// Asking whether a path may be written, as a guest spells it.
///
/// **The one number here that came from somewhere else**, so it is named rather than written
/// inline, and `orbistoun-libc` holds a test comparing it against the harvested table - which
/// is where the header's own value lives. This crate cannot read that table itself, and a
/// constant nobody checks is exactly the kind that turns out to be a different platform's.
pub const W_OK: u64 = 0x2;

/// Turns a host result into what the guest is told.
fn answered(worked: bool) -> u64 {
    if worked { OK } else { FAILED }
}

/// `mkdir(path, mode)` - creates one directory.
///
/// **The mode is not applied**, and that is stated rather than hidden. The host this runs on
/// need not have POSIX permission bits at all, and writing a plausible permission somewhere
/// would be a fact about nothing. What the mode is *for* - keeping a directory private - is
/// not something a guest can check from inside this emulator.
///
/// One directory, not a path of them: `mkdir` creates the last component and fails if a
/// parent is missing, and creating the parents silently would turn a guest's own mistake into
/// a directory tree nobody asked for.
///
/// Reference: POSIX.1-2008 `mkdir(2)`.
fn mkdir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(host) = removable_host_path(args[0]) else {
        return FAILED;
    };
    answered(std::fs::create_dir(host).is_ok())
}

/// `rmdir(path)` - removes one empty directory.
///
/// Empty only, which is the interface: `rmdir` on a directory with anything in it fails, and
/// removing the contents instead would destroy a guest's data on a call that was documented
/// to refuse.
///
/// Reference: POSIX.1-2008 `rmdir(2)`.
fn rmdir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(host) = removable_host_path(args[0]) else {
        return FAILED;
    };
    answered(std::fs::remove_dir(host).is_ok())
}

/// `unlink(path)` - removes one file.
///
/// Refuses a directory rather than removing it. `unlink` on a directory is an error in the
/// interface, and the host call underneath would not agree about that on every platform - so
/// it is checked here rather than left to differ by operating system.
///
/// Reference: POSIX.1-2008 `unlink(2)`.
fn unlink(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(host) = removable_host_path(args[0]) else {
        return FAILED;
    };
    if host.is_dir() {
        return FAILED;
    }
    answered(std::fs::remove_file(host).is_ok())
}

/// `remove(path)` - the C spelling, which takes either.
///
/// **Not an alias for `unlink`.** C's `remove` removes a file *or* an empty directory, and
/// binding it to `unlink` would make a guest tidying up a directory fail on a call that was
/// documented to work.
///
/// Reference: ISO C `remove`; POSIX.1-2008 `remove(3)`.
fn remove(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(host) = removable_host_path(args[0]) else {
        return FAILED;
    };
    let worked = if host.is_dir() {
        std::fs::remove_dir(host).is_ok()
    } else {
        std::fs::remove_file(host).is_ok()
    };
    answered(worked)
}

/// `rename(from, to)` - moves a file within the guest's own storage.
///
/// **Both ends are checked.** A rename whose source is writable and whose destination is not
/// would be a way to write outside the writable mount using a call that only looks like it
/// reads - so the destination goes through the same two gates.
///
/// Reference: POSIX.1-2008 `rename(2)`.
fn rename(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (Some(from), Some(to)) = (removable_host_path(args[0]), removable_host_path(args[1]))
    else {
        return FAILED;
    };
    answered(std::fs::rename(from, to).is_ok())
}

/// `access(path, mode)` - whether a path is there, and whether it may be written.
///
/// # What the mode is compared against
///
/// The read and execute bits are answered by existence, because everything a guest can reach
/// through a mount it can read. The **write** bit is answered by the writable mount rather
/// than by the host's permissions: that is the rule this emulator actually enforces, so it is
/// the rule that should be reported. A guest told it may write to `/app0` and then refused
/// would take its error path somewhere less useful.
///
/// Reference: POSIX.1-2008 `access(2)`; `W_OK` from `sys/sys/unistd.h`.
fn access(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(guest) = crate::read_guest_path(args[0]) else {
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

/// `truncate(path, length)` - sets a file's size.
///
/// Reference: POSIX.1-2008 `truncate(2)`.
fn truncate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(host) = writable_host_path(args[0]) else {
        return FAILED;
    };
    let Ok(file) = std::fs::OpenOptions::new().write(true).open(host) else {
        return FAILED;
    };
    answered(file.set_len(args[1]).is_ok())
}

/// `ftruncate(fd, length)` - the same, by descriptor.
///
/// Reference: POSIX.1-2008 `ftruncate(2)`.
fn ftruncate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    answered(crate::descriptor::set_length(args[0], args[1]))
}

/// How wide one `struct iovec` is, and where its two fields sit.
///
/// `{ void *iov_base; size_t iov_len; }` - two machine words on this target. Reference:
/// POSIX.1-2008 `<sys/uio.h>`.
const IOVEC_BYTES: u64 = 16;

/// Reads one `iovec` from a guest's vector.
fn iovec(vector: u64, index: u64) -> Option<(u64, u64)> {
    if vector == 0 {
        return None;
    }
    let at = vector.checked_add(index.checked_mul(IOVEC_BYTES)?)?;
    let base = usize::try_from(at).ok()?;
    // SAFETY: a guest-supplied `iovec` array under the identity mapping (D014), indexed
    // within the count the guest itself passed - the same contract the real call has.
    let iov_base = unsafe { std::ptr::read_unaligned(base as *const u64) };
    // SAFETY: the second word of the same entry, eight bytes after the first.
    let iov_len = unsafe { std::ptr::read_unaligned((base + 8) as *const u64) };
    Some((iov_base, iov_len))
}

/// The shared body of the scatter/gather calls.
///
/// **Stops at the first short transfer**, as the specification requires: `readv` and `writev`
/// answer the number of bytes actually moved, and continuing past a partial buffer would
/// report a total the file never delivered.
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

/// `readv(fd, iov, iovcnt)` - a read scattered across several buffers.
///
/// Reference: POSIX.1-2008 `readv(2)`.
fn readv(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    gather(args, None, false)
}

/// `writev(fd, iov, iovcnt)` - a write gathered from several buffers.
///
/// Reference: POSIX.1-2008 `writev(2)`.
fn writev(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    gather(args, None, true)
}

/// `preadv(fd, iov, iovcnt, offset)` - [`readv`] at an explicit offset, leaving the file
/// position alone.
///
/// Reference: POSIX.1-2008 `preadv(2)`.
fn preadv(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    gather(args, Some(args[3]), false)
}

/// `pwritev(fd, iov, iovcnt, offset)` - [`writev`] at an explicit offset.
///
/// Reference: POSIX.1-2008 `pwritev(2)`.
fn pwritev(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    gather(args, Some(args[3]), true)
}

/// `creat(path, mode)` - create a file for writing, truncating an existing one.
///
/// Reference: POSIX.1-2008 `creat(2)`, which defines it as exactly
/// `open(path, O_WRONLY | O_CREAT | O_TRUNC, mode)`. The mode is not honoured for the same
/// reason `open`'s is not here: this layer has no permission model to apply it to, and
/// pretending otherwise would report an access control that does not exist.
fn creat(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(path) = crate::read_guest_path(args[0]) else {
        return FAILED;
    };
    crate::descriptor::create(&path).unwrap_or(FAILED)
}

/// `fsync(fd)` - flush a file's contents to the storage behind it.
///
/// Reference: POSIX.1-2008 `fsync(2)`. A **real** flush: the whole point of the call is that a
/// caller learns its bytes have landed, so answering success without asking the operating
/// system gives the assurance and none of the substance.
fn fsync(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    answered(crate::descriptor::sync(args[0]))
}

/// `fdatasync(fd)` - as [`fsync`], without promising to flush metadata.
///
/// Reference: POSIX.1-2008 `fdatasync(2)`. Flushing metadata as well is **more** than the call
/// promises and is therefore conforming; the reverse would not be.
fn fdatasync(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    answered(crate::descriptor::sync(args[0]))
}

/// `getpagesize()` - the size of a page, in bytes.
///
/// Reference: POSIX.1-2001 `getpagesize(2)`. Answered from `GUEST_PAGE_SIZE`, the size this
/// emulator's own address space is built on, rather than from the host's - a guest that rounds
/// an allocation to what this answers must get the number the mapper will actually use.
fn getpagesize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_core::GUEST_PAGE_SIZE
}

/// `madvise(addr, len, advice)` - tell the system how a range will be used.
///
/// Reference: POSIX.1-2008 `posix_madvise(2)`: *"the advice is not binding"* and an
/// implementation may ignore it. Ignoring it is therefore a conforming implementation rather
/// than a stub - which is why this one answers success without acting, and is the rare case
/// where doing nothing is the specification rather than a shortcut.
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
    // SAFETY: a guest-supplied buffer under the identity mapping (D014), with the length the
    // guest itself passed - the same contract the real call has.
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

/// `pread(fd, buffer, count, offset)` - a read at an offset, leaving the position alone.
///
/// **The position is the whole point.** `pread` exists so two threads can read one file at
/// once, and an implementation that seeks, reads and seeks back is not that - it is a race
/// with extra steps.
///
/// Reference: POSIX.1-2008 `pread(2)`.
fn pread(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(into) = guest_bytes_mut(args[1], args[2]) else {
        return FAILED;
    };
    crate::descriptor::read_at(args[0], into, args[3]).map_or(FAILED, |n| n as u64)
}

/// `pwrite(fd, buffer, count, offset)` - the same, writing.
///
/// Reference: POSIX.1-2008 `pwrite(2)`.
fn pwrite(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(bytes) = guest_bytes(args[1], args[2]) else {
        return FAILED;
    };
    crate::descriptor::write_at(args[0], bytes, args[3]).map_or(FAILED, |n| n as u64)
}

/// `dup2(from, to)` - makes one descriptor refer to what another refers to.
///
/// Answers the new descriptor, which is `to`, as the interface does - not zero, and a caller
/// that checks for a negative answer would be misled by either mistake.
///
/// Reference: POSIX.1-2008 `dup2(2)`.
fn dup2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if crate::descriptor::duplicate_into(args[0], args[1]) {
        args[1]
    } else {
        FAILED
    }
}

/// `chmod(path, mode)` - accepted, and applied to nothing.
///
/// # Why accepting is right here and refusing is right for the title directory
///
/// A file server sets a mode on a file it has just created, and **failing that stops the
/// transfer**. But the mode itself cannot be honoured: the host need not have POSIX
/// permission bits, and there is nothing a guest can check from inside this emulator that
/// would depend on them.
///
/// So the path is still checked - a `chmod` on the user's own title is refused exactly as a
/// write to it would be - and a mode on a file the guest owns succeeds without doing
/// anything. That keeps the one guarantee that matters and drops the one that cannot be kept.
///
/// Reference: POSIX.1-2008 `chmod(2)`.
fn chmod(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(host) = writable_host_path(args[0]) else {
        return FAILED;
    };
    // Existence still decides, because `chmod` on a path that is not there fails.
    answered(host.exists())
}

/// `fchmod(fd, mode)` - the same, by descriptor.
///
/// Reference: POSIX.1-2008 `fchmod(2)`.
fn fchmod(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    answered(crate::descriptor::exists(args[0]))
}

/// `mlock(address, length)` - accepted, and pins nothing.
///
/// A guest locks memory so it is not paged out. Nothing here pages anything out: guest memory
/// is host memory this process reserved and holds for the life of the run, so the guarantee
/// the call asks for is one this already keeps by construction.
///
/// Refusing would stop a payload that locks a buffer before using it, over a call whose
/// promise is already true.
///
/// Reference: POSIX.1-2008 `mlock(2)`.
fn mlock(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `munlock(address, length)` - the same, unpinning nothing.
///
/// Reference: POSIX.1-2008 `munlock(2)`.
fn munlock(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `fdopen(fd, mode)` - a stream over a descriptor that is already open.
///
/// **What an FTP server writes its replies through.** It accepts a connection, wraps the
/// descriptor, and uses `fprintf` from then on.
///
/// The mode is not honoured: the descriptor decides what it can do, and a stream that claimed
/// to be writable over a read-only file would fail at the first write instead of here.
///
/// Reference: POSIX.1-2008 `fdopen(3)`.
fn fdopen(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !crate::descriptor::exists(args[0]) {
        // Null rather than an error code: a caller reads this as a pointer (D125).
        return 0;
    }
    crate::open::wrap_descriptor(args[0]).unwrap_or(0)
}

/// `fileno(stream)` - the descriptor behind a stream.
///
/// Answers one only for a stream that **is** a descriptor. A stream from `fopen` owns a host
/// file rather than a descriptor, so it has no number to give - and inventing one would hand
/// a guest a descriptor that names something else entirely.
///
/// Reference: POSIX.1-2008 `fileno(3)`.
fn fileno(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    crate::open::wrapped_descriptor(args[0]).unwrap_or(FAILED)
}

/// `sendfile(fd, s, offset, nbytes, hdtr, sbytes, flags)` - a file straight into a socket.
///
/// # Why implementing it properly matters
///
/// This is how a file server sends a file. Refusing it does not make a server fall back to
/// `read` and `write` - it makes the transfer fail - and answering success without moving any
/// bytes is worse still: the client gets an empty file and no error.
///
/// The copy is done in bounded chunks rather than in one allocation the size of the file,
/// because a guest may hand this a file larger than this process should hold at once.
///
/// **The header and trailer are not honoured.** `hdtr` points at a vendor structure of
/// scatter-gather buffers whose layout is not derivable here; a guest passing one is passing
/// something this cannot read, so it is refused rather than half-performed. Every payload
/// measured passes null.
///
/// Reference: FreeBSD `sendfile(2)`.
fn sendfile(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// Bytes moved per round. Large enough to be a real copy, small enough to bound this.
    const CHUNK: usize = 64 * 1024;

    let (from, to, offset, count, headers) = (args[0], args[1], args[2], args[3], args[4]);
    if headers != 0 {
        return FAILED;
    }
    // Zero means "to the end of the file", which is the interface.
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
            // A short write is the socket refusing more, and reporting it is the interface:
            // the caller reads how much went and sends the rest itself.
            break;
        }
    }

    // How many bytes went, written where the caller asked for it. A caller that passes null
    // is not asking, which is allowed.
    if let Ok(destination) = usize::try_from(args[5])
        && destination != 0
    {
        // SAFETY: a guest-supplied `off_t *` under the identity mapping (D014), written
        // unaligned because nothing promises the guest aligned it.
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
/// **Declared in `libc`**, where FreeBSD puts them, and implemented here, where the mount
/// model that decides whether a guest may touch a path lives. Where a symbol is declared is a
/// claim about the target; where its code lives is a claim about this repository (D367).
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
        // Writability is set from the filesystem manifest rather than by mounting, so a
        // test that only mounts gets a `/data` nothing may write to (D251).
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

    #[test]
    fn a_directory_can_be_made_and_removed_under_the_writable_mount() {
        let _guard = exclusively();
        let root = an_installation("mkdir");
        assert_eq!(call("mkdir", &["/data/saves"], 0o755), 0);
        assert!(root.join("data/saves").is_dir());
        assert_eq!(call("rmdir", &["/data/saves"], 0), 0);
        assert!(!root.join("data/saves").exists());
    }

    /// **The separation D250 exists for.** A guest cannot delete the user's own title.
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

    #[test]
    fn a_positioned_read_does_not_move_the_descriptors_own_position() {
        let _guard = exclusively();
        let root = an_installation("pread");
        std::fs::write(root.join("data/log"), b"0123456789").expect("a file");
        let fd = crate::descriptor::open("/data/log").expect("opens");

        let mut first = [0_u8; 4];
        assert_eq!(crate::descriptor::read(fd, &mut first), Some(4));
        assert_eq!(&first, b"0123");

        // A positioned read from the start, which must not disturb the position above.
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

        // **The standard streams are refused.** They are the host's here, and the worker's
        // protocol lives on the other side of them (D170).
        assert!(!crate::descriptor::duplicate_into(fd, 1));

        assert!(crate::descriptor::close(chosen));
        assert!(crate::descriptor::close(fd));
    }

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

    /// **What an FTP server is built on**: a connection, wrapped, written through.
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

    /// **The whole point of the vector form**: several buffers become one transfer, and the
    /// answer is the total moved rather than the count of buffers.
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

    /// `getpagesize` answers **this emulator's** page size, not the host's - a guest rounding
    /// an allocation must get the number the mapper will actually use.
    #[test]
    fn getpagesize_answers_the_guest_page_size() {
        let regs = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::getpagesize(&regs), orbistoun_core::GUEST_PAGE_SIZE);
    }
}
