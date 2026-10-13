//! The NetCtl callback, state and result calls as a console answered them (obSCEne
//! `REQ-20261012T1822Z-074b`, `000-boot/netctl-callbacks`, `report-1791836505.txt` lines 915-989).
//! One test: the callback table is the process's.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

fn implementation(name: &str) -> GuestFn {
    orbistoun_net::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| panic!("{name} is not implemented"), |(_, f)| *f)
}

fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[..args.len()].copy_from_slice(args);
    implementation(name)(&regs)
}

#[test]
fn callbacks_state_and_results_answer_as_measured() {
    let function = 0x1000_u64;
    for expected in 1..=7_u32 {
        let mut cid = 0xcccc_cccc_u32;
        let slot = std::ptr::addr_of_mut!(cid) as u64;
        assert_eq!(call("sceNetCtlRegisterCallback", &[function, 0, slot]), 0);
        assert_eq!(cid, expected, "ids count from 1");
    }
    let mut cid = 0xcccc_cccc_u32;
    let slot = std::ptr::addr_of_mut!(cid) as u64;
    assert_eq!(
        call("sceNetCtlRegisterCallback", &[function, 0, slot]),
        0x8041_2103,
        "seven slots"
    );
    assert_eq!(cid, 0xcccc_cccc, "untouched when full");
    assert_eq!(
        call("sceNetCtlRegisterCallback", &[0, 0, slot]),
        0x8041_2107
    );
    assert_eq!(
        call("sceNetCtlRegisterCallback", &[function, 0, 0]),
        0x8041_2107
    );
    assert_eq!(call("sceNetCtlCheckCallback", &[]), 0);

    assert_eq!(call("sceNetCtlUnregisterCallback", &[7]), 0);
    assert_eq!(call("sceNetCtlUnregisterCallback", &[7]), 0x8041_2104);
    assert_eq!(call("sceNetCtlUnregisterCallback", &[8]), 0x8041_2105);
    assert_eq!(
        call("sceNetCtlUnregisterCallback", &[u64::from(u32::MAX)]),
        0x8041_2105
    );

    assert_eq!(call("sceNetCtlGetState", &[0]), 0x8041_2107);
    let mut result = 0xcccc_cccc_u32;
    let at = std::ptr::addr_of_mut!(result) as u64;
    assert_eq!(call("sceNetCtlGetResult", &[2, at]), 0x8041_210f);
    assert_eq!(call("sceNetCtlGetResult", &[3, at]), 0x8041_210f);
    assert_eq!(result, 0xcccc_cccc, "untouched");
    assert_eq!(call("sceNetCtlGetResult", &[1, at]), 0);
    assert_eq!(result, 0);
    assert_eq!(call("sceNetCtlGetResult", &[1, 0]), 0x8041_2107);
}
