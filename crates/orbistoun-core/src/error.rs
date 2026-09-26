//! Guest-visible error codes.

use std::fmt;

/// A guest-visible return code from an HLE call.
///
/// Target libraries return a signed 32-bit code with the high bit set for errors, and guests
/// branch on specific codes, so a generic failure where the hardware returns a specific one is a
/// bug. Codes are added only once established by a hardware probe, a documented analogue or an
/// observed guest branch; anything else is [`GuestError::Unimplemented`] (D008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuestError {
    /// The call is not implemented. Distinct from every real code, so it is never mistaken for
    /// established behaviour.
    Unimplemented,
    /// An argument was outside the range the call accepts.
    InvalidArgument,
    /// A handle did not refer to a live object.
    InvalidHandle,
    /// The operation could not get the memory it needed.
    NoMemory,
    /// A code established for this call, carried verbatim as the raw bit pattern the guest sees.
    Raw(u32),
}

/// The high half every error code the target returns has been observed to carry; see
/// [`GuestError::vendor`].
pub const VENDOR_ERROR_BASE: u32 = 0x8002_0000;

/// The POSIX `errno` values this project answers with, in two tiers by how each is known.
///
/// Named so a call site says what was provoked. The first group is measured: the target was
/// watched returning each value. The second is published: the value comes from the documented
/// numbering of the platform's FreeBSD ancestor, under the measured `0x8002_0000` encoding, and
/// each entry names the probe that would promote it (D476).
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
    /// A pointer the call cannot use. Observed from opening a path through a null pointer, answered
    /// `0x8002_000e` (obSCEne `040-file/open-rejects-null`).
    pub const FAULT: u32 = 14;
    /// Held by somebody else, and the call does not wait. Observed from taking a lock the
    /// caller already holds.
    pub const BUSY: u32 = 16;
    /// The argument is outside what the call accepts. Observed from querying memory with an
    /// undefined flag, and from asking for a module description the wrong way.
    pub const INVALID: u32 = 22;
    /// Ask again. Observed from installing a second exception handler for a signal that already
    /// has one (obSCEne `030-thread/exception-handler`), answered `0x8002_0023`.
    ///
    /// Named from the number: 35 is `EAGAIN` in this platform's harvested headers, whatever gloss
    /// accompanied the measurement.
    pub const AGAIN: u32 = 35;
    /// The socket has no peer. Observed from a `recv` on a listening socket, which `libSceNet`
    /// answered `0x8041_0139` (obSCEne `102-net/recv-would-block`).
    ///
    /// Read through the vendor encoding rather than `errno` directly: 57 is the low byte under a
    /// base established by a different pair, and `ENOTCONN` is 57 in the harvested headers.
    pub const NOT_CONNECTED: u32 = 57;

    // Published, not measured: the entries below come from a document rather than the hardware.

    /// A wait gave up before it got what it was waiting for.
    ///
    /// Published: `ETIMEDOUT` is 60 in the documented FreeBSD numbering. POSIX has
    /// `pthread_mutex_timedlock` answer `ETIMEDOUT` and never `EBUSY`, so no measured value will do.
    /// Promoted by one conformance check: `pthread_mutex_timedlock` on a held lock with a deadline a
    /// millisecond out.
    pub const TIMED_OUT: u32 = 60;
}

impl GuestError {
    /// A code the target returns, built from the POSIX `errno` underneath it.
    ///
    /// Measured on hardware: seven failures from five unrelated call families all came back as
    /// `0x8002_0000 | errno` (unlocking an unheld mutex 1, opening a missing path 2, deleting an
    /// event flag by a null handle 3, closing descriptor -1 9, querying memory past the last region
    /// 13, relocking a held lock 16, querying memory with an undefined flag 22). Whether a function
    /// sign-extends the code to 64 bits is that function's return width, not a property of the
    /// code. Source: obSCEne's `data/hardware/ps5-full.txt`.
    #[must_use]
    pub const fn vendor(errno: u32) -> Self {
        Self::Raw(VENDOR_ERROR_BASE | errno)
    }

