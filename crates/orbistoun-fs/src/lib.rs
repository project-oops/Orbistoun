//! Guest filesystem HLE - kernel file IO and the async streaming layer.
//!
//! Two layers with one job. The kernel calls are POSIX-shaped and map almost
//! directly onto host IO; above them sits the vendor asynchronous streaming layer,
//! which is what open-world titles actually use.
//!
//! # Path sandboxing is not optional
//!
//! Guest paths (`/app0/...`, `/savedata0/...`) are mount points, and every one
//! must resolve inside a directory orbistoun owns. A guest path that escapes to a
//! host path is a straightforward arbitrary-write vulnerability, so translation
//! goes through one function with one test suite rather than being open-coded per
//! call.
//!
//! # Status
//!
//! Declarations only. Arities are provisional.

pub mod amprindex;
pub mod descriptor;
pub mod device;
pub mod escape;
pub mod fcntl;
pub mod filesystem;
pub mod ifaddrs;
pub mod kqueue;
pub mod metadata;
pub mod mount;
pub mod open;
pub mod opened;
pub mod posix;
pub mod sandbox;
pub mod select;
pub mod socket;
pub mod wanted;

/// One lock for every test in this crate that touches the process-wide tables.
///
/// # Why it is here and not in each module
///
/// It **was** in each module: `descriptor` and `open` each declared a private
/// `exclusively()` over a private `Mutex`. Two locks, one piece of shared state - the mount
/// table and the descriptor table are process-wide, and both modules' tests call
/// `mount::clear()` before installing their own. So a descriptor test holding its lock and
/// an open test holding *its* lock ran at the same time and unmounted each other, and
/// `open("/app0/game.bin")` returned `None` in a test that had just created the file.
///
/// It failed about twice in five runs. That is the worst frequency a test can have: too
/// rare to be believed, too common to ignore, and the reflex is to re-run rather than to
/// look - so an intermittent red becomes a thing people scroll past, which is where a real
/// failure goes to hide (D241).
///
/// One lock, named for what it protects rather than for the module that happened to need
/// it first.
#[cfg(test)]
pub(crate) fn exclusively() -> std::sync::MutexGuard<'static, ()> {
    static GLOBAL_TABLES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = GLOBAL_TABLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Serialising is not enough: the tables are process-wide statics, so a descriptor, flag, open
    // directory or mount one test left behind shifts what the next test is handed - which is how a
    // `sendfile` that passed alone failed in the suite. Reset them under the lock this returns, so
    // every test that holds the guard starts from empty (D241 was the serialising half; this is the
    // resetting half it turned out also to need).
    descriptor::clear();
    fcntl::clear();
    metadata::clear();
    mount::clear();
    // The escape pipe's address is a static an escape test sets and never cleared. Left set, it turns
    // `write` on descriptor 4 into a no-op (the kernel R/W escape write end), so a later test whose
    // ordinary file descriptor happens to be 4 has its writes silently swallowed - which is exactly
    // what stranded the `sendfile` test in the suite, `moved` = 5 but the file empty. Cleared here.
    escape::set_kernel_read_address(0);
    guard
}

use orbistoun_hle::guest_module;

guest_module! {
    "libkernel_fs" {
        "sceKernelOpen" => 3,
        "sceKernelClose" => 1,
        "sceKernelRead" => 3,
        // Arity 4, the same shape as POSIX `pread`, which this crate implements and
        // `orbistoun-libc` declares at four - a descriptor, a destination and its length, and
        // the offset to read from (D526).
        "sceKernelPread" => 4,
        "sceKernelWrite" => 3,
        "sceKernelLseek" => 3,
        "sceKernelStat" => 2,
        "sceKernelFstat" => 2,
        "sceKernelMkdir" => 2,
        // POSIX, and imported under its bare name: a guest asking how much room a mount has.
        "statfs" => 2,
        "sceKernelDebugOutText" => 2,
    }
}

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// Reads a NUL-terminated path the guest passed.
pub(crate) fn read_guest_path(address: u64) -> Option<String> {
    /// Longer than any path observed, and short enough to stay near its own page.
    const MAX_PATH: usize = 1024;

    let at = usize::try_from(address).ok()?;
    if at == 0 {
        return None;
    }
    let mut bytes = Vec::new();
    for offset in 0..MAX_PATH {
        // SAFETY: a guest-supplied string under the identity mapping (D014), read one
        // byte at a time so the scan cannot straddle the end of a mapping by more than
        // it reads.
        let byte = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u8>(at + offset)) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    String::from_utf8(bytes).ok()
}

