//! `libSceVoice` - voice chat ports.
//!
//! The names come from real import tables (D504). `sceVoiceInit` is implemented, as measured in
//! the shape PPSA04263 calls it; the ports are not.

use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::guest_module;

guest_module! {
    "libSceVoice" {
        // (param, version)
        "sceVoiceInit" => 2,
    }
}

/// What a second `sceVoiceInit` answered (REQ-gt02, `20261016-000300-eboot.log` 4915).
const ALREADY_INITIALISED: u64 = 0x804e_0802;

/// Whether `sceVoiceInit` has answered 0 in this process.
static INITIALISED: AtomicBool = AtomicBool::new(false);

/// `sceVoiceInit(param, version)`: 0 the first time and `0x804e0802` after, the parameter block left
/// as it was, for PPSA04263's shape - a 0x20-byte block holding `0x10000000` at +0, version 100
/// (REQ-gt02, `000-boot/gt02-voice`, lines 4913-4916). A zeroed block was refused with `0x804e0805`
/// (REQ-gt01); any shape but the measured one, and a null block, answer the placeholder.
pub(crate) fn voice_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let placeholder = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    let mut param = [0_u8; 0x20];
    // SAFETY: the guest's parameter block, by the call's contract; read through the checked
    // accessor.
    if args[0] == 0 || !unsafe { orbistoun_mem::guest::read_into(args[0], &mut param) } {
        return placeholder;
    }
    let measured_shape = args[1] == 100
        && param[..4] == 0x1000_0000_u32.to_le_bytes()
        && param[4..].iter().all(|&b| b == 0);
    if !measured_shape {
        return placeholder;
    }
    if INITIALISED.swap(true, Ordering::Relaxed) {
        ALREADY_INITIALISED
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// PPSA04263's `sceVoiceInit(param, 100)` answers 0, then `0x804e0802`, the block left; a
    /// shape not measured answers the placeholder (REQ-gt02).
    #[test]
    fn voice_initialises_once_as_measured() {
        let mut param = [0_u8; 0x20];
        param[..4].copy_from_slice(&0x1000_0000_u32.to_le_bytes());
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = param.as_ptr() as u64;
        args[1] = 100;
        assert_eq!(super::voice_init(&args), 0);
        assert_eq!(super::voice_init(&args), 0x804e_0802);
        assert_eq!(&param[..4], &0x1000_0000_u32.to_le_bytes());
        args[1] = 1;
        assert_ne!(super::voice_init(&args), 0, "an unmeasured version");
    }
}
