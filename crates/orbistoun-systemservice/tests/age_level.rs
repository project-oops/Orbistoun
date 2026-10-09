//! `sceUserServiceGetAgeLevel` as obSCEne measured it (`070-user/oneshot-setup-calls`, arm 3, and
//! `200-census/libSceUserService/sceUserServiceGetAgeLevel`, `20261009-104652-eboot.obs.log`).

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: &[u64]) -> u64 {
    let (_, f) = orbistoun_systemservice::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[..args.len()].copy_from_slice(args);
    f(&regs)
}

/// For the signed-in user the call answers `0x80960002` and writes none of a 0x40-byte buffer
/// filled with `0xa5`; for user 0 it answers `0x80960009`, as the census's zero arguments did.
#[test]
fn the_age_level_answers_as_measured_and_writes_nothing() {
    let mut id = 0_u32;
    assert_eq!(
        call(
            "sceUserServiceGetInitialUser",
            &[std::ptr::addr_of_mut!(id) as u64]
        ),
        0
    );
    let mut out = [0xa5_u8; 0x40];
    assert_eq!(
        call(
            "sceUserServiceGetAgeLevel",
            &[u64::from(id), out.as_mut_ptr() as u64]
        ),
        0x8096_0002
    );
    assert!(out.iter().all(|&b| b == 0xa5), "nothing written");
    assert_eq!(
        call("sceUserServiceGetAgeLevel", &[0, out.as_mut_ptr() as u64]),
        0x8096_0009
    );
}
