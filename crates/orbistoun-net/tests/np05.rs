//! The Np calls obSCEne measured bound through their system modules (REQ-np05,
//! `000-boot/np05-routes-calls`, `reports/hardware/report-20261014-batch4.log` lines 2880-2885).

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(table: &[(&str, orbistoun_core::GuestFn)], name: &str, args: &[u64]) -> u64 {
    let (_, f) = table
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[..args.len()].copy_from_slice(args);
    f(&regs)
}

/// `sceNpGetState(user, &state)` answers 0 and writes the signed-out state, 1, as four bytes: the
/// console is signed in to no account (`state-bytes-4b` `01000000`).
#[test]
fn the_np_state_is_signed_out() {
    let mut out = [0xa5_u8; 4];
    assert_eq!(
        call(
            orbistoun_net::npmanager::implementations(),
            "sceNpGetState",
            &[1, out.as_mut_ptr() as u64]
        ),
        0
    );
    assert_eq!(out, [1, 0, 0, 0]);
}

/// `sceNpSetNpTitleId(&id, &secret)` answers 0 for PPSA04263's shape, a title id and a non-zero
/// secret, twice (REQ-gt01, `20261015-164000-eboot.log` 4841-4846), and `0x8055001e` for zeroed
/// blocks (REQ-np05, `report-20261014-batch4.log` 2885).
#[test]
fn a_title_id_is_set_as_measured() {
    let table = orbistoun_net::npmanager::implementations();
    let mut id = [0_u8; 0x10];
    id[..12].copy_from_slice(b"PPSA01721_00");
    let secret = [0x5a_u8; 0x80];
    let shape = [id.as_ptr() as u64, secret.as_ptr() as u64];
    assert_eq!(call(table, "sceNpSetNpTitleId", &shape), 0);
    assert_eq!(call(table, "sceNpSetNpTitleId", &shape), 0);
    let (zero_id, zero_secret) = ([0_u8; 0x10], [0_u8; 0x80]);
    assert_eq!(
        call(
            table,
            "sceNpSetNpTitleId",
            &[zero_id.as_ptr() as u64, zero_secret.as_ptr() as u64]
        ),
        0x8055_001e
    );
}
