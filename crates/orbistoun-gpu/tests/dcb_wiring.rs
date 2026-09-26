//! The wired command builders, driven through the table the loader dispatches from.
//!
//! `measured_builders.rs` checks that the encoders produce the bytes the hardware produced. This
//! checks that a handler reached through `agc::implementations()` places those bytes into the
//! guest's own writer handle (`+0x10` cursor, `+0x18` limit) and advances its cursor. The handle
//! layout is guest-observed and confirmed on hardware: builders called through a handle of this
//! shape advance the cursor by exactly their own `GetSize` answer.

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

/// The wired set has exactly this size, so a change to it is deliberate.
///
/// Counting the built slice rather than the source text cannot be fooled by formatting, and
/// `implementations()` is the whole wired set, builders and queries (such as
/// `sceAgcGetIsTrinityMode`) alike.
#[test]
fn the_wired_set_is_the_size_the_module_documentation_claims() {
    assert_eq!(
        agc::implementations().len(),
        50,
        concat!(
            "the wired builder count changed - update the count in the agc.rs module ",
            "documentation to match, then update this number"
        )
    );
}

/// Every wired builder is reachable by the name a guest imports.
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
        // DrawIndex, measured in full, and SetIndexSize, measured across eight argument pairs.
        "sceAgcDcbDrawIndex",
        "sceAgcDcbSetIndexSize",
    ] {
        assert!(
            agc::implementations().iter().any(|(n, _)| *n == name),
            "{name} is wired"
        );
    }
}

/// `sceAgcCbNop` writes the measured header-only no-op and advances the cursor by four.
#[test]
fn cb_nop_writes_the_measured_header_only_packet() {
    // Fully measured: `166-agc/cb-nop` says the whole packet is `0xffff1000`, four bytes, no
    // arguments.
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();

    let at = w.cursor();
    assert_eq!(call("sceAgcCbNop", args), at, "returns the packet address");
    assert_eq!(w.written(), 4, "a header-only packet is four bytes");
    assert_eq!(
        w.bytes(),
        &[0x00, 0x10, 0xff, 0xff],
        "the measured no-op word 0xffff1000, little-endian"
    );
}

/// `sceAgcDcbAcquireMem` reserves the measured 32-byte extent with the measured header and a zero
/// body (D696): the header present, the length right, and no argument in the body.
#[test]
fn dcb_acquire_mem_reserves_the_measured_extent_with_a_zero_body() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 0x1111_1111; // an argument that must not appear in the packet

    let at = w.cursor();
    assert_eq!(
        call("sceAgcDcbAcquireMem", args),
        at,
        "returns the packet address"
    );
    assert_eq!(w.written(), 32, "the measured ACQUIRE_MEM extent");
    assert_eq!(
        &w.bytes()[..4],
        &[0x00, 0x58, 0x06, 0xc0],
        "the measured header 0xc0065800, little-endian"
    );
    assert!(
        w.bytes()[4..].iter().all(|b| *b == 0),
        "the body is zero, not a guessed encoding of the argument"
    );
}

/// The three skeletons measured by header and extent each reserve their measured length with the
/// measured header and a zero body (D696). The argument in `arg1` must not leak into the body.
#[test]
fn the_measured_skeletons_reserve_their_extent_with_a_zero_body() {
    for (name, extent, header) in [
        ("sceAgcCbReleaseMem", 32usize, [0x00, 0x49, 0x06, 0xc0]),
        ("sceAgcDcbDmaData", 28, [0x00, 0x50, 0x05, 0xc0]),
        ("sceAgcDcbSetBaseIndirectArgs", 16, [0x00, 0x11, 0x02, 0xc0]),
    ] {
        let w = Writer::new(0x400);
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = w.handle();
        args[1] = 0x1111_1111; // an argument that must not appear in the packet
        args[2] = 0x2222_2222;

        let at = w.cursor();
        assert_eq!(call(name, args), at, "{name} returns the packet address");
        assert_eq!(w.written(), extent, "{name} reserves its measured extent");
        assert_eq!(
            &w.bytes()[..4],
            &header,
            "{name} writes its measured header, little-endian"
        );
        assert!(
            w.bytes()[4..].iter().all(|b| *b == 0),
            "{name} leaves the body zero, not a guessed encoding of the argument"
        );
    }
}

/// A skeleton refuses a null handle with the measured code, like every other builder.
#[test]
fn a_skeleton_refuses_a_null_handle() {
    let args = [0u64; GUEST_ARG_REGISTERS];
    assert_eq!(call("sceAgcDcbDmaData", args), BAD_ARGUMENT);
}

