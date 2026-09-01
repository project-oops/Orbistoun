//! File descriptors, and the standard streams.
//!
//! # Descriptors are not the same handles stdio uses
//!
//! `fopen` answers an address, because the guest dereferences it (D165). A descriptor is a
//! small integer the guest passes back and compares against zero and negative values, and
//! nothing reads through it - so it is a small integer here. Matching what the guest does
//! with a value is the rule; "always use an address" is not (D169).
//!
//! # Why a guest's standard output does not go to standard output
//!
//! The worker speaks its protocol over **stdout**, as newline-delimited JSON. A guest
//! writing bytes there would interleave with that stream, and a half-finished line breaks
//! the reader for good - the same reasoning that already sends fault reports to the error
//! stream.
//!
//! So a guest writing to descriptor 1 or 2 lands on the host's **stderr**, where a
//! truncated line costs nothing. This matters more than it looks: a conformance probe that
//! cannot write to standard output cannot report at all, and one loader examined refuses
//! descriptor 1 outright for exactly that reason (D170).

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
/// Bounded deliberately. A guest that leaks descriptors should hit a wall that says so
/// rather than climbing forever - and an unbounded search for a free number is a loop with
/// no stated end, which is the same defect wearing a different hat.
pub const MAX_DESCRIPTORS: u64 = 4096;

/// The first descriptor handed out for a real file.
///
/// Three, because zero through two are the standard streams and a guest expects them to
/// be. Handing a file descriptor 1 would make its writes appear as program output.
pub const FIRST_FILE: u64 = 3;

