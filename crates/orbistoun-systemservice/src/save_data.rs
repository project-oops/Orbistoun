//! `libSceSaveData_native` - save data.
//!
//! `sceSaveDataInitialize3` answers as obSCEne measured it through a static import (`-d7a1`, sweep
//! 20260927-204316, check `130-layout/savedata-layout`), and `sceSaveDataSetupSaveDataMemory2` as
//! `-5d20` measured it (sweep 20260928-100713, check `130-layout/savedata-memory-5d20`), and
//! `sceSaveDataGetSaveDataMemory2` before set-up as the census measured it. The other names are
//! declared and unserved.
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

/// What `sceSaveDataGetSaveDataMemory2` answered on a console with no save-data memory set up, with
/// every argument a buffer and with every argument zero, writing none of them (obSCEne census
/// `200-census/libSceSaveData_native/sceSaveDataGetSaveDataMemory2`, `20261009-151440-eboot.obs.log`).
const READ_BEFORE_SET_UP: u64 = 0x809f_0001;

/// What a read after set-up answered for the signed-in user, with nothing saved: `0x809f0000`
/// (REQ-20261009T1930Z-sd01, `20261009-215800-eboot.log` 6102 and 6107, before and after a set).
const NOTHING_SAVED: u64 = 0x809f_0000;

/// What a read after set-up answered for user 1, which is no user (line 6105).
const NOT_A_USER: u64 = 0x809f_0011;

/// What `sceSaveDataGetSaveDataMemory2` answers for `user`: before set-up `0x809f0001`; after it,
/// `0x809f0000` for the signed-in user, nothing having been saved, and `0x809f0011` for any other.
fn get_answer(set_up: bool, user: u32) -> u64 {
    if !set_up {
        READ_BEFORE_SET_UP
    } else if user == crate::signed_in_user() {
        NOTHING_SAVED
    } else {
        NOT_A_USER
    }
}

/// `sceSaveDataGetSaveDataMemory2(get)`: answers as [`get_answer`] decides for the user at `get`'s
/// first word, writing nothing - the measured read left its buffer and data block as they were.
pub(crate) fn get_save_data_memory2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's request block, its first word the user by the call's contract.
    let user = unsafe { guest::read_u32(args[0]) }.unwrap_or(u32::MAX);
    get_answer(MEMORY_SET_UP.load(Ordering::Acquire), user)
}

#[cfg(test)]
mod tests {
    use super::{initialize3, setup_save_data_memory2};

    /// Before save-data memory is set up, a read answers `0x809f0001` (obSCEne census
    /// `200-census/libSceSaveData_native/sceSaveDataGetSaveDataMemory2`); after it, `0x809f0000` for
    /// the signed-in user and `0x809f0011` for anyone else (REQ-sd01, `20261009-215800-eboot.log`
    /// 6102-6107).
    #[test]
    fn a_read_answers_as_measured_before_and_after_set_up() {
        let me = crate::signed_in_user();
        let someone_else = me.wrapping_add(1);
        assert_eq!(super::get_answer(false, me), 0x809f_0001);
        assert_eq!(super::get_answer(true, me), 0x809f_0000);
        assert_eq!(super::get_answer(true, someone_else), 0x809f_0011);
    }
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
