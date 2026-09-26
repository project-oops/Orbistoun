//! Controller HLE - `libScePad`.
//!
//! Every declared name is read out of the import table of a real module, which imports both the
//! older and newer spellings of several calls, so both are declared (D340). Names are confirmed
//! and arities are not: a wrong arity degrades a trace, a wrong name is a shim nothing reaches.
//! A pad read answers the 120-byte image obSCEne measured (`100-input/read-extent`), with the
//! state from the window or a script placed where the SDK reads it ([`pad::record`]) and the
//! at-rest image when nothing has sent any. Vendor haptics and adaptive triggers are out of
//! scope; [`pad::PadState`] and the rest of the crate model the controller for the host side.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn, Handle, HandleAllocator};
use orbistoun_hle::guest_module;

pub mod ime;
pub mod ime_dialog;
pub mod keyboard;
pub mod mouse;

pub mod latest;
pub mod mapping;
pub mod pad;
pub mod script;
pub mod shell_button;

pub use mapping::{Conflict, MAX_PORTS, Pads, Port, Push, Source};
pub use pad::{Button, PadState, Stick};
pub use shell_button::{HOLD_MS, ShellButton, ShellPress};

guest_module! {
    "libScePad" {
        // Every name here is imported by a real module; the arities are provisional.
        "scePadInit" => 0,
        "scePadOpen" => 4,
        "scePadOpenExt" => 4,
        "scePadClose" => 1,
        "scePadDisconnectDevice" => 1,
        "scePadIsValidHandle" => 1,
        "scePadSetVibration" => 2,
        "scePadSetVibrationForce" => 2,
        "scePadSetLightBar" => 2,
        // The reading functions; `scePadReadExt` is declared and not implemented.
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
/// Measured: obSCEne's `100-input/close-rejects-bad-handle` answered `0x80920003`, the pad's own
/// `NO_SUCH` in base `0x8092_0000` rather than the kernel's `0x8002_0000`.
const PAD_ERROR_BASE: u32 = 0x8092_0000;

/// Handles this shim has issued, in the pad's own allocator so a pad handle passed to a file
/// call is caught (D017).
static HANDLES: std::sync::Mutex<HandleAllocator> = std::sync::Mutex::new(HandleAllocator::new());

/// The highest handle issued so far, for recognising one back.
///
/// Handles are never recycled, so anything at or below the mark came from here and anything
/// above it was invented by the guest.
static ISSUED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Records that a handle was issued, so [`ours`] recognises it later.
///
/// `fetch_max`, because two threads opening at once can finish out of order, and a plain store
/// would move the mark below a live handle.
fn remember(handle: Handle) {
    ISSUED.fetch_max(handle.as_raw(), std::sync::atomic::Ordering::Relaxed);
}
/// Whether a raw value is a handle this shim gave out.
fn ours(raw: u64) -> Option<Handle> {
    let raw = u32::try_from(raw).ok()?;
    let handle = Handle::from_raw(raw)?;
    (raw <= ISSUED.load(std::sync::atomic::Ordering::Relaxed)).then_some(handle)
}

/// `scePadInit()` - starts the pad service.
///
/// Accepts and succeeds: nothing needs starting, and refusing would stop a title at its first
/// pad call.
fn pad_init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `scePadOpen(...)` and `scePadOpenExt(...)` - open a pad and answer a handle.
///
/// The handle is returned rather than written through an out-pointer, which is assumed. A
/// positive value reads as success and as valid; if the real function answers a status and
/// writes the handle elsewhere, the guest sees an unrecognised code and fails visibly.
fn pad_open(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // A poisoned lock is treated as ordinary: a panic on one guest thread must not turn later pad
    // calls into panics.
    let mut allocator = HANDLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(handle) = allocator.alloc() else {
        // Exhaustion is reported rather than wrapped, since reuse makes a stale handle reach the wrong
        // pad. `NoMemory` is the nearest code; what the real function answers when it runs out is not
        // known.
        return u64::from(GuestError::NoMemory.as_raw());
    };
    remember(handle);
    u64::from(handle.as_raw())
}

/// `scePadIsValidHandle(handle)` - whether a handle is one this shim gave out, rather than
/// agreeing to everything.
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
    // The number is not recycled: reuse turns a stale handle into an access to the wrong pad.
    OK
}

/// Vibration and the light bar - accepted and discarded.
///
/// The call succeeds and nothing rumbles, as with a pad with no motors; refusing would stop
/// titles that set vibration while opening a pad.
fn pad_discard(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(
            GuestError::vendor_in(PAD_ERROR_BASE, orbistoun_core::errno::NO_SUCH).as_raw(),
        );
    }
    OK
}

