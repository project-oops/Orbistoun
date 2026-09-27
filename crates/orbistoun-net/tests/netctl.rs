//! `sceNetCtlGetInfo` against the answers a wired console gave (obSCEne `-5c1e`, `-9e41`, check
//! `102-net/netctl-info`, sweep 20260927-153242), with the host's own link standing in for the
//! console's.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

fn implementation(name: &str) -> GuestFn {
    orbistoun_net::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| panic!("{name} is not implemented"), |(_, f)| *f)
}

/// `sceNetCtlGetInfo(code, info)` into a 256-byte buffer filled with `0xcc`, as the probe did.
fn get_info(code: u64) -> (u32, [u8; 256]) {
    let mut info = [0xcc_u8; 256];
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = code;
    regs[1] = info.as_mut_ptr().expose_provenance() as u64;
    let rc = implementation("sceNetCtlGetInfo")(&regs) as u32;
    (rc, info)
}

/// The NUL-terminated string at the start of `info`.
fn text(info: &[u8]) -> &str {
    let end = info.iter().position(|&b| b == 0).expect("terminated");
    std::str::from_utf8(&info[..end]).expect("ASCII")
}

/// Every measured code writes the whole buffer, whatever it answers (`bytes-written 0x100`).
#[test]
fn every_measured_code_rewrites_the_whole_buffer() {
    for code in 1..=20 {
        let (_, info) = get_info(code);
        assert!(
            info.iter().all(|&b| b != 0xcc),
            "code {code} left part of the buffer unwritten"
        );
    }
}

/// A wired console answers the wireless codes 5-10 with `0x80412109` and a zeroed buffer.
#[test]
fn the_wireless_codes_are_refused_as_on_a_wired_console() {
    for code in 5..=10 {
        let (rc, info) = get_info(code);
        assert_eq!(rc, 0x8041_2109, "code {code}");
        assert!(info.iter().all(|&b| b == 0), "code {code}");
    }
}

/// The device is the wired one (`0`), and codes 11, 12, 13, 19 and 20 answer `0` with nothing in
/// the buffer, as measured.
#[test]
fn the_device_and_the_empty_codes_answer_as_measured() {
    let (rc, info) = get_info(1);
    assert_eq!((rc, &info[..4]), (0, &[0_u8; 4][..]));
    for code in [11, 12, 13, 19, 20] {
        let (rc, info) = get_info(code);
        assert_eq!(rc, 0, "code {code}");
        assert!(info.iter().all(|&b| b == 0), "code {code}");
    }
}

/// The address codes answer the host link's values as dotted strings, the form the console wrote
/// ("192.168.1.205"); with no link out they are refused rather than invented.
#[test]
fn the_address_codes_answer_the_host_link() {
    let (rc, info) = get_info(14);
    match orbistoun_net::host::link() {
        Some(link) => {
            assert_eq!(rc, 0);
            assert_eq!(text(&info), link.address.to_string());
            let (rc, info) = get_info(15);
            assert_eq!((rc, text(&info)), (0, link.netmask.to_string().as_str()));
            let (rc, info) = get_info(4);
            assert_eq!((rc, &info[..4]), (0, &1_u32.to_le_bytes()[..]), "linked");
            let (rc, info) = get_info(3);
            assert_eq!((rc, &info[..4]), (0, &link.mtu.to_le_bytes()[..]));
            let (rc, info) = get_info(2);
            assert_eq!((rc, &info[..6]), (0, &link.hardware[..]));
        }
        None => assert_ne!(rc, 0, "no link out, no address"),
    }
}

/// A code no probe asked about is refused and the buffer left alone.
#[test]
fn an_unmeasured_code_is_refused_untouched() {
    for code in [0, 21, 0x100] {
        let (rc, info) = get_info(code);
        assert_ne!(rc, 0, "code {code}");
        assert!(info.iter().all(|&b| b == 0xcc), "code {code}");
    }
}
