//! One-shot set-up calls as obSCEne measured them for the signed-in user (REQ-cn10,
//! `reports/hardware/20261009-151440-eboot.obs.log`, `070-user/oneshot-setup-calls`).

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

/// Arm 6: both accessibility getters answer `0x80960002` and write none of a buffer filled `0xa5`.
/// Arm 4: the notice-screen skip flag answers 0 and writes one byte, 1.
#[test]
fn the_accessibility_getters_refuse_and_the_skip_flag_is_set() {
    let mut id = 0_u32;
    assert_eq!(
        call(
            "sceUserServiceGetInitialUser",
            &[std::ptr::addr_of_mut!(id) as u64]
        ),
        0
    );
    for name in [
        "sceUserServiceGetAccessibilityVibration",
        "sceUserServiceGetAccessibilityTriggerEffect",
    ] {
        let mut out = [0xa5_u8; 0x40];
        assert_eq!(
            call(name, &[u64::from(id), out.as_mut_ptr() as u64]),
            0x8096_0002,
            "{name}"
        );
        assert!(out.iter().all(|&b| b == 0xa5), "{name} wrote nothing");
    }
    let mut flag = [0xa5_u8; 0x40];
    assert_eq!(
        call(
            "sceSystemServiceGetNoticeScreenSkipFlag",
            &[flag.as_mut_ptr() as u64]
        ),
        0
    );
    assert_eq!(flag[0], 1);
    assert!(flag[1..].iter().all(|&b| b == 0xa5), "one byte written");
}
