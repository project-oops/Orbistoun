//! `libSceVideoRecordingP` - the low-level game-capture recording library.
//!
//! `sceVideoRecordingSetInfo` is implemented as measured: the recording types PPSA04263 passes
//! are rejected by the real export, so the call answers an invalid-argument error and touches
//! nothing. This is a separate library from `libSceVideoRecording` (no `P`), whose names a
//! guest reaches through a different module.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceVideoRecordingP" {
        // (type, info, length, ...): the trampoline's full capture; type and length are read.
        "sceVideoRecordingSetInfo" => 6,
    }
}

/// `SCE_VIDEO_RECORDING_ERROR_INVALID_ARGUMENT`, as the export answered for the recording types
/// PPSA04263 passes (REQ-gt16). The valid types seen in the export's disassembly are 0x8002,
/// 0x8003, 0xa003, 0xa004 and 0xa006; every other type branches straight to this error.
const INVALID_ARGUMENT: u64 = 0x80a8_0003;

/// The recording-info types PPSA04263 passes at its title screen (REQ-gt16), the only ones
/// measured. Both are rejected, so orbistoun answers them the measured error and leaves the
/// `info` block as it was.
const MEASURED_TYPES: [u64; 2] = [6, 2];

/// `sceVideoRecordingSetInfo(type, info, length, ...)`: `0x80a80003` for the types PPSA04263
/// passes - 6 with a 0x24-byte UTF-16 title and 2 with an 8-byte one - leaving the block
/// untouched, as the export answered both before and after an open (REQ-gt16,
/// `000-boot/gt16-share-recording`, `report-20261018-gt16-census.log` rows 3044-3053). A type
/// not measured answers the placeholder.
pub(crate) fn set_info(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if MEASURED_TYPES.contains(&args[0]) {
        INVALID_ARGUMENT
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The recording types PPSA04263 passes are rejected with invalid-argument, the block left
    /// as it was; a type not measured answers the placeholder (REQ-gt16).
    #[test]
    fn the_measured_recording_types_are_rejected() {
        let kind = |ty: u64| {
            let mut info = [0xcc_u8; 0x24];
            let mut args = [0_u64; GUEST_ARG_REGISTERS];
            args[0] = ty;
            args[1] = info.as_mut_ptr() as u64;
            args[2] = 0x24;
            let rc = super::set_info(&args);
            (rc, info.iter().all(|&b| b == 0xcc))
        };
        assert_eq!(kind(6), (0x80a8_0003, true), "type 6, block untouched");
        assert_eq!(kind(2), (0x80a8_0003, true), "type 2, block untouched");
        assert_ne!(kind(0x8002).0, 0x80a8_0003, "a type not measured");
    }
}
