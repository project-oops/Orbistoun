//! Guest-visible error codes.

use std::fmt;

/// A guest-visible return code from an HLE call.
///
/// Target system libraries return a signed 32-bit code where zero (or a
/// small positive value) means success and the high bit is set for errors. The
/// exact negative value matters: guests branch on specific codes, so returning a
/// generic failure where the real firmware returns a specific one is a bug even
/// though both are "an error".
///
/// Codes are added here as they are *established* - by a hardware probe, a
/// documented analogue, or an observed guest branch - never guessed. An
/// unestablished code is [`GuestError::Unimplemented`], which is loud by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuestError {
    /// The call is not implemented. Deliberately distinct from any real firmware
    /// code so it can never be mistaken for established behaviour.
    Unimplemented,
    /// An argument was outside the range the call accepts.
    InvalidArgument,
    /// A handle did not refer to a live object.
    InvalidHandle,
    /// The operation could not get the memory it needed.
    NoMemory,
    /// A code established for this call, carried verbatim.
    ///
    /// Use this once the real value is known. The `u32` is the raw bit pattern as
    /// the guest sees it, so it round-trips exactly.
    Raw(u32),
}

/// The high half every error code the target returns has been observed to carry.
///
/// See [`GuestError::vendor`] for how this is known.
pub const VENDOR_ERROR_BASE: u32 = 0x8002_0000;

/// The POSIX `errno` values the target has been watched to return.
///
/// Named rather than spelled at each call site: `0x8002_0010` says nothing about what
/// happened, and `GuestError::vendor(errno::BUSY)` says what was provoked. Only values
/// somebody has actually observed coming back are listed - this is a record of
/// measurements, not a copy of `errno.h`, and a name here is a claim that the target
/// produced it.
pub mod errno {
    /// The caller does not hold what it is operating on. Observed from unlocking a mutex
    /// nobody holds.
    pub const NOT_OWNER: u32 = 1;
    /// No such file or directory. Observed from opening a path that is not there, and from
    /// asking for a module that is not present.
    pub const NO_ENTRY: u32 = 2;
    /// No such process, used by the target for a handle or name it cannot find. Observed
    /// from deleting an event flag by a null handle, and from resolving an absent symbol.
    pub const NO_SUCH: u32 = 3;
    /// The descriptor is not open. Observed from closing, reading and seeking on -1.
    pub const BAD_DESCRIPTOR: u32 = 9;
    /// Not enough room in the caller's buffer. POSIX/FreeBSD `sysctl` answers this when the
    /// destination is smaller than the value, after setting the needed length.
    pub const NO_MEMORY: u32 = 12;
    /// Permission denied. Observed from querying memory past the last region.
    pub const DENIED: u32 = 13;
    /// A pointer the call cannot use. Observed from opening a path through a null pointer, which the
    /// target answers `0x8002_000e` (obSCEne `040-file/open-rejects-null`).
    pub const FAULT: u32 = 14;
    /// Held by somebody else, and the call does not wait. Observed from taking a lock the
    /// caller already holds.
    pub const BUSY: u32 = 16;
    /// The argument is outside what the call accepts. Observed from querying memory with an
    /// undefined flag, and from asking for a module description the wrong way.
    pub const INVALID: u32 = 22;
}

impl GuestError {
    /// A code the target returns, built from the POSIX `errno` underneath it.
    ///
    /// # How this is known
    ///
    /// **Measured on real hardware**, which is what separates it from everything around it. A
    /// complete conformance run on a target console provoked seven distinct failures across
    /// five unrelated call families, and every one came back as this same pattern:
    ///
    /// | what was provoked | errno | observed |
    /// |---|---|---|
    /// | unlock a mutex nobody holds | 1 | `0x8002_0001` |
    /// | open a path that does not exist | 2 | `0x8002_0002` |
    /// | delete an event flag by a null handle | 3 | `0x8002_0003` |
    /// | close a descriptor of -1 | 9 | `0x8002_0009` |
    /// | query memory past the last region | 13 | `0x8002_000d` |
    /// | take a lock the caller already holds | 16 | `0x8002_0010` |
    /// | query memory with an undefined flag | 22 | `0x8002_0016` |
    ///
    /// Before that run this was a hypothesis resting on **one** value seen on an emulator -
    /// which could itself have been inferring the same rule, so it was evidence of nothing.
    /// Seven values from five families on the machine itself is a different class of thing,
    /// and it is what makes returning these honest rather than plausible: a code built here
    /// is one somebody watched the target produce.
    ///
    /// # What this deliberately does not decide
    ///
    /// Some calls hand the code back sign-extended to sixty-four bits and some do not, in the
    /// same run. That is the **return width of the individual function**, not a property of
    /// the code, so it is not encoded here and belongs with whichever shim returns it.
    ///
    /// Reference: `data/hardware/ps5-full.txt` in the sibling conformance-probe repository,
    /// recorded 2026-08-30, whose run header names the console state it was taken under.
    #[must_use]
    pub const fn vendor(errno: u32) -> Self {
        Self::Raw(VENDOR_ERROR_BASE | errno)
    }

