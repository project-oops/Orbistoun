//! Networking HLE - the libraries a title imports for HTTP, sockets, TLS and the platform's
//! account services.
//!
//! Declaring the names makes a title that asks for the network visible in a report instead
//! of failing on an unresolved import, and lets a guest that tolerates a refused connection
//! carry on. Every name comes from a real import table (D504); unmeasured arities are `6`,
//! the trampoline's full capture, which loses no argument where a low guess would. Only
//! `socket` serves anything; the other libraries are listed in `SERVES_NOTHING`. Online
//! services are out of scope (`docs/SCOPE.md`).

pub mod http;
pub mod http2;
pub mod netctl;
pub mod npmanager;
pub mod npwebapi2;
pub mod socket;
pub mod ssl;

/// The vendor spellings this crate serves, by symbol name.
///
/// Gathered at the crate root because `orbistoun-service` collects one call per crate, so a
/// new module cannot be left unregistered.
#[must_use]
pub fn implementations() -> &'static [(&'static str, orbistoun_core::GuestFn)] {
    socket::implementations()
}

/// The base libSceNet numbers its errors from: `0x8041_0100`, not `0x8041_0000`.
///
/// A libSceNet code is `base | errno`. One obSCEne sweep read both the vendor code and
/// `__error()` for the same conditions: a would-block recv answers `0x8041_0123` with errno
/// 35 (`EAGAIN`), a recv on a listening socket `0x8041_0139` with 57 (`ENOTCONN`). A code whose
/// low byte is not the BSD errno for its condition, or one above `0x8041_01ff`, would falsify
/// the scheme. The analogous `0x8041_0000 | errno` is wrong by `0x100` on every value.
pub const NET_ERROR_BASE: u32 = 0x8041_0100;

#[cfg(test)]
mod tests {
    use super::NET_ERROR_BASE;

    /// The measured codes are this base plus the ordinary BSD errno.
    #[test]
    fn the_measured_codes_are_this_base_plus_a_bsd_errno() {
        /// `EAGAIN`, held in `__error()` after a would-block recv on a connected socket.
        const EAGAIN: u32 = 35;
        /// `ENOTCONN`, held after a recv on a listening socket.
        const ENOTCONN: u32 = 57;
        /// `EADDRINUSE`, the reading of `sceNetBind`'s refusal - corroborating, not measured.
        const EADDRINUSE: u32 = 48;

        assert_eq!(
            orbistoun_core::GuestError::vendor_in(NET_ERROR_BASE, EAGAIN).as_raw(),
            0x8041_0123,
            "the connected-recv code obSCEne read beside errno 35"
        );
        assert_eq!(
            orbistoun_core::GuestError::vendor_in(NET_ERROR_BASE, ENOTCONN).as_raw(),
            0x8041_0139,
            "the listener-recv code obSCEne read beside errno 57"
        );
        assert_eq!(
            orbistoun_core::GuestError::vendor_in(NET_ERROR_BASE, EADDRINUSE).as_raw(),
            0x8041_0130,
            "sceNetBind's refusal, whose errno was not read independently"
        );
    }

    /// The base by analogy with other subsystems, `0x8041_0000`, is not this one.
    #[test]
    fn the_plain_high_half_word_does_not_produce_the_measured_codes() {
        /// What analogy with every other subsystem would suggest.
        const BY_ANALOGY: u32 = 0x8041_0000;

        assert_ne!(
            orbistoun_core::GuestError::vendor_in(BY_ANALOGY, 35).as_raw(),
            0x8041_0123,
            "0x8041_0000 | EAGAIN is 0x80410023, which no console answered"
        );
    }
}
