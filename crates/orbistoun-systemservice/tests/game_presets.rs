//! `sceUserServiceGetGamePresets` as obSCEne measured it (`-9a3e`, sweep 20260927-223648, check
//! `130-layout/user-game-presets`).

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
fn user() -> u64 {
    let mut id = 0_u32;
    assert_eq!(
        call(
            "sceUserServiceGetInitialUser",
            &[std::ptr::addr_of_mut!(id) as u64]
        ),
        0
    );
    u64::from(id)
}

/// A 48-byte record whose first quadword is `size` and the rest `0xcc`, as the probe passed.
fn record(size: u64) -> [u8; 48] {
    let mut bytes = [0xcc_u8; 48];
    bytes[..8].copy_from_slice(&size.to_le_bytes());
    bytes
}

fn presets(user: u64, record: &mut [u8; 48]) -> u64 {
    call(
        "sceUserServiceGetGamePresets",
        &[user, record.as_mut_ptr() as u64],
    )
}

/// A 0x30-byte record for the signed-in user answers 0 and zeroes everything after its size.
#[test]
fn the_signed_in_user_gets_a_zeroed_record() {
    let mut bytes = record(0x30);
    assert_eq!(presets(user(), &mut bytes), 0);
    assert_eq!(bytes[..8], 0x30_u64.to_le_bytes());
    assert!(bytes[8..].iter().all(|&b| b == 0));
}

/// A size other than 0x30, or no record, is `0x80960005` with nothing written; user -1 or 0 is
/// `0x80960105` with nothing written (PPSA21564 asks for user 0 too).
#[test]
fn a_wrong_size_or_user_is_refused_untouched() {
    for size in [0, 0x40] {
        let mut bytes = record(size);
        assert_eq!(presets(user(), &mut bytes), 0x8096_0005, "size {size:#x}");
        assert_eq!(bytes, record(size));
    }
    assert_eq!(
        call("sceUserServiceGetGamePresets", &[user(), 0]),
        0x8096_0005
    );
    for bad in [0xffff_ffff, 0] {
        let mut bytes = record(0x30);
        assert_eq!(presets(bad, &mut bytes), 0x8096_0105, "user {bad:#x}");
        assert_eq!(bytes, record(0x30));
    }
}
