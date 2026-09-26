//! The packet walker, held against command buffers the hardware produced.
//!
//! `packet.rs` is transcribed from the GPU vendor's public documentation. obSCEne called
//! `libSceAgc` command builders on hardware and captured what each wrote into a caller-supplied
//! buffer, with an independently measured length: the bytes that changed, owing nothing to any
//! header field. A length rule off by one, or a field in the wrong bits, walks off the end of a
//! buffer or stops short, and `PacketWalk` reports both. Agreement with these buffers does not
//! cover packets none of them contains.

use orbistoun_gpu::packet::{PacketKind, walk};

/// One builder's output, as obSCEne recorded it.
struct Captured {
    /// The symbol that wrote it.
    name: &'static str,
    /// Bytes obSCEne saw change - measured, not derived from any header.
    extent: usize,
    /// The bytes themselves.
    bytes: &'static [u8],
}

/// The four builders from obSCEne's `166-agc` checks in a native title.
const CAPTURES: &[Captured] = &[
    Captured {
        name: "sceAgcCbNop",
        extent: 4,
        bytes: &[0x00, 0x10, 0xff, 0xff],
    },
    Captured {
        name: "sceAgcCbReleaseMem",
        extent: 32,
        bytes: &[
            0x00, 0x49, 0x06, 0xc0, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x45, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xb0, 0xbb, 0xff, 0xee, 0x07, 0x00, 0x00, 0x00,
            0x80, 0x44, 0x74, 0x00,
        ],
    },
    Captured {
        name: "sceAgcDcbDmaData",
        extent: 28,
        bytes: &[
            0x00, 0x50, 0x05, 0xc0, 0x00, 0x40, 0x00, 0x00, 0xb0, 0xbb, 0xff, 0xee, 0x07, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xb0, 0xbb, 0xff, 0x82,
        ],
    },
    Captured {
        name: "sceAgcDcbWaitRegMem",
        extent: 56,
        bytes: &[
            0x04, 0x79, 0x02, 0xc0, 0x42, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01, 0xc8, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x3c, 0x05, 0xc0, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x7a, 0x03, 0x00, 0x00, 0xb0, 0xbb, 0xff, 0xee, 0xff, 0xff,
            0x00, 0x00, 0x04, 0x79, 0x01, 0xc0, 0x42, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc8,
        ],
    },
];

/// The transcribed length rule agrees with what hardware wrote: each buffer is consumed exactly,
/// with no overrun, no desynchronisation and nothing left over. `sceAgcCbNop` has its own test.
#[test]
fn a_walk_of_a_measured_command_buffer_consumes_it_exactly() {
    for capture in CAPTURES.iter().filter(|c| c.name != "sceAgcCbNop") {
        assert_eq!(
            capture.bytes.len(),
            capture.extent,
            "{}: the transcription and the measured extent disagree",
            capture.name
        );

        let walk = walk(capture.bytes);
        assert!(
            walk.is_trustworthy(),
            "{}: desynchronised={} overran={} trailing={} - the length rule is wrong somewhere",
            capture.name,
            walk.desynchronised,
            walk.overran,
            walk.trailing_bytes
        );
        let consumed: u32 = walk.packets.iter().map(|p| p.length).sum();
        assert_eq!(
            consumed as usize, capture.extent,
            "{}: the walk consumed {consumed} of {} measured bytes",
            capture.name, capture.extent
        );
    }
}

/// One builder emits three packets: `sceAgcDcbWaitRegMem` writes 56 bytes as 16 + 28 + 12, and
/// the walk finds all three rather than stopping at a plausible prefix.
#[test]
fn one_builder_can_emit_several_packets() {
    let wait = CAPTURES
        .iter()
        .find(|c| c.name == "sceAgcDcbWaitRegMem")
        .expect("the capture is in the table");
    let walk = walk(wait.bytes);

    let lengths: Vec<u32> = walk.packets.iter().map(|p| p.length).collect();
    assert_eq!(
        lengths,
        vec![16, 28, 12],
        "one call wrote three packets and the walk found {lengths:?}"
    );
    let opcodes: Vec<u8> = walk
        .packets
        .iter()
        .filter_map(|p| match p.kind {
            PacketKind::Command { opcode } => Some(opcode),
            _ => None,
        })
        .collect();
    assert_eq!(opcodes, vec![0x79, 0x3c, 0x79], "opcodes as measured");
}

