//! Waiting for a descriptor to be ready.
//!
//! # Why a server needs this before it needs anything else
//!
//! A network server's loop is *wait, accept, serve*. `klogsrv` calls `select` between its
//! `listen` and its `accept`, so without it the loop never turns - the guest is told the call
//! failed and takes its error path having never served anything.
//!
//! # What an `fd_set` is here
//!
//! A bitmap, and its shape is in the checkout:
//!
//! ```text
//! sys/sys/select.h
//!     typedef unsigned long __fd_mask;             64 bits on this data model
//!     #define FD_SETSIZE 1024
//!     struct fd_set { __fd_mask __fds_bits[FD_SETSIZE / 64]; };
//! ```
//!
//! So sixteen words, and descriptor `n` is bit `n % 64` of word `n / 64`. Nothing about that
//! is guessed.
//!
//! # Readiness is asked by polling, and that is stated
//!
//! The standard library has no readiness primitive, so this asks each descriptor in turn and
//! sleeps a millisecond between rounds. That costs latency a real `select` would not, and it
//! is honest about what it can and cannot promise: a guest that measures its own wakeup
//! latency would see the difference.
//!
//! What it must not do is **consume** anything while asking. Finding out whether a listener
//! has a connection means accepting one, so the connection is kept on the listener and the
//! guest's own `accept` takes it (D373).

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Descriptors an `fd_set` can hold, from `FD_SETSIZE` in `sys/sys/select.h`.
pub const FD_SETSIZE: u64 = 1024;

/// Bits in one word of an `fd_set`, from `__fd_mask` being an `unsigned long`.
const BITS_PER_WORD: u64 = 64;

/// Words in an `fd_set`.
const WORDS: usize = (FD_SETSIZE / BITS_PER_WORD) as usize;

/// How long to wait between asking every descriptor again.
///
/// A millisecond. Short enough that a server's accept loop is not visibly slower than it
/// would be, long enough that a guest blocked for a second does not spend that second
/// spinning this process at full speed.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1);

/// One `fd_set`, read out of guest memory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FdSet {
    /// The bitmap, one bit per descriptor.
    words: [u64; WORDS],
}

impl FdSet {
    /// Reads the set at `address`, or an empty one for a null pointer.
    ///
    /// A null set is not an error: it is how a caller says *nothing in this direction*, and
    /// every one of the three sets is separately optional.
    ///
    /// # Safety
    ///
    /// `address`, when non-null, must point at an `fd_set` in guest memory - the same
    /// contract the real call has under the identity mapping (D014).
    pub unsafe fn read(address: u64) -> Self {
        let mut set = Self::default();
        let Ok(at) = usize::try_from(address) else {
            return set;
        };
        if at == 0 {
            return set;
        }
        let base = std::ptr::with_exposed_provenance::<u64>(at);
        for (index, word) in set.words.iter_mut().enumerate() {
            // SAFETY: the caller guarantees an `fd_set` here, which is exactly `WORDS`
            // words, so every index below is inside it.
            let slot = unsafe { base.add(index) };
            // SAFETY: `slot` is in bounds by the line above. Read unaligned because
            // nothing promises the guest aligned it.
            *word = unsafe { std::ptr::read_unaligned(slot) };
        }
        set
    }

    /// Writes the set back to `address`, doing nothing for a null pointer.
    ///
    /// # Safety
    ///
    /// As [`Self::read`].
    pub unsafe fn write(self, address: u64) {
        let Ok(at) = usize::try_from(address) else {
            return;
        };
        if at == 0 {
            return;
        }
        let base = std::ptr::with_exposed_provenance_mut::<u64>(at);
        for (index, word) in self.words.iter().enumerate() {
            // SAFETY: as in `read` - an `fd_set` the caller supplied, so every index is
            // inside it.
            let slot = unsafe { base.add(index) };
            // SAFETY: `slot` is in bounds by the line above.
            unsafe { std::ptr::write_unaligned(slot, *word) };
        }
    }

    /// Whether descriptor `fd` is in the set.
    #[must_use]
    pub fn contains(&self, fd: u64) -> bool {
        let (word, bit) = Self::position(fd);
        self.words.get(word).is_some_and(|w| w & (1 << bit) != 0)
    }

