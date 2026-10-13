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
        // Known by NID only: answers `0` and writes nothing, whatever it is passed (obSCEne
        // census, `200-census/libScePad/0xda809fad5f12799f`, sweep 20261007-113500).
        "0xda809fad5f12799f" => 0,
        "scePadOpen" => 4,
        "scePadOpenExt" => 4,
        "scePadClose" => 1,
        "scePadDisconnectDevice" => 1,
        "scePadIsValidHandle" => 1,
        "scePadSetVibration" => 2,
        "scePadSetVibrationForce" => 2,
        "scePadSetLightBar" => 2,
        // (handle) and (handle, param)
        "scePadResetLightBar" => 1,
        "scePadSetTriggerEffect" => 2,
        // (handle, info) and (handle)
        "scePadGetControllerInformation" => 2,
        // (handle, info)
        "scePadDeviceClassGetExtendedInformation" => 2,
        "scePadResetOrientation" => 1,
        // The reading functions; `scePadReadExt` is declared and not implemented.
        "scePadReadState" => 2,
        "scePadReadStateExt" => 2,
        "scePadRead" => 3,
        "scePadReadExt" => 3,
        // (user, type, index) and (privilege): both refused as measured.
        "scePadGetHandle" => 3,
        "scePadSetProcessPrivilege" => 1,
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

/// What `scePadGetHandle` and `scePadSetProcessPrivilege` answered whatever they were passed,
/// before `scePadInit` and after it (`200-census/libScePad/*`, `20261009-151440-eboot.obs.log`).
const PAD_REFUSED_TO_THIS_PROCESS: u64 = PAD_ERROR_BASE as u64 | 0x08;

/// `scePadGetHandle` and `scePadSetProcessPrivilege`: refused, as measured. oops-sdk's input
/// falls back to `scePadOpen` on the refusal, as it does on the console.
fn pad_refused_to_this_process(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    PAD_REFUSED_TO_THIS_PROCESS
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

/// What `scePadSetTriggerEffect` answers a null `param` on an open pad (REQ-pd02,
/// `000-boot/ux01-calls`, `report-20261013-172400.log` 3447): the pad's invalid-argument code.
const PAD_INVALID_ARGUMENT: u64 = PAD_ERROR_BASE as u64 | 0x01;

/// `scePadSetTriggerEffect(handle, param)`: 0 for an open handle with a parameter block, as the
/// console answered one whose first byte was 3; `0x80920001` for a null block and `0x80920003`
/// for a handle never issued (REQ-pd02). The effect is discarded: there are no adaptive triggers.
/// A never-issued handle with a null block was not measured; the handle is checked first.
fn pad_set_trigger_effect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(
            GuestError::vendor_in(PAD_ERROR_BASE, orbistoun_core::errno::NO_SUCH).as_raw(),
        );
    }
    if args[1] == 0 {
        return PAD_INVALID_ARGUMENT;
    }
    OK
}