/// `scePadReadState(handle, into)` - what the pad is doing now.
///
/// Writes the whole 120-byte extent every call, as measured (`changed 120`), so no stale bytes
/// remain in the guest's buffer (D713).
fn pad_read_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, into) = (args[0], args[1]);
    // A scripted pad is sampled when the guest asks: `script::poll` is a pure function of run
    // progress, so the run stays repeatable with no feeder thread.
    if let Some(state) = script::poll() {
        latest::arrived(&[state]);
    }
    if ours(handle).is_none() {
        return u64::from(
            GuestError::vendor_in(PAD_ERROR_BASE, orbistoun_core::errno::NO_SUCH).as_raw(),
        );
    }
    if into == 0 {
        return u64::from(
            GuestError::vendor_in(PAD_ERROR_BASE, orbistoun_core::errno::FAULT).as_raw(),
        );
    }
    // The pad the window or a script sent, placed where the SDK reads it; the measured
    // at-rest image when nothing has sent any. Port 0 for every handle, since handles do not record
    // a port and the window sends one pad.
    let delivered = latest::delivered(0);
    // A recording keeps what the guest is handed, not what the window sent.
    if let Some(state) = &delivered {
        script::delivered(state);
    }
    let bytes = delivered.map_or(*pad::AT_REST, |state| pad::record(&state));
    // SAFETY: a guest-supplied destination, written for exactly the extent the hardware was
    // measured writing. The source is a local array of that length; guest memory and this stack
    // frame cannot overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), into as *mut u8, pad::STATE_BYTES);
    }
    OK
}

/// `scePadRead(handle, into, count)` - the same structure, asked for in a batch.
///
/// obSCEne's `100-input/batched-read` measures the same extent and contents, so it answers what
/// [`pad_read_state`] answers. `count` is not honoured: what the hardware returns for more than
/// one sample is not measured, so one sample is written and the caller is told one.
fn pad_read(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let wrote = pad_read_state(args);
    if wrote != OK {
        return wrote;
    }
    1
}

