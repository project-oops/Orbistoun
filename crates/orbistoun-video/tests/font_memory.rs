//! `sceFontMemoryInit` and `sceFontMemoryTerm` as obSCEne measured them in PPSA21564's own call
//! shape (REQ-fm01, `reports/hardware/20261007-082134-eboot.obs.log` lines 4339-4377,
//! `130-layout/font-memory-init`). Its own test binary, because the destroy callback's record is
//! process state.

use orbistoun_core::GUEST_ARG_REGISTERS;
use std::sync::Mutex;

/// The objects the destroy callback was called with.
static DESTROYED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

extern "sysv64" fn destroy(object: u64) -> u64 {
    DESTROYED.lock().expect("record").push(object);
    0
}

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_video::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn font_memory_holds_what_it_was_given_and_term_calls_the_destroy_callback() {
    let mut object = [0xcc_u8; 0x48];
    let this = object.as_mut_ptr() as usize as u64;
    let (base, msp, interface) = (0x86_a000_u64, 0x86_a020_u64, 0x7_eeff_b7e0_u64);
    let callback = destroy as extern "sysv64" fn(u64) -> u64 as usize as u64;
    let mut args = [this, base, 0x1_0000, interface, msp, callback];

    assert_eq!(call("sceFontMemoryInit", args), 0, "`arm1-init` rc 0x0");
    let mut expected = [0_u8; 0x40];
    expected[..8].copy_from_slice(&[0x00, 0x0f, 0, 0, 0x00, 0x00, 0x01, 0x00]);
    expected[8..16].copy_from_slice(&base.to_le_bytes());
    expected[16..24].copy_from_slice(&msp.to_le_bytes());
    expected[24..32].copy_from_slice(&interface.to_le_bytes());
    expected[32..40].copy_from_slice(&callback.to_le_bytes());
    assert_eq!(object[..0x40], expected, "the measured 64 bytes");
    assert!(
        object[0x40..].iter().all(|&b| b == 0xcc),
        "nothing past them"
    );

    assert_eq!(
        call("sceFontMemoryTerm", [this, 0, 0, 0, 0, 0]),
        0,
        "`arm3b-term`"
    );
    assert_eq!(
        *DESTROYED.lock().expect("record"),
        [this],
        "`cb-calls 1`, with the object"
    );
    expected[1] = 0;
    assert_eq!(
        object[..0x40],
        expected,
        "only byte 1 cleared (`obj-term-cb 0000`)"
    );

    // A null object is refused; a size of zero is not (`arm2-null-obj`, `arm2-zero-size`).
    args[0] = 0;
    assert_eq!(call("sceFontMemoryInit", args), 0x8046_0002);
    args[0] = this;
    args[2] = 0;
    assert_eq!(call("sceFontMemoryInit", args), 0);
}