/// The 28 bytes `scePadGetControllerInformation` wrote for the console's open pad (REQ-pu01,
/// `000-boot/pad-ime-uds`, `report-1791801495.txt` 400-405): the float 44.86 first, then the
/// half-words 1920 and 1080, two bytes of 13, a zero, two ones, and zeros to the end.
const CONTROLLER_INFORMATION: [u8; 28] = [
    0xa4, 0x70, 0x33, 0x42, 0x80, 0x07, 0x38, 0x04, 0x0d, 0x0d, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// `scePadGetControllerInformation(handle, info)`: for an open handle 0, with
/// [`CONTROLLER_INFORMATION`] written and nothing past it; for one never issued `0x80920003`
/// (REQ-pu01). A null `info` is unmeasured and answers the placeholder.
fn pad_get_controller_information(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(
            GuestError::vendor_in(PAD_ERROR_BASE, orbistoun_core::errno::NO_SUCH).as_raw(),
        );
    }
    if args[1] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's information block, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_bytes(args[1], &CONTROLLER_INFORMATION) } {
        OK
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// What `scePadDeviceClassGetExtendedInformation` wrote for the console's open pad: twenty zero
/// bytes, and nothing past them in a 0x100-byte block (REQ-ux01, `000-boot/ux01-calls`,
/// `report-1791818993.txt` 574-603).
const EXTENDED_INFORMATION_BYTES: usize = 20;

/// `scePadDeviceClassGetExtendedInformation(handle, info)`: for an open handle 0, with
/// [`EXTENDED_INFORMATION_BYTES`] zero bytes written; for one never issued `0x80920003` (REQ-ux01).
/// A null `info` is unmeasured and answers the placeholder.
fn pad_get_extended_information(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(
            GuestError::vendor_in(PAD_ERROR_BASE, orbistoun_core::errno::NO_SUCH).as_raw(),
        );
    }
    if args[1] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's information block, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_bytes(args[1], &[0; EXTENDED_INFORMATION_BYTES]) } {
        OK
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `scePadReadState(handle, into)` - what the pad is doing now.
///
/// Writes the whole 120-byte extent every call, as measured (`changed 120`), so no stale bytes
/// remain in the guest's buffer (D713).
fn pad_read_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, into) = (args[0], args[1]);
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
    // at-rest image when nothing has sent any. While the system keyboard has the controller the
    // title is handed a neutral pad, connected with nothing pressed (D826).
    let delivered = real_pad().map(|state| {
        if ime_dialog::has_focus() {
            PadState::neutral()
        } else {
            state
        }
    });
    let bytes = delivered.map_or(*pad::AT_REST, |state| pad::record(&state));
    // SAFETY: a guest-supplied destination, written for exactly the extent the hardware was
    // measured writing. The source is a local array of that length; guest memory and this stack
    // frame cannot overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), into as *mut u8, pad::STATE_BYTES);
    }
    OK
}