/// A guest buffer, as a slice.
///
/// # Safety
///
/// `address` and `len` must describe memory the guest owns. That is the same contract the
/// real call has - the guest declares both - and under the identity mapping an address it
/// has not mapped faults here exactly as it would have faulted in the guest.
unsafe fn guest_slice<'a>(address: u64, len: u64) -> Option<&'a mut [u8]> {
    let at = usize::try_from(address).ok()?;
    let len = usize::try_from(len).ok()?;
    if at == 0 || len == 0 {
        return None;
    }
    // SAFETY: the caller's contract, restated: guest-owned memory of the declared length.
    Some(unsafe {
        std::slice::from_raw_parts_mut(std::ptr::with_exposed_provenance_mut::<u8>(at), len)
    })
}

/// `sceKernelOpen(path, flags, mode)`.
///
/// **Write intent is honoured only under `/data`.** The flags used to be ignored entirely,
/// because the only writable place would have been the user's own title directory. With
/// storage the installation owns, the answer is where rather than whether: a write mode
/// under `/data` creates, and one under `/app0` still opens for reading so a guest that
/// asked for more than it needed is not stopped (D250).
fn kernel_open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// `O_WRONLY | O_RDWR | O_CREAT`, the bits that say a caller intends to write.
    const WRITE_INTENT: u64 = 0x1 | 0x2 | 0x200;

    // **A refused path answers the same way a refused open does.** This returned an
    // argument error, which is a small positive number, which a caller reads as a perfectly
    // good descriptor - so a null path "opened successfully" and the probe said so (D273).
    let Some(path) = read_guest_path(args[0]) else {
        // A null (or unreadable) path pointer is `EFAULT` on hardware, not a made-up descriptor:
        // obSCEne's `040-file/open-rejects-null` measured `0x8002000e` (D439).
        return u64::from(GuestError::vendor(orbistoun_core::errno::FAULT).as_raw());
    };
    let wants_write = args[1] & WRITE_INTENT != 0;
    let opened = if wants_write && mount::is_writable(&path) {
        descriptor::create(&path)
    } else {
        descriptor::open(&path)
    };
    // **A failed open must not look like a descriptor.** It used to answer a `GuestError`
    // placeholder, which deliberately avoids the high bit so it can never be mistaken for
    // an established firmware value - and that same choice makes it a small positive
    // integer, which is exactly what a valid descriptor is. The conformance probe opened a
    // file that was not there, got `0x7fff0002`, and handed it straight to `read` as a
    // descriptor; six commercial titles never surfaced it (D252).
    //
    // **Measured now.** obSCEne's `040-file/open-rejects-missing` answered `0x80020002` - the vendor
    // `ENOENT` (`NO_ENTRY`), returned directly rather than sign-extended, since `open` hands back an
    // error code where the byte-count calls hand back a negative count. This was `-1` while the errno
    // was "a question for the probe on hardware"; the probe answered (D252, D439). A failed open here
    // is a path that was not found, which is what `ENOENT` names.
    opened
        .unwrap_or_else(|| u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw()))
}

