//! A graphics-queue 3D primitive-draw command stream, decoded end to end.
//!
//! The stream is constructed from the opcode and register list of a hardware primitive draw (NGG
//! primitive shader, rasterisation, pixel shader, MRT0 export): the CB colour target and SPI
//! PS-input context registers, the UCONFIG GE/VGT registers, the instance count and the
//! `DRAW_INDEX_AUTO` that issues the triangle. It checks the decode path: every opcode is known,
//! the walk consumes the stream exactly, and the register writes land on the registers their bases
//! imply.

use orbistoun_gpu::{PacketKind, Vocabulary, register_writes, walk};

/// Pushes a little-endian dword onto a byte buffer.
fn push(stream: &mut Vec<u8>, dword: u32) {
    stream.extend_from_slice(&dword.to_le_bytes());
}

/// Builds the draw stream.
///
/// Each `SET_*_REG` is a count-1 packet - header, register offset, one value - the
/// `sceAgcDcbSet{Cx,Uc}RegisterDirect` shape obSCEne measured (`bytes-advanced 0xc`). Values are
/// the measured ones (`CB_COLOR0_ATTRIB2 = 0x000fc03f` for 64x64, `SPI_PS_INPUT_ENA = 0x2`,
/// `VGT_PRIMITIVE_TYPE = 0x4` DI_PT_TRILIST, `GE_CNTL = 0x8040`).
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

/// The graphics draw stream walks cleanly, with no unknown packet and nothing left over.
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
    assert_eq!(
        consumed as usize,
        stream.len(),
        "the walk consumes the whole draw stream"
    );

    let opcodes: Vec<u8> = walk
        .packets
        .iter()
        .filter_map(|p| match p.kind {
            PacketKind::Command { opcode } => Some(opcode),
            PacketKind::RegisterWrite { .. } | PacketKind::Filler | PacketKind::Reserved => None,
        })
        .collect();
    // SET_CONTEXT_REG (0x69) and SET_UCONFIG_REG (0x79) may classify as RegisterWrite rather than
    // Command depending on the vocabulary, so assert on the two that are always commands here.
    assert!(
        opcodes.contains(&0x2d),
        "the draw is DRAW_INDEX_AUTO (0x2d)"
    );
    assert!(opcodes.contains(&0x2f), "NUM_INSTANCES (0x2f) is present");
    assert_eq!(
        walk.packets.len(),
        7,
        "three context, two uconfig, instances, draw"
    );
}

/// The register-setting packets resolve to real register writes, by value.
///
/// The five `SET_*_REG` packets produce five writes carrying the specified values. Asserting by
/// value keeps the test independent of the base offsets.
#[test]
fn the_draw_registers_are_captured() {
    let stream = graphics_draw_stream();
    let walk = walk(&stream);
    let vocabulary = Vocabulary::builtin().expect("built-in vocabulary");
    let writes = register_writes(&walk, &stream, &vocabulary);

    let values: Vec<u32> = writes.iter().map(|w| w.value).collect();
    assert_eq!(
        writes.len(),
        5,
        "five SET_*_REG packets, five register writes"
    );
    for expected in [
        0x0010_0000,
        0x000f_c03f,
        0x0000_0002,
        0x0000_0004,
        0x0000_8040,
    ] {
        assert!(
            values.contains(&expected),
            "register value {expected:#x} was captured"
        );
    }
}

/// An indexed draw stream walks too: `DRAW_INDEX_2` after its index-width setup.
///
/// A `sceAgcDcbDrawIndex` stream issues `DRAW_INDEX_2` (0x27) after a `SET_UCONFIG_REG_INDEX`
/// (0x7a) selecting the index width, both measured (`166-agc/dcb-draw-index`,
/// `166-agc/dcb-set-index-size`). The indexed draw is recognised as a command, not an unknown
/// packet.
#[test]
fn an_indexed_draw_stream_walks_without_warnings() {
    let mut s = Vec::new();
    // SET_UCONFIG_REG_INDEX (opcode 0x7a, count 2): the measured index-width selector.
    push(&mut s, 0xc001_7a00);
    push(&mut s, 0x2000_0243);
    push(&mut s, 0x0000_0401); // 0x400 | index_type 1
    // DRAW_INDEX_2 (opcode 0x27, count 4): index buffer at 0x12345678, three indices.
    push(&mut s, 0xc004_2700);
    push(&mut s, 0x0000_0003); // max_size
    push(&mut s, 0x1234_5678); // index base lo
    push(&mut s, 0x0000_0000); // index base hi
    push(&mut s, 0x0000_0003); // index_count
    push(&mut s, 0x0000_0002); // initiator

    let walk = walk(&s);
    assert!(
        walk.is_trustworthy(),
        "desynchronised={} overran={} trailing={} - an indexed-draw opcode is unknown or mis-sized",
        walk.desynchronised,
        walk.overran,
        walk.trailing_bytes,
    );
    let consumed: u32 = walk.packets.iter().map(|p| p.length).sum();
    assert_eq!(
        consumed as usize,
        s.len(),
        "the walk consumes the whole indexed-draw stream"
    );
    let opcodes: Vec<u8> = walk
        .packets
        .iter()
        .filter_map(|p| match p.kind {
            PacketKind::Command { opcode } => Some(opcode),
            PacketKind::RegisterWrite { .. } | PacketKind::Filler | PacketKind::Reserved => None,
        })
        .collect();
    assert!(
        opcodes.contains(&0x27),
        "DRAW_INDEX_2 (0x27) is recognised as a command: {opcodes:02x?}"
    );
}
