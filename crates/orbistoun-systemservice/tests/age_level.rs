//! The user-service getters Unity calls before it opens a pad, as a console answered them in a fresh
//! process after `sceUserServiceInitialize` (obSCEne REQ-us01, `000-boot/unity-user-service-chain`,
//! `reports/hardware/report-1791758178.txt` lines 154-212).

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

/// For the signed-in user the age level and each accessibility setting answer 0 and write one word:
/// the age level 21, chat transcription and press-and-hold delay 0, trigger effect and vibration 1.
/// For user 0xff, or 0, the age level answers `0x80960009` and writes nothing.
#[test]
fn the_getters_answer_as_a_fresh_process_measured() {
    let mut id = 0_u32;
    assert_eq!(
        call(
            "sceUserServiceGetInitialUser",
            &[std::ptr::addr_of_mut!(id) as u64]
        ),
        0
    );
    for (name, value) in [
        ("sceUserServiceGetAgeLevel", 0x15_u32),
        ("sceUserServiceGetAccessibilityChatTranscription", 0),
        ("sceUserServiceGetAccessibilityPressAndHoldDelay", 0),
        ("sceUserServiceGetAccessibilityTriggerEffect", 1),
        ("sceUserServiceGetAccessibilityVibration", 1),
        ("sceUserServiceGetAccessibilityZoomEnabled", 0),
        ("sceUserServiceGetAccessibilityZoomFollowFocus", 1),
    ] {
        let mut out = [0xa5_u8; 0x40];
        assert_eq!(
            call(name, &[u64::from(id), out.as_mut_ptr() as u64]),
            0,
            "{name}"
        );
        assert_eq!(out[..4], value.to_le_bytes(), "{name} writes its word");
        assert!(
            out[4..].iter().all(|&b| b == 0xa5),
            "{name} writes one word"
        );
    }
    for user in [0xff, 0] {
        let mut out = [0xa5_u8; 0x40];
        assert_eq!(
            call(
                "sceUserServiceGetAgeLevel",
                &[user, out.as_mut_ptr() as u64]
            ),
            0x8096_0009
        );
        assert!(out.iter().all(|&b| b == 0xa5), "nothing written");
    }
}

/// Zoom follow-focus refuses users 0, -1 and 0x12345678 with `0x80960009`, leaving the slot as it
/// was, and a null out pointer for the signed-in user with `0x80960005` (REQ-8b13,
/// `000-boot/user-service-zoom-follow-focus`, `report-1791833977.txt` lines 724-734).
#[test]
fn zoom_follow_focus_refuses_as_measured() {
    let mut id = 0_u32;
    call(
        "sceUserServiceGetInitialUser",
        &[std::ptr::addr_of_mut!(id) as u64],
    );
    for user in [0, u64::from(u32::MAX), 0x1234_5678] {
        let mut out = [0xcc_u8; 8];
        assert_eq!(
            call(
                "sceUserServiceGetAccessibilityZoomFollowFocus",
                &[user, out.as_mut_ptr() as u64]
            ),
            0x8096_0009,
            "user {user:#x}"
        );
        assert_eq!(out, [0xcc; 8]);
    }
    assert_eq!(
        call(
            "sceUserServiceGetAccessibilityZoomFollowFocus",
            &[u64::from(id), 0]
        ),
        0x8096_0005
    );
}
