//! `libSceHttp2`'s library context, by D151: a handle the guest only compares and passes back is a
//! small integer from one. PPSA28061 starts its leaderboard client with `sceHttp2Init` and aborts on
//! anything but a context (`[LEADERBOARD SYSTEM] sceHttp2Init failed`). The id is assumed, not
//! measured (REQ-ht01 asks).

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_net::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn a_context_is_a_positive_id_that_term_retires_once() {
    let first = call("sceHttp2Init", [1, 1, 0x4_0000, 1, 0, 0]);
    let second = call("sceHttp2Init", [1, 1, 0x4_0000, 1, 0, 0]);
    for id in [first, second] {
        assert!((1..0x8000_0000).contains(&id), "a positive int: {id:#x}");
    }
    assert_ne!(first, second, "each context its own");
    assert_eq!(call("sceHttp2Term", [first, 0, 0, 0, 0, 0]), 0);
    assert_ne!(
        call("sceHttp2Term", [first, 0, 0, 0, 0, 0]),
        0,
        "a retired context is refused"
    );
    assert_eq!(call("sceHttp2Term", [second, 0, 0, 0, 0, 0]), 0);
}
