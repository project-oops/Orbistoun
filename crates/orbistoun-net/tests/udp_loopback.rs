//! `sceNetSendto` and `sceNetRecvfrom` on a UDP loopback pair, against the console's answers
//! (obSCEne `REQ-20261012T1816Z-c901`, `000-boot/udp-loopback`, `report-1791836505.txt` lines
//! 992-1030). Out buffers are filled 0xa5 first, as the probe's were.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_fs::socket::{af_inet, sock_dgram, sock_stream};

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

/// `MSG_DONTWAIT` on this platform, FreeBSD's value.
const DONTWAIT: u64 = 0x80;

const MESSAGE: &[u8; 13] = b"obscene-dgram";

fn loopback(port: u16) -> [u8; 16] {
    let mut bytes = [0_u8; 16];
    bytes[0] = 16;
    bytes[1] = af_inet() as u8;
    bytes[2..4].copy_from_slice(&port.to_be_bytes());
    bytes[4..8].copy_from_slice(&[127, 0, 0, 1]);
    bytes
}

/// A sender, a receiver bound to a loopback port, and the receiver's address.
struct Pair {
    a: u64,
    b: u64,
    to: [u8; 16],
}

impl Pair {
    fn new() -> Self {
        let a = call("sceNetSocket", &[0, af_inet(), sock_dgram(), 0]);
        let b = call("sceNetSocket", &[0, af_inet(), sock_dgram(), 0]);
        assert!(a < 0x8000_0000 && b < 0x8000_0000, "{a:#x} {b:#x}");
        let any = loopback(0);
        assert_eq!(call("sceNetBind", &[b, any.as_ptr() as u64, 16]), 0);
        let name = [0_u8; 16];
        let length = 16_u32;
        let length_at = std::ptr::from_ref(&length) as u64;
        assert_eq!(
            call("sceNetGetsockname", &[b, name.as_ptr() as u64, length_at]),
            0
        );
        let to = loopback(u16::from_be_bytes([name[2], name[3]]));
        Self { a, b, to }
    }

    fn send(&self, to_at: u64, to_len: u64) -> u64 {
        call(
            "sceNetSendto",
            &[self.a, MESSAGE.as_ptr() as u64, 13, 0, to_at, to_len],
        )
    }

    fn send_to_b(&self) -> u64 {
        self.send(self.to.as_ptr() as u64, 16)
    }

    fn receive(&self, into: &mut [u8], flags: u64, from: u64, from_length: u64) -> u64 {
        call(
            "sceNetRecvfrom",
            &[
                self.b,
                into.as_mut_ptr() as u64,
                into.len() as u64,
                flags,
                from,
                from_length,
            ],
        )
    }

    fn close(self) {
        for socket in [self.a, self.b] {
            assert_eq!(call("sceNetSocketClose", &[socket]), 0);
        }
    }
}

/// A datagram arrives whole, with a 16-byte source and its length; null source pointers are
/// accepted; room for four bytes of source gets four.
#[test]
fn a_datagram_and_its_source_arrive_as_measured() {
    let pair = Pair::new();
    assert_eq!(pair.send_to_b(), 13);
    let mut buffer = [0xa5_u8; 64];
    let mut from = [0xa5_u8; 32];
    let mut from_length = 32_u32;
    let length_at = std::ptr::addr_of_mut!(from_length) as u64;
    assert_eq!(
        pair.receive(&mut buffer, 0, from.as_mut_ptr() as u64, length_at),
        13
    );
    assert_eq!(&buffer[..13], MESSAGE);
    assert!(buffer[13..].iter().all(|&byte| byte == 0xa5));
    assert_eq!(&from[..2], &[16, 2], "sin_len and family");
    assert_eq!(&from[4..16], &[127, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(from[16..].iter().all(|&byte| byte == 0xa5), "16 bytes");
    assert_eq!(from_length, 16);

    assert_eq!(pair.send_to_b(), 13);
    assert_eq!(
        pair.receive(&mut buffer, 0, 0, 0),
        13,
        "null from and fromlen"
    );

    assert_eq!(pair.send_to_b(), 13);
    let mut from = [0xa5_u8; 32];
    let mut from_length = 4_u32;
    let length_at = std::ptr::addr_of_mut!(from_length) as u64;
    assert_eq!(
        pair.receive(&mut buffer, 0, from.as_mut_ptr() as u64, length_at),
        13
    );
    assert_eq!(&from[..2], &[16, 2]);
    assert!(from[4..].iter().all(|&byte| byte == 0xa5), "four bytes");
    assert_eq!(from_length, 4);
    pair.close();
}

/// An empty socket would block; a short read takes what fits and discards the rest.
#[test]
fn a_short_read_discards_the_rest_of_the_datagram() {
    let pair = Pair::new();
    let mut buffer = [0xa5_u8; 64];
    assert_eq!(pair.receive(&mut buffer, DONTWAIT, 0, 0), 0x8041_0123);
    assert_eq!(pair.send_to_b(), 13);
    let mut short = [0xa5_u8; 4];
    assert_eq!(pair.receive(&mut short, 0, 0, 0), 4);
    assert_eq!(&short, b"obsc");
    assert_eq!(
        pair.receive(&mut buffer, DONTWAIT, 0, 0),
        0x8041_0123,
        "the tail is discarded"
    );
    pair.close();
}

/// The refusals: short or absent destinations, bad and closed descriptors, and a stream that
/// never connected.
#[test]
fn the_refusals_answer_as_measured() {
    let pair = Pair::new();
    let to = pair.to.as_ptr() as u64;
    assert_eq!(pair.send(to, 0), 0x8041_0116, "tolen 0");
    assert_eq!(pair.send(to, 8), 0x8041_0116, "tolen 8");
    assert_eq!(pair.send_to_b(), 13);
    assert_eq!(pair.send(0, 16), 0x8041_0127, "no destination");
    let none = MESSAGE.as_ptr() as u64;
    assert_eq!(call("sceNetSendto", &[pair.a, none, 0, 0, to, 16]), 0);
    assert_eq!(
        call("sceNetSendto", &[9999, none, 13, 0, to, 16]),
        0x8041_0109
    );
    let mut buffer = [0xa5_u8; 64];
    let into = buffer.as_mut_ptr() as u64;
    assert_eq!(
        call("sceNetRecvfrom", &[9999, into, 64, 0, 0, 0]),
        0x8041_0109
    );
    let stream = call("sceNetSocket", &[0, af_inet(), sock_stream(), 0]);
    assert_eq!(
        call("sceNetRecvfrom", &[stream, into, 64, 0, 0, 0]),
        0x8041_013d,
        "never connected"
    );
    assert_eq!(call("sceNetSocketClose", &[stream]), 0);
    let a = pair.a;
    pair.close();
    assert_eq!(
        call("sceNetSendto", &[a, none, 13, 0, to, 16]),
        0x8041_0109,
        "a closed socket"
    );
}
