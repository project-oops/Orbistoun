//! `libSceSaveData_native` - save data.
//!
//! `sceSaveDataInitialize3` answers as obSCEne measured it through a static import (`-d7a1`, sweep
//! 20260927-204316, check `130-layout/savedata-layout`), and `sceSaveDataSetupSaveDataMemory2` as
//! `-5d20` measured it (sweep 20260928-100713, check `130-layout/savedata-memory-5d20`). The other
//! names are declared and unserved.
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

/// What a second `sceSaveDataSetupSaveDataMemory2` answered, with a null result pointer and with a
/// `0xcc`-filled one alike, writing nothing (measured).
const MEMORY_ALREADY_SET_UP: u64 = 0x809f_0000;

/// Whether save-data memory has been set up.
static MEMORY_SET_UP: AtomicBool = AtomicBool::new(false);

/// `sceSaveDataSetupSaveDataMemory2(param, result)`: sets up the title's save-data memory. Measured
/// after a null-parameter `sceSaveDataInitialize3` (`-5d20`): PPSA21564's parameter block - u32 0,
/// u32 the user, u64 `0x200000`, zeros to 64 bytes - with a null result answers `0` and leaves the
/// block unwritten; every call after that answers `0x809f0000` and writes nothing. A call before
/// initialisation, and a first call with a result pointer, whose writes were not measured, are
/// refused by name.
pub(crate) fn setup_save_data_memory2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (param, result) = (args[0], args[1]);
    if !INITIALISED.load(Ordering::Acquire) || param == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    if MEMORY_SET_UP.load(Ordering::Acquire) {
        return MEMORY_ALREADY_SET_UP;
    }
    if result != 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    if MEMORY_SET_UP.swap(true, Ordering::AcqRel) {
        MEMORY_ALREADY_SET_UP
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::{initialize3, setup_save_data_memory2};
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The first set-up after initialisation answers `0` and writes nothing; every later one answers
    /// `0x809f0000` and writes nothing, result pointer or not (obSCEne `-5d20`).
    #[test]
    fn save_data_memory_is_set_up_once_as_measured() {
        let mut param = [0u8; 64];
        param[4..8].copy_from_slice(&0x1ea2_f4d9_u32.to_le_bytes());
        param[8..16].copy_from_slice(&0x20_0000_u64.to_le_bytes());
        let before = param;
        let mut result = [0xccu8; 64];
        let at = |bytes: &mut [u8; 64]| std::ptr::from_mut(bytes).expose_provenance() as u64;
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        assert_eq!(initialize3(&args), 0, "initialised with a null parameter");
        args[0] = at(&mut param);
        assert_eq!(setup_save_data_memory2(&args), 0);
        assert_eq!(param, before, "the block is left unwritten");
        assert_eq!(setup_save_data_memory2(&args), 0x809f_0000, "a second call");
        args[1] = at(&mut result);
        assert_eq!(setup_save_data_memory2(&args), 0x809f_0000);
        assert!(
            result.iter().all(|&b| b == 0xcc),
            "nothing written through the result"
        );
    }
}
