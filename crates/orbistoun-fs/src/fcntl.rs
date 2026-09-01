//! The descriptor's own settings: what a guest reads back and what it sets.
//!
//! # Why an unimplemented `fcntl` is worse than most
//!
//! It is a **pair**. A guest reads the flags, changes one bit, and writes them back - so a
//! placeholder answer to `F_GETFL` does not stay where it was put. `zftpd` did exactly that
//! with its web listener:
//!
//! ```text
//! fcntl(5, F_GETFL)            -> 0x7fff0005   an orbistoun placeholder
//! fcntl(5, F_SETFL, 0x7fff0005)               handed straight back
//! close(5)
//! ```
//!
//! Which is D125 in its purest form: a function answering an error code where a caller
//! expects data, and the damage happening one call later under a different name.
//!
//! # What is honoured and what is only remembered
//!
//! `O_NONBLOCK` is **honoured**: it is set on the underlying socket, so an `accept` or a
//! `read` that would have waited answers straight away instead. That is the one flag a
//! server's event loop depends on, and reporting it set while blocking anyway would hang the
//! loop on the first connection that went away.
//!
//! `FD_CLOEXEC` is **remembered and does nothing**, which is honest here rather than lazy:
//! nothing in this emulator ever `exec`s, so there is no moment at which the flag could have
//! an effect. It reads back as it was set, because a guest that sets it and checks it is
//! entitled to a consistent answer.
//!
//! Every other command is refused. A lock, an owner, a seal - each would need machinery
//! nothing here has, and answering success would tell a guest it held a lock it does not.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// What a call answers when it did not work.
const FAILED: u64 = -1_i64 as u64;

/// One command or flag, read from the harvested `sys/sys/fcntl.h`.
///
/// A name the table cannot answer becomes a command no guest can ask for, so the call is
/// refused rather than matching whatever happened to be zero - which is `F_DUPFD`, the one
/// command where a wrong match would hand back a working descriptor.
fn number(section: &str, name: &str) -> u64 {
    /// A value no guest can pass in a 32-bit argument.
    const UNNAMEABLE: u64 = u64::MAX;

    orbistoun_hle::constants::abi_constant(section, name)
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(UNNAMEABLE)
}

/// The flags each descriptor has been given, by descriptor.
///
/// **Kept here rather than in the descriptor table** because they outlive nothing: a guest
/// setting a flag on a descriptor it then closes has said something about that descriptor
/// number, and the next `open` to reuse the number must not inherit it. So [`forget`] is
/// called from `close`.
fn held() -> &'static Mutex<BTreeMap<u64, Flags>> {
    static HELD: OnceLock<Mutex<BTreeMap<u64, Flags>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Empties the descriptor-flag table between tests - see [`crate::descriptor::clear`].
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
/// **`O_RDWR` is the default and it is a claim.** Everything this crate hands a guest is
/// opened for both directions or is a socket, which is bidirectional - so it is true of every
/// descriptor that exists here. A read-only file would need this to say so, and there is not
/// one yet.
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
        // **The bit that has to be real.** Everything else here is bookkeeping; this one
        // decides whether the guest's event loop blocks.
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
            // A plain duplicate does not carry the close-on-exec flag, which is the whole
            // difference between the two commands.
            copied.descriptor &= !number("fcntl", "FD_CLOEXEC");
        }
        remember(copy, copied);
        return copy;
    }
    // A command nothing here performs. Refused rather than answered zero: a guest told its
    // lock was taken behaves quite differently from one told it was not.
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

    /// **A descriptor nothing opened is refused**, rather than given flags of its own.
    ///
    /// This is the half that matters: the bug this module exists for was a guest reading a
    /// value back out of a call that had no descriptor to speak about.
    #[test]
    fn a_descriptor_that_is_not_open_is_refused() {
        let answered = call([9999, number("F_GETFL"), 0, 0, 0, 0]);
        assert_eq!(answered, super::FAILED);
    }

    /// **What is set reads back**, which is the pair that was broken.
    #[test]
    fn a_flag_set_on_a_standard_stream_reads_back() {
        // Serialised: the flag table is a process-wide static that another test's setup resets, and a
        // reset landing between this set and get would read the flag back as zero.
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