/// What a byte-count or offset call (`read`, `write`, `lseek`) answers on a bad descriptor.
///
/// # Measured, where it used to be `-1`
///
/// These calls return a signed count or offset, so a failure has to be negative for a caller testing
/// `< 0` to see it - which is why a `GuestError` placeholder, a small **positive** integer, was wrong
/// (D273). It was `-1` while the specific errno was "a question for the probe on hardware"; the probe
/// answered. obSCEne's `040-file/read-rejects-bad-fd`, `040-file/lseek-rejects-bad-fd` and
/// `000-boot/write-rejects-bad-fd` all returned `0xffffffff80020009` - the vendor `EBADF` (`0x80020009`)
/// as a signed 32-bit value sign-extended into the 64-bit return register (D439). Built from the errno
/// rather than spelled, so it stays tied to the one figure a guest checks against.
const FAILED_DESCRIPTOR: u64 =
    GuestError::vendor(orbistoun_core::errno::BAD_DESCRIPTOR).as_raw() as i32 as i64 as u64;

/// `sceKernelClose(fd)`.
fn kernel_close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if descriptor::close(args[0]) {
        OK
    } else {
        // EBADF, the vendor code obSCEne's `040-file/close-rejects-bad-fd` measured on hardware
        // (`0x80020009`) - not the `0x7fff…` placeholder, which a guest testing for a bad descriptor
        // would never recognise (D125).
        u64::from(GuestError::vendor(orbistoun_core::errno::BAD_DESCRIPTOR).as_raw())
    }
}

