//! `sceVideoOutGetOutputStatus` on a port opened the way a title opens one: rc 0 and the 48-byte
//! block a 1080p, 60 Hz console wrote (obSCEne `130-layout/video-out-output-status`, REQ 335a), and
//! the measured refusals for a null block and an unopened handle.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: &[u64]) -> u64 {
    let (_, handler) = orbistoun_video::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[..args.len()].copy_from_slice(args);
    handler(&regs)
}

#[test]
fn an_open_port_reports_the_measured_output() {
    let handle = call("sceVideoOutOpen", &[0xff, 0, 0, 0]);
    assert!(handle > 0 && handle < 0x8000_0000, "{handle:#x}");
    let mut status = [0xa5_u8; 0x40];
    assert_eq!(
        call(
            "sceVideoOutGetOutputStatus",
            &[handle, status.as_mut_ptr() as u64]
        ),
        0
    );
    let words: Vec<u32> = status[..48]
        .chunks(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    assert_eq!(words, [1, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(status[48..].iter().all(|&b| b == 0xa5), "48 bytes");
    assert_eq!(
        call("sceVideoOutGetOutputStatus", &[handle, 0]),
        0x8029_0002
    );
    assert_eq!(
        call(
            "sceVideoOutGetOutputStatus",
            &[0x7777, status.as_mut_ptr() as u64]
        ),
        0x8029_000b
    );
    assert_eq!(call("sceVideoOutClose", &[handle]), 0);
}
