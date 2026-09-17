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
//! [`pad::PadState`] and the rest of this crate model a controller for the *host* side. What a
//! title reads is a 120-byte image obSCEne measured on hardware (`100-input/read-extent`, sweep
//! 20260909-110725), so `pad_read_state` and its batched twin are implemented and answer the
//! whole extent every call.
//!
//! **What is still missing is input itself, and only that.** Those functions write
//! [`pad::AT_REST`] unconditionally: the *extent and contents* are measured, but which offset
//! within them carries the buttons is an inference, so mapping a live pad state onto the bytes
//! would publish that inference as though it were the measurement. [`latest`] holds what the
//! window sent and stays unread until obSCEne runs a button held down
//! (`REQ-20260910T0650Z-d1c4`, open).
//!
//! This paragraph previously said the structure was unmeasured and the functions unimplemented,
//! which stopped being true on 2026-09-09.

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

/// Records that a handle was issued, so [`ours`] recognises it later.
///
/// **`fetch_max`, not `store`.** A plain store lets the mark move *backwards*: two guest
/// threads opening at once can have the second allocate the higher number and the first write
/// its lower one afterwards, leaving a live handle above the mark and refused by every call
/// that takes it. The allocator is behind a lock and the mark was not, so the two could
/// disagree - which is a race a sequential test can never show, and which turned up as an
/// intermittent `cargo test --workspace` failure inside the gate that passed on every direct
/// re-run (D674).
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
    remember(handle);
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

/// `scePadReadState(handle, into)` - what the pad is doing now.
///
/// # Why this could be written at last
///
/// D345 left it unimplemented, and was right to: the transport carrying pad state between the
/// window and the guest was ours and testable, but the *structure a guest reads* was "a size
/// and layout nobody here has measured", and building a shim around a guessed encoding is how
/// a confident wrong answer gets made.
///
/// It has been measured since. obSCEne fills a buffer with a sentinel, calls this, and reports
/// how far the change reached: 120 bytes, all 120 written, with the full contents at rest
/// (`100-input/read-extent`, title leg of sweep 20260909-110725). So the bytes are transcribed
/// from that run - see [`pad::AT_REST`] for why they are bytes rather than a structure (D671).
///
/// **The whole extent, every call.** The measurement says `changed 120`, so a shim writing
/// only the fields it thought it understood would leave the guest's buffer holding whatever
/// was there before in the rest - which is the failure `100-input/read-extent` exists to catch.
fn pad_read_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, into) = (args[0], args[1]);
    // **A scripted pad is sampled here, and then not delivered - on purpose.**
    //
    // `script::poll` is a pure function of how long the run has been going, so sampling it at
    // the moment the guest asks keeps the run repeatable with no feeder thread to race.
    //
    // What cannot happen yet is writing it into the bytes below, because *which* byte carries a
    // button has never been measured - the extent and the at-rest contents are measured, the
    // field positions are an inference from that one image (D345, D704). Writing a guessed
    // offset is the confident wrong answer this subsystem exists to avoid.
    //
    // So it is handed to `latest`, whose arrived-versus-read counters exist for exactly this
    // gap, and the run report says how many updates never reached the guest. That turns a block
    // that was previously invisible - a script pressing buttons into a void, forever, with
    // nothing to show for it - into a counted, named one. When obSCEne's
    // `REQ-20260910T0650Z-d1c4` lands, what changes is the copy below.
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
    // SAFETY: a guest-supplied destination under the identity mapping (D014), written for
    // exactly the extent the console was measured writing and no further. The source is a
    // `'static` array of that length, and the two cannot overlap - one is guest memory and the
    // other is this binary's own read-only data.
    unsafe {
        std::ptr::copy_nonoverlapping(pad::AT_REST.as_ptr(), into as *mut u8, pad::STATE_BYTES);
    }
    OK
}