/// What a descriptor refers to.
///
/// **One table for files and sockets, because a guest has one.** `close`, `read` and `write`
/// take either without being told which, and two tables would mean two numbering spaces and
/// a descriptor that means different things to different calls (D371).
#[derive(Debug)]
enum Target {
    /// A file on the host.
    File(std::fs::File),
    /// A socket, at whatever stage of its life it has reached.
    Socket(crate::socket::Socket),
    /// An event queue, and what a guest has asked it to watch.
    ///
    /// **In the descriptor table rather than beside it**, because that is what it is: a
    /// guest closes a queue with `close`, and it must not have to know which kind of thing
    /// it is closing.
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
/// The table is a process-wide static, and [`crate::exclusively`] serialises the tests but did not
/// reset between them - so a descriptor one test left open shifted the descriptor numbers the next
/// test was handed, which is how a `sendfile` that passed alone failed in the suite (dropping each
/// `Target` closes the host file it holds).
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
/// Read-only through `/app0`, which is the user's own title directory and the material
/// being measured. See [`create`] for the writable half.
pub fn open(guest_path: &str) -> Option<u64> {
    // **Devices before the mount table**, because a device has no host path to resolve to
    // and asking the table about one would answer "not there" for something that is (D389).
    if let Some(device) = crate::device::named(guest_path) {
        return insert(Target::Device(device));
    }
    let Some(host) = crate::mount::resolve(guest_path) else {
        // A path nothing here holds, recorded as the work item it is (D387).
        crate::wanted::note(guest_path);
        return None;
    };
    let Ok(file) = std::fs::File::open(host) else {
        crate::wanted::note(guest_path);
        return None;
    };
    insert_file(file)
}

/// Opens a guest path for writing, creating it, answering a descriptor.
///
/// Refused outside `/data`. The separation is the whole decision: storage the installation
/// owns is a guest's to write, and the title directory is not (D250).
pub fn create(guest_path: &str) -> Option<u64> {
    if !crate::mount::is_writable(guest_path) {
        return None;
    }
    let host = crate::mount::resolve(guest_path)?;
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
/// Answers [`None`] when the descriptor is not a queue, which is what `kevent` reports as a
/// failure - passing a file to it is a guest's mistake and should be said so.
///
/// **The lock is held across the call**, which is why the caller must not ask about
/// readiness from inside one: that takes this same lock (D385).
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
    // Lowest free descriptor, which is what a caller expects and what makes a leak
    // visible as a number that climbs.
    let fd = (FIRST_FILE..FIRST_FILE + MAX_DESCRIPTORS).find(|n| !table.contains_key(n))?;
    table.insert(fd, target);
    Some(fd)
}

/// Does something with the socket a descriptor holds, if it holds one.
///
/// Answers [`None`] when the descriptor is not a socket, which a caller turns into the
/// failure the interface documents - a file descriptor passed to `listen` is a guest's
/// mistake and should be reported as one rather than acted on.
///
/// **The lock is held across the call**, which is what makes a two-step change - read the
/// bound address, replace with a listener - atomic against another guest thread. It also
/// means a blocking operation inside here blocks the table; `accept` is the one that does,
/// and it is documented there.
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
/// Reading from a standard stream answers zero rather than blocking on the host's own
/// input: a guest that reads stdin here would otherwise hang the worker on a terminal
/// nobody is typing into.
pub fn read(fd: u64, into: &mut [u8]) -> Option<usize> {
    use std::io::Read as _;
    if is_standard(fd) {
        return Some(0);
    }
    let mut table = table().lock().ok()?;
    if let Some(target) = table.get_mut(&fd) {
        return match target {
            // Nothing is read from a queue: `kevent` is how its events are taken, and a
            // `read` on one fails on the platform too.
            Target::Queue(_) => None,
            Target::Device(device) => Some(device.read(into)),
            Target::File(file) => Some(file.read(into).unwrap_or(0)),
            // A socket reads exactly as a file does, which is the point of one table: a server
            // that reads a request with `read` and one that reads it with `recv` are the same
            // program to everything below this line.
            Target::Socket(socket) => match socket {
                crate::socket::Socket::Stream(stream) => Some(stream.read(into).unwrap_or(0)),
                // Reading a listener is a guest's mistake, reported rather than answered with
                // an empty read that looks like a closed connection.
                _ => None,
            },
        };
    }
    // If fd is 3 and not an opened file/socket in the table, it is the payload's kernel R/W escape pipe.
    if fd == 3 && crate::escape::get_kernel_read_address() != 0 {
        return Some(crate::escape::read_kernel_pipe(into));
    }
    None
}

/// Writes `bytes`, answering how many were taken.
///
/// **Standard output and standard error both go to the host's error stream**, never its
/// output stream, which carries the worker's protocol. Anything else is refused: files are
/// opened read-only, so a write to one would be a write into the user's own title.
pub fn write(fd: u64, bytes: &[u8]) -> Option<usize> {
    use std::io::Write as _;
    if fd == STDOUT || fd == STDERR {
        let mut stderr = std::io::stderr();
        let _ = stderr.write_all(bytes);
        // Flushed per write, because a probe's output is only useful if it survives the
        // guest faulting immediately afterwards - which is the ordinary case here.
        let _ = stderr.flush();
        return Some(bytes.len());
    }
    // A file, which is writable only if it was opened through `create` - anything opened
    // for reading refuses here, and the error is the guest's answer rather than a panic.
    let mut table = table().lock().ok()?;
    match table.get_mut(&fd) {
        // A device that refuses writes says so rather than discarding them.
        Some(Target::Device(device)) => device.writable().then_some(bytes.len()),
        Some(Target::File(file)) => file.write(bytes).ok(),
        Some(Target::Socket(crate::socket::Socket::Stream(stream))) => stream.write(bytes).ok(),
        // Not an open descriptor. FD 4 is the kernel R/W escape pipe write end (`rwpipe[1]`) when the
        // escape address is set, and its writes are swallowed as the pipe would. Checked *after* the
        // table, not before, so a real file that happens to land at descriptor 4 is written rather
        // than silently discarded - the collision that swallowed a `sendfile` and hung a socket test.
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
    // Seeking a socket is meaningless and is refused rather than answered with a position.
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

/// Sets an open file's length. Answers whether it could.
///
/// A standard stream has no length, so it is refused rather than being given one - and a
/// descriptor nobody opened is refused for the same reason `read` and `write` refuse it.
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
/// # The position is the whole point, and one host moves it anyway
///
/// `pread` exists so two threads can read one file at once without agreeing about where the
/// position is. Both hosts this builds for have a positioned read - and **Windows' updates
/// the file pointer**, which is exactly what `pread` promises not to do. It was found by the
/// test below reading three bytes from the wrong place.
///
/// So on that host the position is put back afterwards. That is a seek-read-seek, which is
/// the thing this was meant to avoid - except that every call here happens **while the
/// descriptor table is locked**, so no other guest call can observe the position in between.
/// The lock provides the atomicity the syscall does not.
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
    // Put it back whether the read worked or not: a failed positioned read that moved the
    // position is worse than one that failed cleanly.
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
/// # What a duplicate shares, and what it does not
///
/// On the platform this imitates, two descriptors from `dup2` share one file position: a
/// read through either moves both. **Here they do not** - each holds its own handle, so each
/// has its own position. That difference is stated rather than hidden, because it is the one
/// thing a program could notice, and it costs nothing until a program reads through both.
///
/// What `dup2` is overwhelmingly used *for* - putting a file on descriptor 0, 1 or 2 before
/// handing them to a child - does not depend on the shared position at all.
pub fn duplicate_into(from: u64, to: u64) -> bool {
    if from == to {
        // Documented: duplicating a descriptor onto itself succeeds and does nothing, not
        // even the close.
        return exists(from);
    }
    let Ok(mut table) = table().lock() else {
        return false;
    };
    let copy = match table.get(&from) {
        Some(Target::File(file)) => file.try_clone().ok().map(Target::File),
        Some(Target::Socket(socket)) => duplicate_socket(socket),
        // A queue is not duplicated: two descriptors onto one registration set is a sharing
        // model nothing here has, and inventing one would be worse than refusing. Same answer
        // as no descriptor at all, which is what the caller does with it.
        // A device is not duplicated: two descriptors draining one log would each get
        // half of it, which is worse than refusing.
        Some(Target::Queue(_) | Target::Device(_)) | None => None,
    };
    let Some(copy) = copy else {
        return false;
    };
    if is_standard(to) {
        // Refused rather than performed. The standard streams here are the host's, and
        // replacing one would send a guest's output somewhere this process still needs -
        // the worker's protocol lives on the other side of them (D170).
        return false;
    }
    table.insert(to, copy);
    true
}

/// A second handle onto the same socket, where the kind allows one.
fn duplicate_socket(socket: &crate::socket::Socket) -> Option<Target> {
    let copy = match socket {
        crate::socket::Socket::Stream(stream) => {
            crate::socket::Socket::Stream(stream.try_clone().ok()?)
        }
        crate::socket::Socket::Listener { listener, .. } => crate::socket::Socket::Listener {
            listener: listener.try_clone().ok()?,
            // A connection `select` left on the original stays there: it is one connection
            // and only one descriptor can be given it.
            pending: None,
        },
        // Nothing to duplicate yet, and inventing a second pending socket would give the
        // guest two descriptors that would later bind the same address.
        crate::socket::Socket::Pending { .. } => return None,
    };
    Some(Target::Socket(copy))
}

/// What the host says about the file behind a descriptor.
///
/// Answers nothing for a socket and for a standard stream: neither has a size or a
/// modification time, and reporting one would be inventing both.
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
/// # Asking must not take
///
/// The only way to find out whether a listener has a connection is to accept one, so it is
/// accepted and **kept on the listener** for the guest's own `accept` to take. A `select`
/// that consumed connections would lose every one of them to the call that only asked
/// (D373).
///
/// A stream is asked by peeking, which is a read that does not advance. Nothing is
/// consumed there either.
///
/// A file is always ready: a read from one does not wait. That is true rather than
/// convenient - the wait a `select` exists to avoid is a network wait.
pub fn readable(fd: u64) -> bool {
    let Ok(mut table) = table().lock() else {
        return false;
    };
    match table.get_mut(&fd) {
        Some(Target::File(_)) => true,
        Some(Target::Socket(socket)) => match socket {
            crate::socket::Socket::Listener { listener, pending } => {
                if pending.is_some() {
                    return true;
                }
                // Non-blocking for the question, and left that way: `accept` sets it back
                // before it waits, which is where the guest asked to block.
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
            crate::socket::Socket::Stream(stream) => {
                if stream.set_nonblocking(true).is_err() {
                    return false;
                }
                let mut byte = [0_u8; 1];
                // A peek rather than a read, so the byte stays for the guest. Zero bytes
                // is an end-of-file, which is *also* ready - a read there returns at once.
                let ready = match stream.peek(&mut byte) {
                    Ok(_) => true,
                    Err(e) => e.kind() != std::io::ErrorKind::WouldBlock,
                };
                let _ = stream.set_nonblocking(false);
                ready
            }
            // A socket with nothing behind it yet cannot be read from at all.
            crate::socket::Socket::Pending { .. } => false,
        },
        // A device is ready when it has something waiting - which for a log is the whole
        // question a server's event loop asks about it.
        Some(Target::Device(device)) => device.readable(),
        // **A queue is never reported ready here**, and that is a stated limit rather than
        // an answer: a kqueue registered inside another kqueue is ready when it has events,
        // and nothing here nests them. Saying yes would wake a guest for nothing, forever.
        Some(Target::Queue(_)) | None => false,
    }
}

/// Whether a write on this descriptor would return without waiting.
///
/// Yes for anything connected. Writes here go straight to the host and do not buffer, so a
/// write will not block - and saying otherwise would park a guest waiting for a readiness
/// that had already arrived.
pub fn writable(fd: u64) -> bool {
    let Ok(table) = table().lock() else {
        return false;
    };
    match table.get(&fd) {
        Some(Target::File(_)) => true,
        Some(Target::Socket(socket)) => {
            matches!(socket, crate::socket::Socket::Stream(_))
        }
        Some(Target::Device(device)) => device.writable(),
        // Nothing is written to a queue.
        Some(Target::Queue(_)) | None => false,
    }
}

/// Closes a descriptor. Answers whether there was one.
///
/// Closing a standard stream answers success and does nothing: a guest tidying up on exit
/// should not be told it failed, and actually closing the host's error stream would take
/// the fault reporter with it.
pub fn close(fd: u64) -> bool {
    // What was set on this number, forgotten with it - a descriptor that comes back around
    // must not inherit the last one's flags (D385).
    crate::fcntl::forget(fd);
    if is_standard(fd) {
        return true;
    }
    table().lock().is_ok_and(|mut t| t.remove(&fd).is_some())
}

/// Puts a descriptor into or out of non-blocking mode, answering whether it could.
///
/// **Only a socket has the distinction.** A file read here does not wait, and a standard
/// stream answers immediately either way - so setting it on one is accepted and changes
/// nothing, which is what the host would also do.
pub fn set_nonblocking(fd: u64, wanted: bool) -> bool {
    if is_standard(fd) {
        return true;
    }
    let Ok(mut table) = table().lock() else {
        return false;
    };
    match table.get_mut(&fd) {
        Some(Target::Socket(socket)) => match socket {
            crate::socket::Socket::Listener { listener, .. } => {
                listener.set_nonblocking(wanted).is_ok()
            }
            crate::socket::Socket::Stream(stream) => stream.set_nonblocking(wanted).is_ok(),
            // Nothing exists behind it yet, so there is nothing to set it on. Accepted,
            // because `bind` has not been called and the guest has said nothing wrong.
            crate::socket::Socket::Pending { .. } => true,
        },
        // A device answers immediately or not at all; there is no blocking read to turn
        // off, so the setting is accepted and changes nothing - which is true rather than
        // convenient.
        Some(Target::File(_) | Target::Queue(_) | Target::Device(_)) => true,
        None => false,
    }
}

/// Copies a descriptor to the lowest free number at or above `floor`.
///
/// What `F_DUPFD` documents, and the one place a descriptor number is chosen by anything
/// other than "the lowest free one".
pub fn duplicate_above(from: u64, floor: u64) -> Option<u64> {
    let copy = {
        let table = table().lock().ok()?;
        match table.get(&from)? {
            Target::File(file) => file.try_clone().ok().map(Target::File),
            Target::Socket(socket) => duplicate_socket(socket),
            // As with `dup2`: two descriptors onto one registration set is a sharing model
            // nothing here has, and two draining one log would each get half of it.
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
    /// Descriptors and mounts are process-global - they describe one guest - and the test
    /// harness runs in parallel. Without this the assertions about *which* descriptor
    /// comes back depend on what another test had open at the time, which is a flaky test
    /// about a diagnostic and worse than no test.
    use crate::exclusively;

    fn a_title_with(name: &str, contents: &[u8]) {
        let root = std::env::temp_dir().join(format!("orbistoun-fd-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        std::fs::write(root.join("game.bin"), contents).expect("file");
        crate::mount::clear();
        crate::mount::mount(crate::mount::APP_MOUNT, root);
    }

    #[test]
    fn a_guest_can_write_to_standard_output() {
        // The point of the whole module. A conformance probe that cannot write to
        // standard output cannot report at all, and a loader that refuses descriptor one
        // is a loader nothing can talk to (D170).
        assert_eq!(write(STDOUT, b"hello"), Some(5));
        assert_eq!(write(STDERR, b"there"), Some(5));
    }

    #[test]
    fn a_file_never_gets_a_standard_descriptor() {
        let _guard = exclusively();
        // Handing a file descriptor one would make its writes appear as program output.
        a_title_with("first", b"x");
        let fd = open("/app0/game.bin").expect("opens");
        assert!(fd >= FIRST_FILE);
        assert!(!is_standard(fd));
        assert!(close(fd));
    }

    #[test]
    fn writing_to_a_file_is_refused_rather_than_performed() {
        let _guard = exclusively();
        // Files are opened read-only: a write here would be a write into the user's own
        // title directory.
        a_title_with("readonly", b"x");
        let fd = open("/app0/game.bin").expect("opens");
        assert_eq!(write(fd, b"nope"), None);
        close(fd);
    }

    #[test]
    fn reading_a_standard_stream_answers_nothing_rather_than_blocking() {
        // A guest reading stdin would otherwise hang the worker on a terminal nobody is
        // typing into, and a hang costs the whole trace.
        let mut buf = [0_u8; 4];
        assert_eq!(read(super::STDIN, &mut buf), Some(0));
    }

    #[test]
    fn closing_a_standard_stream_succeeds_without_closing_it() {
        // A guest tidying up on exit should not be told it failed - and actually closing
        // the host's error stream would take the fault reporter with it.
        assert!(close(STDOUT));
        assert_eq!(write(STDOUT, b"still here"), Some(10));
    }

    #[test]
    fn descriptors_are_reused_lowest_first() {
        let _guard = exclusively();
        // What a caller expects, and it makes a leak visible as a number that climbs.
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

    #[test]
    fn running_out_of_descriptors_is_refused_rather_than_searched_forever() {
        // A guest that leaks should hit a wall that says so. An unbounded search for a
        // free number is a loop with no stated end.
        let _guard = exclusively();
        a_title_with("exhaust", b"x");
        let mut held = Vec::new();
        while let Some(fd) = open("/app0/game.bin") {
            held.push(fd);
            if held.len() > super::MAX_DESCRIPTORS as usize {
                break;
            }
        }
        // Bounded, and *which* bound bites is not the point: the host runs out of open
        // files first on this machine, at around three hundred. What matters is that
        // exhaustion is a refusal rather than a search that never ends.
        //
        // Deliberately **not** asserting that any open succeeded. The host's limit is
        // process-wide, so a test in another module holding handles can legitimately
        // leave none - and a flaky test about a diagnostic is worse than no test.
        assert!(
            held.len() <= super::MAX_DESCRIPTORS as usize,
            "never more than the stated ceiling"
        );
        for fd in held {
            close(fd);
        }
    }

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
