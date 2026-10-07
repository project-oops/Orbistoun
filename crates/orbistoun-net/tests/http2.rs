//! `libSceHttp2`'s library context, as obSCEne measured it (REQ-ht01,
//! `reports/hardware/20261007-102530-eboot.obs.log` 4410-4423, `130-layout/http2-init`): contexts
//! number from `0x30000001` up, a pool id of 0 is accepted and an SSL id of 0 refused with
//! `0x8095f006`, and a second `sceHttp2Term` on one context answers `0x817b1100`. PPSA28061 starts
//! its leaderboard client with `sceHttp2Init` and aborts on anything but a context.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_net::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn contexts_number_from_the_measured_base_and_term_retires_once() {
    let first = call("sceHttp2Init", [0xbb9, 2, 0x4_0000, 1, 0, 0]);
    let second = call("sceHttp2Init", [0xbb9, 2, 0x4_0000, 1, 0, 0]);
    assert_eq!(first, 0x3000_0001, "`arm2-init` rc");
    assert_eq!(second, 0x3000_0002, "`rc-second`");
    assert_eq!(
        call("sceHttp2Init", [0, 2, 0x4_0000, 1, 0, 0]),
        0x3000_0003,
        "`rc-pool0`: a pool id of 0 is accepted"
    );
    assert_eq!(
        call("sceHttp2Init", [0xbb9, 0, 0x4_0000, 1, 0, 0]),
        0x8095_f006,
        "`rc-ssl0`"
    );
    assert_eq!(call("sceHttp2Term", [first, 0, 0, 0, 0, 0]), 0);
    assert_eq!(
        call("sceHttp2Term", [first, 0, 0, 0, 0, 0]),
        0x817b_1100,
        "`arm4-term` `rc-second`"
    );
}
