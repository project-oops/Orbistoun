//! File descriptors, and the standard streams.
//!
//! A descriptor is a small integer the guest compares against zero and never reads through,
//! so it is a small integer here, where a `FILE *` is an address. A guest writing to
//! descriptor 1 or 2 lands on the host's stderr, because the worker speaks newline-delimited
//! JSON over stdout and interleaved guest bytes would break the reader (D170).

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// Standard input, as a guest numbers it.
pub const STDIN: u64 = 0;
/// Standard output.
pub const STDOUT: u64 = 1;
/// Standard error.
pub const STDERR: u64 = 2;

/// Most descriptors a guest may hold at once.
///
/// Bounded, so a guest that leaks descriptors hits a refusal, and the search for a free
/// number has a stated end.
pub const MAX_DESCRIPTORS: u64 = 4096;

/// The first descriptor handed out for a real file.
///
/// Zero through two are the standard streams; a file on descriptor 1 would write program
/// output.
pub const FIRST_FILE: u64 = 3;

/// What a descriptor refers to.
///
/// One table for files and sockets, because a guest has one: `close`, `read` and `write`
/// take either without being told which (D371).
#[derive(Debug)]
enum Target {
    /// A file on the host.
    File(std::fs::File),
    /// A socket, at whatever stage of its life it has reached.
    Socket(crate::socket::Socket),
    /// An event queue, and what a guest has asked it to watch.
    ///
    /// In the descriptor table, since a guest closes a queue with `close`.
    Queue(crate::kqueue::Registrations),
    /// A device: a stream with no host file behind it (D389).
    Device(crate::device::Device),
}

/// Open descriptors above the standard streams.
fn table() -> &'static Mutex<BTreeMap<u64, Target>> {
    static TABLE: OnceLock<Mutex<BTreeMap<u64, Target>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Empties the descriptor table so one test's open files cannot leak into the next.
///
/// The table is a process-wide static; [`crate::exclusively`] resets it so descriptor numbers
/// one test left behind cannot shift the next test's. Dropping each `Target` closes its host
/// file.
#[cfg(test)]
pub(crate) fn clear() {
    if let Ok(mut table) = table().lock() {
        table.clear();
    }
}

/// Whether a descriptor is one of the standard streams.
pub const fn is_standard(fd: u64) -> bool {
    fd == STDIN || fd == STDOUT || fd == STDERR
}

/// Opens a guest path for reading, answering a descriptor.
///
/// Read-only through `/app0`, the title directory being run; see [`create`] for the writable
/// half.
pub fn open(guest_path: &str) -> Option<u64> {
    // Devices before the mount table: a device has no host path to resolve (D389).
    if let Some(device) = crate::device::named(guest_path) {
        crate::opened::note(guest_path);
        return insert(Target::Device(device));
    }
    let Some(host) = crate::mount::resolve_existing(guest_path) else {
        // A path nothing here holds, recorded (D387).
        crate::wanted::note(guest_path);
        return None;
    };
    let Ok(file) = open_readable(&host) else {
        crate::wanted::note(guest_path);
        return None;
    };
    crate::opened::note(guest_path);
    let is_directory = host.is_dir();
    let fd = insert_file(file)?;
    // A directory's listing, for a guest that reads it through the descriptor.
    if is_directory {
        crate::dirent::note_directory(fd, guest_path);
    }
    Some(fd)
}

