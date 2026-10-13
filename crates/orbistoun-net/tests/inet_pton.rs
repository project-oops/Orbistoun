//! `sceNetInetPton` on every arm a console answered (obSCEne `REQ-20261012T1816Z-7af9`,
//! `000-boot/net-inet-pton`, `report-1791836505.txt` lines 837-907): a 32-byte destination filled
//! 0xa5, and the bytes after the call.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

fn implementation(name: &str) -> GuestFn {
    orbistoun_net::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| panic!("{name} is not implemented"), |(_, f)| *f)
}

fn pton(family: u64, text: &str) -> (u64, [u8; 32]) {
    let source = std::ffi::CString::new(text).expect("no interior NUL");
    let mut dst = [0xa5_u8; 32];
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[..3].copy_from_slice(&[family, source.as_ptr() as u64, dst.as_mut_ptr() as u64]);
    (implementation("sceNetInetPton")(&regs), dst)
}

fn written(bytes: &[u8]) -> [u8; 32] {
    let mut dst = [0xa5_u8; 32];
    dst[..bytes.len()].copy_from_slice(bytes);
    dst
}

#[test]
fn the_text_arms_answer_as_measured() {
    for (text, bytes) in [
        ("192.168.1.2", &[192, 168, 1, 2][..]),
        ("0.0.0.0", &[0, 0, 0, 0]),
        ("255.255.255.255", &[255, 255, 255, 255]),
        ("1.2.3.4 ", &[1, 2, 3, 4]),
    ] {
        assert_eq!(pton(2, text), (1, written(bytes)), "{text:?}");
    }
    for text in [
        "256.1.1.1",
        "1.2.3",
        "1.2.3.4.5",
        "01.2.3.4",
        " 1.2.3.4",
        "",
    ] {
        assert_eq!(pton(2, text), (0, [0xa5; 32]), "{text:?}");
    }
    let mut mapped = [0_u8; 16];
    mapped[10..].copy_from_slice(&[0xff, 0xff, 1, 2, 3, 4]);
    let mut link_local = [0_u8; 16];
    link_local[..2].copy_from_slice(&[0xfe, 0x80]);
    link_local[12..].copy_from_slice(&[0, 1, 0, 2]);
    let mut loopback = [0_u8; 16];
    loopback[15] = 1;
    let full: Vec<u8> = (1..=8_u8).flat_map(|n| [n * 0x11, n * 0x11]).collect();
    for (text, bytes) in [
        ("::1", &loopback[..]),
        ("::ffff:1.2.3.4", &mapped),
        ("fe80::1:2", &link_local),
        ("1111:2222:3333:4444:5555:6666:7777:8888", &full),
    ] {
        assert_eq!(pton(28, text), (1, written(bytes)), "{text:?}");
    }
    for text in [
        "1::2::3",
        "12345::1",
        "1111:2222:3333:4444:5555:6666:7777:8888:9999",
    ] {
        assert_eq!(pton(28, text), (0, [0xa5; 32]), "{text:?}");
    }
}

#[test]
fn the_refusals_answer_as_measured() {
    assert_eq!(pton(99, "192.168.1.1"), (0x8041_012f, [0xa5; 32]));
    assert_eq!(pton(1, "192.168.1.1"), (0x8041_012f, [0xa5; 32]));
    let call = implementation("sceNetInetPton");
    let mut dst = [0xa5_u8; 32];
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[..3].copy_from_slice(&[2, 0, dst.as_mut_ptr() as u64]);
    assert_eq!(call(&regs), 0x8041_0116, "null src");
    let source = c"192.168.1.1";
    regs[..3].copy_from_slice(&[2, source.as_ptr() as u64, 0]);
    assert_eq!(call(&regs), 1, "null dst with good text");
}
