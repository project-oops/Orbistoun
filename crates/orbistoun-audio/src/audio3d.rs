//! `libSceAudio3d` - positional audio, as a console with no 3D audio output answers it.
//!
//! Every answer is the console's (REQ-20261020T1125Z-au01,
//! `reports/hardware/report-20261020-au01-census.log`, `000-boot/au01-audio3d`):
//! - the library initialises and terminates once each;
//! - a port open is refused, so no port exists for the calls that take one.
//!
//! A title meets the refusal at `sceAudio3dPortOpen`, where it would on that console. Shapes the
//! census did not call answer the placeholder.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use std::sync::atomic::{AtomicU8, Ordering};

guest_module! {
    "libSceAudio3d" {
        // (0), as measured.
        "sceAudio3dInitialize" => 1,
        // (port, &object).
        "sceAudio3dObjectReserve" => 2,
        // (port, object, count, &attributes).
        "sceAudio3dObjectSetAttributes" => 4,
        // (port).
        "sceAudio3dPortClose" => 1,
        // (port).
        "sceAudio3dPortFlush" => 1,
        // (user, &parameters, &port).
        "sceAudio3dPortOpen" => 3,
        // ().
        "sceAudio3dTerminate" => 0,
    }
}

/// What a second `sceAudio3dInitialize`, and a second `sceAudio3dTerminate`, answered.
const ALREADY: u64 = 0x80ea_0007;

/// What `sceAudio3dPortOpen` answered, for the signed-in user and for user 255, twice each.
const PORT_REFUSED: u64 = 0x80ea_0004;

/// What every call naming a port answered: no port had opened.
const NO_SUCH_PORT: u64 = 0x80ea_0002;

/// What `sceAudio3dObjectSetAttributes` answered with no attributes, count 0, on no port.
const NO_ATTRIBUTES: u64 = 0x80ea_0004;

/// The user `sceAudio3dPortOpen` writes `0xffffffff` into the port slot for, though it refuses.
const ALL_USERS: u64 = 255;

/// Where the library stands: never initialised, initialised, or terminated.
static STATE: AtomicU8 = AtomicU8::new(NEVER);

/// [`STATE`] before the first `sceAudio3dInitialize`.
const NEVER: u8 = 0;

/// [`STATE`] between `sceAudio3dInitialize` and `sceAudio3dTerminate`.
const LIVE: u8 = 1;

/// [`STATE`] after `sceAudio3dTerminate`.
const ENDED: u8 = 2;

/// The answer to `sceAudio3dInitialize` in `state`, and the state after it: 0 the first time,
/// [`ALREADY`] while initialised. `None` after a terminate, which the census did not call.
fn initialize_from(state: u8) -> Option<(u64, u8)> {
    match state {
        NEVER => Some((0, LIVE)),
        LIVE => Some((ALREADY, LIVE)),
        _ => None,
    }
}

/// The answer to `sceAudio3dTerminate` in `state`, and the state after it: 0 once initialised,
/// [`ALREADY`] once terminated. `None` before any initialise.
fn terminate_from(state: u8) -> Option<(u64, u8)> {
    match state {
        LIVE => Some((0, ENDED)),
        ENDED => Some((ALREADY, ENDED)),
        _ => None,
    }
}

/// The placeholder an unmeasured shape answers.
fn placeholder() -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// Applies `step` to the library's state, answering the placeholder where it answers `None`.
fn transition(step: fn(u8) -> Option<(u64, u8)>) -> u64 {
    let state = STATE.load(Ordering::Acquire);
    match step(state) {
        Some((answer, next)) => {
            STATE.store(next, Ordering::Release);
            answer
        }
        None => placeholder(),
    }
}

/// `sceAudio3dInitialize(0)`: 0, then `0x80ea0007` (`audio3d-init`).
pub(crate) fn initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] != 0 {
        return placeholder();
    }
    transition(initialize_from)
}

/// `sceAudio3dTerminate()`: 0, then `0x80ea0007` (`audio3d-terminate`).
pub(crate) fn terminate(_: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    transition(terminate_from)
}

/// `sceAudio3dPortOpen(user, &parameters, &port)`: refused with `0x80ea0004`. For user 255 the
/// console writes `0xffffffff` into the port slot's first four bytes; for the signed-in user it
/// leaves the slot as it was (`audio3d-port-open-user`, `audio3d-port-open-255`). Before
/// initialising it is unmeasured.
pub(crate) fn port_open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if STATE.load(Ordering::Acquire) != LIVE {
        return placeholder();
    }
    if args[0] == ALL_USERS {
        // SAFETY: the guest's port slot, by the call's contract; four bytes written, as the
        // console wrote, through the checked accessor.
        if !unsafe { orbistoun_mem::guest::write_u32(args[2], u32::MAX) } {
            return placeholder();
        }
    }
    PORT_REFUSED
}

/// `sceAudio3dObjectReserve(port, &object)`: `0x80ea0002`, no port being open, the slot left as
/// it was (`audio3d-obj-reserve`).
pub(crate) fn object_reserve(_: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NO_SUCH_PORT
}

/// `sceAudio3dObjectSetAttributes(port, object, count, &attributes)`: `0x80ea0004` for count 0
/// and no attributes (`audio3d-set-attr`). With attributes it is unmeasured.
pub(crate) fn object_set_attributes(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[2] == 0 && args[3] == 0 {
        NO_ATTRIBUTES
    } else {
        placeholder()
    }
}

/// `sceAudio3dPortFlush(port)`: `0x80ea0002`, no port being open (`audio3d-flush`).
pub(crate) fn port_flush(_: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NO_SUCH_PORT
}

/// `sceAudio3dPortClose(port)`: `0x80ea0002`, no port being open (`audio3d-port-close-255`).
pub(crate) fn port_close(_: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NO_SUCH_PORT
}

#[cfg(test)]
mod tests {
    use super::{ALREADY, ENDED, LIVE, NEVER, initialize_from, terminate_from};

    /// The library initialises once and terminates once; a repeat of either answers
    /// `0x80ea0007`, and an order the census did not call is unmeasured.
    #[test]
    fn the_library_initialises_and_terminates_once() {
        assert_eq!(initialize_from(NEVER), Some((0, LIVE)));
        assert_eq!(initialize_from(LIVE), Some((ALREADY, LIVE)));
        assert_eq!(terminate_from(LIVE), Some((0, ENDED)));
        assert_eq!(terminate_from(ENDED), Some((ALREADY, ENDED)));
        assert_eq!(terminate_from(NEVER), None);
        assert_eq!(initialize_from(ENDED), None);
    }
}