/// `sceKernelRead(fd, buffer, length)`.
fn kernel_read(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: a guest-supplied destination and its declared length, which is the contract
    // the real call has.
    let Some(into) = (unsafe { guest_slice(args[1], args[2]) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    descriptor::read(args[0], into).map_or_else(|| FAILED_DESCRIPTOR, |n| n as u64)
}

/// `sceKernelPread(fd, buffer, length, offset)` - the vendor-named positioned read.
///
/// # What is shared and what is not
///
/// The positioned read itself is `descriptor::read_at`, which POSIX `pread` already uses - it
/// reads at an offset **without moving the descriptor's position**, which is the whole point of
/// the call and is why it is not a seek, a read and a seek back (that is a race with extra
/// steps). Sharing it keeps one implementation of the thing that is hard.
///
/// What differs is failure, exactly as it does for `stat` (D525): POSIX answers `-1`, and a
/// `sceKernel*` file call answers [`FAILED_DESCRIPTOR`] - the vendor `EBADF` sign-extended,
/// which obSCEne measured on hardware for `read`, `lseek` and `write` against a bad descriptor
/// (D439). A caller testing for that number would not recognise `-1`.
///
/// The guest reached this by opening its first asset file: `sceKernelStat` answering (D525) let
/// it get as far as `sceKernelOpen`, and this is the next call it makes.
fn kernel_pread(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: as `kernel_read` - a guest-supplied destination and its declared length, which
    // is the contract the real call has.
    let Some(into) = (unsafe { guest_slice(args[1], args[2]) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    descriptor::read_at(args[0], into, args[3]).map_or_else(|| FAILED_DESCRIPTOR, |n| n as u64)
}

/// `sceKernelWrite(fd, buffer, length)`.
///
/// # The call that decides whether a probe can talk to us
///
/// A conformance probe reports by writing to standard output. One loader examined
/// implements this purely as a filesystem call - it requires a real opened descriptor and
/// refuses descriptor 1 - and **that single choice is why it cannot emit a report at all**
/// (D170).
///
/// So descriptors 1 and 2 are writable here, and they land on the host's *error* stream:
/// the worker's standard output carries its protocol as newline-delimited JSON, and guest
/// bytes interleaved into that would break the reader permanently.
///
/// Writes to a file are refused, because files are opened read-only.
fn kernel_write(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // A zero-length write touches no guest memory and writes nothing: POSIX and the platform
    // both answer 0 (obSCEne 000-boot/write-returns-count, confirmed on hardware). It is handled
    // before `guest_slice`, which rejects a zero length the way it rejects a null pointer - so
    // without this the call returned `InvalidArgument`, a non-zero value the probe reads as a
    // claim to have written bytes. The descriptor is still consulted, so an unwritable one
    // refuses rather than falsely answering 0.
    if args[2] == 0 {
        return descriptor::write(args[0], &[]).map_or_else(|| FAILED_DESCRIPTOR, |n| n as u64);
    }
    // SAFETY: as `kernel_read` - guest-supplied memory of a declared length. Written
    // through a shared reference only.
    let Some(bytes) = (unsafe { guest_slice(args[1], args[2]) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    descriptor::write(args[0], bytes).map_or_else(|| FAILED_DESCRIPTOR, |n| n as u64)
}

/// `sceKernelLseek(fd, offset, whence)`.
fn kernel_lseek(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(from) = open::From::from_whence(args[2]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    descriptor::seek(args[0], from, args[1] as i64).unwrap_or(FAILED_DESCRIPTOR)
}

/// `sceKernelMkdir(path, mode)`.
///
/// # Sandboxed like the console, and never a placeholder
///
/// The console gives a title writable, sandboxed device storage, and a `mkdir` under it
/// succeeds; a `mkdir` outside it is refused with an error code, not a crash. Left
/// unimplemented this answered [`GuestError::Unimplemented`] - a small positive number a caller
/// reads as a descriptor-shaped success - and that is exactly what broke obSCEne's file sink: it
/// "made" the directory, opened a report file that was therefore never really there, and read
/// the sink back into a fault (the D125/D273 shape, one layer up).
///
/// So a directory under a writable mount is created for real - it lands in the per-title overlay
/// like every other write (D250, D251) - and anywhere else answers the code the console answers,
/// `0x8002_00xx`, which a caller can test rather than mistake for a handle.
fn kernel_mkdir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(path) = read_guest_path(args[0]) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    if mount::is_writable(&path) {
        if let Some(host) = mount::resolve(&path) {
            return match std::fs::create_dir_all(&host) {
                Ok(()) => OK,
                // A real host failure - out of space, a name clash with a file - is denied
                // rather than dressed up as success.
                Err(_) => u64::from(GuestError::vendor(orbistoun_core::errno::DENIED).as_raw()),
            };
        }
    }
    // Read-only, or a mount that does not exist: the sandbox refuses it, cleanly.
    u64::from(GuestError::vendor(orbistoun_core::errno::DENIED).as_raw())
}

/// Offsets into `struct statfs`, from FreeBSD's `sys/mount.h`.
///
/// **Published rather than inferred, which is what makes writing them defensible.** The target
/// kernel is FreeBSD-derived (principle 1's first oracle), and these four have held across every
/// FreeBSD this could descend from. obSCEne reads two of them at exactly these offsets on real
/// hardware and gets a believable number back, which is a second, independent check that the
/// layout is the one in use (obSCEne D272).
mod statfs_at {
    /// `f_bsize`, the fragment size. `u64`.
    pub(super) const BSIZE: usize = 0x10;
    /// `f_blocks`, total data blocks. `u64`.
    pub(super) const BLOCKS: usize = 0x20;
    /// `f_bfree`, blocks free. `u64`.
    pub(super) const BFREE: usize = 0x28;
    /// `f_bavail`, blocks free to a non-superuser. `i64`.
    pub(super) const BAVAIL: usize = 0x30;
    /// How far this writes, and no further.
    ///
    /// **The structure is longer than this and the rest is deliberately untouched.** Its full
    /// size on the target is not measured, and zeroing out to FreeBSD's length would overrun a
    /// caller whose buffer is the size the *target's* header says. Writing only as far as the
    /// last field it has a real value for cannot overrun anything, and a field this leaves alone
    /// is one this could only have guessed.
    pub(super) const WRITTEN: usize = 0x38;
}

/// The block size `statfs` reports.
///
/// A unit for the counts rather than a measurement of anything: the host is asked for bytes, and
/// bytes have to be divided by something to become the blocks the interface answers in. 4096 is
/// the page size and the commonest fragment size; the product `f_bavail * f_bsize` - which is all
/// any caller can do with the pair - is exact whatever is chosen, because the division that
/// produced the count used this same number.
const STATFS_BLOCK: u64 = 4096;

/// `statfs(path, buf)` - how much room the mount holding `path` has.
///
/// # Why this is written from a host measurement and not a constant
///
/// A guest asks this to decide whether a save will fit. Answering with a plausible number is the
/// failure principle 3 exists to forbid, and it is not hypothetical here: answering the call
/// `0x0` without filling the buffer - which is all `ORBISTOUN_RETURN` can do - turns obSCEne's
/// honest `storage|unconfirmed|unknown` into a confident `storage|known|0M`. A wrong answer
/// arrived faster than the right one, and looked better.
///
/// So the numbers come from the volume actually backing the mount, and a path under no mount is
/// refused rather than given a figure.
///
/// # What it writes, and what it does not
///
/// The four capacity fields at [`statfs_at`], and nothing else. Everything past
/// [`statfs_at::WRITTEN`] - the file counts, the mount names, the filesystem type - is left as
/// the caller had it, because this knows none of them and the structure's real length on the
/// target has never been measured.
fn kernel_statfs(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(path) = read_guest_path(args[0]) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    let into = args[1];
    if into == 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::FAULT).as_raw());
    }
    // A path the sandbox does not have is not a filesystem with no room; it is not there.
    let Some(host) = mount::resolve_existing(&path) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw());
    };
    // The mount is there and the host would not say how big it is. Denied rather than zero:
    // "I cannot tell you" and "there is no room" are different answers and only one is true.
    let Some((available, total)) = host_capacity(&host) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::DENIED).as_raw());
    };

    let mut fields = [0_u8; statfs_at::WRITTEN];
    let put = |fields: &mut [u8; statfs_at::WRITTEN], at: usize, value: u64| {
        fields[at..at + 8].copy_from_slice(&value.to_le_bytes());
    };
    put(&mut fields, statfs_at::BSIZE, STATFS_BLOCK);
    put(&mut fields, statfs_at::BLOCKS, total / STATFS_BLOCK);
    put(&mut fields, statfs_at::BFREE, available / STATFS_BLOCK);
    put(&mut fields, statfs_at::BAVAIL, available / STATFS_BLOCK);

    let Ok(at) = usize::try_from(into) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::FAULT).as_raw());
    };
    // SAFETY: a guest-supplied destination under the identity mapping (D014), written for
    // exactly `WRITTEN` bytes - a prefix of a structure the caller sized - from a local array of
    // that length. The two cannot overlap: one is guest memory and the other is this stack.
    unsafe {
        std::ptr::copy_nonoverlapping(
            fields.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            statfs_at::WRITTEN,
        );
    }
    OK
}

