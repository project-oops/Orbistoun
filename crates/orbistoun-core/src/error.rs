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

/// The POSIX `errno` values this project answers with, in two tiers by how each is known.
///
/// Named rather than spelled at each call site: `0x8002_0010` says nothing about what
/// happened, and `GuestError::vendor(errno::BUSY)` says what was provoked.
///
/// # The two tiers, and why there are two
///
/// **Measured** is the first group below, and its claim is the strong one: somebody watched
/// the target hand that value back. This module held nothing else until a call arrived that
/// had to report a condition nobody had provoked on hardware yet - a wait that ran out of
/// time - and the three ways to answer it were all worse than admitting the difference
/// (D476). So the second group is **published**: the value comes from the documented
/// numbering of the platform's ancestor rather than from a console, and each entry says so
/// and cites it.
///
/// What is *not* uncertain about a published entry is the encoding around it: the
/// `0x8002_0000` half was measured across seven values from five unrelated call families,
/// so the only unverified part is the small number underneath. That is a narrow gap and a
/// nameable one, which is what makes it worth carrying openly rather than hiding by
/// answering a measured-but-wrong code instead.
///
/// **A published entry is promoted, not kept.** Each names the probe that would settle it,
/// and moving it up is a one-line change once somebody runs that probe.
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
    /// Ask again. Observed from installing a second exception handler for a signal that already
    /// has one (obSCEne `030-thread/exception-handler`, sweep 20260909-140114), which answers
    /// `0x8002_0023`.
    ///
    /// **Named from the number, against the reading that came with it.** The sweep glossed that
    /// value as `EEXIST`; `EEXIST` is 17 in this platform's own harvested headers and 35 is
    /// `EAGAIN`. The measurement is the number, the gloss is not part of it, and taking the name
    /// on trust would have put a wrong constant in this module for every later call to reuse
    /// (D648).
    pub const AGAIN: u32 = 35;

    // --- published, not measured ------------------------------------------------------
    //
    // Everything below this line comes from a document rather than from a console. See the
    // module note for why they are here at all, and what promotes one.

    /// A wait gave up before it got what it was waiting for.
    ///
    /// **Published, not measured.** `ETIMEDOUT` is 60 in the documented `errno` numbering of
    /// the platform's FreeBSD ancestor, which is a citable source and not a console.
    ///
    /// Every timed call needs it and no other value will do: POSIX has
    /// `pthread_mutex_timedlock` answer `ETIMEDOUT` and never `EBUSY`, and a guest that
    /// retries on busy and gives up on timed-out would spin forever on the wrong one. That
    /// is the trade this entry exists to avoid - an unverified value is a smaller lie than a
    /// verified value that means something else.
    ///
    /// **Promoted by**: taking a lock, calling `pthread_mutex_timedlock` on it with a
    /// deadline a millisecond out, and recording what comes back. One conformance check
    /// settles it.
    pub const TIMED_OUT: u32 = 60;
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

/// The lowest placeholder value, and the base of the block they occupy.
///
/// Public because a *reader* of a fault needs it as much as a writer of a stub: an address
/// in this block is not memory, it is one of these codes being used as a pointer.
pub const PLACEHOLDER_BASE: u32 = 0x7FFF_0000;

/// Names the placeholder an address is, when it is one of orbistoun's own.
///
/// # Why a fault reporter needs this
///
/// A stub answers `0x7FFF_0001` for a function nobody has written. A guest that reads it as a
/// pointer and jumps through it faults **at that value** - and to anything checking "is this
/// address inside a region orbistoun placed", the answer is no, so the fault reads as
/// *orbistoun's own code crashing*. It is the opposite: the emulator behaved exactly as
/// designed, and the guest used a refusal as an address (D128, D154, D186, D299).
///
/// The two diagnoses send a reader to opposite places - one to debug this codebase, the other
/// to implement the function the guest asked for - so telling them apart is worth a lookup.
///
/// `Exact` where the value is a placeholder itself; `Offset` where it is inside the same block,
/// which is a guest that took one and added to it before dereferencing. The second is a weaker
/// claim and is labelled as one.
#[must_use]
pub const fn placeholder_named(address: u64) -> Option<(&'static str, bool)> {
    // Above 32 bits it cannot be one of these: they are `u32` codes, and a guest that
    // sign-extended one would land somewhere else entirely.
    if address > u32::MAX as u64 {
        return None;
    }
    let value = address as u32;
    let exact = match value {
        0x7FFF_0001 => Some("unimplemented"),
        0x7FFF_0002 => Some("invalid argument"),
        0x7FFF_0003 => Some("invalid handle"),
        0x7FFF_0004 => Some("out of memory"),
        _ => None,
    };
    if let Some(name) = exact {
        return Some((name, true));
    }
    // **Bounded above as well as below.** `>= PLACEHOLDER_BASE` alone claims every vendor
    // code too - `0x8002_0016` is a value a console answered, and naming it as orbistoun's own
    // would report a measurement as an invention. The negative test caught exactly that.
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

    /// **The value a stub answers is recognised as one**, so a fault at it is not read as a
    /// crash in orbistoun's own code.
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

    /// A value inside the block but not one of the four is reported as a weaker claim.
    ///
    /// A guest that takes a placeholder and offsets into it before dereferencing lands here,
    /// and "close to one of ours" is still far more useful than "somewhere in the emulator".
    #[test]
    fn an_offset_from_a_placeholder_is_reported_as_the_weaker_claim() {
        assert_eq!(
            super::placeholder_named(0x7FFF_0001 + 0x18),
            Some(("a placeholder answer", false))
        );
    }

    /// **An ordinary address is not one of ours**, which is the half that must not fire.
    ///
    /// A guest image sits at `0x40…` and the stack at `0x60…`; reporting either as a
    /// placeholder would replace one wrong diagnosis with another.
    #[test]
    fn an_ordinary_address_is_not_a_placeholder() {
        assert_eq!(super::placeholder_named(0x4000_0000_0000), None);
        assert_eq!(super::placeholder_named(0x6000_007f_ca68), None);
        assert_eq!(super::placeholder_named(0), None);
        assert_eq!(super::placeholder_named(0x7FFE_FFFF), None);
    }

    /// A vendor error is not a placeholder, however much it looks like a code.
    ///
    /// `0x8002_0016` is a real value a console answered; naming it as orbistoun's own would
    /// claim a measurement was an invention.
    #[test]
    fn a_vendor_code_is_not_one_of_ours() {
        assert_eq!(super::placeholder_named(0x8002_0016), None);
    }

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