    /// Puts descriptor `fd` in the set.
    pub fn insert(&mut self, fd: u64) {
        let (word, bit) = Self::position(fd);
        if let Some(w) = self.words.get_mut(word) {
            *w |= 1 << bit;
        }
    }

    /// How many descriptors are in the set.
    #[must_use]
    pub fn len(&self) -> u32 {
        self.words.iter().map(|w| w.count_ones()).sum()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Which word and which bit a descriptor lives in.
    fn position(fd: u64) -> (usize, u64) {
        (
            usize::try_from(fd / BITS_PER_WORD).unwrap_or(usize::MAX),
            fd % BITS_PER_WORD,
        )
    }
}

/// How long a `select` may wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// Until something is ready, however long that takes.
    Forever,
    /// This long, which may be no time at all.
    Until(std::time::Duration),
}

/// Reads the `timeval` a caller passed, or [`Wait::Forever`] for a null one.
///
/// A null timeout means block; a zeroed one means ask once and answer. Both are ordinary and
/// a caller means quite different things by them.
///
/// # Safety
///
/// `address`, when non-null, must point at a `struct timeval` in guest memory.
unsafe fn read_timeout(address: u64) -> Wait {
    let Ok(at) = usize::try_from(address) else {
        return Wait::Forever;
    };
    if at == 0 {
        return Wait::Forever;
    }
    let base = std::ptr::with_exposed_provenance::<u64>(at);
    // SAFETY: the caller guarantees a `timeval` here - two machine words, from
    // `sys/sys/_timeval.h`, as `orbistoun-libc`'s clock module records.
    let seconds = unsafe { std::ptr::read_unaligned(base) };
    // SAFETY: the second field of the same structure.
    let micros_at = unsafe { base.add(1) };
    // SAFETY: in bounds by the line above.
    let micros = unsafe { std::ptr::read_unaligned(micros_at) };
    Wait::Until(std::time::Duration::from_secs(seconds) + std::time::Duration::from_micros(micros))
}

