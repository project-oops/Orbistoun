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
        83,
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

/// `sceAgcCbNop(cb, dwords)` writes a `NOP` `dwords` long. Measured (`166-agc/cb-nop`): 1 writes
/// the header-only word `0xffff1000`, four bytes, and answers the packet's address; 0 writes
/// nothing, and is refused here since its answer was not reported. Longer ones follow the PM4
/// header (count = length - 2, the rule 1 is the wrapped case of): PPSA02664 asks for 3 and fills
/// the two body dwords through `0x7d86501b8094ef57`, so the body is reserved and left unwritten.
#[test]
fn cb_nop_writes_a_nop_of_the_length_asked() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 1;
    let at = w.cursor();
    assert_eq!(call("sceAgcCbNop", args), at, "returns the packet address");
    assert_eq!(w.written(), 4, "a header-only packet is four bytes");
    assert_eq!(w.bytes(), &[0x00, 0x10, 0xff, 0xff], "0xffff1000");

    let w = Writer::new(0x400);
    args[0] = w.handle();
    args[1] = 3;
    let at = w.cursor();
    assert_eq!(call("sceAgcCbNop", args), at);
    assert_eq!(w.written(), 12, "header and two body dwords");
    assert_eq!(&w.bytes()[..4], &0xc001_1000_u32.to_le_bytes());

    let w = Writer::new(0x400);
    args[0] = w.handle();
    args[1] = 0;
    assert_ne!(call("sceAgcCbNop", args), w.cursor(), "length 0 is refused");
    assert_eq!(w.written(), 0);
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

/// `sceAgcCbReleaseMem` and `sceAgcDcbDmaData` write their packets through the handler with the
/// arguments where the measurements put them (obSCEne `-1044`). A direct call publishes no stack
/// area, so the arguments from the seventh on read as zero.
#[test]
fn release_mem_and_dma_data_write_their_arguments() {
    let w = Writer::new(0x400);
    let at = w.cursor();
    assert_eq!(
        call(
            "sceAgcCbReleaseMem",
            [w.handle(), 0x2b, 0, 1, 3, 0x1234_5678]
        ),
        at
    );
    assert_eq!(w.written(), 32);
    let words: Vec<u32> = w
        .bytes()
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    assert_eq!(
        words,
        [
            0xc006_4900,
            0x0600_052b,
            0x0001_0000,
            0x1234_5678,
            0,
            0,
            0,
            0
        ]
    );

    let w = Writer::new(0x400);
    assert_eq!(
        call("sceAgcDcbDmaData", [w.handle(), 1, 0, 0, 0xabcd_0000, 2]),
        w.cursor() - 28
    );
    assert_eq!(w.written(), 28);
    let words: Vec<u32> = w
        .bytes()
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    assert_eq!(words, [0xc005_5000, 0x4000_0001, 0, 0, 0xabcd_0000, 0, 0]);
}

/// The DmaData address patches write into the packet `sceAgcDcbDmaData` returned: the
/// destination into dw4 and dw5, the source into dw2 and dw3, leaving the rest (obSCEne
/// `166-agc/patch-dma-data-*`). PPSA03416 builds a fill with a zero destination and patches it.
#[test]
fn the_dma_data_patches_write_the_address_into_the_packet() {
    let w = Writer::new(0x400);
    let packet = call("sceAgcDcbDmaData", [w.handle(), 1, 0, 0, 0, 2]);
    let dword = |i: usize| u32::from_le_bytes(w.bytes()[i * 4..i * 4 + 4].try_into().unwrap());
    let mut patch = [0u64; GUEST_ARG_REGISTERS];
    patch[0] = packet;
    patch[1] = 0x0000_7400_01e4_1e38;
    assert_eq!(call("sceAgcDmaDataPatchSetDstAddressOrOffset", patch), 0);
    assert_eq!((dword(4), dword(5)), (0x01e4_1e38, 0x7400));
    patch[1] = 0x0000_0002_3000_0000;
    assert_eq!(
        call("sceAgcDmaDataPatchSetSrcAddressOrOffsetOrImmediate", patch),
        0
    );
    assert_eq!((dword(2), dword(3)), (0x3000_0000, 2));
    assert_eq!(
        (dword(0), dword(1), dword(6)),
        (0xc005_5000, 0x4000_0001, 0)
    );
    assert_eq!(w.written(), 28, "the patches amend in place");
}

