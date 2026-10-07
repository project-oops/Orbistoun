//! `sceUserServiceGetEvent` and `sceSystemServiceGetStatus` as obSCEne's census measured them
//! (`200-census/libSceUserService/sceUserServiceGetEvent`, `200-census/libSceSystemService/
//! sceSystemServiceGetStatus`, `reports/hardware/20261007-113500-eboot.obs.log`).

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

/// The signed-in user, as the title learns it.
fn user() -> u32 {
    let mut id = 0_u32;
    assert_eq!(
        call(
            "sceUserServiceGetInitialUser",
            &[std::ptr::addr_of_mut!(id) as u64]
        ),
        0
    );
    id
}

/// The first call answers the signed-in user's login: type 0, then the user, eight bytes and no
/// more. The next has no event to give, and a null destination is refused.
#[test]
fn the_user_service_gives_one_login_event() {
    let mut event = [0xa1_u8; 16];
    assert_eq!(
        call("sceUserServiceGetEvent", &[event.as_mut_ptr() as u64]),
        0
    );
    assert_eq!(event[..4], 0_u32.to_le_bytes(), "a login");
    assert_eq!(event[4..8], user().to_le_bytes(), "the signed-in user");
    assert_eq!(event[8..], [0xa1; 8], "eight bytes and no more");

    let mut again = [0xa1_u8; 16];
    assert_eq!(
        call("sceUserServiceGetEvent", &[again.as_mut_ptr() as u64]),
        0x8096_0007,
        "no event"
    );
    assert_eq!(again, [0xa1; 16], "nothing written without one");
    assert_eq!(call("sceUserServiceGetEvent", &[0]), 0x8096_0005);
}

/// The system status is 136 bytes, all zero on the measured console: no event, nothing overlaid,
/// in the foreground.
#[test]
fn the_system_status_is_136_zero_bytes() {
    let mut status = [0xa1_u8; 160];
    assert_eq!(
        call("sceSystemServiceGetStatus", &[status.as_mut_ptr() as u64]),
        0
    );
    assert_eq!(status[..136], [0; 136]);
    assert_eq!(status[136..], [0xa1; 24], "136 bytes and no more");
}