    /// A vendor error in a subsystem's own base, rather than the kernel's `0x8002_0000`.
    ///
    /// Each subsystem numbers its errors from its own high half-word (audio `0x8026_0000`, the pad
    /// `0x8092_0000`), and a guest checks against that constant. Measured per subsystem by obSCEne's
    /// `*-rejects-bad-handle` checks; the shim that knows its subsystem passes the base.
    #[must_use]
    pub const fn vendor_in(base: u32, errno: u32) -> Self {
        Self::Raw(base | errno)
    }

    /// The raw 32-bit value a guest observes for this error.
    pub const fn as_raw(self) -> u32 {
        match self {
            // Placeholder bit patterns in a range no real code occupies, so a leaking stub is obvious in a
            // trace, and negative so the guest's own check catches it (D670).
            Self::Unimplemented => PLACEHOLDER_BASE | 0x1,
            Self::InvalidArgument => PLACEHOLDER_BASE | 0x2,
            Self::InvalidHandle => PLACEHOLDER_BASE | 0x3,
            Self::NoMemory => PLACEHOLDER_BASE | 0x4,
            Self::Raw(v) => v,
        }
    }
}

/// The lowest placeholder value, and the base of the block they occupy.
///
/// Public because a fault reader needs it: an address in this block is one of these codes used
/// as a pointer. The high bit is set because every measured error sets it and a guest checks
/// `rc < 0`; `0xF7` differs from the `0x80` every measured vendor code begins with, so a
/// placeholder is never mistaken for firmware behaviour (D670).
pub const PLACEHOLDER_BASE: u32 = 0xF7FF_0000;

