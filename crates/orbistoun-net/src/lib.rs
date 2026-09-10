//! Networking HLE - the libraries a title imports and nothing answers yet.
//!
//! # Why a crate with no implementations
//!
//! A title in the corpus imports **sixty-eight** functions across seven networking
//! libraries - HTTP, sockets, TLS, and the platform's account services - and orbistoun
//! declared none of them. Every one was reported as an unresolved import or, where the
//! library id did not map, as `unknown::`, which meant a guest reaching the network could
//! not be named or counted.
//!
//! This crate is the honest home for those names: it is where an implementation would go,
//! so putting the declarations anywhere else would have to be undone later. Nothing here is
//! implemented and every library is listed in `SERVES_NOTHING` with that reason - see
//! `orbistoun-gpu`'s `agc` module for the argument in full (D504).
//!
//! # Names confirmed, arities not
//!
//! Every name is read out of a real module's import table. The arities are `6`, the
//! trampoline's full capture, which is not a claim that these take six arguments: with
//! nothing established, recording every argument register loses no information where
//! guessing low discards it.
//!
//! # What networking is not going to be
//!
//! Worth saying now, because a networking stack is the kind of thing that grows by
//! accident. `docs/SCOPE.md` puts online services out of scope; what these declarations
//! buy is that a title asking for the network is **visible in a report** rather than dying
//! on an unresolved import, and that a guest which tolerates a refused connection can carry
//! on. Answering a socket call with success it can act on is a different project.

pub mod http;
pub mod http2;
pub mod netctl;
pub mod npmanager;
pub mod npwebapi2;
pub mod socket;
pub mod ssl;

/// The vendor spellings this crate serves, by symbol name.
///
/// Only `socket` has any - the rest of the libraries here are declarations, and say so in
/// their own module notes. Gathered at the crate root because that is the shape
/// `orbistoun-service` collects: one call per crate, so a new module cannot be added and
/// silently left unregistered.
#[must_use]
pub fn implementations() -> &'static [(&'static str, orbistoun_core::GuestFn)] {
    socket::implementations()
}

/// The base libSceNet numbers its errors from: `0x8041_0100`, **not** `0x8041_0000`.
///
/// # Two spellings of one condition, in one sweep
///
/// Every other subsystem measured so far uses a plain high half-word - the kernel's
/// `0x8002_0000`, audio's `0x8026_0000`, the pad's `0x8092_0000` - and by analogy this one would
/// be `0x8041_0000`. It is not, and the low byte is not noise in the errno either.
///
/// A single obSCEne sweep exercised the same two conditions through **both** the vendor calls and
/// the POSIX ones, and recorded the vendor code beside the `__error()` value:
///
/// | condition | `sceNet*` answered | `errno` held | errno in hex |
/// |---|---|---|---|
/// | recv on a connected socket with nothing to read | `0x8041_0123` | 35 `EAGAIN` | `0x23` |
/// | recv on a listening socket | `0x8041_0139` | 57 `ENOTCONN` | `0x39` |
///
/// Both fit `base | errno` with `base = 0x8041_0100`, and the pairing is what establishes it:
/// either code alone says nothing about which byte is the errno. A third measured value,
/// `sceNetBind` refusing with `0x8041_0130`, is consistent - `0x30` is 48, `EADDRINUSE`, and the
/// POSIX `bind` in the same check had already taken the address - but its errno was not read
/// independently, so it corroborates rather than confirms.
///
/// # What would falsify it
///
/// A libSceNet code whose low byte is not a BSD errno for the condition that produced it, or one
/// above `0x8041_01ff`. Two points fit a line; nothing here has yet seen a code that could not.
/// Recorded at this strength rather than asserted as the scheme, because a reimplementation that
/// guessed `0x8041_0000 | errno` would be wrong by exactly `0x100` on every value - close enough
/// to look right in a log and never match a guest's comparison.
pub const NET_ERROR_BASE: u32 = 0x8041_0100;

#[cfg(test)]
mod tests {
    use super::NET_ERROR_BASE;

    /// **The three measured codes, reconstructed from their errnos** (D627).
    ///
    /// Asserted as failures rather than as a count: each line is a value a console actually
    /// answered, and the test is that this base plus the ordinary BSD number produces it. A
    /// change to the base that still compiled would break every one of them.
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

    /// **The obvious guess is wrong, and wrong by a whole `0x100`.**
    ///
    /// The negative case, written because a guard nobody has watched reject something is a guard
    /// nobody knows anything about. Every other subsystem's base ends in four zero digits, so
    /// this is the one somebody will "correct".
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