/// Implementations this crate provides, by symbol name.
///
/// `scePadReadExt` is declared but not served: nothing has measured it, and binding it to the
/// same body on its name alone is a guess.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("scePadInit", pad_init),
        // Both spellings, as for `scePadOpen`.
        ("scePadReadState", pad_read_state),
        ("scePadReadStateExt", pad_read_state),
        ("scePadRead", pad_read),
        // Both spellings, served by one function: the library exports both and nothing here tells them
        // apart.
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

    /// Recording handles out of order never lowers the mark.
    #[test]
    fn remembering_a_handle_out_of_order_never_lowers_the_mark() {
        let high = orbistoun_core::Handle::from_raw(4096).expect("a handle");
        let low = orbistoun_core::Handle::from_raw(4095).expect("a handle");
        super::remember(high);
        super::remember(low);
        assert!(
            super::ours(4096).is_some(),
            "a live handle stayed recognised after a lower one was recorded"
        );
    }
    use orbistoun_core::GUEST_ARG_REGISTERS;

    fn args(first: u64) -> [u64; GUEST_ARG_REGISTERS] {
        let mut a = [0; GUEST_ARG_REGISTERS];
        a[0] = first;
        a
    }

    /// Every implementation is also declared; a declaration with no implementation is ordinary.
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
    #[test]
    fn a_closed_handle_is_not_reissued() {
        let first = super::pad_open(&args(0));
        assert_eq!(super::pad_close(&args(first)), super::OK);
        let second = super::pad_open(&args(0));

        assert_ne!(first, second);
    }

    /// A pad read writes the 120 bytes the hardware wrote at rest, and a sent pad's state once the
    /// window sends one.
    #[test]
    fn a_pad_read_writes_the_bytes_the_console_wrote() {
        let _guard = super::latest::exclusively();
        super::latest::forget();
        let handle = call("scePadOpen", &args3(1, 0, 0));
        assert!((handle as i64) > 0, "a handle");

        let mut into = [0xAA_u8; 200];
        let rc = call(
            "scePadReadState",
            &args3(handle, into.as_mut_ptr().expose_provenance() as u64, 0),
        );
        assert_eq!(rc, 0, "the console answers 0x0 (100-input/oops-sdk-poll)");
        assert_eq!(
            &into[..super::pad::STATE_BYTES],
            super::pad::AT_REST,
            "the 120 bytes obSCEne read back off a console"
        );
        assert!(
            into[super::pad::STATE_BYTES..].iter().all(|b| *b == 0xAA),
            "and not one byte past the extent that was measured"
        );

        // Once the window sends a pad the guest reads it (D713): cross held lands on bit 14.
        let mut held = super::pad::PadState::neutral();
        held.set(super::pad::Button::South, true);
        super::latest::arrived(&[held]);
        call(
            "scePadReadState",
            &args3(handle, into.as_mut_ptr().expose_provenance() as u64, 0),
        );
        assert_eq!(u32::from_le_bytes(into[0..4].try_into().unwrap()), 1 << 14);
        super::latest::forget();
    }

    /// A batched read writes what a state read writes.
    #[test]
    fn a_batched_read_writes_what_a_state_read_writes() {
        let handle = call("scePadOpen", &args3(1, 0, 0));
        let mut state = [0_u8; super::pad::STATE_BYTES];
        let mut batched = [0_u8; super::pad::STATE_BYTES];
        call(
            "scePadReadState",
            &args3(handle, state.as_mut_ptr().expose_provenance() as u64, 0),
        );
        // The third argument is a count of entries; one, as the check asks for.
        call(
            "scePadRead",
            &args3(handle, batched.as_mut_ptr().expose_provenance() as u64, 1),
        );
        assert_eq!(state, batched, "one structure, two spellings");
    }

    /// A read through a handle nobody opened is refused, and nothing is written.
    #[test]
    fn a_read_on_a_handle_nobody_opened_writes_nothing() {
        let mut into = [0xAA_u8; super::pad::STATE_BYTES];
        let rc = call(
            "scePadReadState",
            &args3(0x7FFF, into.as_mut_ptr().expose_provenance() as u64, 0),
        );
        assert_ne!(rc, 0, "a handle this never handed out is not readable");
        assert!(
            into.iter().all(|b| *b == 0xAA),
            "and the guest's buffer is untouched"
        );
    }

    /// A null destination is refused rather than written through.
    #[test]
    fn a_read_into_nothing_is_refused() {
        let handle = call("scePadOpen", &args3(1, 0, 0));
        assert_ne!(call("scePadReadState", &args3(handle, 0, 0)), 0);
    }

    /// Three arguments, for the calls that take them.
    fn args3(a: u64, b: u64, c: u64) -> [u64; GUEST_ARG_REGISTERS] {
        let mut args = [0; GUEST_ARG_REGISTERS];
        args[0] = a;
        args[1] = b;
        args[2] = c;
        args
    }

    /// An implementation by name, so a test cannot reach one the guest cannot.
    fn call(name: &str, args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not served, so no guest can reach it"));
        function(args)
    }
}