/// The no-op closes under the header-only rule.
///
/// `sceAgcCbNop` writes four bytes whose header carries a count of `0x3fff`, which under the
/// ordinary rule would describe a 65,540-byte packet. A count of all ones is a header-only packet:
/// the GL cube capture ends in such words after its fence event, and the hardware retires the
/// fence.
#[test]
fn the_no_op_closes_as_a_header_only_packet() {
    let nop = CAPTURES
        .iter()
        .find(|c| c.name == "sceAgcCbNop")
        .expect("the capture is in the table");
    assert_eq!(nop.extent, 4, "hardware wrote one dword");

    let walk = walk(nop.bytes);
    assert!(
        walk.is_trustworthy(),
        concat!(
            "the no-op's count of all ones is a header-only packet; ",
            "the GL cube capture ran sixteen of them past its fence"
        )
    );
    assert_eq!(walk.packets.len(), 1);
    assert_eq!(walk.packets[0].length, 4);
}

/// A header from live GPU memory decodes the same in two independent readers.
///
/// obSCEne's `170-gpu-capture` walked the running compositor's submitted command stream and decoded
/// a live PM4 header with its own reader: `pm4-header 0xc0599328`, `pm4-opcode 0x93`, `pm4-count
/// 0x59`. This holds orbistoun's field layout against that decode. The body is zero padding: only
/// the header was captured, so only its field extraction is under test.
#[test]
fn a_live_memory_pm4_header_decodes_as_obscenes_own_reader_did() {
    // The captured header, then a body of the length its count field declares (90 dwords), zeroed.
    const CAPTURED_HEADER: u32 = 0xc059_9328;
    let mut bytes = CAPTURED_HEADER.to_le_bytes().to_vec();
    bytes.extend(std::iter::repeat_n(0u8, 90 * 4));

    let walk = walk(&bytes);
    assert!(
        walk.is_trustworthy(),
        "desynchronised={} overran={} trailing={}",
        walk.desynchronised,
        walk.overran,
        walk.trailing_bytes
    );

    let first = walk.packets.first().expect("one packet");
    assert_eq!(
        first.kind,
        PacketKind::Command { opcode: 0x93 },
        "obSCEne's reader called it a type-3 command with opcode 0x93"
    );
    assert_eq!(
        first.length,
        4 + 90 * 4,
        "count 0x59 means 90 body dwords, so 4 + 360 bytes - the length obSCEne's count implies"
    );
    assert_eq!(
        walk.packets.len(),
        1,
        "the header plus its declared body is exactly one packet"
    );
}

/// The same builders from a second hardware run (`166-agc`, a native Prospero title), called with
/// different arguments: its bodies are the argument-cleared case, all zeros where the first run
/// carried live addresses. The header opcode and the length rule do not depend on the arguments.
const CAPTURES_20260914: &[Captured] = &[
    Captured {
        name: "sceAgcDcbDmaData",
        extent: 28,
        bytes: &[
            0x00, 0x50, 0x05, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
    },
    Captured {
        name: "sceAgcCbReleaseMem",
        extent: 32,
        bytes: &[
            0x00, 0x49, 0x06, 0xc0, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ],
    },
    Captured {
        name: "sceAgcDcbWaitRegMem",
        extent: 56,
        bytes: &[
            0x04, 0x79, 0x02, 0xc0, 0x42, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01, 0xc8, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x3c, 0x05, 0xc0, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x04, 0x79, 0x01, 0xc0, 0x42, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc8,
        ],
    },
];

/// A second hardware run walks the same, so the length rule does not ride on the arguments. The
/// three-packet `WaitRegMem` decomposes the same way from bytes that share only its opcodes and
/// lengths.
#[test]
fn an_independent_measurement_confirms_the_walk() {
    for capture in CAPTURES_20260914 {
        assert_eq!(
            capture.bytes.len(),
            capture.extent,
            "{}: the transcription and the measured extent disagree",
            capture.name
        );
        let walk = walk(capture.bytes);
        assert!(
            walk.is_trustworthy(),
            "{}: desynchronised={} overran={} trailing={}",
            capture.name,
            walk.desynchronised,
            walk.overran,
            walk.trailing_bytes
        );
        let consumed: u32 = walk.packets.iter().map(|p| p.length).sum();
        assert_eq!(
            consumed as usize, capture.extent,
            "{}: the walk consumed {consumed} of {} measured bytes",
            capture.name, capture.extent
        );
    }

    // The multi-packet case, on the argument-independent witness.
    let wait = CAPTURES_20260914
        .iter()
        .find(|c| c.name == "sceAgcDcbWaitRegMem")
        .expect("the capture is in the table");
    let walk = walk(wait.bytes);
    let lengths: Vec<u32> = walk.packets.iter().map(|p| p.length).collect();
    assert_eq!(
        lengths,
        vec![16, 28, 12],
        "the same 16+28+12 split as the first run, from different argument bytes"
    );
    let opcodes: Vec<u8> = walk
        .packets
        .iter()
        .filter_map(|p| match p.kind {
            PacketKind::Command { opcode } => Some(opcode),
            _ => None,
        })
        .collect();
    assert_eq!(
        opcodes,
        vec![0x79, 0x3c, 0x79],
        "the same opcodes, measured again"
    );
}
