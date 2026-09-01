//! Controller HLE - libScePad.
//!
//! The one subsystem where a stub is genuinely safe for a long time: a guest that
//! reads an all-zero pad state simply sees nobody pressing anything. That makes
//! it useful early for a different reason - it is a cheap way to prove the whole
//! call path works end to end, from guest import through registry to a shim that
//! returns real data.
//!
//! Vendor-specific haptics and adaptive triggers have no general PC analogue and are
//! deliberately out of scope until something asks for them.
//!
//! # Where these names come from
//!
//! A binary in the library here imports **ninety-seven** functions from this library, and
//! every name declared below is one of them - read out of that module's own import table,
//! which is the strongest kind of confirmation available: not a name this project derived
//! and hoped matched, but one a real module demonstrably asks for.
//!
//! The library exports **both** an older and a newer spelling of several calls -
//! `scePadOpen` beside `scePadOpenExt`, `scePadReadState` beside `scePadReadStateExt`,
//! `scePadSetVibration` beside `scePadSetVibrationForce` - and that module imports both. So
//! both are declared. Guessing which of a pair a title will reach for is not a guess that
//! needs making (D340).
//!
//! # Status
//!
//! The names are confirmed. **The arities are not**, and that asymmetry is deliberate: a
//! wrong arity degrades a call trace and does not break the call, while a wrong name means
//! a NID that matches no import and a shim that can never be reached.
//!
//! [`pad::PadState`] and the rest of this crate model a controller for the *host* side. What
//! a title reads is a structure nobody here has measured, which is why the two functions
//! that write one are declared and not implemented (D326).

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn, Handle, HandleAllocator};
use orbistoun_hle::guest_module;

pub mod latest;
pub mod mapping;
pub mod pad;
pub mod shell_button;

pub use mapping::{Conflict, MAX_PORTS, Pads, Port, Push, Source};
pub use pad::{Button, PadState, Stick};
pub use shell_button::{HOLD_MS, ShellButton, ShellPress};

guest_module! {
    "libScePad" {
        // Every name here is imported by a real module in the library. The arities are
        // provisional - see the module note on why that asymmetry is allowed.
        "scePadInit" => 0,
        "scePadOpen" => 4,
        "scePadOpenExt" => 4,
        "scePadClose" => 1,
        "scePadDisconnectDevice" => 1,
        "scePadIsValidHandle" => 1,
        "scePadSetVibration" => 2,
        "scePadSetVibrationForce" => 2,
        "scePadSetLightBar" => 2,
        // Declared so a trace can name them, and deliberately not implemented: every one
        // writes a pad-state structure whose size and layout are unmeasured (D326).
        "scePadReadState" => 2,
        "scePadReadStateExt" => 2,
        "scePadRead" => 3,
        "scePadReadExt" => 3,
    }
}

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// The pad subsystem's error base, from which its codes are numbered.
///
/// **Measured.** obSCEne's `100-input/close-rejects-bad-handle` answered `0x80920003` on hardware -
/// the pad's own `NO_SUCH`, in base `0x8092_0000` rather than the kernel's `0x8002_0000`. A guest that
/// checks for `SCE_PAD_ERROR_*` never matches the kernel base or a placeholder.
const PAD_ERROR_BASE: u32 = 0x8092_0000;

/// Handles this shim has issued.
///
/// Per-subsystem rather than global, which is the reason [`HandleAllocator`] is built that
/// way: a pad handle and a file handle sharing a number space would hide the bug where a
/// guest passes one to the other.
static HANDLES: std::sync::Mutex<HandleAllocator> = std::sync::Mutex::new(HandleAllocator::new());

/// The highest handle issued so far, for recognising one back.
///
/// A high-water mark rather than a set: handles are never recycled, so anything at or below
/// the mark came from here and anything above it was invented by the guest. A set answers
/// the same question and adds state that can be forgotten.
static ISSUED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Whether a raw value is a handle this shim gave out.
fn ours(raw: u64) -> Option<Handle> {
    let raw = u32::try_from(raw).ok()?;
    let handle = Handle::from_raw(raw)?;
    (raw <= ISSUED.load(std::sync::atomic::Ordering::Relaxed)).then_some(handle)
}

/// `scePadInit()` - starts the pad service.
///
/// Accepts and succeeds. Nothing here needs starting, and refusing would stop a title at
/// the first pad call it makes - which is the one call that is certain to be reached.
fn pad_init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `scePadOpen(...)` and `scePadOpenExt(...)` - open a pad and answer a handle.
///
/// **The handle is returned rather than written through an out-pointer, and that is an
/// assumption.** It is the same one `sceUserServiceGetInitialUser` makes about its
/// identifier (D274): a positive value, which a guest checking `< 0` reads as success and a
/// guest checking against zero reads as valid. If the real function instead answers a
/// status and writes the handle elsewhere, the guest sees a small positive code it does not
/// recognise - which fails visibly rather than quietly.
fn pad_open(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // A poisoned lock is treated as ordinary, as everywhere else here: a panic on one
    // guest thread must not turn every later pad call into a panic on a different one.
    let mut allocator = HANDLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(handle) = allocator.alloc() else {
        // Exhaustion is reported rather than wrapped, for the reason the allocator gives:
        // reuse makes a stale-handle bug look like a valid access to the wrong pad. The
        // code is the nearest honest one - a resource this call needed could not be had.
        // Nothing here knows what the real function answers when it runs out.
        return u64::from(GuestError::NoMemory.as_raw());
    };
    ISSUED.store(handle.as_raw(), std::sync::atomic::Ordering::Relaxed);
    u64::from(handle.as_raw())
}

