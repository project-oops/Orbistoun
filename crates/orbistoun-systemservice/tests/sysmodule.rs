//! The sysmodule calls as a console answered them in a fresh process (obSCEne
//! `REQ-20261013T1600Z-sm41`, `000-boot/sysmodule-isloaded-lifecycle`,
//! `reports/hardware/report-20261013-172400.log`). One test: what is loaded is the process's.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, id: u64) -> u64 {
    let (_, f) = orbistoun_systemservice::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = id;
    f(&regs)
}

/// Each id's five steps: is-loaded, load, is-loaded, unload, is-loaded.
fn lifecycle(id: u64) -> [u64; 5] {
    [
        call("sceSysmoduleIsLoaded", id),
        call("sceSysmoduleLoadModule", id),
        call("sceSysmoduleIsLoaded", id),
        call("sceSysmoduleUnloadModule", id),
        call("sceSysmoduleIsLoaded", id),
    ]
}

#[test]
fn the_sysmodule_lifecycle_answers_as_measured() {
    const UNLOADED: u64 = 0x805a_1001;
    const INVALID: u64 = 0x805a_1000;
    for id in [0xb4, 0x1b, 0x1c] {
        assert_eq!(lifecycle(id), [UNLOADED, 0, 0, 0, UNLOADED], "{id:#x}");
    }
    for id in [0x07, 0x96] {
        assert_eq!(lifecycle(id), [0; 5], "{id:#x} is loaded from the start");
    }
    for id in [0x08, 0x0e, 0x15, 0x24, 0x80] {
        assert_eq!(lifecycle(id), [INVALID; 5], "{id:#x}");
    }
    assert_eq!(call("sceSysmoduleIsLoaded", 0), INVALID);
    assert_eq!(call("sceSysmoduleIsLoaded", 0x7fff), INVALID);
}