/// The end-of-pipe and wait-reg-mem address patches write where obSCEne measured them: the
/// `RELEASE_MEM`'s dw3 (`166-agc/patch-queue-eop-address`), and bytes 12 and 24 of the
/// wait-reg-mem compound (`166-agc/patch-wait-reg-mem-address`), the high half after the poll
/// address's low. PPSA03416 patches both before submitting.
#[test]
fn the_eop_and_wait_reg_mem_patches_write_the_address() {
    let address = 0x0000_7400_020c_1e38_u64;
    let w = Writer::new(0x400);
    let packet = call("sceAgcCbReleaseMem", [w.handle(), 0x2b, 0, 1, 3, 0x10]);
    let mut patch = [0u64; GUEST_ARG_REGISTERS];
    patch[0] = packet;
    patch[1] = address;
    assert_eq!(call("sceAgcQueueEndOfPipeActionPatchAddress", patch), 0);
    let dword =
        |w: &Writer, i: usize| u32::from_le_bytes(w.bytes()[i * 4..i * 4 + 4].try_into().unwrap());
    assert_eq!((dword(&w, 3), dword(&w, 4)), (0x020c_1e38, 0x7400));
    assert_eq!(dword(&w, 0), 0xc006_4900);

    let w = Writer::new(0x400);
    patch[0] = call("sceAgcDcbWaitRegMem", [w.handle(), 0, 0, 0, 0, 0]);
    assert_eq!(call("sceAgcWaitRegMemPatchAddress", patch), 0);
    assert_eq!(dword(&w, 3), 0x020c_1e38);
    assert_eq!((dword(&w, 6), dword(&w, 7)), (0x020c_1e38, 0x7400));
    assert_eq!(
        (dword(&w, 4), dword(&w, 5), dword(&w, 8)),
        (0xc005_3c00, 0x10, 0),
        "the header, dw1 with only MEM_SPACE set (pm4-pass0), the reference"
    );
    assert_eq!(w.written(), 56, "the patch amends in place");
}

