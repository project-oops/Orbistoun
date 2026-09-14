//! The packet walker, held against command buffers a console actually produced.
//!
//! # Why this test could not be written until today
//!
//! `packet.rs` is built from AMD's public documentation and says so, with a caveat and a
//! prescription:
//!
//! > The values below are **transcribed and not yet verified line by line** against the published
//! > document. [...] a walk over a real command buffer that desynchronises immediately is how a
//! > mistake here announces itself.
//!
//! There were no real command buffers. There are now: obSCEne called four `libSceAgc` command
//! builders on hardware - firmware 12.40, prospero, in a native title - and captured exactly
//! what each wrote into a caller-supplied buffer (D565). This is that prescription, filled.
//!
//! # What makes it evidence rather than a fixture
//!
//! Each capture carries an **independently measured length**: obSCEne recorded how many bytes
//! changed, without reference to any header field. So the walk has something to be wrong about.
//! A length rule that is off by one, or a field in the wrong bits, walks off the end of a
//! 28-byte buffer or stops short of it, and `PacketWalk` reports both.
//!
//! # What it cannot establish
//!
//! **That the transcription is right**, only that it agrees with four buffers. A rule that erred
//! on packets none of these four contain would pass. Three of the four are a single packet, so
//! the multi-packet case rests on one capture.

use orbistoun_gpu::packet::{PacketKind, walk};

/// One builder's output, as obSCEne recorded it.
struct Captured {
    /// The symbol that wrote it.
    name: &'static str,
    /// Bytes obSCEne saw change - **measured, not derived from any header**.
    extent: usize,
    /// The bytes themselves.
    bytes: &'static [u8],
}

/// The four Class B builders, from `166-agc` in `reports/hardware/run-native-title.txt`.
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

/// **The transcribed length rule agrees with what hardware wrote.**
///
/// The whole point. Each buffer is walked and the packets must consume it **exactly** - no
/// overrun, no desynchronisation, nothing left over. `sceAgcCbNop` is excluded and gets its own
/// test below, because it is the one that does not fit.
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

/// **One builder emitted three packets, and that is the finding a parser must survive.**
///
/// A command-stream reader cannot assume one call is one packet. `sceAgcDcbWaitRegMem` writes 56
/// bytes as 16 + 28 + 12, and the walk has to find all three - a rule that stopped after the
/// first would still consume a plausible-looking prefix and report nothing wrong.
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

/// **The no-op closes under the header-only rule, which it was the evidence for.**
///
/// `sceAgcCbNop` wrote **four** bytes whose header carries a count of `0x3fff`. Under the rule the
/// other builders obey that would describe a 65,540-byte packet, and this test used to record the
/// disagreement rather than smooth it over. The GL cube capture settled it: its stream ends in
/// sixteen of these words after the fence event, the hardware retired the fence every frame, and a
/// walk that read them as 64 KiB packets overran the buffer. A count of all ones is a header-only
/// packet, and now the walker says so.
///
/// Asserted as agreement now: if the rule ever regressed, this is the capture that would say.
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

/// **A header from live GPU memory, decoded the same by two independent readers.**
///
/// The four captures above are `libSceAgc` *command builders* - an API call, and the bytes it
/// appended. This is a different provenance and a stronger cross-check: obSCEne's `170-gpu-capture`
/// walked the **running compositor's** submitted command stream in kernel memory and decoded a
/// live PM4 header with its own C reader, reporting `it_op 0x93`, `payload_dwords 0x59`. The header
/// dword it captured is `0xc059_9328`.
///
/// So this is orbistoun's transcribed field layout (`TYPE_SHIFT`, `OPCODE_SHIFT`, `COUNT_SHIFT` and
/// their masks) held against a *second implementation* that read the same bytes off hardware - the
/// kind of agreement `packet.rs` was transcribed-but-unverified about, now reached from live memory
/// rather than an API capture.
///
/// The body is ninety zero dwords: a padding, not a measurement. Only the header's field extraction
/// is under test, because only the header was independently decoded - the window obSCEne captured is
/// four dwords of a ninety-dword packet, so the body is not available to walk.
///
/// Reference: obSCEne `reports/hardware/payload-klog.obs.log`, section `170-gpu-capture/command-stream`
/// (`AgcCompositor.elf`, pid 0x39), records `pm4-header 0xc0599328`, `pm4-opcode 0x93`,
/// `pm4-count 0x59`.
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

/// The same builders, from a **second, independent** hardware run - obSCEne sweep
/// `20260914-000606`, section `166-agc`, a native Prospero title. Different day, different
/// firmware leg, and the probe called each builder with **different arguments** than the
/// `run-native-title` capture above did (its bodies are the argument-cleared case, all zeros where
/// the first run carried live addresses). Same structure regardless: the header opcode and the
/// length rule cannot depend on the argument bytes, and a second witness is what shows that.
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

/// **A second hardware run walks the same, so the length rule does not ride on the arguments.**
///
/// The captures above (`run-native-title`) and these (`20260914-000606`) are the same builders
/// called with different arguments. Each fresh buffer must still consume exactly, and the
/// three-packet `WaitRegMem` - the case the first set could only witness once - must decompose the
/// same way from bytes that share only its opcodes and lengths, not its body. That is the arm the
/// single-capture note above asked for.
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

    // The multi-packet case, now on a second, argument-independent witness.
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
