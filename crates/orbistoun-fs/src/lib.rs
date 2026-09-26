//! Guest filesystem and socket HLE: kernel file IO and the vendor async streaming layer.
//!
//! The kernel calls are POSIX-shaped and map almost directly onto host IO; the vendor
//! asynchronous streaming layer sits above them. Guest paths (`/app0/...`, `/data/...`) are
//! mount points, and every one resolves inside a directory orbistoun owns: translation goes
//! through [`mount`] and its one test suite, never open-coded per call, since a path escaping
//! to the host would be an arbitrary host write.

pub mod amprindex;
pub mod descriptor;
pub mod device;
pub mod dirent;
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
/// The mount, descriptor and flag tables are process-wide and shared by every module's tests,
/// so one lock named for what it protects serialises them all; per-module locks would let two
/// tests unmount each other.
#[cfg(test)]
pub(crate) fn exclusively() -> std::sync::MutexGuard<'static, ()> {
    static GLOBAL_TABLES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = GLOBAL_TABLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Serialising is not enough: a descriptor, flag, open directory or mount one test left
    // behind shifts what the next is handed. Reset them under the lock, so every test holding
    // the guard starts from empty.
    descriptor::clear();
    fcntl::clear();
    metadata::clear();
    mount::clear();
    // The kernel pipe's address is a static a test sets. Left set, it turns `write` on
    // descriptor 4 into a no-op, swallowing a later test's writes to an ordinary descriptor 4.
    escape::set_kernel_read_address(0);
    guard
}

use orbistoun_hle::guest_module;

guest_module! {
    "libkernel_fs" {
        "sceKernelOpen" => 3,
        "sceKernelClose" => 1,
        "sceKernelRead" => 3,
        // Arity 4, the same shape as POSIX `pread`: a descriptor, a destination and its length,
        // and the offset to read from.
        "sceKernelPread" => 4,
        "sceKernelWrite" => 3,
        "sceKernelLseek" => 3,
        "sceKernelStat" => 2,
        "sceKernelFstat" => 2,
        "sceKernelMkdir" => 2,
        // POSIX, imported under its bare name: how much room a mount has.
        "statfs" => 2,
        // Reading a directory through its descriptor: the POSIX name with the current record
        // layout, and the vendor pair with the FreeBSD 11 one.
        "getdirentries" => 4,
        "sceKernelGetdirentries" => 4,
        "sceKernelGetdents" => 3,
        "sceKernelDebugOutText" => 2,
    }
}

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// A guest buffer, as a slice.
///
/// # Safety
///
/// `address` and `len` must describe memory the guest owns, the same contract the real call
/// has; under the identity mapping an unmapped address faults here as it would in the guest.
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
/// Write intent is honoured only under a writable mount such as `/data` (D250): a write mode
/// there creates, and one under `/app0` still opens for reading, so a guest that asked for
/// more than it needed is not stopped.
fn kernel_open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// `O_WRONLY | O_RDWR | O_CREAT`, the bits that say a caller intends to write.
    const WRITE_INTENT: u64 = 0x1 | 0x2 | 0x200;

    // A refused path answers as a refused open does, never as a small positive number a caller
    // reads as a descriptor (D273).
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(path) = (unsafe { orbistoun_mem::guest::read_path(args[0]) }) else {
        // A null or unreadable path pointer is `EFAULT` on hardware (`0x8002000e`, obSCEne's
        // `040-file/open-rejects-null`).
        return u64::from(GuestError::vendor(orbistoun_core::errno::FAULT).as_raw());
    };
    let wants_write = args[1] & WRITE_INTENT != 0;
    let opened = if wants_write && mount::is_writable(&path) {
        descriptor::create(&path)
    } else {
        descriptor::open(&path)
    };
    // A failed open must not look like a descriptor. A missing path answers the vendor
    // `ENOENT`, `0x80020002`, returned directly rather than sign-extended, as measured on
    // hardware by obSCEne's `040-file/open-rejects-missing`.
    opened
        .unwrap_or_else(|| u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw()))
}

