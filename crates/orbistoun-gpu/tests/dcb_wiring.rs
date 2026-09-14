//! The wired command builders, driven through the table the loader actually dispatches from.
//!
//! # What this covers that `measured_builders.rs` does not
//!
//! That file proves the *encoders* produce the bytes hardware produced. This proves the other
//! half: that a handler reached through `agc::implementations()` places those bytes **into the
//! guest's own writer handle** and advances its cursor - what a title's next call needs.
//!
//! The handle layout under test (`+0x10` cursor, `+0x18` limit) was read off PPSA02664's stack and
//! then confirmed by the library's own behaviour: obSCEne built a handle to that shape, called the
//! real builders through it on hardware, and each advanced the cursor by exactly what its own
//! `GetSize` had answered (worklog 534).

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_gpu::agc;

/// `GuestError::Unimplemented` - what the overflow path answers, since it is not implemented.
const UNIMPLEMENTED: u64 = 0xf7ff_0001;
/// The measured `libSceAgc` code for a refused argument.
const BAD_ARGUMENT: u64 = 0x8a6c_000a;

/// A writer handle plus the command buffer it points at.
struct Writer {
    /// `begin`, `end`, `cur`, `limit`, `overflow_cb`, `overflow_ctx`, `reserved_dw`.
    fields: Box<[u64; 7]>,
    buffer: Box<[u8]>,
}

impl Writer {
    fn new(capacity: usize) -> Self {
        let mut buffer = vec![0u8; capacity].into_boxed_slice();
        let base = buffer.as_mut_ptr() as u64;
        let fields = Box::new([
            base,
            base + capacity as u64,
            base,
            base + capacity as u64,
            0,
            0,
            0,
        ]);
        Self { fields, buffer }
    }

    /// The address a guest would pass in `arg0`.
    fn handle(&self) -> u64 {
        self.fields.as_ptr() as u64
    }

    fn cursor(&self) -> u64 {
        self.fields[2]
    }

    /// How far the cursor has moved from the start of the buffer.
    fn written(&self) -> usize {
        (self.cursor() - self.fields[0]) as usize
    }

    fn bytes(&self) -> &[u8] {
        &self.buffer[..self.written()]
    }
}

/// Calls one builder through the dispatch table, by the name a guest imports.
fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, f) = agc::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is not wired"));
    f(&args)
}

/// The eight wired builders are all reachable by the name a guest imports.
#[test]
fn every_wired_builder_is_reachable_by_its_import_name() {
    for name in [
        "sceAgcDcbEventWrite",
        "sceAgcDcbSetIndexCount",
        "sceAgcDcbSetNumInstances",
        "sceAgcDcbDrawIndexAuto",
        "sceAgcDcbSetIndexBuffer",
        "sceAgcDcbSetCxRegisterDirect",
        "sceAgcDcbSetUcRegisterDirect",
        "sceAgcCbSetShRegisterRangeDirect",
    ] {
        assert!(
            agc::implementations().iter().any(|(n, _)| *n == name),
            "{name} is wired"
        );
    }
}

/// `sceAgcDcbEventWrite(dcb, 62, 0)` writes the measured packet and advances the cursor by 8.
#[test]
fn event_write_lands_in_the_handle_and_advances_the_cursor() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 62;

    let at = w.cursor();
    assert_eq!(
        call("sceAgcDcbEventWrite", args),
        at,
        "a builder returns the address of the packet it wrote"
    );
    assert_eq!(w.written(), 8, "the cursor advanced by the measured length");
    assert_eq!(
        w.bytes(),
        &[0x00, 0x46, 0x00, 0xc0, 0x3e, 0x00, 0x00, 0x00],
        "header 0xc0004600 then the event type, little-endian"
    );
}

