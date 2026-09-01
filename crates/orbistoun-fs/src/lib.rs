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
    crate::descriptor::clear();
    crate::fcntl::clear();
    crate::metadata::clear();
    crate::mount::clear();
    // The escape pipe's address is a static an escape test sets and never cleared. Left set, it turns
    // `write` on descriptor 4 into a no-op (the kernel R/W escape write end), so a later test whose
    // ordinary file descriptor happens to be 4 has its writes silently swallowed - which is exactly
    // what stranded the `sendfile` test in the suite, `moved` = 5 but the file empty. Cleared here.
    crate::escape::set_kernel_read_address(0);
    guard
}

use orbistoun_hle::guest_module;

guest_module! {
    "libkernel_fs" {
        "sceKernelOpen" => 3,
        "sceKernelClose" => 1,
        "sceKernelRead" => 3,
        "sceKernelWrite" => 3,
        "sceKernelLseek" => 3,
        "sceKernelStat" => 2,
        "sceKernelMkdir" => 2,
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
        ("sceKernelWrite", kernel_write),
        ("sceKernelLseek", kernel_lseek),
        ("sceKernelMkdir", kernel_mkdir),
        ("sceKernelDebugOutText", kernel_debug_out_text),
    ]
}

#[cfg(test)]
mod tests {
    use super::{GUEST_ARG_REGISTERS, kernel_debug_out_text, kernel_mkdir};

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

    /// **A directory under a writable mount is created; one outside it is refused with the
    /// console's error code, never a placeholder a caller reads as success.**
    ///
    /// This is the failure that mattered: the default stub answered `0x7fff_0001`, a small
    /// positive number, so obSCEne's file sink "made" a directory that was not there and read
    /// the sink back into a fault. The guard is that a refusal is `0x8002_00xx`, not that range.
    #[test]
    fn mkdir_creates_under_a_writable_mount_and_refuses_elsewhere_without_a_placeholder() {
        let _guard = super::exclusively();
        super::mount::clear();

        let root = std::env::temp_dir().join(format!("orbistoun-mkdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        super::mount::mount_data(root.clone());
        super::mount::allow_writes(super::mount::DATA_MOUNT);

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

        super::mount::clear();
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
