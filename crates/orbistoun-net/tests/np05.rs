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