/// The controller as it is now: the pad the window or a script sent, port 0 for every reader,
/// since handles do not record a port and the window sends one pad. `None` when nothing has sent
/// any. A scripted pad is sampled here, when somebody asks: `script::poll` is a pure function of
/// run progress, so the run stays repeatable with no feeder thread. A recording keeps every state
/// read (D721), whether the title or the system keyboard read it (D826), so playing it back
/// presses what was pressed.
fn real_pad() -> Option<PadState> {
    if let Some(state) = script::poll() {
        latest::arrived(&[state]);
    }
    let delivered = latest::delivered(0);
    if let Some(state) = &delivered {
        script::delivered(state);
    }
    delivered
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
    static ALL: std::sync::OnceLock<Vec<(&'static str, GuestFn)>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| [OWN, keyboard::implementations()].concat())
}

/// This file's own implementations; [`implementations`] adds the keyboard's.
const OWN: &[(&str, GuestFn)] = &[
    ("sceImeDialogAbort", ime_dialog::abort),
    ("sceImeDialogGetResult", ime_dialog::get_result),
    ("sceImeDialogGetStatus", ime_dialog::get_status),
    ("sceImeDialogInit", ime_dialog::init),
    ("sceImeDialogTerm", ime_dialog::term),
    ("scePadInit", pad_init),
    ("sceImeUpdate", ime::update),
    ("sceImeKeyboardOpen", ime::keyboard_open),
    ("sceImeKeyboardClose", ime::keyboard_close),
    ("0xda809fad5f12799f", pad_census_nid),
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
    // `scePadResetLightBar(handle)`: 0 for an open handle, `0x80920003` for one never issued
    // (REQ-pd02), which is what the discard answers.
    ("scePadResetLightBar", pad_discard),
    ("scePadSetTriggerEffect", pad_set_trigger_effect),
    // `scePadResetOrientation(handle)`: 0 for an open handle, `0x80920003` for one never issued
    // (REQ-pu01), which is what the discard answers.
    ("scePadResetOrientation", pad_discard),
    (
        "scePadGetControllerInformation",
        pad_get_controller_information,
    ),
    (
        "scePadDeviceClassGetExtendedInformation",
        pad_get_extended_information,
    ),
    ("sceImeKeyboardGetResourceId", ime::keyboard_get_resource_id),
    ("scePadGetHandle", pad_refused_to_this_process),
    ("scePadSetProcessPrivilege", pad_refused_to_this_process),
];

/// libScePad's NID `0xda809fad5f12799f`, which PPSA28061 calls: answers `0` and writes nothing,
/// with every argument a buffer and with every argument zero, as obSCEne's census measured it
/// (`200-census/libScePad/0xda809fad5f12799f`, sweep 20261007-113500).
fn pad_census_nid(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    /// `scePadGetHandle(user, 0, 0)` answered `0x80920008` before `scePadInit` and after it, and
    /// `scePadSetProcessPrivilege(1)` the same, with every argument a buffer and with every
    /// argument zero (`200-census/libScePad/*`, `20261009-151440-eboot.obs.log`).
    #[test]
    fn the_pad_handle_and_privilege_calls_refuse_as_measured() {
        for name in ["scePadGetHandle", "scePadSetProcessPrivilege"] {
            let (_, f) = super::implementations()
                .iter()
                .find(|(n, _)| *n == name)
                .unwrap_or_else(|| panic!("{name} is implemented"));
            let mut args = [0_u64; GUEST_ARG_REGISTERS];
            assert_eq!(f(&args), 0x8092_0008);
            args[0] = 0x1ea2_f4d9;
            args[1] = 1;
            assert_eq!(f(&args), 0x8092_0008);
        }
    }

    /// The keyboard as REQ-ik01 measured it (`20261009-151440-eboot.obs.log`,
    /// `101-input-ext/ime-keyboard-lifecycle`): user `0xffffffff` is refused with `0x80bc0010`;
    /// a user opens once (0), a second open answers `0x80bc0001`, close answers 0 and then
    /// `0x80bc0002`; `sceImeUpdate` answers 0 while a keyboard is open and `0x80bc0002` otherwise,
    /// whatever it is passed.
    #[test]
    fn the_keyboard_opens_once_per_user_and_update_follows_it() {
        let call = |name: &str, args: [u64; GUEST_ARG_REGISTERS]| {
            let (_, f) = super::implementations()
                .iter()
                .find(|(n, _)| *n == name)
                .expect("implemented");
            f(&args)
        };
        let user = 0x1ea2_f4d9;
        assert_eq!(call("sceImeUpdate", [0; GUEST_ARG_REGISTERS]), 0x80bc_0002);
        assert_eq!(
            call("sceImeUpdate", [0xa1; GUEST_ARG_REGISTERS]),
            0x80bc_0002
        );
        assert_eq!(
            call("sceImeKeyboardOpen", [0xffff_ffff, 0, 0, 0, 0, 0]),
            0x80bc_0010
        );
        assert_eq!(
            call("sceImeKeyboardClose", [0xffff_ffff, 0, 0, 0, 0, 0]),
            0x80bc_0002
        );
        assert_eq!(call("sceImeKeyboardOpen", [user, 0, 0, 0, 0, 0]), 0);
        assert_eq!(call("sceImeUpdate", [0; GUEST_ARG_REGISTERS]), 0);
        assert_eq!(
            call("sceImeKeyboardOpen", [user, 0, 0, 0, 0, 0]),
            0x80bc_0001
        );
        assert_eq!(call("sceImeKeyboardClose", [user, 0, 0, 0, 0, 0]), 0);
        assert_eq!(
            call("sceImeKeyboardClose", [user, 0, 0, 0, 0, 0]),
            0x80bc_0002
        );
        assert_eq!(call("sceImeUpdate", [0; GUEST_ARG_REGISTERS]), 0x80bc_0002);
    }

    /// The NID-only call links to the NID the guest imports: a name in `0x` hex is taken as the NID
    /// itself, where any other spelling is hashed as a symbol name and links to nothing called.
    #[test]
    fn the_census_nid_links_to_the_nid_the_guest_imports() {
        let mut registry = orbistoun_hle::Registry::new(
            orbistoun_nid::NidHasher::new(*b"test-suffix"),
            orbistoun_hle::StubPolicy::default(),
        );
        registry.register(super::MODULE);
        assert!(
            registry
                .resolve(orbistoun_nid::Nid::from_raw(0xda80_9fad_5f12_799f))
                .is_some()
        );
    }

    /// The NID-only call answers `0` whatever it is passed, as measured.
    #[test]
    fn the_census_nid_answers_zero() {
        let (_, f) = super::implementations()
            .iter()
            .find(|(name, _)| *name == "0xda809fad5f12799f")
            .expect("implemented");
        assert_eq!(f(&[0; GUEST_ARG_REGISTERS]), 0);
        assert_eq!(f(&[0xa1; GUEST_ARG_REGISTERS]), 0);
    }

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
                super::MODULE
                    .imports
                    .iter()
                    .chain(super::ime::MODULE.imports)
                    .chain(super::ime_dialog::MODULE.imports)
                    .chain(super::keyboard::MODULE.imports)
                    .any(|i| i.name == *name),
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

    /// The light bar reset and the trigger effect answer as the console did on an open pad: 0,
    /// `0x80920003` for handle 0x7fffffff, and for a null trigger block `0x80920001` (REQ-pd02).
    #[test]
    fn the_light_bar_reset_and_trigger_effect_answer_as_measured() {
        let call = |name: &str, first: u64, second: u64| {
            let (_, f) = super::implementations()
                .iter()
                .find(|(n, _)| *n == name)
                .unwrap_or_else(|| panic!("{name} is implemented"));
            let mut args = [0_u64; GUEST_ARG_REGISTERS];
            args[0] = first;
            args[1] = second;
            f(&args)
        };
        let handle = super::pad_open(&args(0));
        let mut param = [0_u8; 0x80];
        param[0] = 3;
        let block = param.as_mut_ptr() as u64;
        assert_eq!(call("scePadResetLightBar", handle, 0), 0);
        assert_eq!(call("scePadResetLightBar", 0x7fff_ffff, 0), 0x8092_0003);
        assert_eq!(call("scePadSetTriggerEffect", handle, block), 0);
        assert_eq!(call("scePadSetTriggerEffect", handle, 0), 0x8092_0001);
        assert_eq!(
            call("scePadSetTriggerEffect", 0x7fff_ffff, block),
            0x8092_0003
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

    /// The controller information is the 28 measured bytes for an open handle, and nothing past
    /// them; an invented handle is refused with `0x80920003` and resetting the orientation answers
    /// as the handle does (REQ-pu01).
    #[test]
    fn controller_information_is_the_measured_record() {
        let handle = super::pad_open(&args(0));
        let mut info = [0xcc_u8; 0x80];
        let mut argv = args(handle);
        argv[1] = info.as_mut_ptr() as u64;
        assert_eq!(call("scePadGetControllerInformation", &argv), 0);
        assert_eq!(info[..28], super::CONTROLLER_INFORMATION);
        assert_eq!(info[..4], 44.86_f32.to_le_bytes());
        assert!(
            info[28..].iter().all(|&b| b == 0xcc),
            "28 bytes and no more"
        );
        argv[0] = handle + 10_000;
        assert_eq!(call("scePadGetControllerInformation", &argv), 0x8092_0003);
        assert_eq!(call("scePadResetOrientation", &args(handle)), 0);
        assert_eq!(
            call("scePadResetOrientation", &args(handle + 10_000)),
            0x8092_0003
        );
    }

    /// The extended information is twenty zero bytes for an open handle and nothing past them; an
    /// invented handle is refused with `0x80920003` (REQ-ux01).
    #[test]
    fn extended_information_is_twenty_zero_bytes() {
        let handle = super::pad_open(&args(0));
        let mut info = [0xcc_u8; 0x100];
        let mut argv = args(handle);
        argv[1] = info.as_mut_ptr() as u64;
        assert_eq!(call("scePadDeviceClassGetExtendedInformation", &argv), 0);
        assert!(info[..20].iter().all(|&b| b == 0));
        assert!(
            info[20..].iter().all(|&b| b == 0xcc),
            "20 bytes and no more"
        );
        argv[0] = handle + 10_000;
        assert_eq!(
            call("scePadDeviceClassGetExtendedInformation", &argv),
            0x8092_0003
        );
    }

    /// With no keyboard open, the resource id is refused with `0x80bc0002` and nothing written
    /// (REQ-pu01).
    #[test]
    fn a_keyboard_resource_id_needs_an_open_keyboard() {
        let mut out = [0xcc_u8; 32];
        let mut argv = args(0x6d3e_0001);
        argv[1] = out.as_mut_ptr() as u64;
        assert_eq!(call("sceImeKeyboardGetResourceId", &argv), 0x80bc_0002);
        assert!(out.iter().all(|&b| b == 0xcc));
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