/// Bytes available to this process, and bytes in total, on the volume holding `path`.
///
/// [`None`] when the host will not say - which is reported to the guest as an I/O failure rather
/// than as an empty disk, for the reason the whole function exists.
#[cfg(windows)]
fn host_capacity(path: &std::path::Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let (mut available, mut total, mut free) = (0_u64, 0_u64, 0_u64);
    // SAFETY: `wide` is NUL-terminated and outlives the call; the three out-parameters are live
    // locals. The call only reads the path and writes the three words.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &raw mut available,
            &raw mut total,
            &raw mut free,
        )
    };
    (ok != 0).then_some((available, total))
}

/// Away from Windows this is not wired, and says so rather than answering a number.
#[cfg(not(windows))]
fn host_capacity(_path: &std::path::Path) -> Option<(u64, u64)> {
    None
}

/// How much of a debug-log string to read before giving up on its terminator.
///
/// A report record is a line, not a path, so this is larger than [`read_guest_path`]'s cap - but
/// still bounded, because an unterminated string is a guest bug and the scan must not run off the
/// end of memory chasing a NUL that is not coming.
const MAX_DEBUG_TEXT: usize = 16 * 1024;

/// `sceKernelDebugOutText(channel, text)` - a NUL-terminated line to the system log.
///
/// # Why this is captured rather than stubbed
///
/// obSCEne writes **every** report record here, unconditionally, as a second destination beside
/// its file sink and its socket - its own note calls it "a second destination, not a candidate",
/// there precisely so a build whose other channels produce nothing still emits a full report
/// through the kernel log. A homebrew klog reader is how that is read on hardware. Forwarding it
/// to the same host stream the write path uses (D170) means orbistoun captures that report too,
/// and a guest whose only working channel is this one is no longer silent.
///
/// The channel argument is not modelled: both of the console's are the operator's log, so every
/// channel lands on the one stream. The call returns a status, not a byte count, so a plain `0`
/// is the whole of success.
fn kernel_debug_out_text(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::io::Write as _;

    let Ok(at) = usize::try_from(args[1]) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    if at == 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    let mut bytes = Vec::new();
    for offset in 0..MAX_DEBUG_TEXT {
        // SAFETY: a guest-supplied string under the identity mapping (D014), read one byte at a
        // time so the scan cannot straddle the end of a mapping by more than it reads.
        let byte = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u8>(at + offset)) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    // The host stream descriptors 1 and 2 land on - the worker's stderr, kept clear of the
    // stdout protocol it speaks (D170). Raw bytes, because a log line is not promised to be UTF-8.
    // Kept for the run report as well as forwarded: a guest that only reaches this channel is
    // saying the same thing an engine says through `printf`, and the report should not care
    // which of the two a title happened to use (D658).
    orbistoun_core::said::note(&bytes);
    let mut stderr = std::io::stderr();
    let _ = stderr.write_all(&bytes);
    let _ = stderr.flush();
    OK
}

