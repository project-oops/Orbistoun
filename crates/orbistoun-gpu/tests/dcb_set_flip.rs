//! `sceAgcDcbSetFlip` through the dispatch table, with a display installed that knows one port
//! (D728). Its own test binary, since the installed display is the process's.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_gpu::{agc, display};

/// The port the stand-in display knows, and the buffers registered on it.
const PORT: u64 = 1;
const REGISTERED: u64 = 2;
/// The context id and label the stand-in answers: the first a process queues, and buffer
/// `index`'s label on hardware.
const CONTEXT: u32 = 0x0800_0101;

fn queue(handle: u64, index: u64, _arg: u64) -> Option<(u32, u64)> {
    (handle == PORT && index < REGISTERED).then_some((CONTEXT, 0xC_8000_40A0 + 8 * index))
}

fn released(_label: u64, _context: u32) -> bool {
    false
}

fn wait_label(handle: u64, index: u64) -> Option<u64> {
    (handle == PORT).then_some(0xC_8000_40A0 + 8 * index)
}

fn install() {
    display::install(display::Display {
        queue_flip: queue,
        released,
        wait_label,
    });
}

fn call(args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, f) = agc::implementations()
        .iter()
        .find(|(n, _)| *n == "sceAgcDcbSetFlip")
        .expect("sceAgcDcbSetFlip is wired");
    f(&args)
}

/// A writer handle (`begin`, `end`, `cur`, `limit`, ...) over a poisoned command buffer.
struct Writer {
    fields: Box<[u64; 7]>,
    buffer: Box<[u8]>,
}

impl Writer {
    fn new() -> Self {
        let mut buffer = vec![0xcd_u8; 0x400].into_boxed_slice();
        let base = buffer.as_mut_ptr() as u64;
        let fields = Box::new([base, base + 0x400, base, base + 0x400, 0, 0, 0]);
        Self { fields, buffer }
    }
    fn handle(&self) -> u64 {
        self.fields.as_ptr() as u64
    }
    fn begin(&self) -> u64 {
        self.fields[0]
    }
    fn advanced(&self) -> u64 {
        self.fields[2] - self.fields[0]
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// A flip of a registered buffer writes the measured 76 bytes, skips the rest of its 256 unwritten,
/// and answers the packet's address (obSCEne `-1d54`).
#[test]
fn a_flip_writes_the_measured_packet_and_advances_256_bytes() {
    install();
    let writer = Writer::new();
    let rc = call([writer.handle(), PORT, 1, 1, 0x1122_3344_5566_7788, 0]);
    assert_eq!(rc, writer.begin(), "the packet's address");
    assert_eq!(writer.advanced(), 0x100);
    assert_eq!(
        hex(&writer.buffer[..76]),
        concat!(
            "047902c042030000090101c700000000",
            "043704c00000010643c3000000000000",
            "8877665544332211004906c004052006",
            "00000142a84000800c00000001000000",
            "000000000101000800102cc0",
        )
    );
    assert!(
        writer.buffer[76..0x100].iter().all(|&b| b == 0xcd),
        "the NOP body is skipped, not written"
    );
}

/// An unregistered buffer and an unknown port write nothing and answer `0`, as on hardware; a flip
/// mode other than the measured one is refused.
#[test]
fn nothing_is_written_for_an_unregistered_buffer_or_port() {
    install();
    for (handle, index) in [(PORT, 4), (7, 0)] {
        let writer = Writer::new();
        assert_eq!(call([writer.handle(), handle, index, 1, 5, 0]), 0);
        assert_eq!(writer.advanced(), 0);
        assert!(writer.buffer.iter().all(|&b| b == 0xcd));
    }
    let writer = Writer::new();
    assert_eq!(call([writer.handle(), PORT, 0, 2, 5, 0]), 0xf7ff_0001);
    assert_eq!(writer.advanced(), 0);
}

fn call_wait(args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, f) = agc::implementations()
        .iter()
        .find(|(n, _)| *n == "sceAgcDcbWaitUntilSafeForRendering")
        .expect("sceAgcDcbWaitUntilSafeForRendering is wired");
    f(&args)
}

/// With a port open, the wait for buffer 4 writes the 64 bytes obSCEne dumped for index 4, skips the
/// `NOP` body, advances 128 bytes and answers the packet's address (`-5a17`, sweep 20260927-153242,
/// arm `wait-open-idx4-c1`). Index 4 is not registered on the stand-in port, as on hardware.
#[test]
fn a_wait_until_safe_writes_the_measured_packet_and_advances_128_bytes() {
    install();
    let writer = Writer::new();
    let rc = call_wait([writer.handle(), PORT, 4, 0, 0, 0]);
    assert_eq!(rc, writer.begin(), "the packet's address");
    assert_eq!(writer.advanced(), 0x80);
    assert_eq!(
        hex(&writer.buffer[..64]),
        concat!(
            "047901c042030000040000cb009307c0",
            "13010006c04000800c00000000000000",
            "00000000ffffffffffffffff40000000",
            "047901c042030000240000cb00100fc0",
        )
    );
    assert!(
        writer.buffer[64..0x80].iter().all(|&b| b == 0xcd),
        "the NOP body is skipped, not written"
    );
}

/// With no port open the wait writes nothing and answers `0` (`-5a17`, the no-port arms).
#[test]
fn a_wait_until_safe_on_no_port_writes_nothing() {
    install();
    let writer = Writer::new();
    assert_eq!(call_wait([writer.handle(), 7, 0, 0, 0, 0]), 0);
    assert_eq!(writer.advanced(), 0);
    assert!(writer.buffer.iter().all(|&b| b == 0xcd));
}