/// Opens `host` for reading, whether it is a file or a directory.
///
/// A guest opens its own `/app0` mount to check it is there and gets a descriptor on the
/// hardware. `std::fs::File::open` opens a directory read-only on Unix but fails on Windows
/// without `FILE_FLAG_BACKUP_SEMANTICS`, so only the directory case on Windows needs the flag.
#[cfg(windows)]
fn open_readable(host: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_FLAG_BACKUP_SEMANTICS` from `winbase.h`: the documented Win32 flag that lets
    /// `CreateFile` return a handle to a directory. A host-API constant.
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    if host.is_dir() {
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    options.open(host)
}

/// The non-Windows half: `File::open` already opens a directory read-only.
#[cfg(not(windows))]
fn open_readable(host: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(host)
}

/// Opens a guest path for writing, creating it, answering a descriptor.
///
/// Refused outside a writable mount such as `/data`: storage the installation owns is a
/// guest's to write, and the title directory is not (D250).
pub fn create(guest_path: &str) -> Option<u64> {
    let host = crate::mount::resolve_for_create(guest_path)?;
    if let Some(parent) = host.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(host)
        .ok()?;
    crate::opened::note(guest_path);
    insert_file(file)
}

/// Puts an open file in the table at the lowest free descriptor.
fn insert_file(file: std::fs::File) -> Option<u64> {
    insert(Target::File(file))
}

/// Puts a socket in the table, answering the descriptor a guest will hold it by.
pub(crate) fn insert_socket(socket: crate::socket::Socket) -> Option<u64> {
    insert(Target::Socket(socket))
}

/// Puts an empty event queue in the table.
pub(crate) fn insert_queue() -> Option<u64> {
    insert(Target::Queue(crate::kqueue::Registrations::new()))
}

/// Does something with the registrations a descriptor holds, if it holds any.
///
/// Answers [`None`] when the descriptor is not a queue, which `kevent` reports as a failure.
/// The lock is held across the call, so the caller must not ask about readiness inside it:
/// that takes the same lock.
pub(crate) fn with_queue<T>(
    fd: u64,
    act: impl FnOnce(&mut crate::kqueue::Registrations) -> T,
) -> Option<T> {
    let mut table = table().lock().ok()?;
    match table.get_mut(&fd)? {
        Target::Queue(held) => Some(act(held)),
        Target::File(_) | Target::Socket(_) | Target::Device(_) => None,
    }
}

/// Puts anything in the table at the lowest free descriptor.
fn insert(target: Target) -> Option<u64> {
    let mut table = table().lock().ok()?;
    // Lowest free descriptor, as a caller expects; a leak shows as a number that climbs.
    let fd = (FIRST_FILE..FIRST_FILE + MAX_DESCRIPTORS).find(|n| !table.contains_key(n))?;
    table.insert(fd, target);
    Some(fd)
}

/// Does something with the socket a descriptor holds, if it holds one.
///
/// Answers [`None`] when the descriptor is not a socket, which a caller turns into the
/// documented failure. The lock is held across the call, so a two-step change (read the
/// bound address, replace with a listener) is atomic against another guest thread; a
/// blocking operation inside blocks the table, which `accept` documents.
pub(crate) fn with_socket<T>(
    fd: u64,
    act: impl FnOnce(&mut crate::socket::Socket) -> T,
) -> Option<T> {
    let mut table = table().lock().ok()?;
    match table.get_mut(&fd)? {
        Target::Socket(socket) => Some(act(socket)),
        Target::File(_) | Target::Queue(_) | Target::Device(_) => None,
    }
}

/// Reads into `into`, answering how many bytes arrived.
///
/// Reading from a standard stream answers zero rather than blocking the worker on the host's
/// own input.
pub fn read(fd: u64, into: &mut [u8]) -> Option<usize> {
    use std::io::Read as _;
    if is_standard(fd) {
        return Some(0);
    }
    let mut table = table().lock().ok()?;
    if let Some(target) = table.get_mut(&fd) {
        return match target {
            // Nothing is read from a queue: `kevent` takes its events, and `read` on one fails
            // on the platform too.
            Target::Queue(_) => None,
            Target::Device(device) => Some(device.read(into)),
            Target::File(file) => Some(file.read(into).unwrap_or(0)),
            // A socket reads as a file does, so `read` and `recv` are the same to everything
            // below.
            Target::Socket(socket) => match socket {
                crate::socket::Socket::Stream { stream, .. } => {
                    Some(stream.read(into).unwrap_or(0))
                }
                // Reading a listener is a guest's mistake, reported rather than answered with an
                // empty read that looks like a closed connection.
                _ => None,
            },
        };
    }
    // Descriptor 3 outside the table is the read end of the kernel pipe while it is set.
    if fd == 3 && crate::escape::get_kernel_read_address() != 0 {
        return Some(crate::escape::read_kernel_pipe(into));
    }
    None
}

/// Whether a socket call is allowed to wait for what it asked for.
///
/// `MSG_DONTWAIT` is a property of one call, not of the socket: honouring it makes the host
/// socket non-blocking for that call and then restores it. The two halves are one type so
/// the restore cannot be forgotten (D667).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// The socket's own mode decides, which is the ordinary case.
    AsTheSocketIs,
    /// The call carried `MSG_DONTWAIT`: answer now, whatever the socket's mode.
    Never,
}

/// Whether a call changed the socket's mode and therefore owes it a restore.
#[derive(Debug, Clone, Copy)]
#[must_use = "a forced mode has to be put back, or it outlives the call that forced it"]
pub struct Restore(bool);

impl Wait {
    /// Forces non-blocking for one call where the flag asks for it, saying what is owed back.
    fn forced_on(self, stream: &std::net::TcpStream, nonblocking: bool) -> Restore {
        // Already non-blocking, so the flag asks for nothing and nothing is owed.
        if self == Self::AsTheSocketIs || nonblocking {
            return Restore(false);
        }
        Restore(stream.set_nonblocking(true).is_ok())
    }
}

impl Restore {
    /// Puts the socket back the way the guest left it.
    fn put_back(self, stream: &std::net::TcpStream, nonblocking: bool) {
        if self.0 {
            let _ = stream.set_nonblocking(nonblocking);
        }
    }
}

/// Reads into `into` for a socket call, naming the errno when it could not.
///
/// [`read`] collapses the two conditions a socket call must name: a would-block becomes
/// `Some(0)`, which reads as the peer hanging up, and a read of a listener becomes `None`.
/// The hardware answers `0x8041_0123` (`EAGAIN`) and `0x8041_0139` (`ENOTCONN`), so the
/// vendor spelling needs the number (D667). Anything that is not a socket goes to [`read`].
pub fn socket_read(fd: u64, into: &mut [u8], wait: Wait) -> Result<usize, u32> {
    use std::io::Read as _;
    let mut table = table().lock().map_err(|_| crate::socket::UNNAMED)?;
    match table.get_mut(&fd) {
        Some(Target::Socket(crate::socket::Socket::Stream {
            stream,
            nonblocking,
        })) => {
            let restore = wait.forced_on(stream, *nonblocking);
            let answered = match stream.read(into) {
                Ok(n) => Ok(n),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    Err(orbistoun_core::errno::AGAIN)
                }
                Err(_) => Err(crate::socket::UNNAMED),
            };
            restore.put_back(stream, *nonblocking);
            answered
        }
        // A listener has no peer to read from: `ENOTCONN` on the hardware (obSCEne
        // `102-net/recv-would-block`).
        Some(Target::Socket(_)) => Err(orbistoun_core::errno::NOT_CONNECTED),
        _ => {
            drop(table);
            read(fd, into).ok_or(crate::socket::UNNAMED)
        }
    }
}

/// Writes `bytes` for a socket call, naming the errno when it could not.
///
/// The counterpart to [`socket_read`]: a send on a saturated socket is a would-block, which
/// the hardware answers `0x8041_0123` (obSCEne `102-net/accept-inherits`), where [`write()`]
/// reports only `None`.
pub fn socket_write(fd: u64, bytes: &[u8], wait: Wait) -> Result<usize, u32> {
    use std::io::Write as _;
    let mut table = table().lock().map_err(|_| crate::socket::UNNAMED)?;
    match table.get_mut(&fd) {
        Some(Target::Socket(crate::socket::Socket::Stream {
            stream,
            nonblocking,
        })) => {
            let restore = wait.forced_on(stream, *nonblocking);
            let answered = match stream.write(bytes) {
                Ok(n) => Ok(n),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    Err(orbistoun_core::errno::AGAIN)
                }
                Err(_) => Err(crate::socket::UNNAMED),
            };
            restore.put_back(stream, *nonblocking);
            answered
        }
        Some(Target::Socket(_)) => Err(orbistoun_core::errno::NOT_CONNECTED),
        _ => {
            drop(table);
            write(fd, bytes).ok_or(crate::socket::UNNAMED)
        }
    }
}

/// Writes `bytes`, answering how many were taken.
///
/// Standard output and standard error both go to the host's error stream, since its output
/// stream carries the worker's protocol. Anything else is refused unless opened through
/// `create`.
pub fn write(fd: u64, bytes: &[u8]) -> Option<usize> {
    use std::io::Write as _;
    if fd == STDOUT || fd == STDERR {
        // Kept for the run report beside the other output channels (D658).
        orbistoun_core::said::note(bytes);
        // Guest output, not a log: the guest's own write, so it stays a direct write.
        let mut stderr = std::io::stderr();
        let _ = stderr.write_all(bytes);
        // Flushed per write, so a probe's output survives the guest faulting immediately
        // afterwards.
        let _ = stderr.flush();
        return Some(bytes.len());
    }
    // A file is writable only if it was opened through `create`; anything opened for reading
    // refuses, and the error is the guest's answer.
    let mut table = table().lock().ok()?;
    match table.get_mut(&fd) {
        // A device that refuses writes says so rather than discarding them.
        Some(Target::Device(device)) => device.writable().then_some(bytes.len()),
        Some(Target::File(file)) => file.write(bytes).ok(),
        Some(Target::Socket(crate::socket::Socket::Stream { stream, .. })) => {
            stream.write(bytes).ok()
        }
        // Not an open descriptor. Descriptor 4 is the kernel pipe's write end while its
        // address is set, and its writes are swallowed as the pipe would. Checked after the
        // table, so a real file at descriptor 4 is still written.
        None if fd == 4 && crate::escape::get_kernel_read_address() != 0 => Some(bytes.len()),
        // A queue, a datagram socket, or a descriptor that names nothing: not writable here.
        _ => None,
    }
}

/// Moves the read position, answering where it ended up.
pub fn seek(fd: u64, from: crate::open::From, offset: i64) -> Option<u64> {
    use std::io::Seek as _;
    if is_standard(fd) {
        return None;
    }
    let mut table = table().lock().ok()?;
    // Seeking a socket is meaningless and is refused.
    let Target::File(file) = table.get_mut(&fd)? else {
        return None;
    };
    let to = match from {
        crate::open::From::Start => std::io::SeekFrom::Start(offset.max(0) as u64),
        crate::open::From::Current => std::io::SeekFrom::Current(offset),
        crate::open::From::End => std::io::SeekFrom::End(offset),
    };
    Some(file.seek(to).unwrap_or(0))
}

/// Flushes an open file's contents to the storage behind it. Answers whether it could.
///
/// A real flush: a caller relies on its bytes having landed. A standard stream has nothing to
/// flush and answers success, since its bytes have already left this process.
pub fn sync(fd: u64) -> bool {
    if is_standard(fd) {
        return true;
    }
    let Ok(table) = table().lock() else {
        return false;
    };
    let Some(Target::File(file)) = table.get(&fd) else {
        return false;
    };
    file.sync_all().is_ok()
}

/// Sets an open file's length. Answers whether it could.
///
/// A standard stream has no length, so it is refused, as is a descriptor nobody opened.
pub fn set_length(fd: u64, length: u64) -> bool {
    if is_standard(fd) {
        return false;
    }
    let Ok(mut table) = table().lock() else {
        return false;
    };
    let Some(Target::File(file)) = table.get_mut(&fd) else {
        return false;
    };
    file.set_len(length).is_ok()
}

/// Reads at an offset without moving the descriptor's own position.
///
/// `pread` lets two threads read one file without agreeing about the position. Windows'
/// positioned read updates the file pointer, so on that host the position is put back
/// afterwards. That is a seek-read-seek, but every call happens while the descriptor table is
/// locked, so no other guest call can observe the position in between.
pub fn read_at(fd: u64, into: &mut [u8], offset: u64) -> Option<usize> {
    if is_standard(fd) {
        // A stream has no positions, and answering zero would look like an end of file.
        return None;
    }
    let mut table = table().lock().ok()?;
    let Target::File(file) = table.get_mut(&fd)? else {
        return None;
    };
    positioned_read(file, into, offset)
}

/// Writes at an offset without moving the descriptor's own position.
pub fn write_at(fd: u64, bytes: &[u8], offset: u64) -> Option<usize> {
    if is_standard(fd) {
        return None;
    }
    let mut table = table().lock().ok()?;
    let Target::File(file) = table.get_mut(&fd)? else {
        return None;
    };
    positioned_write(file, bytes, offset)
}

#[cfg(unix)]
fn positioned_read(file: &mut std::fs::File, into: &mut [u8], offset: u64) -> Option<usize> {
    use std::os::unix::fs::FileExt as _;
    file.read_at(into, offset).ok()
}

#[cfg(windows)]
fn positioned_read(file: &mut std::fs::File, into: &mut [u8], offset: u64) -> Option<usize> {
    use std::io::Seek as _;
    use std::os::windows::fs::FileExt as _;

    let was = file.stream_position().ok()?;
    let read = file.seek_read(into, offset).ok();
    // Put it back whether the read worked or not.
    let _ = file.seek(std::io::SeekFrom::Start(was));
    read
}

#[cfg(unix)]
fn positioned_write(file: &mut std::fs::File, bytes: &[u8], offset: u64) -> Option<usize> {
    use std::os::unix::fs::FileExt as _;
    file.write_at(bytes, offset).ok()
}

#[cfg(windows)]
fn positioned_write(file: &mut std::fs::File, bytes: &[u8], offset: u64) -> Option<usize> {
    use std::io::Seek as _;
    use std::os::windows::fs::FileExt as _;

    let was = file.stream_position().ok()?;
    let written = file.seek_write(bytes, offset).ok();
    let _ = file.seek(std::io::SeekFrom::Start(was));
    written
}

/// Makes `to` refer to whatever `from` refers to, closing whatever `to` was.
///
/// On the platform, two descriptors from `dup2` share one file position. Here each holds its
/// own handle and position; a program notices only if it reads through both. The common use,
/// putting a file on descriptor 0, 1 or 2, does not depend on the shared position.
pub fn duplicate_into(from: u64, to: u64) -> bool {
    if from == to {
        // Duplicating a descriptor onto itself succeeds and does nothing, not even the close,
        // as documented.
        return exists(from);
    }
    let Ok(mut table) = table().lock() else {
        return false;
    };
    let copy = match table.get(&from) {
        Some(Target::File(file)) => file.try_clone().ok().map(Target::File),
        Some(Target::Socket(socket)) => duplicate_socket(socket),
        // A queue is not duplicated: two descriptors onto one registration set is a sharing
        // model nothing here has. A device is not duplicated: two descriptors draining one log
        // would each get half of it.
        Some(Target::Queue(_) | Target::Device(_)) | None => None,
    };
    let Some(copy) = copy else {
        return false;
    };
    if is_standard(to) {
        // Refused: the standard streams are the host's, and the worker's protocol is on the
        // other side of them (D170).
        return false;
    }
    table.insert(to, copy);
    true
}

/// A second handle onto the same socket, where the kind allows one.
fn duplicate_socket(socket: &crate::socket::Socket) -> Option<Target> {
    let copy = match socket {
        crate::socket::Socket::Stream {
            stream,
            nonblocking,
        } => crate::socket::Socket::Stream {
            stream: stream.try_clone().ok()?,
            nonblocking: *nonblocking,
        },
        crate::socket::Socket::Listener {
            listener,
            nonblocking,
            ..
        } => crate::socket::Socket::Listener {
            listener: listener.try_clone().ok()?,
            // A connection `select` left on the original stays there: only one descriptor can
            // be given it.
            pending: None,
            // The mode travels with the copy: `accept` reads this rather than the host.
            nonblocking: *nonblocking,
        },
        // Nothing to duplicate yet; a second pending socket would later bind the same address.
        crate::socket::Socket::Pending { .. } => return None,
    };
    Some(Target::Socket(copy))
}

/// What the host says about the file behind a descriptor.
///
/// Answers nothing for a socket or a standard stream: neither has a size or a modification
/// time.
pub fn facts(fd: u64) -> Option<std::fs::Metadata> {
    if is_standard(fd) {
        return None;
    }
    let table = table().lock().ok()?;
    match table.get(&fd)? {
        Target::File(file) => file.metadata().ok(),
        Target::Socket(_) | Target::Queue(_) | Target::Device(_) => None,
    }
}

/// Whether a descriptor names anything.
pub fn exists(fd: u64) -> bool {
    is_standard(fd) || table().lock().is_ok_and(|t| t.contains_key(&fd))
}

/// Whether a read on this descriptor would return without waiting.
///
/// Asking must not take (D373): whether a listener has a connection is found by accepting
/// one, which is kept on the listener for the guest's own `accept`. A stream is asked by
/// peeking, which does not advance. A file is always ready, since a read from one does not
/// wait.
pub fn readable(fd: u64) -> bool {
    let Ok(mut table) = table().lock() else {
        return false;
    };
    match table.get_mut(&fd) {
        Some(Target::File(_)) => true,
        Some(Target::Socket(socket)) => match socket {
            crate::socket::Socket::Listener {
                listener, pending, ..
            } => {
                if pending.is_some() {
                    return true;
                }
                // Non-blocking for the question, and left that way: `accept` sets it back before
                // it waits.
                if listener.set_nonblocking(true).is_err() {
                    return false;
                }
                match listener.accept() {
                    Ok(ready) => {
                        *pending = Some(ready);
                        true
                    }
                    Err(_) => false,
                }
            }
            crate::socket::Socket::Stream {
                stream,
                nonblocking,
            } => {
                if stream.set_nonblocking(true).is_err() {
                    return false;
                }
                let mut byte = [0_u8; 1];
                // A peek, so the byte stays for the guest. Zero bytes is end-of-file, which is
                // also ready.
                let ready = match stream.peek(&mut byte) {
                    Ok(_) => true,
                    Err(e) => e.kind() != std::io::ErrorKind::WouldBlock,
                };
                // Back to the mode the guest set, not to blocking (D667).
                let _ = stream.set_nonblocking(*nonblocking);
                ready
            }
            // A socket with nothing behind it yet cannot be read from.
            crate::socket::Socket::Pending { .. } => false,
        },
        // A device is ready when it has something waiting.
        Some(Target::Device(device)) => device.readable(),
        // A queue is never reported ready: nothing here nests one kqueue inside another, and
        // saying yes would wake a guest for nothing.
        Some(Target::Queue(_)) | None => false,
    }
}

/// Whether a write on this descriptor would return without waiting.
///
/// Yes for anything connected: writes go straight to the host without buffering, so a write
/// does not block.
pub fn writable(fd: u64) -> bool {
    let Ok(table) = table().lock() else {
        return false;
    };
    match table.get(&fd) {
        Some(Target::File(_)) => true,
        Some(Target::Socket(socket)) => {
            matches!(socket, crate::socket::Socket::Stream { .. })
        }
        Some(Target::Device(device)) => device.writable(),
        // Nothing is written to a queue.
        Some(Target::Queue(_)) | None => false,
    }
}

/// Closes a descriptor. Answers whether there was one.
///
/// Closing a standard stream answers success and does nothing: closing the host's error
/// stream would take the fault reporter with it.
pub fn close(fd: u64) -> bool {
    // Flags set on this number are forgotten with it, so a reused number does not inherit
    // them.
    crate::fcntl::forget(fd);
    crate::dirent::forget(fd);
    if is_standard(fd) {
        return true;
    }
    table().lock().is_ok_and(|mut t| t.remove(&fd).is_some())
}

/// Puts a descriptor into or out of non-blocking mode, answering whether it could.
///
/// Only a socket has the distinction. A file read here does not wait and a standard stream
/// answers immediately, so setting it on one is accepted and changes nothing.
pub fn set_nonblocking(fd: u64, wanted: bool) -> bool {
    if is_standard(fd) {
        return true;
    }
    let Ok(mut table) = table().lock() else {
        return false;
    };
    match table.get_mut(&fd) {
        Some(Target::Socket(socket)) => match socket {
            crate::socket::Socket::Listener {
                listener,
                nonblocking,
                ..
            } => {
                // Remembered as well as applied: `accept` needs it, and the host listener has a
                // setter with no getter.
                *nonblocking = wanted;
                listener.set_nonblocking(wanted).is_ok()
            }
            crate::socket::Socket::Stream {
                stream,
                nonblocking,
            } => {
                // Remembered as well as applied, so a `MSG_DONTWAIT` call can put it back.
                *nonblocking = wanted;
                stream.set_nonblocking(wanted).is_ok()
            }
            // Nothing exists behind it yet, so the flag is kept until `listen` or `connect`
            // makes something to put it on (D667).
            crate::socket::Socket::Pending { nonblocking, .. } => {
                *nonblocking = wanted;
                true
            }
        },
        // A device answers immediately or not at all, so the setting is accepted and changes
        // nothing.
        Some(Target::File(_) | Target::Queue(_) | Target::Device(_)) => true,
        None => false,
    }
}

/// Copies a descriptor to the lowest free number at or above `floor`.
///
/// What `F_DUPFD` documents: the one place a descriptor number is not simply the lowest free
/// one.
pub fn duplicate_above(from: u64, floor: u64) -> Option<u64> {
    let copy = {
        let table = table().lock().ok()?;
        match table.get(&from)? {
            Target::File(file) => file.try_clone().ok().map(Target::File),
            Target::Socket(socket) => duplicate_socket(socket),
            // As with `dup2`: no shared registration sets, and no log drained by two.
            Target::Queue(_) | Target::Device(_) => None,
        }?
    };
    let mut table = table().lock().ok()?;
    let start = floor.max(FIRST_FILE);
    let fd = (start..FIRST_FILE + MAX_DESCRIPTORS).find(|n| !table.contains_key(n))?;
    table.insert(fd, copy);
    Some(fd)
}

#[cfg(test)]
mod tests {
    use super::{FIRST_FILE, STDERR, STDOUT, close, is_standard, open, read, write};

    /// Serialises the tests that touch the descriptor table.
    ///
    /// Descriptors and mounts are process-global and the harness runs tests in parallel.
    use crate::exclusively;

    fn a_title_with(name: &str, contents: &[u8]) {
        let root = std::env::temp_dir().join(format!("orbistoun-fd-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        std::fs::write(root.join("game.bin"), contents).expect("file");
        crate::mount::clear();
        crate::mount::mount(crate::mount::APP_MOUNT, root);
    }

    /// A guest can write to standard output (D170).
    #[test]
    fn a_guest_can_write_to_standard_output() {
        assert_eq!(write(STDOUT, b"hello"), Some(5));
        assert_eq!(write(STDERR, b"there"), Some(5));
    }

    /// A file never gets a standard stream's descriptor.
    #[test]
    fn a_file_never_gets_a_standard_descriptor() {
        let _guard = exclusively();
        a_title_with("first", b"x");
        let fd = open("/app0/game.bin").expect("opens");
        assert!(fd >= FIRST_FILE);
        assert!(!is_standard(fd));
        assert!(close(fd));
    }

    /// A wrong-case path is not found, as on the case-sensitive platform filesystem.
    #[test]
    fn a_wrong_case_path_is_not_found_because_the_console_is_case_sensitive() {
        let _guard = exclusively();
        a_title_with("case", b"x"); // creates game.bin
        // `GAME.BIN` is a different name from `game.bin`. A Windows host's `File::open` is
        // case-insensitive; on Unix this already holds.
        assert!(
            open("/app0/game.bin").is_some(),
            "the exact-case name opens"
        );
        assert_eq!(
            open("/app0/GAME.BIN"),
            None,
            "a wrong-case name does not exist, as on the console"
        );
    }

    /// A directory opens to a descriptor, not `ENOENT`.
    #[test]
    fn a_directory_opens_to_a_descriptor_not_enoent() {
        let _guard = exclusively();
        a_title_with("dir-open", b"x");
        // A guest opens its own mount root `/app0/` to check it is there, and the hardware
        // answers a descriptor. On Windows `File::open` needs FILE_FLAG_BACKUP_SEMANTICS for a
        // directory.
        let fd = open("/app0/").expect("a directory opens to a descriptor, not ENOENT");
        assert!(
            fd >= FIRST_FILE,
            "a real descriptor, above the standard streams"
        );
        assert!(!is_standard(fd));
        assert!(close(fd));
    }

    /// Writing to a file opened for reading is refused.
    #[test]
    fn writing_to_a_file_is_refused_rather_than_performed() {
        let _guard = exclusively();
        a_title_with("readonly", b"x");
        let fd = open("/app0/game.bin").expect("opens");
        assert_eq!(write(fd, b"nope"), None);
        close(fd);
    }

    /// Reading a standard stream answers zero rather than blocking.
    #[test]
    fn reading_a_standard_stream_answers_nothing_rather_than_blocking() {
        let mut buf = [0_u8; 4];
        assert_eq!(read(super::STDIN, &mut buf), Some(0));
    }

    /// Closing a standard stream succeeds without closing it.
    #[test]
    fn closing_a_standard_stream_succeeds_without_closing_it() {
        assert!(close(STDOUT));
        assert_eq!(write(STDOUT, b"still here"), Some(10));
    }

    /// Descriptors are reused lowest first.
    #[test]
    fn descriptors_are_reused_lowest_first() {
        let _guard = exclusively();
        a_title_with("reuse", b"x");
        let first = open("/app0/game.bin").expect("opens");
        let second = open("/app0/game.bin").expect("opens");
        assert_eq!(second, first + 1);
        assert!(close(first));
        let third = open("/app0/game.bin").expect("opens");
        assert_eq!(third, first, "the freed descriptor comes back");
        close(second);
        close(third);
    }

    /// Running out of descriptors is a refusal, not an endless search.
    #[test]
    fn running_out_of_descriptors_is_refused_rather_than_searched_forever() {
        let _guard = exclusively();
        a_title_with("exhaust", b"x");
        let mut held = Vec::new();
        while let Some(fd) = open("/app0/game.bin") {
            held.push(fd);
            if held.len() > super::MAX_DESCRIPTORS as usize {
                break;
            }
        }
        // Which bound bites is not the point: the host's open-file limit may come first, and
        // it is process-wide, so no open is asserted to have succeeded. Exhaustion must be a
        // refusal.
        assert!(
            held.len() <= super::MAX_DESCRIPTORS as usize,
            "never more than the stated ceiling"
        );
        for fd in held {
            close(fd);
        }
    }

    /// Reading a file gives back its contents.
    #[test]
    fn reading_a_file_gives_back_its_contents() {
        let _guard = exclusively();
        a_title_with("contents", b"abcd");
        let fd = open("/app0/game.bin").expect("opens");
        let mut buf = [0_u8; 4];
        assert_eq!(read(fd, &mut buf), Some(4));
        assert_eq!(&buf, b"abcd");
        close(fd);
    }
}