/// Implementations this crate provides, by symbol name.
///
/// Names rather than hashes: the hash is derived from the name, so a table written in
/// hashes could not be read by a person or checked against the declarations above.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceKernelOpen", kernel_open),
        ("sceKernelClose", kernel_close),
        ("sceKernelRead", kernel_read),
        ("sceKernelPread", kernel_pread),
        ("sceKernelWrite", kernel_write),
        ("sceKernelLseek", kernel_lseek),
        ("sceKernelMkdir", kernel_mkdir),
        ("statfs", kernel_statfs),
        // The body is in `metadata`, beside the POSIX form it shares its success path with;
        // the name is declared here, so this is where it is offered (D525).
        ("sceKernelStat", metadata::kernel_stat),
        ("sceKernelFstat", metadata::kernel_fstat),
        ("sceKernelDebugOutText", kernel_debug_out_text),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        FAILED_DESCRIPTOR, GUEST_ARG_REGISTERS, kernel_debug_out_text, kernel_mkdir, kernel_open,
        kernel_pread, kernel_read, mount,
    };

    /// A NUL-terminated guest string, from a leaked host buffer under the identity mapping.
    fn guest_cstr(text: &str) -> u64 {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        Box::leak(bytes.into_boxed_slice()).as_ptr() as usize as u64
    }

    fn args_with_path(path: u64) -> [u64; GUEST_ARG_REGISTERS] {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = path;
        args
    }

    /// **`statfs` answers a real measurement, refuses what it cannot measure, and stops writing
    /// where its knowledge stops.**
    ///
    /// # Why each half is here
    ///
    /// The *positive* half is the whole reason the function exists: answering the call
    /// successfully without filling the buffer is one environment variable away, and it turns
    /// obSCEne's honest `storage|unconfirmed|unknown` into a confident `storage|known|0M`. So a
    /// non-zero product is the thing to assert, not a zero return.
    ///
    /// The *extent* half is the one that would go unnoticed. The structure is longer than the
    /// four fields this knows, its real length on the target has never been measured, and a
    /// write that ran past what it knows would either overrun a caller's buffer or publish zeroes
    /// as though they were file counts. The sentinel past [`statfs_at::WRITTEN`] has to survive.
    #[test]
    fn statfs_measures_what_it_can_reach_and_writes_no_further() {
        // The mount table is process-wide, and a test that installs one without this lock
        // unmounts whatever a concurrent test just mounted - the intermittent red D241 exists
        // to stop. The guard also resets the tables, so nothing needs clearing first.
        let _tables = super::exclusively();
        mount::mount("/statfstest", std::env::temp_dir());

        // A sentinel the call must not touch, past everything it claims to write.
        let mut buffer = [0xAB_u8; 256];
        let mut args = args_with_path(guest_cstr("/statfstest"));
        args[1] = buffer.as_mut_ptr() as usize as u64;
        assert_eq!(
            super::kernel_statfs(&args),
            super::OK,
            "a mount that exists"
        );

        let word =
            |at: usize| u64::from_le_bytes(buffer[at..at + 8].try_into().expect("eight bytes"));
        let bsize = word(super::statfs_at::BSIZE);
        let bavail = word(super::statfs_at::BAVAIL);
        assert_eq!(bsize, super::STATFS_BLOCK, "the unit the counts are in");
        assert!(
            bavail > 0 && bavail.checked_mul(bsize).is_some(),
            "a real volume has room and the product does not overflow: {bavail} x {bsize}"
        );
        assert!(
            word(super::statfs_at::BLOCKS) >= word(super::statfs_at::BFREE),
            "a volume cannot have more free blocks than blocks"
        );

        // **Measured from the last field, not from `WRITTEN`.** A first draft asserted against
        // `WRITTEN` itself, which moves the boundary and the assertion together - widening the
        // write to 0x80 still passed. The invariant is "nothing past the last field this has a
        // value for", so the bound is that field's end and the constant has to agree with it.
        let past_last_field = super::statfs_at::BAVAIL + 8;
        assert!(
            buffer[past_last_field..].iter().all(|b| *b == 0xAB),
            "everything past the last known field is left exactly as the caller had it"
        );

        // A path under no mount is not a full disk and not an empty one.
        let mut missing = args_with_path(guest_cstr("/nowhere/at/all"));
        missing[1] = buffer.as_mut_ptr() as usize as u64;
        assert_ne!(
            super::kernel_statfs(&missing),
            super::OK,
            "a path the sandbox does not have is refused, not given a figure"
        );

        // Nowhere to put the answer.
        let mut nowhere = args_with_path(guest_cstr("/statfstest"));
        nowhere[1] = 0;
        assert_ne!(super::kernel_statfs(&nowhere), super::OK, "a null buffer");
    }

    /// **A positioned read reads at the offset and leaves the position alone**, and a bad
    /// descriptor answers the vendor code rather than the POSIX one.
    ///
    /// # What this asserts
    ///
    /// The position is the point. A `pread` that seeks, reads and seeks back would pass a test
    /// that only checked the bytes - so this reads at an offset and then reads *sequentially*,
    /// which only lands where it does if the descriptor never moved.
    ///
    /// And it pins the failure at the vendor `EBADF` sign-extended, never `-1`. Registering the
    /// POSIX form under the vendor name is the mistake this shape invites, and it is the one the
    /// `stat` pair already made once (D525, D526).
    ///
    /// # What it cannot assert
    ///
    /// That the console reads short at the end of a file the same way. That is the host's
    /// `read_at`, unchanged from the POSIX form this shares it with, and no run has measured the
    /// console's behaviour there.
    #[test]
    fn a_positioned_read_does_not_move_the_descriptor() {
        let _guard = super::exclusively();
        let root = std::env::temp_dir().join("orbistoun-pread-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a root");
        std::fs::write(root.join("bytes.bin"), b"0123456789").expect("a file");
        mount::clear();
        mount::mount_data(root.clone());

        let path = guest_cstr("/data/bytes.bin");
        let mut open_args = [0_u64; GUEST_ARG_REGISTERS];
        open_args[0] = path;
        let fd = kernel_open(&open_args);
        assert!((fd as i64) >= 0, "the file opened: {fd:#x}");

        let mut buffer = [0_u8; 4];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = fd;
        args[1] = buffer.as_mut_ptr() as usize as u64;
        args[2] = 4;
        args[3] = 6;
        assert_eq!(kernel_pread(&args), 4, "four bytes read from offset six");
        assert_eq!(&buffer, b"6789", "and they are the bytes at that offset");

        // The position must still be zero, so an ordinary read starts at the beginning.
        let mut after = [0_u8; 4];
        let mut read_args = [0_u64; GUEST_ARG_REGISTERS];
        read_args[0] = fd;
        read_args[1] = after.as_mut_ptr() as usize as u64;
        read_args[2] = 4;
        assert_eq!(kernel_read(&read_args), 4);
        assert_eq!(
            &after, b"0123",
            concat!(
                "the positioned read left the descriptor where it was - a ",
                "seek/read/seek-back would pass the assertion above and fail this one"
            )
        );

        let mut bad = [0_u64; GUEST_ARG_REGISTERS];
        bad[0] = 0xDEAD_BEEF;
        bad[1] = buffer.as_mut_ptr() as usize as u64;
        bad[2] = 4;
        let refused = kernel_pread(&bad);
        assert_eq!(
            refused, FAILED_DESCRIPTOR,
            "a bad descriptor answers the vendor EBADF obSCEne measured, not the POSIX -1"
        );
        assert_ne!(refused, u64::MAX, "and specifically not -1");
        mount::clear();
    }

    /// **A directory under a writable mount is created; one outside it is refused with the
    /// console's error code, never a placeholder a caller reads as success.**
    ///
    /// This is the failure that mattered: the default stub answered `0x7fff_0001`, a small
    /// positive number, so obSCEne's file sink "made" a directory that was not there and read
    /// the sink back into a fault. The guard is that a refusal is `0x8002_00xx`, not that range.
    #[test]
    fn mkdir_creates_under_a_writable_mount_and_refuses_elsewhere_without_a_placeholder() {
        let _guard = super::exclusively();
        mount::clear();

        let root = std::env::temp_dir().join(format!("orbistoun-mkdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        mount::mount_data(root.clone());
        mount::allow_writes(mount::DATA_MOUNT);

        let made = kernel_mkdir(&args_with_path(guest_cstr("/data/obscene")));
        assert_eq!(made, 0, "a mkdir under /data succeeds");
        assert!(
            root.join("obscene").is_dir(),
            "and the host directory exists"
        );

        let refused = kernel_mkdir(&args_with_path(guest_cstr("/app0/nope")));
        assert_ne!(
            refused, 0,
            "a mkdir outside the writable sandbox is refused"
        );
        assert_eq!(
            refused & 0xffff_0000,
            0x8002_0000,
            "refused with the console's 0x8002_00xx, not a 0x7fff placeholder a caller misreads"
        );

        mount::clear();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **DebugOutText answers success for a real string and refuses a null pointer cleanly.**
    ///
    /// It cannot assert what reached the host log without capturing stderr, so it pins the two
    /// things that are its own: a valid line returns `0`, and a null text is `0x8002_00xx` rather
    /// than the placeholder that first sent a guest's whole report nowhere.
    #[test]
    fn debug_out_text_answers_success_and_refuses_a_null_pointer() {
        let mut ok = [0_u64; GUEST_ARG_REGISTERS];
        ok[1] = guest_cstr("OBS|res|000-boot/x|pass\n");
        assert_eq!(kernel_debug_out_text(&ok), 0, "a real log line is accepted");

        let null = [0_u64; GUEST_ARG_REGISTERS];
        let refused = kernel_debug_out_text(&null);
        assert_eq!(
            refused & 0xffff_0000,
            0x8002_0000,
            "a null text is refused with a vendor code, not a placeholder"
        );
    }
}