/// `scePadIsValidHandle(handle)` - whether a handle is one this shim gave out.
///
/// **Answers a real question rather than always agreeing.** A stub that said yes to
/// everything would let a guest carry a handle nothing here issued all the way to the call
/// that uses it, and the failure would surface somewhere with no connection to the mistake.
fn pad_is_valid_handle(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(ours(args[0]).is_some())
}

/// `scePadClose(handle)` and `scePadDisconnectDevice(handle)` - close a pad.
fn pad_close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(
            GuestError::vendor_in(PAD_ERROR_BASE, orbistoun_core::errno::NO_SUCH).as_raw(),
        );
    }
    // The number is not handed back to the allocator. Reuse is what turns a stale handle
    // into a silent access to the wrong pad, and running out is the better failure.
    OK
}

/// Vibration and the light bar - accepted and discarded.
///
/// The header says haptics are out of scope until something asks for them, and discarding
/// is the honest form of that: the call succeeds and nothing rumbles, which is exactly what
/// a pad with no motors does. Refusing instead would stop titles that set vibration as part
/// of opening a pad.
fn pad_discard(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(
            GuestError::vendor_in(PAD_ERROR_BASE, orbistoun_core::errno::NO_SUCH).as_raw(),
        );
    }
    OK
}

/// Implementations this crate provides, by symbol name.
///
/// The four reading functions are absent on purpose. Every one writes a structure
/// whose size and layout nothing here has measured, so the choice is between leaving them
/// unimplemented - where the guest gets a placeholder error it can act on - and writing
/// invented bytes into guest memory, which is principle 3's forbidden case with a title
/// reading the result (D326).
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("scePadInit", pad_init),
        // Both spellings, served by one function. The library exports both and the module
        // here imports both; deciding which a title "really" uses is a guess with no upside.
        ("scePadOpen", pad_open),
        ("scePadOpenExt", pad_open),
        ("scePadClose", pad_close),
        ("scePadDisconnectDevice", pad_close),
        ("scePadIsValidHandle", pad_is_valid_handle),
        ("scePadSetVibration", pad_discard),
        ("scePadSetVibrationForce", pad_discard),
        ("scePadSetLightBar", pad_discard),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    fn args(first: u64) -> [u64; GUEST_ARG_REGISTERS] {
        let mut a = [0; GUEST_ARG_REGISTERS];
        a[0] = first;
        a
    }

    /// **Every implementation is also declared.**
    ///
    /// A function served under a name the module does not declare is never reached. The
    /// reverse - a declaration with no implementation - is the ordinary case here, so only
    /// one direction is an error.
    #[test]
    fn every_implementation_is_also_declared() {
        for (name, _) in super::implementations() {
            assert!(
                super::MODULE.imports.iter().any(|i| i.name == *name),
                "{name} is served but not declared"
            );
        }
    }

    /// A handle this shim issued is recognised, and one it did not is refused.
    ///
    /// Asserted on the refusal as much as the acceptance: a check that always agreed would
    /// let a guest carry an invented handle to the call that uses it.
    #[test]
    fn a_handle_is_recognised_only_if_this_shim_issued_it() {
        let handle = super::pad_open(&args(0));
        assert!(handle > 0, "a handle is positive, so zero stays invalid");
        assert_eq!(super::pad_is_valid_handle(&args(handle)), 1);

        assert_eq!(
            super::pad_is_valid_handle(&args(0)),
            0,
            "zero is never a handle"
        );
        assert_eq!(
            super::pad_is_valid_handle(&args(handle + 10_000)),
            0,
            "a number nobody issued is not ours"
        );
    }

    /// The discarded calls still check the handle they were given.
    #[test]
    fn discarded_calls_still_check_their_handle() {
        let handle = super::pad_open(&args(0));
        assert_eq!(super::pad_discard(&args(handle)), super::OK);
        assert_eq!(super::pad_discard(&args(handle)), super::OK);

        assert_ne!(
            super::pad_discard(&args(handle + 10_000)),
            super::OK,
            "an invented handle is refused rather than quietly accepted"
        );
    }

    /// Closing a pad does not hand its number back.
    ///
    /// Recycling turns a stale handle into a valid access to the wrong pad, which is far
    /// harder to find than running out of numbers.
    #[test]
    fn a_closed_handle_is_not_reissued() {
        let first = super::pad_open(&args(0));
        assert_eq!(super::pad_close(&args(first)), super::OK);
        let second = super::pad_open(&args(0));

        assert_ne!(first, second);
    }
}