/// The markers reserve their measured 12-byte header (`0xc0017904`, the reserved-byte bit kept),
/// and `WaitRegMem` reserves its measured 56-byte compound with all three packet headers present
/// and bodies zero.
#[test]
fn the_markers_and_wait_reg_mem_reserve_their_measured_shapes() {
    // Markers: 12 bytes, the measured header with its reserved-byte bit, body zero.
    for name in ["sceAgcDcbPushMarker", "sceAgcDcbPopMarker"] {
        let w = Writer::new(0x400);
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = w.handle();
        args[1] = 0x1111_1111;
        let at = w.cursor();
        assert_eq!(call(name, args), at, "{name} returns the packet address");
        assert_eq!(
            w.written(),
            12,
            "{name} reserves the measured 12-byte marker"
        );
        assert_eq!(
            &w.bytes()[..4],
            &0xc001_7904u32.to_le_bytes(),
            "{name} writes the measured header 0xc0017904, reserved bit and all"
        );
        assert!(
            w.bytes()[4..].iter().all(|b| *b == 0),
            "{name} leaves the body zero"
        );
    }
    // WaitRegMem: 56 bytes, three packet headers, bodies zero.
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    let at = w.cursor();
    assert_eq!(call("sceAgcDcbWaitRegMem", args), at);
    assert_eq!(w.written(), 56, "the measured compound extent");
    assert_eq!(
        &w.bytes()[0..4],
        &0xc002_7904u32.to_le_bytes(),
        "SET_UCONFIG"
    );
    assert_eq!(
        &w.bytes()[16..20],
        &0xc005_3c00u32.to_le_bytes(),
        "WAIT_REG_MEM at offset 16"
    );
    assert_eq!(
        &w.bytes()[44..48],
        &0xc001_7904u32.to_le_bytes(),
        "the closing SET_UCONFIG at offset 44"
    );
}

/// `sceAgcDcbResetQueue` reserves its measured 32-byte stream: a NOP filler, then two
/// `SET_UCONFIG_REG` headers, all bodies zero, with the argument in `arg1` kept out.
#[test]
fn dcb_reset_queue_reserves_its_measured_writer_struct_with_a_zero_body() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 0x1111_1111; // must not appear in the packet

    let at = w.cursor();
    assert_eq!(
        call("sceAgcDcbResetQueue", args),
        at,
        "returns the packet address"
    );
    assert_eq!(w.written(), 32, "the measured 32-byte writer-struct");
    assert_eq!(
        &w.bytes()[0..4],
        &0xffff_1000u32.to_le_bytes(),
        "the NOP filler leads the struct, as sceAgcCbNop emits it"
    );
    assert_eq!(
        &w.bytes()[4..8],
        &0xc002_7904u32.to_le_bytes(),
        "the first SET_UCONFIG_REG header, count 2"
    );
    assert_eq!(
        &w.bytes()[20..24],
        &0xc001_7904u32.to_le_bytes(),
        "the second SET_UCONFIG_REG header, count 1, at offset 20"
    );
    assert!(
        w.bytes()[8..20].iter().all(|b| *b == 0) && w.bytes()[24..32].iter().all(|b| *b == 0),
        "both bodies are zero, not a guessed encoding of the marker values"
    );
}

/// Each reservation skeleton reserves its measured extent with its measured header and a zero body.
/// The header is asserted little-endian so a wrong opcode or count cannot pass, and the argument in
/// `arg1` must not leak into the body.
#[test]
fn the_a70f_cluster_reserves_its_measured_headers_and_extents() {
    for (name, extent, header) in [
        ("sceAgcCbDispatch", 20usize, 0xc003_1500u32),
        ("sceAgcDcbDispatchIndirect", 12, 0xc001_1600),
        ("sceAgcAcbDispatchIndirect", 16, 0xc002_1600),
        ("sceAgcDcbDrawIndirect", 20, 0xc003_2400),
        ("sceAgcDcbDrawIndexIndirect", 20, 0xc003_2500),
        ("sceAgcDcbSetShRegistersIndirect", 20, 0xc003_6300),
        ("sceAgcDcbSetUcRegistersIndirect", 20, 0xc003_6400),
        ("sceAgcDcbStallCommandBufferParser", 8, 0xc000_4200),
        ("sceAgcAcbAcquireMem", 32, 0xc006_5800),
    ] {
        let w = Writer::new(0x400);
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = w.handle();
        args[1] = 0x1111_1111; // must not appear in the packet

        let at = w.cursor();
        assert_eq!(call(name, args), at, "{name} returns the packet address");
        assert_eq!(w.written(), extent, "{name} reserves its measured extent");
        assert_eq!(
            &w.bytes()[..4],
            &header.to_le_bytes(),
            "{name} writes its measured header, little-endian"
        );
        assert!(
            w.bytes()[4..].iter().all(|b| *b == 0),
            "{name} leaves the body zero, not a guessed encoding of the argument"
        );
    }
}

