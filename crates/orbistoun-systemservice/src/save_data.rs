//! `libSceSaveData_native` - save data.
//!
//! `sceSaveDataInitialize3` answers as obSCEne measured it through a static import (`-d7a1`, sweep
//! 20260927-204316, check `130-layout/savedata-layout`). The other names are declared and unserved.
//!
//! The other arities are `6`, the trampoline's full capture, not a claim about how many arguments
//! a function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceSaveData_native" {
        "sceSaveDataGetSaveDataMemory2" => 6,
        "sceSaveDataInitialize3" => 1,
        "sceSaveDataSetSaveDataMemory2" => 6,
        "sceSaveDataSetupSaveDataMemory2" => 6,
        "sceSaveDataSyncSaveDataMemory" => 6,
    }
}

/// What `sceSaveDataInitialize3` answered a zeroed 64-byte parameter block, before the library was
/// initialised and after it was terminated alike (measured).
const ZEROED_PARAMETER: u64 = 0x809f_0000;

/// Bytes of the parameter block oops-sdk passes and the probe measured.
const PARAMETER_BYTES: u64 = 64;

/// Whether a null-parameter initialisation has succeeded.
static INITIALISED: AtomicBool = AtomicBool::new(false);

/// `sceSaveDataInitialize3(param)`: with a null parameter, initialises the library and answers
/// `0` - PPSA21564's call; with a zeroed 64-byte block, answers `0x809f0000` and writes nothing,
/// as measured twice before initialisation and once after termination. A second null call, and
/// any block with something in it, were not measured and are refused by name.
pub(crate) fn initialize3(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let param = args[0];
    if param == 0 {
        return if INITIALISED.swap(true, Ordering::AcqRel) {
            u64::from(GuestError::Unimplemented.as_raw())
        } else {
            0
        };
    }
    let zeroed = (0..PARAMETER_BYTES / 8).all(|word| {
        // SAFETY: the guest's parameter block, 64 bytes by the call's contract.
        (unsafe { guest::read_u64(param + word * 8) }) == Some(0)
    });
    if zeroed {
        ZEROED_PARAMETER
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}