/// Names the placeholder an address is, when it is one of orbistoun's own.
///
/// A guest that uses a refusal as a pointer faults at that value, which would otherwise read as
/// orbistoun's own code crashing rather than an unimplemented call (D299). `Exact` where the
/// value is a placeholder; `Offset` where it is elsewhere in the block, a guest that added to
/// one first, which is the weaker claim.
#[must_use]
pub const fn placeholder_named(address: u64) -> Option<(&'static str, bool)> {
    // A sign-extended code is still one of ours: widening a value with the high bit set gives
    // `0xFFFF_FFFF_F7FF_0001`. Anything else above 32 bits is not one of these.
    let address = if address >> 32 == 0xFFFF_FFFF {
        address & 0xFFFF_FFFF
    } else {
        address
    };
    if address > u32::MAX as u64 {
        return None;
    }
    let value = address as u32;
    let exact = match value {
        v if v == PLACEHOLDER_BASE | 0x1 => Some("unimplemented"),
        v if v == PLACEHOLDER_BASE | 0x2 => Some("invalid argument"),
        v if v == PLACEHOLDER_BASE | 0x3 => Some("invalid handle"),
        v if v == PLACEHOLDER_BASE | 0x4 => Some("out of memory"),
        _ => None,
    };
    if let Some(name) = exact {
        return Some((name, true));
    }
    // Bounded above as well as below, so a vendor code such as `0x8002_0016` is never named as
    // orbistoun's own.
    if value >= PLACEHOLDER_BASE && value <= PLACEHOLDER_BASE | 0xFFFF {
        return Some(("a placeholder answer", false));
    }
    None
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

    /// The value a stub answers is recognised as one, so a fault at it is not read as a crash in
    /// orbistoun's own code.
    #[test]
    fn each_placeholder_is_named_exactly() {
        for (error, name) in [
            (GuestError::Unimplemented, "unimplemented"),
            (GuestError::InvalidArgument, "invalid argument"),
            (GuestError::InvalidHandle, "invalid handle"),
            (GuestError::NoMemory, "out of memory"),
        ] {
            assert_eq!(
                super::placeholder_named(u64::from(error.as_raw())),
                Some((name, true)),
                "{error} is one of ours and must be recognised"
            );
        }
    }

    /// A value inside the block but not one of the codes is reported as the weaker claim.
    #[test]
    fn an_offset_from_a_placeholder_is_reported_as_the_weaker_claim() {
        assert_eq!(
            super::placeholder_named(u64::from(GuestError::Unimplemented.as_raw()) + 0x18),
            Some(("a placeholder answer", false))
        );
    }

    /// An ordinary address is not one of ours: guest image and stack addresses are never named as
    /// placeholders.
    #[test]
    fn an_ordinary_address_is_not_a_placeholder() {
        assert_eq!(super::placeholder_named(0x4000_0000_0000), None);
        assert_eq!(super::placeholder_named(0x6000_007f_ca68), None);
        assert_eq!(super::placeholder_named(0), None);
        assert_eq!(super::placeholder_named(0x7FFE_FFFF), None);
    }

    /// A vendor error is not a placeholder.
    #[test]
    fn a_vendor_code_is_not_one_of_ours() {
        assert_eq!(super::placeholder_named(0x8002_0016), None);
    }

    /// A raw code round-trips bit-identical.
    #[test]
    fn raw_round_trips_exactly() {
        // A code established from hardware reaches the guest bit-identical, so probe results can be
        // encoded directly.
        assert_eq!(GuestError::Raw(0x8002_0016).as_raw(), 0x8002_0016);
    }

    /// The seven codes the hardware was watched to return fit the vendor encoding; a failure names
    /// the observation contradicted.
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

    /// A measured code is never mistaken for a placeholder: both are negative, and the facility
    /// byte separates them (D670).
    #[test]
    fn vendor_codes_are_outside_the_placeholder_range() {
        let measured = GuestError::vendor(super::errno::BUSY).as_raw();
        assert_ne!(
            measured & 0x8000_0000,
            0,
            "a code the console returned has the high bit"
        );
        assert_eq!(
            super::placeholder_named(u64::from(measured)),
            None,
            "and is not claimed as one of ours"
        );
    }

    /// A refusal is negative, so a guest that tests `rc < 0` sees a failure (D670).
    #[test]
    fn a_placeholder_is_negative_to_a_guest_that_tests_the_sign() {
        for error in [
            GuestError::Unimplemented,
            GuestError::InvalidArgument,
            GuestError::InvalidHandle,
            GuestError::NoMemory,
        ] {
            assert!(
                (error.as_raw() as i32) < 0,
                "{error:?} answers {:#x}, which a guest testing `rc < 0` reads as success",
                error.as_raw()
            );
        }
    }

    /// A placeholder is still not mistakable for a measured code; the low half is unchanged.
    #[test]
    fn a_placeholder_is_still_not_mistakable_for_a_measured_code() {
        for error in [
            GuestError::Unimplemented,
            GuestError::InvalidArgument,
            GuestError::InvalidHandle,
            GuestError::NoMemory,
        ] {
            assert_ne!(
                error.as_raw() & 0xFF00_0000,
                0x8000_0000,
                "{error:?} is shaped like a vendor code"
            );
        }
        // Named rather than derived: a guest branching on these branches on a measurement, and no stub
        // may answer one.
        for measured in [
            GuestError::vendor(super::errno::NO_SUCH).as_raw(),
            0x8041_0123,
        ] {
            assert_eq!(
                super::placeholder_named(u64::from(measured)),
                None,
                "{measured:#x} is a value a console answered"
            );
        }
    }

    /// A sign-extended placeholder is still named as one.
    #[test]
    fn a_sign_extended_placeholder_is_named_as_one() {
        let widened = u64::from(GuestError::Unimplemented.as_raw()) | 0xFFFF_FFFF_0000_0000;
        assert_eq!(
            super::placeholder_named(widened),
            Some(("unimplemented", true)),
            "a guest that sign-extended the refusal is still holding ours"
        );
    }
}
