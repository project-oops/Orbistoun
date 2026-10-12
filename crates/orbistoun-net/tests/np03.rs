//! The Np calls obSCEne measured link-bound in a fresh process for PPSA02664 and PPSA03416 (REQ-np03,
//! `000-boot/np-appcontent-startup`, `reports/hardware/report-1791758178.txt` lines 223-266).

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

/// `sceNpGetAccountIdA(user, &out)` answers the console's not-signed-in refusal and zeroes the
/// eight bytes of `out`, twice alike.
#[test]
fn an_account_id_is_refused_and_zeroed() {
    for _ in 0..2 {
        let mut out = [0xa5_u8; 16];
        assert_eq!(
            call(
                orbistoun_net::npmanager::implementations(),
                "sceNpGetAccountIdA",
                &[1, out.as_mut_ptr() as u64]
            ),
            0x8055_0006
        );
        assert_eq!(out[..8], [0; 8], "the account id zeroed");
        assert_eq!(out[8..], [0xa5; 8], "nothing past it");
    }
}

/// `sceNpUniversalDataSystemCreateContext(&ctx, user, 0, 0)` answers 0 and writes a 32-bit context
/// into the low half of `ctx`, the next each time (`0x400d3` then `0x400d4` on the console), leaving
/// the high half as it was.
#[test]
fn a_uds_context_is_the_next_number() {
    let mut contexts = Vec::new();
    for _ in 0..2 {
        let mut ctx = 0xa5a5_a5a5_a5a5_a5a5_u64;
        assert_eq!(
            call(
                orbistoun_net::npuds::implementations(),
                "sceNpUniversalDataSystemCreateContext",
                &[std::ptr::addr_of_mut!(ctx) as u64, 1, 0, 0]
            ),
            0
        );
        assert_eq!(ctx >> 32, 0xa5a5_a5a5, "the high half left as it was");
        contexts.push(ctx & 0xffff_ffff);
    }
    assert_eq!(contexts[1], contexts[0] + 1, "the next each time");
}
