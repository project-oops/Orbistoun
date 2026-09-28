//! `sceSaveDataInitialize3` as obSCEne measured it through a static import (`-d7a1`, sweep
//! 20260927-204316, check `130-layout/savedata-layout`). Its own test binary, since whether the
//! library is initialised is the process's.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn initialize3(param: u64) -> u64 {
    let (_, f) = orbistoun_systemservice::implementations()
        .iter()
        .find(|(n, _)| *n == "sceSaveDataInitialize3")
        .expect("sceSaveDataInitialize3 is implemented");
    let mut args = [0_u64; GUEST_ARG_REGISTERS];
    args[0] = param;
    f(&args)
}

/// A zeroed 64-byte parameter block answers `0x809f0000` and is left as it was, before the library
/// is initialised and after; a null one initialises it and answers `0` - the call PPSA21564 makes.
/// A second null call was not measured and is refused by name.
#[test]
fn a_null_parameter_initialises_and_a_zeroed_block_is_refused() {
    let mut block = [0_u8; 64];
    let at = block.as_mut_ptr() as u64;
    assert_eq!(initialize3(at), 0x809f_0000);
    assert_eq!(block, [0; 64], "nothing written");
    assert_eq!(initialize3(0), 0);
    assert_eq!(initialize3(at), 0x809f_0000);
    assert_eq!(
        initialize3(0),
        u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()),
        "a second initialisation is unmeasured"
    );
}