/// The skeleton measured by header and extent reserves its measured length with the measured
/// header and a zero body (D696). The argument in `arg1` must not leak into the body.
#[test]
fn the_measured_skeletons_reserve_their_extent_with_a_zero_body() {
    let name = "sceAgcDcbSetBaseIndirectArgs";
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 0x1111_1111; // an argument that must not appear in the packet
    args[2] = 0x2222_2222;

    let at = w.cursor();
    assert_eq!(call(name, args), at, "{name} returns the packet address");
    assert_eq!(w.written(), 16, "{name} reserves its measured extent");
    assert_eq!(
        &w.bytes()[..4],
        &[0x00, 0x11, 0x02, 0xc0],
        "{name} writes its measured header, little-endian"
    );
    assert!(
        w.bytes()[4..].iter().all(|b| *b == 0),
        "{name} leaves the body zero, not a guessed encoding of the argument"
    );
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
        ("sceAgcDcbDispatchIndirect", 12usize, 0xc001_1600u32),
        ("sceAgcAcbDispatchIndirect", 16, 0xc002_1600),
        ("sceAgcDcbDrawIndirect", 20, 0xc003_2400),
        ("sceAgcDcbDrawIndexIndirect", 20, 0xc003_2500),
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

/// `sceAgcCbDispatch(cb, x, y, z, ..)` writes the dispatch hardware wrote for the same arguments:
/// the thread-group counts in DW1-DW3 and the initiator `0x41` whatever `arg4` and `arg5` are
/// (obSCEne `reports/report-1791286625.txt` 12105-12185, REQ cd15). PPSA03416's own call first.
#[test]
fn cb_dispatch_writes_its_dimensions_as_hardware_did() {
    // Rows `pm4-title`, `pm4-arg1-only` .. `pm4-arg5-only`: the header, DW1-DW3, the initiator.
    for (arguments, dimensions) in [
        ([0xa00, 1, 1, 1, 0xc], [0xa00, 1, 1]),
        ([0x11, 0, 0, 0, 0], [0x11, 0, 0]),
        ([0, 0x22, 0, 0, 0], [0, 0x22, 0]),
        ([0, 0, 0x33, 0, 0], [0, 0, 0x33]),
        ([0, 0, 0, 0x44, 0], [0, 0, 0]),
        ([0, 0, 0, 0, 0x55], [0, 0, 0]),
    ] {
        let w = Writer::new(0x400);
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = w.handle();
        args[1..].copy_from_slice(&arguments);
        let at = w.cursor();
        assert_eq!(call("sceAgcCbDispatch", args), at, "{arguments:x?}");
        let expected: Vec<u8> = [
            0xc003_1500_u32,
            dimensions[0],
            dimensions[1],
            dimensions[2],
            0x41,
        ]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
        assert_eq!(w.bytes(), &expected[..], "{arguments:x?}");
    }
}

/// The Sh and Uc indirect producers and patches, as measured (sweep 20260928-230724): the producer
/// called with `0x1000` writes its header, dw1 `0x1000` and the format word `0x80000000`
/// (`166-agc/dcb-set-{sh,uc}-registers-indirect` `pm4-pass1`); `AddRegisters(packet, 1)` adds one
/// to dw4 and `SetAddress(packet, 0x2000_0000)` writes dw1, as the Cx pair does
/// (`166-agc/patch-{sh,uc}-reg-*`, `diff-pass-a`).
#[test]
fn the_sh_and_uc_indirect_patches_amend_their_packet() {
    for (family, header) in [("Sh", 0xc003_6300_u32), ("Uc", 0xc003_6400)] {
        let w = Writer::new(0x400);
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = w.handle();
        args[1] = 0x1000;
        let packet = call(&format!("sceAgcDcbSet{family}RegistersIndirect"), args);
        let dword = |i: usize| u32::from_le_bytes(w.bytes()[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!(
            (dword(0), dword(1), dword(2), dword(3), dword(4)),
            (header, 0x1000, 0, 0x8000_0000, 0),
            "{family} producer"
        );

        let mut patch = [0u64; GUEST_ARG_REGISTERS];
        patch[0] = packet;
        patch[1] = 1;
        let add = format!("sceAgcSet{family}RegIndirectPatchAddRegisters");
        assert_eq!(call(&add, patch), 0);
        assert_eq!(dword(4), 1, "{family} count");

        patch[1] = 0x2000_0000;
        let set = format!("sceAgcSet{family}RegIndirectPatchSetAddress");
        assert_eq!(call(&set, patch), 0);
        assert_eq!((dword(1), dword(2)), (0x2000_0000, 0), "{family} address");
        assert_eq!(w.written(), 20, "{family}: the patches amend in place");
    }
}

/// `sceAgcDcbWaitUntilSafeForRendering` for a port that is not open answers `0x0` and writes
/// nothing, on a real writer or none (`-5a17`'s no-port arms). No display is installed in this test
/// binary, so no port is open; `dcb_set_flip.rs` covers the written packet.
#[test]
fn wait_until_safe_for_rendering_with_no_port_answers_zero() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    let rc = call("sceAgcDcbWaitUntilSafeForRendering", args);
    assert_eq!(rc, 0, "the measured 0x0");
    assert_ne!(rc, UNIMPLEMENTED, "not the placeholder");
    assert_eq!(w.written(), 0, "no port, no packet");

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

/// With no values the run is reserved, payload zeroed, and its address returned; the payload
/// pointer the unnamed helper gives for it is where the guest then writes the values.
#[test]
fn a_register_run_with_no_values_is_reserved_for_the_guest_to_fill() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 0x240;
    args[3] = 3;

    let at = w.cursor();
    assert_eq!(call("sceAgcCbSetShRegisterRangeDirect", args), at);
    assert_eq!(w.written(), 20, "n + 2 dwords for n = 3");
    assert_eq!(
        &w.bytes()[..8],
        &[0x00, 0x76, 0x03, 0xc0, 0x40, 0x02, 0x00, 0x00]
    );
    assert!(w.bytes()[8..].iter().all(|&b| b == 0));

    let mut payload = 0u64;
    let mut query = [0u64; GUEST_ARG_REGISTERS];
    query[0] = std::ptr::addr_of_mut!(payload) as u64;
    query[1] = at;
    query[2] = 1;
    assert_eq!(call("0x7d86501b8094ef57", query), 0);
    assert_eq!(payload, at + 8);
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

/// The probe's sequence (`166-agc/patch-cx-registers-indirect`): the producer called as
/// `(dcb, 2, table)`, one `AddRegisters(packet, 1)`, `SetAddress(packet, 0x2_0086_0000)`, then a
/// second `AddRegisters`, gives the measured dw1, dw2 and dw4 at each step.
#[test]
fn the_indirect_register_patches_replay_the_measured_sequence() {
    let w = Writer::new(0x400);
    let table = 0x0000_0002_0085_3880_u64;
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 2;
    args[2] = table;
    let packet = call("sceAgcDcbSetCxRegistersIndirect", args);
    let dword = |i: usize| u32::from_le_bytes(w.bytes()[i * 4..i * 4 + 4].try_into().unwrap());
    assert_eq!((dword(1), dword(2), dword(4)), (0, 0, 0x3880));

    let mut patch = [0u64; GUEST_ARG_REGISTERS];
    patch[0] = packet;
    patch[1] = 1;
    assert_eq!(call("sceAgcSetCxRegIndirectPatchAddRegisters", patch), 0);
    assert_eq!(dword(4), 0x3881);

    patch[1] = 0x0000_0002_0086_0000;
    assert_eq!(call("sceAgcSetCxRegIndirectPatchSetAddress", patch), 0);
    assert_eq!(
        (dword(1), dword(2), dword(3)),
        (0x0086_0000, 2, 0x8000_0000)
    );

    patch[1] = 1;
    assert_eq!(call("sceAgcSetCxRegIndirectPatchAddRegisters", patch), 0);
    assert_eq!(dword(4), 0x3882);
    assert_eq!(w.written(), 20, "the patches amend in place");
}

/// The two init gates, in the one order this process runs them (the gate is process state, so a
/// second test calling either would race this one). Before any raw call the named `sceAgcInit`
/// accepts 12, 13 and 14 (`-7e41`). PPSA02664's `0x53bbd82b51d172db(state, 8)` succeeds in a fresh
/// process and fixes the version: 13 is then refused by both gates and 8 still passes (`-7e52`,
/// sweep 20260927-185507).
#[test]
fn the_raw_init_gate_keeps_the_first_version_it_accepts() {
    let at = |version: u64| {
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[1] = version;
        args
    };
    for version in [12, 13, 14] {
        assert_eq!(call("sceAgcInit", at(version)), 0, "named {version}");
    }
    assert_eq!(call("0x53bbd82b51d172db", at(8)), 0, "the title's call");
    for version in [13, 12, 14] {
        assert_eq!(
            call("0x53bbd82b51d172db", at(version)),
            0x8a6c_0004,
            "raw {version}"
        );
    }
    assert_eq!(call("sceAgcInit", at(13)), 0x8a6c_0004, "named after raw 8");
    assert_eq!(call("0x53bbd82b51d172db", at(8)), 0, "8 again");
}

/// `0x7d86501b8094ef57(out, packet, kind)` on the probe's own packets (`-r1b0`, sweep
/// 20260927-204316, check `166-agc/cb-unnamed-ef57`): kind 1 on a `SET_SH_REG` answers packet + 8;
/// kind 0 on an ordinary `NOP` packet + 4 and on the header-only `0xffff1000` `NOP` null; kind 2 on
/// an ordinary `NOP` packet + 8; `0` returned every time. A kind on a packet nobody measured it on
/// is refused.
#[test]
fn packet_payload_answers_the_measured_kinds() {
    let payload = |packet: &[u32], kind: u64| -> (u64, u64) {
        let mut out = 0xcccc_cccc_cccc_cccc_u64;
        let mut query = [0u64; GUEST_ARG_REGISTERS];
        query[0] = std::ptr::addr_of_mut!(out) as u64;
        query[1] = packet.as_ptr() as u64;
        query[2] = kind;
        (call("0x7d86501b8094ef57", query), out)
    };
    let sh_reg = [0xc003_7600_u32, 0x240, 0, 0, 0, 0, 0, 0];
    let nop_ordinary = [0xc002_1000_u32, 0, 0, 0];
    let nop_header_only = [0xffff_1000_u32, 0, 0, 0];
    let at = |packet: &[u32], bytes: u64| packet.as_ptr() as u64 + bytes;

    assert_eq!(payload(&sh_reg, 1), (0, at(&sh_reg, 8)));
    assert_eq!(payload(&nop_ordinary, 0), (0, at(&nop_ordinary, 4)));
    assert_eq!(payload(&nop_header_only, 0), (0, 0));
    assert_eq!(payload(&nop_ordinary, 2), (0, at(&nop_ordinary, 8)));
    let (rc, out) = payload(&sh_reg, 2);
    assert_ne!(rc, 0, "kind 2 was measured on a NOP only");
    assert_eq!(out, 0xcccc_cccc_cccc_cccc, "and nothing written");
}

/// `sceAgcCbSetShRegistersDirect(cb, pairs, count)` writes each `{offset, value}` pair as its own
/// `SET_SH_REG` - the GFX10 form of a scattered SH register write (Mesa `radeon_set_sh_reg`; the
/// pair packet is GFX11's) - and answers the first packet's address. PPSA28061 binds its compute
/// shader so: `COMPUTE_PGM_LO`/`HI` and the rest of the shader's ten registers. The encoding is
/// assumed (measured only with a count of 0, which writes nothing and answers 0); REQ-cn01 asks.
#[test]
fn set_sh_registers_direct_writes_each_pair_as_a_register_write() {
    let w = Writer::new(0x400);
    let pairs: [u32; 6] = [0x20c, 0x1c2c, 0x20d, 0x40, 0x22a, 0x2800_00c0];
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = pairs.as_ptr() as usize as u64;
    args[2] = 3;
    let at = w.cursor();
    assert_eq!(call("sceAgcCbSetShRegistersDirect", args), at);
    let expected: Vec<u8> = [
        0xc001_7600_u32,
        0x20c,
        0x1c2c,
        0xc001_7600,
        0x20d,
        0x40,
        0xc001_7600,
        0x22a,
        0x2800_00c0,
    ]
    .iter()
    .flat_map(|word| word.to_le_bytes())
    .collect();
    assert_eq!(w.bytes(), &expected[..]);

    let empty = Writer::new(0x40);
    args[0] = empty.handle();
    args[2] = 0;
    assert_eq!(
        call("sceAgcCbSetShRegistersDirect", args),
        0,
        "a count of 0: measured"
    );
    assert_eq!(empty.written(), 0, "writes nothing");
}

/// `sceAgcDcbDrawIndexOffset(dcb, index_offset, index_count, modifier)` writes a
/// `DRAW_INDEX_OFFSET_2` (Mesa `sid.h` `0x35`, body `[max_size, index_offset, index_count,
/// initiator]`) in the 20 bytes its builder was measured to take (`166-agc/*-getsize`), with the
/// index count as `max_size` and the modifier as the initiator word, as `sceAgcDcbDrawIndex`
/// was measured to place them. PPSA28061 draws this way, `(dcb, 0, 6, 0x40000000)`, every frame.
/// The body is assumed; REQ-cn01 asks.
#[test]
fn draw_index_offset_writes_an_offset_draw_in_its_measured_twenty_bytes() {
    let w = Writer::new(0x400);
    let mut args = [0u64; GUEST_ARG_REGISTERS];
    args[0] = w.handle();
    args[1] = 0;
    args[2] = 6;
    args[3] = 0x4000_0000;
    let at = w.cursor();
    assert_eq!(call("sceAgcDcbDrawIndexOffset", args), at);
    let expected: Vec<u8> = [0xc003_3500_u32, 6, 0, 6, 0x4000_0000]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    assert_eq!(w.bytes(), &expected[..]);
}

/// Each fixed-length builder's `GetSize` answers, in bytes, what obSCEne measured with its
/// arguments zero, and `sceAgcCbNopGetSize(n)` answers `4 * n`
/// (`reports/hardware/20261009-151440-eboot.obs.log`, the `166-agc/*` checks). PPSA04263 sizes a
/// NOP fill from two of these, and a placeholder answer made that fill 0x3dffc002 dwords.
#[test]
fn the_getsize_queries_answer_the_measured_sizes() {
    for (name, bytes) in [
        ("sceAgcAcbAcquireMemGetSize", 0x20),
        ("sceAgcDcbAcquireMemGetSize", 0x20),
        ("sceAgcAcbDmaDataGetSize", 0x1c),
        ("sceAgcDcbDmaDataGetSize", 0x1c),
        ("sceAgcAcbDispatchIndirectGetSize", 0x10),
        ("sceAgcDcbDispatchIndirectGetSize", 0xc),
        ("sceAgcAcbEventWriteGetSize", 0x8),
        ("sceAgcAcbJumpGetSize", 0x10),
        ("sceAgcDcbJumpGetSize", 0x10),
        ("sceAgcCbBranchGetSize", 0x38),
        ("sceAgcCbQueueEndOfPipeActionGetSize", 0x20),
        ("sceAgcDcbCondExecGetSize", 0x14),
        ("sceAgcDcbDrawIndexIndirectGetSize", 0x14),
        ("sceAgcDcbDrawIndexIndirectMultiGetSize", 0x40),
        ("sceAgcDcbDrawIndexOffsetGetSize", 0x14),
        ("sceAgcDcbDrawIndirectGetSize", 0x14),
        ("sceAgcDcbGetLodStatsGetSize", 0x14),
        ("sceAgcDcbRewindGetSize", 0x8),
        ("sceAgcDcbSetCxRegistersIndirectGetSize", 0x14),
        ("sceAgcDcbSetShRegistersIndirectGetSize", 0x14),
        ("sceAgcDcbSetUcRegistersIndirectGetSize", 0x14),
        ("sceAgcDcbSetUcRegisterDirectGetSize", 0xc),
        ("sceAgcDcbStallCommandBufferParserGetSize", 0x8),
        ("sceAgcDcbSetIndexCountGetSize", 0x8),
    ] {
        assert_eq!(call(name, [0; GUEST_ARG_REGISTERS]), bytes, "{name}");
    }
    for n in [0_u64, 1, 2, 4, 8] {
        assert_eq!(
            call("sceAgcCbNopGetSize", [n, 0, 0, 0, 0, 0]),
            4 * n,
            "a NOP of {n} dwords"
        );
    }
}
