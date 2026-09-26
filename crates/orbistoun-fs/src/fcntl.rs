//! The descriptor's own settings: what a guest reads back and what it sets.
//!
//! A guest reads the flags, changes one bit and writes them back, so a placeholder answer to
//! `F_GETFL` is handed straight back to `F_SETFL` (D125). `O_NONBLOCK` is honoured on the
//! underlying socket, since a server's event loop depends on it. `FD_CLOEXEC` is remembered
//! and reads back as set, with no effect because nothing here ever `exec`s. Every other
//! command is refused: answering success for a lock, an owner or a seal would claim machinery
//! nothing here has.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// What a call answers when it did not work.
const FAILED: u64 = -1_i64 as u64;

/// One command or flag, read from the harvested `sys/sys/fcntl.h`.
///
/// A name the table cannot answer becomes a command no guest can ask for, rather than zero,
/// which is `F_DUPFD` and would hand back a working descriptor.
fn number(section: &str, name: &str) -> u64 {
    /// A value no guest can pass in a 32-bit argument.
    const UNNAMEABLE: u64 = u64::MAX;

    orbistoun_hle::constants::abi_constant(section, name)
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(UNNAMEABLE)
}

/// The flags each descriptor has been given, by descriptor.
///
/// Kept here rather than in the descriptor table and dropped by [`forget`] on `close`, so the
/// next `open` that reuses the number does not inherit them.
fn held() -> &'static Mutex<BTreeMap<u64, Flags>> {
    static HELD: OnceLock<Mutex<BTreeMap<u64, Flags>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Empties the descriptor-flag table between tests; see [`crate::descriptor::clear`].
#[cfg(test)]
pub(crate) fn clear() {
    if let Ok(mut held) = held().lock() {
        held.clear();
    }
}

/// What one descriptor has been set to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Flags {
    /// The status flags, as `F_GETFL` reports them.
    status: u64,
    /// The descriptor flags, as `F_GETFD` reports them.
    descriptor: u64,
}

/// Drops what was remembered about a descriptor.
///
/// Called when one is closed, so a number that comes back around starts clean.
pub(crate) fn forget(fd: u64) {
    if let Ok(mut all) = held().lock() {
        all.remove(&fd);
    }
}

/// What a descriptor is set to, or the default for one nothing has touched.
///
/// `O_RDWR` is the default: everything this crate hands a guest is opened for both
/// directions or is a socket.
fn flags_of(fd: u64) -> Flags {
    held()
        .lock()
        .ok()
        .and_then(|all| all.get(&fd).copied())
        .unwrap_or(Flags {
            status: number("fcntl", "O_RDWR"),
            descriptor: 0,
        })
}

/// Remembers what a descriptor was set to.
fn remember(fd: u64, flags: Flags) {
    if let Ok(mut all) = held().lock() {
        all.insert(fd, flags);
    }
}

/// `fcntl(fd, command, argument)`.
///
/// Answers what the command documents: the flags for a `GET`, zero for a `SET`, a descriptor
/// for a duplicate, and `-1` for a command nothing here performs.
///
/// Reference: POSIX.1-2008 `fcntl(2)`; the commands and `O_NONBLOCK` from `sys/sys/fcntl.h`.
fn fcntl(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (fd, command, argument) = (args[0], args[1], args[2]);
    if !crate::descriptor::exists(fd) {
        return FAILED;
    }
    let mut flags = flags_of(fd);

    if command == number("fcntl", "F_GETFL") {
        return flags.status;
    }
    if command == number("fcntl", "F_SETFL") {
        // The bit that has to be real: it decides whether the guest's event loop blocks.
        let nonblocking = argument & number("fcntl", "O_NONBLOCK") != 0;
        if !crate::descriptor::set_nonblocking(fd, nonblocking) {
            return FAILED;
        }
        flags.status = argument;
        remember(fd, flags);
        return 0;
    }
    if command == number("fcntl", "F_GETFD") {
        return flags.descriptor;
    }
    if command == number("fcntl", "F_SETFD") {
        flags.descriptor = argument;
        remember(fd, flags);
        return 0;
    }
    if command == number("fcntl", "F_DUPFD") || command == number("fcntl", "F_DUPFD_CLOEXEC") {
        let Some(copy) = crate::descriptor::duplicate_above(fd, argument) else {
            return FAILED;
        };
        let mut copied = flags;
        if command == number("fcntl", "F_DUPFD_CLOEXEC") {
            copied.descriptor |= number("fcntl", "FD_CLOEXEC");
        } else {
            // A plain duplicate does not carry the close-on-exec flag.
            copied.descriptor &= !number("fcntl", "FD_CLOEXEC");
        }
        remember(copy, copied);
        return copy;
    }
    // A command nothing here performs. Refused rather than answered zero: a guest told its
    // lock was taken behaves differently from one told it was not.
    FAILED
}

/// Implementations this module provides, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("fcntl", fcntl)]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    fn call(args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        super::implementations()[0].1(&args)
    }

    fn number(name: &str) -> u64 {
        super::number("fcntl", name)
    }

    /// The commands are harvested rather than written down.
    #[test]
    fn the_commands_come_from_the_header() {
        assert_eq!(number("F_GETFL"), 3);
        assert_eq!(number("F_SETFL"), 4);
        assert_eq!(number("F_GETFD"), 1);
        assert_eq!(number("F_SETFD"), 2);
        assert_eq!(number("F_DUPFD"), 0);
        assert_eq!(number("O_NONBLOCK"), 0x0004);
    }

    /// A descriptor nothing opened is refused rather than given flags of its own.
    #[test]
    fn a_descriptor_that_is_not_open_is_refused() {
        let answered = call([9999, number("F_GETFL"), 0, 0, 0, 0]);
        assert_eq!(answered, super::FAILED);
    }

    /// What is set reads back.
    #[test]
    fn a_flag_set_on_a_standard_stream_reads_back() {
        // Serialised: the flag table is a process-wide static that another test's setup
        // resets.
        let _guard = crate::exclusively();
        // A standard stream, because it is open without anything having to open it.
        let out = 1;
        let cloexec = number("FD_CLOEXEC");
        assert_eq!(call([out, number("F_SETFD"), cloexec, 0, 0, 0]), 0);
        assert_eq!(call([out, number("F_GETFD"), 0, 0, 0, 0]), cloexec);
        assert_eq!(call([out, number("F_SETFD"), 0, 0, 0, 0]), 0);
        assert_eq!(call([out, number("F_GETFD"), 0, 0, 0, 0]), 0);
        super::forget(out);
    }

    /// A command nothing performs is refused rather than answered zero.
    #[test]
    fn an_unserved_command_is_refused() {
        assert_eq!(call([1, number("F_GETLK"), 0, 0, 0, 0]), super::FAILED);
        assert_eq!(call([1, number("F_SETOWN"), 0, 0, 0, 0]), super::FAILED);
    }

    /// Closing a descriptor forgets what was set on it, so a reused number starts clean.
    #[test]
    fn a_closed_descriptor_forgets_its_flags() {
        let _guard = crate::exclusively();
        let err = 2;
        assert_eq!(
            call([err, number("F_SETFD"), number("FD_CLOEXEC"), 0, 0, 0]),
            0
        );
        super::forget(err);
        assert_eq!(call([err, number("F_GETFD"), 0, 0, 0, 0]), 0);
    }
}
