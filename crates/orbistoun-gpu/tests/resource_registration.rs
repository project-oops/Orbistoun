//! The resource-registration subsystem is a stub on hardware: `sceAgcDriverRegisterOwner`,
//! `RegisterResource` and `InitResourceRegistration` answer `0x8a6c9018` and write nothing
//! (measured). Its two queries the REQ-cn01 census names at tier 1 - `GetDefaultOwner` and
//! `GetResourceRegistrationMaxNameLength`, called by PPSA28061 and PPSA04263 - are taken to answer
//! the same, writing nothing. Assumed from the measured siblings.

use orbistoun_core::GUEST_ARG_REGISTERS;

#[test]
fn the_registration_queries_answer_the_subsystems_measured_refusal() {
    for name in [
        "sceAgcDriverGetDefaultOwner",
        "sceAgcDriverGetResourceRegistrationMaxNameLength",
    ] {
        let (_, handler) = orbistoun_gpu::agc_driver::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is implemented"));
        let mut out = [0xcc_u8; 0x40];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = out.as_mut_ptr() as usize as u64;
        assert_eq!(handler(&args), 0x8a6c_9018, "{name}");
        assert!(out.iter().all(|&b| b == 0xcc), "{name} writes nothing");
    }
}