/// What a byte-count or offset call (`read`, `write`, `lseek`) answers on a bad descriptor.
///
/// These calls return a signed count or offset, so a failure is negative for a caller testing
/// `< 0` (D273). On hardware, obSCEne's `040-file/read-rejects-bad-fd`,
/// `040-file/lseek-rejects-bad-fd` and `000-boot/write-rejects-bad-fd` all return
/// `0xffffffff80020009`: the vendor `EBADF` as a signed 32-bit value sign-extended into the
/// 64-bit return register. Built from the errno, not spelled.
const FAILED_DESCRIPTOR: u64 =
    GuestError::vendor(orbistoun_core::errno::BAD_DESCRIPTOR).as_raw() as i32 as i64 as u64;

/// `sceKernelClose(fd)`.
fn kernel_close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if descriptor::close(args[0]) {
        OK
    } else {
        // EBADF, the vendor code measured on hardware by obSCEne's
        // `040-file/close-rejects-bad-fd` (`0x80020009`), which a guest recognises (D125).
        u64::from(GuestError::vendor(orbistoun_core::errno::BAD_DESCRIPTOR).as_raw())
    }
}

/// `sceKernelRead(fd, buffer, length)`.
fn kernel_read(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: a guest-supplied destination and its declared length, the contract the real
    // call has.
    let Some(into) = (unsafe { guest_slice(args[1], args[2]) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    descriptor::read(args[0], into).map_or_else(|| FAILED_DESCRIPTOR, |n| n as u64)
}

/// `sceKernelPread(fd, buffer, length, offset)`: the vendor-named positioned read.
///
/// The read is `descriptor::read_at`, shared with POSIX `pread`: it reads at an offset
/// without moving the descriptor's position, which a seek, a read and a seek back cannot do
/// race-free. Only failure differs (D525): POSIX answers `-1`, and a `sceKernel*` file call
/// answers [`FAILED_DESCRIPTOR`], the vendor `EBADF` sign-extended.
fn kernel_pread(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: as `kernel_read`: a guest-supplied destination and its declared length, the
    // contract the real call has.
    let Some(into) = (unsafe { guest_slice(args[1], args[2]) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    descriptor::read_at(args[0], into, args[3]).map_or_else(|| FAILED_DESCRIPTOR, |n| n as u64)
}

/// `sceKernelWrite(fd, buffer, length)`.
///
/// A conformance probe reports by writing to standard output, so descriptors 1 and 2 are
/// writable and land on the host's error stream (D170): the worker's standard output carries
/// its newline-delimited JSON protocol. Writes to a file are refused, since files are opened
/// read-only.
fn kernel_write(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // A zero-length write touches no guest memory and answers 0, as POSIX and the platform do
    // (confirmed on hardware by obSCEne's `000-boot/write-returns-count`). Handled before
    // `guest_slice`, which rejects a zero length; the descriptor is still consulted, so an
    // unwritable one refuses.
    if args[2] == 0 {
        return descriptor::write(args[0], &[]).map_or_else(|| FAILED_DESCRIPTOR, |n| n as u64);
    }
    // SAFETY: as `kernel_read`: guest-supplied memory of a declared length, written through a
    // shared reference only.
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
/// A directory under a writable mount is created, in the per-title overlay like every other
/// write (D250). Anywhere else answers the platform's `0x8002_00xx` code, never a
/// placeholder a caller would read as success.
fn kernel_mkdir(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(path) = (unsafe { orbistoun_mem::guest::read_path(args[0]) }) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    if mount::is_writable(&path) {
        if let Some(host) = mount::resolve_for_create(&path) {
            return match std::fs::create_dir_all(&host) {
                Ok(()) => OK,
                // A real host failure, such as a name clash with a file, is denied.
                Err(_) => u64::from(GuestError::vendor(orbistoun_core::errno::DENIED).as_raw()),
            };
        }
    }
    // Read-only, or a mount that does not exist: refused.
    u64::from(GuestError::vendor(orbistoun_core::errno::DENIED).as_raw())
}

/// Offsets into `struct statfs`, from FreeBSD's `sys/mount.h`.
///
/// Published in FreeBSD's `sys/mount.h` and unchanged across every FreeBSD the target kernel
/// could descend from. obSCEne reads two of them at these offsets on hardware and gets
/// plausible values.
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
    /// The structure is longer, but its size on the target is unmeasured, so writing only to
    /// the last field with a real value cannot overrun a caller's buffer.
    pub(super) const WRITTEN: usize = 0x38;
}

/// The block size `statfs` reports.
///
/// A unit for the counts, not a measurement: host bytes are divided by it to give blocks.
/// 4096 is the page size and the commonest fragment size; `f_bavail * f_bsize` is exact
/// whatever is chosen.
const STATFS_BLOCK: u64 = 4096;

/// `statfs(path, buf)` - how much room the mount holding `path` has.
///
/// A guest asks this to decide whether a save will fit, so the numbers come from the volume
/// backing the mount, and a path under no mount is refused rather than given a figure. Only
/// the four capacity fields at [`statfs_at`] are written; everything past
/// [`statfs_at::WRITTEN`] is left as the caller had it.
fn kernel_statfs(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's path argument, a NUL-terminated string by the call's contract.
    let Some(path) = (unsafe { orbistoun_mem::guest::read_path(args[0]) }) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    let into = args[1];
    if into == 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::FAULT).as_raw());
    }
    // A path the sandbox does not have is not there, not a filesystem with no room.
    let Some(host) = mount::resolve_existing(&path) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw());
    };
    // The host would not say how big the mount is. Denied rather than zero: "cannot tell" and
    // "no room" are different answers.
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
    // SAFETY: a guest-supplied destination under the identity mapping, written for exactly
    // `WRITTEN` bytes (a prefix of a structure the caller sized) from a local array of that
    // length. Guest memory and this stack cannot overlap.
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
/// [`None`] when the host will not say, reported to the guest as an I/O failure rather than
/// as an empty disk.
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
/// A report record is a line, not a path, so this is larger than the guest path reader's cap,
/// and bounded so an unterminated string cannot run the scan off the end of memory.
const MAX_DEBUG_TEXT: usize = 16 * 1024;