/// `scePadRead(handle, into, count)` - the same structure, asked for in a batch.
///
/// A separate import, and obSCEne measures it separately: `100-input/batched-read` reports the
/// identical extent and the identical contents. So it answers what [`pad_read_state`] answers,
/// and the test says so rather than leaving two implementations free to drift apart.
///
/// **The count is not honoured, and that is stated rather than silent.** The real call reads up
/// to `count` samples and answers how many it got; nothing has measured what a console returns
/// for more than one, and writing `count` copies of one sample would be inventing a history the
/// pad does not have. One sample, and the caller is told one.
fn pad_read(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let wrote = pad_read_state(args);
    if wrote != OK {
        return wrote;
    }
    1
}

/// Implementations this crate provides, by symbol name.
///
/// **The reading functions are here now.** They were absent on purpose: each writes a
/// structure whose size and layout nothing here had measured, so the choice was between a
/// placeholder the guest can act on and invented bytes in guest memory, which is principle 3's
/// forbidden case with a title reading the result (D326, D345).
///
/// obSCEne measured it - 120 bytes, all of them written, and the image at rest - so the third
/// option exists at last: the bytes a console produced, transcribed (D671).
///
/// `scePadReadExt` stays out. It is declared, nothing has measured it, and binding it to the
/// same body on the strength of the name alone is the guess the pair above did not have to
/// make.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("scePadInit", pad_init),
        // Both spellings again, and for the same reason as `scePadOpen`: the library exports
        // both and nothing distinguishes them from here.
        ("scePadReadState", pad_read_state),
        ("scePadReadStateExt", pad_read_state),
        ("scePadRead", pad_read),
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

    /// **Recording a handle never lowers the mark.**
    ///
    /// The bug this pins is a race and a sequential test cannot show it: two guest threads
    /// opening at once can have the second allocate the higher number and the first store its
    /// lower one afterwards, leaving a live handle above the mark and refused by every call
    /// that takes it. `ISSUED.store` did exactly that.
    ///
    /// So the *property* is tested instead of the interleaving - remember out of order, and the
    /// mark must still be the highest. Deterministic, and it fails on the old code (D674).
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

    /// **The pad state a guest reads is 120 bytes, and these are the bytes.**
    ///
    /// D345 left `scePadReadState` unimplemented on the grounds that its structure was "a
    /// size and layout nobody here has measured", and building the transport without the
    /// encoding was the right call. It has been measured since: obSCEne's
    /// `100-input/read-extent` on a title leg reports `extent 120`, `changed 120`, and the
    /// full contents at rest.
    ///
    /// Transcribed rather than modelled. What is measured is *the bytes a console produced
    /// for a pad at rest*, and that is what this writes; which offset is a button and which
    /// is a stick is an inference, and nothing here needs it to answer this call correctly.
    #[test]
    fn a_pad_read_writes_the_bytes_the_console_wrote() {
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
    }

    /// **A batched read answers the same structure**, which is the half a guess would differ on.
    ///
    /// `scePadRead` and `scePadReadState` are separate imports and obSCEne measures both:
    /// `100-input/batched-read` reports the identical `extent 120` and the identical contents.
    /// So they write the same thing here, and the test says that rather than leaving two
    /// implementations free to drift.
    #[test]
    fn a_batched_read_writes_what_a_state_read_writes() {
        let handle = call("scePadOpen", &args3(1, 0, 0));
        let mut state = [0_u8; super::pad::STATE_BYTES];
        let mut batched = [0_u8; super::pad::STATE_BYTES];
        call(
            "scePadReadState",
            &args3(handle, state.as_mut_ptr().expose_provenance() as u64, 0),
        );
        // The third argument is a count of entries; one, which is what the check asks for.
        call(
            "scePadRead",
            &args3(handle, batched.as_mut_ptr().expose_provenance() as u64, 1),
        );
        assert_eq!(state, batched, "one structure, two spellings");
    }

    /// A read through a handle nobody opened is refused, and nothing is written.
    ///
    /// The negative case. A shim that wrote the neutral state for any handle would pass both
    /// tests above and tell a guest a pad it never opened is sitting there at rest.
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