/// The whole `sceAgc*Patch*` family answers the measured `0x0`, not a placeholder. They are
/// returns, not builders: no packet is appended and no writer touched.
#[test]
fn every_patch_answers_the_measured_success_not_a_placeholder() {
    for name in [
        "sceAgcSetCxRegIndirectPatchAddRegisters",
        "sceAgcSetCxRegIndirectPatchSetAddress",
        "sceAgcSetShRegIndirectPatchAddRegisters",
        "sceAgcSetShRegIndirectPatchSetAddress",
        "sceAgcSetUcRegIndirectPatchAddRegisters",
        "sceAgcSetUcRegIndirectPatchSetAddress",
        "sceAgcDmaDataPatchSetDstAddressOrOffset",
        "sceAgcDmaDataPatchSetSrcAddressOrOffsetOrImmediate",
        "sceAgcWaitRegMemPatchAddress",
        "sceAgcQueueEndOfPipeActionPatchAddress",
    ] {
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = 0x7400_0224_7d9c; // a real packet address, as the producer skeletons return
        let rc = call(name, args);
        assert_eq!(rc, 0, "{name} answers the measured 0x0");
        assert_ne!(
            rc, UNIMPLEMENTED,
            "{name} specifically not the placeholder the guest reads as a pointer"
        );
    }
}

/// `sceAgcDcbWaitUntilSafeForRendering` is a measured library no-op: it answers `0x0` and writes
/// nothing, on a real writer or none.
#[test]
fn wait_until_safe_for_rendering_is_a_no_op_that_answers_zero() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    let rc = call("sceAgcDcbWaitUntilSafeForRendering", args);
    assert_eq!(rc, 0, "the measured 0x0");
    assert_ne!(rc, UNIMPLEMENTED, "not the placeholder");
    assert_eq!(w.written(), 0, "a no-op writes no packet");

    // A null handle is still 0x0: nothing is dereferenced.
    assert_eq!(
        call(
            "sceAgcDcbWaitUntilSafeForRendering",
            [0u64; GUEST_ARG_REGISTERS]
        ),
        0
    );
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

/// Consecutive builders append rather than overwrite: the property a title depends on when it
/// builds a whole command buffer.
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

    // Three calls, three returns, each 8 bytes on from the last: the return is the packet's
    // address, not the buffer start.
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

/// A packet that does not fit is refused and nothing is written. The real library calls the
/// overflow callback; this answers the placeholder instead of writing past the buffer.
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

/// `sceAgcDcbSetCxRegistersIndirect` writes the 20-byte skeleton with the confirmed format word.
#[test]
fn set_cx_registers_indirect_writes_measured_header_and_format() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();

    let at = w.cursor();
    assert_eq!(call("sceAgcDcbSetCxRegistersIndirect", args), at);
    assert_eq!(w.written(), 20, "5 dwords / 20 bytes");
    assert_eq!(
        w.bytes(),
        &[
            0x00, 0x9f, 0x03, 0xc0, // dw0: header (opcode 0x9f, count 4)
            0x00, 0x00, 0x00, 0x00, // dw1: mem_lo
            0x00, 0x00, 0x00, 0x00, // dw2: mem_hi
            0x00, 0x00, 0x00, 0x80, // dw3: format (data_format = 1, reg_offset = 0)
            0x00, 0x00, 0x00, 0x00, // dw4: count
        ],
        "header 0xc0039f00, zeros, format 0x80000000, zero count"
    );
}

/// The phantom `0x7d86501b8094ef57` query helper writes the workload size into `*arg0`.
#[test]
fn phantom_get_size_writes_workload_size() {
    let mut size: u64 = 0;
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = std::ptr::addr_of_mut!(size) as u64;

    assert_eq!(call("0x7d86501b8094ef57", args), 0);
    assert_eq!(size, 0xa8, "workload buffer size is 168 (0xa8) bytes");
}

/// `sceAgcInit` (and alias `0x53bbd82b51d172db`) validates version 13 and returns 0.
#[test]
fn sce_agc_init_wired_and_validates_version() {
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[1] = 13;
    assert_eq!(call("sceAgcInit", args), 0);
    assert_eq!(call("0x53bbd82b51d172db", args), 0);

    args[1] = 1;
    assert_eq!(call("sceAgcInit", args), 0x8a6c_0004);
    assert_eq!(call("0x53bbd82b51d172db", args), 0x8a6c_0004);
}