/// `select(nfds, readfds, writefds, exceptfds, timeout)`.
///
/// # What each direction answers
///
/// **Read** is asked of the descriptor: a listener with a connection waiting, a stream with
/// bytes or an end-of-file, a file (always - a read from one does not block), and a standard
/// stream (also always, because reading one here answers zero immediately rather than waiting
/// on a terminal nobody is typing into).
///
/// **Write** is answered yes for anything connected. Writes here go straight to the host and
/// do not buffer, so a write will not block - and saying otherwise would park a guest waiting
/// for a readiness that had already arrived.
///
/// **Exceptional** is always empty. Nothing here generates out-of-band data or the other
/// conditions that set it, so reporting one would be inventing an event.
///
/// Answers the number of descriptors left set, as the interface does, and rewrites the sets
/// in place - which is why a caller rebuilds them every time round its loop.
///
/// Reference: POSIX.1-2008 `select(2)`; `fd_set` from `sys/sys/select.h`.
fn select(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (nfds, read_at, write_at, except_at, timeout_at) =
        (args[0], args[1], args[2], args[3], args[4]);

    // SAFETY: a guest-supplied `fd_set` under the identity mapping (D014); a null one reads
    // as empty without being dereferenced.
    let wanted_read = unsafe { FdSet::read(read_at) };
    // SAFETY: as above, for the other direction.
    let wanted_write = unsafe { FdSet::read(write_at) };
    // SAFETY: a guest-supplied `timeval`, or null for "wait as long as it takes".
    let wait = unsafe { read_timeout(timeout_at) };

    let started = std::time::Instant::now();
    let ceiling = nfds.min(FD_SETSIZE);
    loop {
        let mut ready_read = FdSet::default();
        let mut ready_write = FdSet::default();
        for fd in 0..ceiling {
            if wanted_read.contains(fd) && readable(fd) {
                ready_read.insert(fd);
            }
            if wanted_write.contains(fd) && writable(fd) {
                ready_write.insert(fd);
            }
        }

        let found = ready_read.len() + ready_write.len();
        let expired = match wait {
            Wait::Forever => false,
            Wait::Until(limit) => started.elapsed() >= limit,
        };
        if found > 0 || expired {
            // SAFETY: the same guest set, written back where it was read from.
            unsafe { ready_read.write(read_at) };
            // SAFETY: as above, for the other direction.
            unsafe { ready_write.write(write_at) };
            // Exceptional conditions: none, ever, and said so by clearing rather than by
            // leaving whatever the caller had there.
            //
            // SAFETY: a guest-supplied set, or null, which writes nothing.
            unsafe { FdSet::default().write(except_at) };
            return u64::from(found);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Whether a read on this descriptor would return without waiting.
fn readable(fd: u64) -> bool {
    if crate::descriptor::is_standard(fd) {
        // Reading one answers zero immediately rather than waiting on a terminal nobody is
        // typing into, so it is always ready - which is true, if not useful.
        return true;
    }
    crate::descriptor::readable(fd)
}

/// Whether a write on this descriptor would return without waiting.
fn writable(fd: u64) -> bool {
    if crate::descriptor::is_standard(fd) {
        return true;
    }
    crate::descriptor::writable(fd)
}

/// Implementations this module provides, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("select", select)]
}

#[cfg(test)]
mod tests {
    use super::{BITS_PER_WORD, FD_SETSIZE, FdSet, WORDS};

    /// **Descriptor `n` is bit `n % 64` of word `n / 64`**, which is the whole layout.
    #[test]
    fn a_descriptor_lives_in_the_bit_the_header_says_it_does() {
        let mut set = FdSet::default();
        set.insert(0);
        set.insert(63);
        set.insert(64);
        assert_eq!(set.words[0], (1 << 63) | 1, "the first word holds 0 and 63");
        assert_eq!(set.words[1], 1, "and 64 starts the second");
        assert!(set.contains(64) && !set.contains(65));
    }

    /// The set is exactly the size the header says, and nothing beyond it is touched.
    #[test]
    fn the_set_is_the_size_the_header_states() {
        assert_eq!(WORDS, 16, "1024 descriptors at 64 bits a word");
        assert_eq!(FD_SETSIZE / BITS_PER_WORD, WORDS as u64);

        let mut set = FdSet::default();
        set.insert(FD_SETSIZE + 10);
        assert!(set.is_empty(), "a descriptor past the end goes nowhere");
    }

    /// A set survives a round trip through guest memory unchanged.
    #[test]
    fn a_set_reads_back_as_it_was_written() {
        let mut set = FdSet::default();
        set.insert(3);
        set.insert(200);
        let mut memory = [0_u64; WORDS];
        let at = memory.as_mut_ptr() as u64;
        // SAFETY: `memory` is exactly an `fd_set` in size and lives for the test.
        unsafe { set.write(at) };
        // SAFETY: as above.
        let back = unsafe { FdSet::read(at) };
        assert_eq!(back, set);
        assert_eq!(back.len(), 2);
    }

    /// A null set is empty rather than a fault, which is how a caller says "not this one".
    #[test]
    fn a_null_set_is_empty_rather_than_a_fault() {
        // SAFETY: the address is zero, which is the case this answers without reading.
        let set = unsafe { FdSet::read(0) };
        assert!(set.is_empty());
        // SAFETY: as above - a null destination is not written.
        unsafe { set.write(0) };
    }

    /// A zero timeout asks once and answers, rather than waiting.
    #[test]
    fn a_zero_timeout_returns_promptly_and_reports_nothing_ready() {
        let timeout = [0_u64; 2];
        let mut readfds = [0_u64; WORDS];
        // Descriptor 7, which nothing has opened.
        readfds[0] = 1 << 7;

        let started = std::time::Instant::now();
        let answered = super::select(&[
            8,
            readfds.as_mut_ptr() as u64,
            0,
            0,
            std::ptr::addr_of!(timeout) as u64,
            0,
        ]);
        assert_eq!(answered, 0, "nothing was ready");
        assert_eq!(
            readfds[0], 0,
            "and the set was cleared, as the interface says"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "a zero timeout asks once"
        );
    }

    /// A standard stream is always ready, which is true here and worth asserting.
    #[test]
    fn a_standard_stream_is_reported_ready() {
        let timeout = [0_u64; 2];
        let mut readfds = [0_u64; WORDS];
        readfds[0] = 1 << crate::descriptor::STDIN;

        let answered = super::select(&[
            4,
            readfds.as_mut_ptr() as u64,
            0,
            0,
            std::ptr::addr_of!(timeout) as u64,
            0,
        ]);
        assert_eq!(answered, 1);
        assert_eq!(readfds[0], 1 << crate::descriptor::STDIN);
    }
}