/// `sceKernelDebugOutText(channel, text)`: a NUL-terminated line to the system log.
///
/// obSCEne writes every report record here as well as to its file sink and socket, so a
/// build whose other channels produce nothing still reports. Forwarded to the same host
/// stream as the write path (D170). The channel is not modelled: every channel lands on the
/// one stream. Success is a plain `0` status.
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
        // SAFETY: a guest-supplied string under the identity mapping, read one byte at a time
        // so the scan cannot pass the end of a mapping by more than it reads.
        let byte = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u8>(at + offset)) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    // Kept for the run report, which treats this channel like `printf` output (D658), and
    // forwarded to the host stream descriptors 1 and 2 land on. Raw bytes, since a
    // log line is not promised to be UTF-8.
    orbistoun_core::said::note(&bytes);
    // Guest output, not a log: the guest's own write, so it stays a direct write.
    let mut stderr = std::io::stderr();
    let _ = stderr.write_all(&bytes);
    let _ = stderr.flush();
    OK
}

/// Implementations this crate provides, by symbol name.
///
/// Names rather than hashes: the hash is derived from the name, and names can be checked
/// against the declarations above.
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
        // The body is in `metadata` beside the POSIX form it shares its success path with; the
        // name is declared here (D525).
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

    /// `statfs` answers a real measurement, refuses what it cannot measure, and writes no
    /// further than its last known field.
    ///
    /// A non-zero product is asserted, since answering without filling the buffer would also
    /// return zero. A sentinel past [`statfs_at::WRITTEN`] must survive.
    #[test]
    fn statfs_measures_what_it_can_reach_and_writes_no_further() {
        // The mount table is process-wide; the guard serialises and resets the tables.
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

        // Measured from the last field, not from `WRITTEN`, so widening the write cannot move
        // the bound along with it.
        let past_last_field = super::statfs_at::BAVAIL + 8;
        assert!(
            buffer[past_last_field..].iter().all(|b| *b == 0xAB),
            "everything past the last known field is left exactly as the caller had it"
        );

        // A path under no mount is neither a full disk nor an empty one.
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

    /// A positioned read leaves the position alone, and a bad descriptor answers the vendor
    /// code rather than `-1` (D525).
    ///
    /// It reads at an offset and then sequentially, which lands correctly only if the
    /// descriptor never moved. Short reads at the end of a file follow the host's `read_at`;
    /// the hardware's behaviour there is unmeasured.
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

    /// A directory under a writable mount is created; one elsewhere is refused with the
    /// platform's `0x8002_00xx` code, never a placeholder read as success.
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

    /// `sceKernelDebugOutText` returns `0` for a real string and a `0x8002_00xx` code for a
    /// null pointer.
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