/// Consecutive builders append, rather than each overwriting the last. This is the property the
/// cursor exists for, and the one a title depends on when it builds a whole command buffer.
#[test]
fn consecutive_builders_append_back_to_back() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();

    let first = w.cursor();
    args[1] = 36;
    assert_eq!(call("sceAgcDcbSetIndexCount", args), first);
    assert_eq!(w.written(), 8);

    let second = w.cursor();
    args[1] = 1;
    assert_eq!(call("sceAgcDcbSetNumInstances", args), second);
    assert_eq!(w.written(), 16, "the second packet followed the first");

    let third = w.cursor();
    args[1] = 3;
    args[2] = 2;
    assert_eq!(call("sceAgcDcbDrawIndexAuto", args), third);
    assert_eq!(w.written(), 28, "12 more for the three-dword draw");

    // The property that separates "the packet address" from "the buffer start": three calls,
    // three different returns, each 8 and then 8 bytes on from the last.
    assert_eq!(second, first + 8, "the second return is the second packet");
    assert_eq!(third, first + 16, "and the third is the third");

    assert_eq!(
        w.bytes(),
        &[
            0x00, 0x13, 0x00, 0xc0, 0x24, 0x00, 0x00, 0x00, // INDEX_BUFFER_SIZE 36
            0x00, 0x2f, 0x00, 0xc0, 0x01, 0x00, 0x00, 0x00, // NUM_INSTANCES 1
            0x00, 0x2d, 0x01, 0xc0, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
        ],
        "three measured packets, in order, nothing between them"
    );
}

/// The register-direct builders take offset and value packed into one quadword, and unpack it into
/// the packet in that order - measured in `166-agc/dcb-set-cx-reg` and `dcb-set-uc-reg`.
#[test]
fn a_packed_register_entry_unpacks_into_the_packet() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = (0x1234_5678u64 << 32) | 0x200;

    let at = w.cursor();
    assert_eq!(call("sceAgcDcbSetCxRegisterDirect", args), at);
    assert_eq!(
        w.bytes(),
        &[
            0x00, 0x69, 0x01, 0xc0, 0x00, 0x02, 0x00, 0x00, 0x78, 0x56, 0x34, 0x12
        ],
        "header, then offset 0x200, then the value"
    );
}

/// The register-range builder reads its values out of guest memory, and costs `n + 2` dwords.
#[test]
fn a_register_run_is_read_from_guest_memory() {
    let w = Writer::new(0x400);
    let values: Box<[u32]> = vec![0x1234_5678u32, 0x9abc_def0].into_boxed_slice();
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 8;
    args[2] = values.as_ptr() as u64;
    args[3] = 2;

    let at = w.cursor();
    assert_eq!(call("sceAgcCbSetShRegisterRangeDirect", args), at);
    assert_eq!(w.written(), 16, "n + 2 dwords for n = 2");
    assert_eq!(
        w.bytes(),
        &[
            0x00, 0x76, 0x02, 0xc0, 0x08, 0x00, 0x00, 0x00, 0x78, 0x56, 0x34, 0x12, 0xf0, 0xde,
            0xbc, 0x9a,
        ],
        "header, offset, then both values - and no marker in front"
    );
}

/// **A packet that will not fit is refused, and nothing is written.** The real library calls the
/// overflow callback and grows the buffer; this does not, and says so loudly rather than writing
/// past a guest's buffer.
#[test]
fn a_full_buffer_is_refused_rather_than_overrun() {
    let w = Writer::new(4); // room for a header and not one dword more
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 62;

    assert_eq!(
        call("sceAgcDcbEventWrite", args),
        UNIMPLEMENTED,
        "the overflow path is not implemented and answers the loud placeholder"
    );
    assert_eq!(w.written(), 0, "the cursor did not move");
    assert_eq!(w.buffer, vec![0u8; 4].into_boxed_slice(), "nothing written");
}

/// A null handle is refused with the measured code, not dereferenced.
#[test]
fn a_null_handle_is_refused() {
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[1] = 62;
    assert_eq!(call("sceAgcDcbEventWrite", args), BAD_ARGUMENT);
}
