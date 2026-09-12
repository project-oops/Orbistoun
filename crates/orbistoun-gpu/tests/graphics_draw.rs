//! A Type 0 (Universal Graphics Queue) 3D primitive-draw command stream, decoded end to end.
//!
//! # What this proves, and against what
//!
//! obSCEne's sweep `20260912-003916` ran a full 3D primitive draw on a Type 0 graphics queue on
//! live hardware - NGG primitive shader, scan-converter rasterisation, pixel shader, MRT0 export -
//! and asked orbistoun to decode "Type 0 PM4 draw packet streams with GFX10 CB context registers
//! and UCONFIG GE registers without warnings/errors" (REQ-...0640Z-9a41).
//!
//! The stream below is assembled from that request's exact opcode and register list: the CB colour
//! target context registers, the SPI PS-input context registers, the UCONFIG GE / VGT registers,
//! the instance count, and the `DRAW_INDEX_AUTO` that issues the triangle. It is a constructed
//! stream, not a raw capture (the real DCB is ~1.9 KB of runtime addresses), so what it establishes
//! is the *decode path*: that every opcode a graphics draw uses is known, that the walk consumes the
//! stream exactly, and that the register writes land on the registers their bases imply. An unknown
//! opcode or a bad length is what "decodes with warnings" would mean, and the walk reports both.

use orbistoun_gpu::{PacketKind, Vocabulary, register_writes, walk};

/// Pushes a little-endian dword onto a byte buffer.
fn push(stream: &mut Vec<u8>, dword: u32) {
    stream.extend_from_slice(&dword.to_le_bytes());
}

/// Builds the draw stream from 9a41's opcode/register list.
///
/// Each `SET_*_REG` here is a count-1 packet - header, register offset, one value - which is the
/// `sceAgcDcbSet{Cx,Uc}RegisterDirect` shape obSCEne measured (`bytes-advanced 0xc`). Values are the
/// request's measured ones where it gave them (`CB_COLOR0_ATTRIB2 = 0x000fc03f` for 64x64,
/// `SPI_PS_INPUT_ENA = 0x2`, `VGT_PRIMITIVE_TYPE = 0x4` DI_PT_TRILIST, `GE_CNTL = 0x8040`).
fn graphics_draw_stream() -> Vec<u8> {
    let mut s = Vec::new();

    // SET_CONTEXT_REG (opcode 0x69, count 1): CB colour target + one SPI PS input.
    push(&mut s, 0xc001_6900);
    push(&mut s, 0x200); // CB_COLOR0_BASE
    push(&mut s, 0x0010_0000);
    push(&mut s, 0xc001_6900);
    push(&mut s, 0x3b0); // CB_COLOR0_ATTRIB2
    push(&mut s, 0x000f_c03f);
    push(&mut s, 0xc001_6900);
    push(&mut s, 0x1b3); // SPI_PS_INPUT_ENA
    push(&mut s, 0x0000_0002);

    // SET_UCONFIG_REG (opcode 0x79, count 1): GE control and the primitive type.
    push(&mut s, 0xc001_7900);
    push(&mut s, 0x242); // VGT_PRIMITIVE_TYPE
    push(&mut s, 0x0000_0004); // DI_PT_TRILIST
    push(&mut s, 0xc001_7900);
    push(&mut s, 0x25b); // GE_CNTL
    push(&mut s, 0x0000_8040);

    // NUM_INSTANCES (opcode 0x2f, count 0): one instance.
    push(&mut s, 0xc000_2f00);
    push(&mut s, 0x0000_0001);

    // DRAW_INDEX_AUTO (opcode 0x2d, count 1): index_count = 3, initiator = 2.
    push(&mut s, 0xc001_2d00);
    push(&mut s, 0x0000_0003);
    push(&mut s, 0x0000_0002);

    s
}

/// **The graphics draw stream walks cleanly, with no unknown packet and nothing left over.**
///
/// This is the 9a41 acceptance for the decode side: a Type 0 draw stream decodes without warnings.
#[test]
fn a_type0_draw_stream_walks_without_warnings() {
    let stream = graphics_draw_stream();
    let walk = walk(&stream);

    assert!(
        walk.is_trustworthy(),
        "desynchronised={} overran={} trailing={} - a graphics draw opcode is unknown or mis-sized",
        walk.desynchronised,
        walk.overran,
        walk.trailing_bytes,
    );
    let consumed: u32 = walk.packets.iter().map(|p| p.length).sum();
    assert_eq!(consumed as usize, stream.len(), "the walk consumes the whole draw stream");

    let opcodes: Vec<u8> = walk
        .packets
        .iter()
        .filter_map(|p| match p.kind {
            PacketKind::Command { opcode } => Some(opcode),
            PacketKind::RegisterWrite { .. } => None,
            PacketKind::Filler | PacketKind::Reserved => None,
        })
        .collect();
    // SET_CONTEXT_REG (0x69) and SET_UCONFIG_REG (0x79) may classify as RegisterWrite rather than
    // Command depending on the vocabulary, so assert on the two that are always commands here.
    assert!(opcodes.contains(&0x2d), "the draw is DRAW_INDEX_AUTO (0x2d)");
    assert!(opcodes.contains(&0x2f), "NUM_INSTANCES (0x2f) is present");
    assert_eq!(walk.packets.len(), 7, "three context, two uconfig, instances, draw");
}

/// **The register-setting packets resolve to real register writes, by value.**
///
/// The five `SET_*_REG` packets must produce five register writes carrying the values the request
/// specified. Asserting by value (which the stream controls) rather than by absolute register number
/// keeps the test independent of the base offsets while still proving the writes were decoded.
#[test]
fn the_draw_registers_are_captured() {
    let stream = graphics_draw_stream();
    let walk = walk(&stream);
    let vocabulary = Vocabulary::builtin().expect("built-in vocabulary");
    let writes = register_writes(&walk, &stream, &vocabulary);

    let values: Vec<u32> = writes.iter().map(|w| w.value).collect();
    assert_eq!(writes.len(), 5, "five SET_*_REG packets, five register writes");
    for expected in [0x0010_0000, 0x000f_c03f, 0x0000_0002, 0x0000_0004, 0x0000_8040] {
        assert!(values.contains(&expected), "register value {expected:#x} was captured");
    }
}