    /// A vendor error in a **subsystem's own base**, rather than the kernel's `0x8002_0000`.
    ///
    /// Each subsystem numbers its errors from its own high half-word - audio from `0x8026_0000`, the
    /// pad from `0x8092_0000` - and a guest checks against that subsystem's constant, so answering the
    /// kernel base (or a `0x7fff…` placeholder) never matches. Measured per subsystem by obSCEne's
    /// `*-rejects-bad-handle` checks; the base belongs with whichever shim knows its subsystem, which
    /// passes it here.
    #[must_use]
    pub const fn vendor_in(base: u32, errno: u32) -> Self {
        Self::Raw(base | errno)
    }

    /// The raw 32-bit value a guest observes for this error.
    pub const fn as_raw(self) -> u32 {
        match self {
            // Placeholder bit patterns in a range no real SCE code occupies, so a
            // stub leaking into guest-visible behaviour is obvious in a trace
            // rather than plausible.
            Self::Unimplemented => 0x7FFF_0001,
            Self::InvalidArgument => 0x7FFF_0002,
            Self::InvalidHandle => 0x7FFF_0003,
            Self::NoMemory => 0x7FFF_0004,
            Self::Raw(v) => v,
        }
    }
}

impl fmt::Display for GuestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unimplemented => f.write_str("unimplemented"),
            Self::InvalidArgument => f.write_str("invalid argument"),
            Self::InvalidHandle => f.write_str("invalid handle"),
            Self::NoMemory => f.write_str("out of memory"),
            Self::Raw(v) => write!(f, "sce error {v:#010x}"),
        }
    }
}

impl std::error::Error for GuestError {}

/// The result of an HLE call, before it is flattened to a guest return value.
pub type GuestResult<T> = Result<T, GuestError>;

#[cfg(test)]
mod tests {
    use super::GuestError;

    #[test]
    fn raw_round_trips_exactly() {
        // A code established from hardware must reach the guest bit-identical -
        // this is the property that lets us encode probe results directly.
        assert_eq!(GuestError::Raw(0x8002_0016).as_raw(), 0x8002_0016);
    }

    /// The seven codes a target console was watched to return, and the rule they all fit.
    ///
    /// **This is the measurement, written down as a test.** If the encoding is ever changed
    /// the failure names which observation it contradicts, rather than a number changing
    /// somewhere and nobody knowing what it cost.
    #[test]
    fn vendor_codes_match_what_hardware_returned() {
        for (errno, observed, provoked) in [
            (
                super::errno::NOT_OWNER,
                0x8002_0001,
                "unlock a mutex nobody holds",
            ),
            (
                super::errno::NO_ENTRY,
                0x8002_0002,
                "open a path that is not there",
            ),
            (
                super::errno::NO_SUCH,
                0x8002_0003,
                "delete an event flag by null handle",
            ),
            (
                super::errno::BAD_DESCRIPTOR,
                0x8002_0009,
                "close a descriptor of -1",
            ),
            (
                super::errno::DENIED,
                0x8002_000d,
                "query memory past the last region",
            ),
            (super::errno::BUSY, 0x8002_0010, "take a lock already held"),
            (
                super::errno::INVALID,
                0x8002_0016,
                "query memory with an undefined flag",
            ),
        ] {
            assert_eq!(
                GuestError::vendor(errno).as_raw(),
                observed,
                "the console returned {observed:#x} when asked to {provoked}"
            );
        }
    }

    /// A measured code is not a placeholder, and must never be mistaken for one.
    #[test]
    fn vendor_codes_are_outside_the_placeholder_range() {
        assert_ne!(
            GuestError::vendor(super::errno::BUSY).as_raw() & 0x8000_0000,
            0,
            "a code the console returned has the high bit the placeholders avoid"
        );
    }

    #[test]
    fn placeholders_are_distinguishable_from_real_codes() {
        // Real SCE error codes have the high bit set. Ours deliberately do not,
        // so an unimplemented stub can never be misread as firmware behaviour.
        for e in [
            GuestError::Unimplemented,
            GuestError::InvalidArgument,
            GuestError::InvalidHandle,
            GuestError::NoMemory,
        ] {
            assert_eq!(
                e.as_raw() & 0x8000_0000,
                0,
                "{e:?} collides with real codes"
            );
        }
    }
}
