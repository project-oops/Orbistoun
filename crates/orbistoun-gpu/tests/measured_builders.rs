//! The AGC packet builders, held against what the console's own builders wrote.
//!
//! # What makes this evidence
//!
//! `tests/measured_packets.rs` walks buffers a console produced and asks whether the *walker*
//! agrees. This asks the other half: whether what orbistoun **emits** is what the console emitted,
//! dword for dword, for the same arguments.
//!
//! obSCEne sweep `20260914-100833` called ten `libSceAgc` builders with known arguments on firmware
//! 12.40 and recorded the header, the body dwords it read back, and - separately, without reference
//! to any header field - how many bytes the builder advanced. Each case below replays those exact
//! arguments through `packet::build` and asserts the same bytes come out (orbistoun worklog 534).
//!
//! # What it cannot establish
//!
//! That a builder is right for arguments nobody passed. Every case here is one argument set, and
//! two builders are argument-sensitive in ways this sweep proved rather than resolved
//! (`sceAgcDcbWaitRegMem` wrote a different extent for different arguments; `sceAgcDcbEventWrite`
//! has a longer address-carrying form that was not called). A builder agreeing on one call is not a
//! builder verified.

use orbistoun_gpu::packet::{PacketKind, build, walk};

/// Bytes of a dword sequence, as the console would see them.
fn stream(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Asserts a built packet matches its capture: header, measured length, and a clean single-packet
/// walk. `advanced` is obSCEne's independently measured byte count.
fn agrees(name: &str, built: &[u32], header: u32, advanced: usize, opcode: u8) {
    assert_eq!(built[0], header, "{name}: header");
    assert_eq!(
        built.len() * 4,
        advanced,
        "{name}: emitted length against the measured advance"
    );

    let bytes = stream(built);
    let walk = walk(&bytes);
    assert!(walk.is_trustworthy(), "{name}: walk is trustworthy");
    assert_eq!(walk.packets.len(), 1, "{name}: exactly one packet");
    assert_eq!(
        walk.packets[0].kind,
        PacketKind::Command { opcode },
        "{name}: opcode"
    );
    assert_eq!(
        walk.packets[0].length as usize, advanced,
        "{name}: the walker reads back the measured length"
    );
}

/// `sceAgcDcbEventWrite(dcb, 62, 0)` wrote `0xc0004600 0x0000003e`, 8 bytes.
#[test]
fn event_write_matches_the_capture() {
    let built = build::event_write(62);
    agrees(
        "event_write",
        &built,
        0xc000_4600,
        8,
        build::measured::EVENT_WRITE,
    );
    assert_eq!(built[1], 0x3e, "the event type passes through unchanged");
}

/// `sceAgcDcbSetIndexCount(dcb, 36)` wrote `0xc0001300 0x00000024`, 8 bytes.
#[test]
fn set_index_count_matches_the_capture() {
    let built = build::set_index_count(36);
    agrees(
        "set_index_count",
        &built,
        0xc000_1300,
        8,
        build::measured::INDEX_BUFFER_SIZE,
    );
    assert_eq!(built[1], 0x24);
}

/// `sceAgcDcbSetNumInstances(dcb, 1)` wrote `0xc0002f00 0x00000001`, 8 bytes.
#[test]
fn set_num_instances_matches_the_capture() {
    let built = build::set_num_instances(1);
    agrees(
        "set_num_instances",
        &built,
        0xc000_2f00,
        8,
        build::measured::NUM_INSTANCES,
    );
    assert_eq!(built[1], 1);
}

/// `sceAgcDcbDrawIndexAuto(dcb, 3, 2)` wrote `0xc0012d00 3 2`, 12 bytes.
#[test]
fn draw_index_auto_matches_the_capture() {
    let built = build::draw_index_auto(3, 2);
    agrees(
        "draw_index_auto",
        &built,
        0xc001_2d00,
        12,
        build::measured::DRAW_INDEX_AUTO,
    );
    assert_eq!(built[1..], [3, 2], "count then initiator, in order");
}

/// `sceAgcDcbSetIndexBuffer(dcb, 0x12345678)` wrote `0xc0012600 0x12345678 0`, 12 bytes.
#[test]
fn set_index_base_matches_the_capture() {
    let built = build::set_index_base(0x1234_5678);
    agrees(
        "set_index_base",
        &built,
        0xc001_2600,
        12,
        build::measured::INDEX_BASE,
    );
    assert_eq!(built[1..], [0x1234_5678, 0], "low half first");
}

/// `sceAgcDcbSetCxRegisterDirect(dcb, (0x12345678 << 32) | 0x200)` wrote
/// `0xc0016900 0x200 0x12345678`, 12 bytes.
#[test]
fn set_context_register_matches_the_capture() {
    let built = build::set_context_register(0x200, 0x1234_5678);
    agrees(
        "set_context_register",
        &built,
        0xc001_6900,
        12,
        build::measured::SET_CONTEXT_REG,
    );
    assert_eq!(built[1..], [0x200, 0x1234_5678], "offset then value");
}

/// `sceAgcDcbSetUcRegisterDirect(dcb, (4 << 32) | 0x242)` wrote `0xc0017900 0x242 4`, 12 bytes.
#[test]
fn set_uconfig_register_matches_the_capture() {
    let built = build::set_uconfig_register(0x242, 4);
    agrees(
        "set_uconfig_register",
        &built,
        0xc001_7900,
        12,
        build::measured::SET_UCONFIG_REG,
    );
    assert_eq!(built[1..], [0x242, 4]);
}

/// `sceAgcCbSetShRegisterRangeDirect(cb, 8, [0x12345678, 0x9abcdef0], 2)` wrote
/// `0xc0027600 8 0x12345678 0x9abcdef0`, 16 bytes - and **no leading marker**.
#[test]
fn set_sh_register_range_matches_the_capture() {
    let built = build::set_sh_register_range(8, &[0x1234_5678, 0x9abc_def0]);
    agrees(
        "set_sh_register_range",
        &built,
        0xc002_7600,
        16,
        build::measured::SET_SH_REG,
    );
    assert_eq!(built[1..], [8, 0x1234_5678, 0x9abc_def0]);
    assert_ne!(
        built[0], 0x6875_000d,
        "the real library prepends no NOP marker - the packet starts with the register write"
    );
}

/// `n` registers cost `n + 2` dwords, which is the rule the capture establishes at `n == 2`.
#[test]
fn set_sh_register_range_costs_two_dwords_of_overhead() {
    for n in 1..8u32 {
        let values: Vec<u32> = (0..n).collect();
        let built = build::set_sh_register_range(8, &values);
        assert_eq!(built.len(), n as usize + 2, "{n} registers cost n + 2");
        let walk = walk(&stream(&built));
        assert!(walk.is_trustworthy(), "{n}: walk is trustworthy");
        assert_eq!(walk.packets.len(), 1, "{n}: one packet");
    }
}

/// An empty run is not a packet, and must not become a header claiming a body.
#[test]
fn an_empty_register_run_emits_nothing() {
    assert!(build::set_sh_register_range(8, &[]).is_empty());
}

/// `sceAgcDcbDrawIndex(dcb, 3, 0x12345678, 0)` wrote header `0xc0042700` and advanced 24 bytes,
/// with the address halves and the index count read back at body positions 1, 2 and 3.
#[test]
fn draw_index_2_matches_the_capture_where_it_was_read() {
    let built = build::draw_index_2(0xdead_beef, 0x1234_5678, 3, 0xfeed);
    agrees(
        "draw_index_2",
        &built,
        0xc004_2700,
        24,
        build::measured::DRAW_INDEX_2,
    );
    assert_eq!(built[2], 0x1234_5678, "body[1] is the address low half");
    assert_eq!(built[3], 0, "body[2] is the address high half");
    assert_eq!(built[4], 3, "body[3] is the index count");
    assert_eq!(built[1], 0xdead_beef, "body[0] is the caller's max_size");
    assert_eq!(built[5], 0xfeed, "body[4] is the caller's initiator");
}

/// The length rule closes on every one: `(count + 2) * 4` equals the measured advance.
#[test]
fn every_measured_builder_closes_its_own_length_arithmetic() {
    let cases: [(&str, Vec<u32>, usize); 8] = [
        ("event_write", build::event_write(62).to_vec(), 8),
        ("set_index_count", build::set_index_count(36).to_vec(), 8),
        ("set_num_instances", build::set_num_instances(1).to_vec(), 8),
        ("draw_index_auto", build::draw_index_auto(3, 2).to_vec(), 12),
        (
            "set_index_base",
            build::set_index_base(0x1234_5678).to_vec(),
            12,
        ),
        (
            "set_context_register",
            build::set_context_register(0x200, 0x1234_5678).to_vec(),
            12,
        ),
        (
            "set_uconfig_register",
            build::set_uconfig_register(0x242, 4).to_vec(),
            12,
        ),
        (
            "draw_index_2",
            build::draw_index_2(0, 0x1234_5678, 3, 0).to_vec(),
            24,
        ),
    ];
    for (name, words, advanced) in cases {
        let count = ((words[0] >> 16) & 0x3fff) as usize;
        assert_eq!(
            (count + 2) * 4,
            advanced,
            "{name}: (count + 2) dwords is the measured advance"
        );
    }
}

/// **All eight measured argument pairs**, from the sweep that made this builder implementable.
///
/// `sceAgcDcbSetIndexSize(dcb, type, flags)` for `type` 0-3 and `flags` 0-1
/// (`166-agc/dcb-set-index-size`, sweep `20260914-222710`). One point fixes nothing - worklog 536
/// refused this builder on exactly that ground - and eight fix the two low bits and bit 6.
#[test]
fn set_index_size_matches_every_measured_pair() {
    let measured = [
        ((0, 0), 0x400_u32),
        ((0, 1), 0x440),
        ((1, 0), 0x401),
        ((1, 1), 0x441),
        ((2, 0), 0x402),
        ((2, 1), 0x442),
        ((3, 0), 0x403),
        ((3, 1), 0x443),
    ];
    for ((index_type, flags), value) in measured {
        let built = build::set_index_size(index_type, flags);
        assert_eq!(
            built,
            [0xc001_7a00, 0x2000_0243, value],
            "type {index_type}, flags {flags}"
        );
    }
}

/// `sceAgcDcbDrawIndex(dcb, 3, 0x12345678, 0)` wrote `0xc0042700 3 0x12345678 0 3 0`, 24 bytes -
/// including the two dwords worklog 536 could not place.
#[test]
fn draw_index_2_matches_the_capture_in_full() {
    let built = build::draw_index_2(3, 0x1234_5678, 3, 0);
    agrees(
        "draw_index_2",
        &built,
        0xc004_2700,
        24,
        build::measured::DRAW_INDEX_2,
    );
    assert_eq!(
        built,
        [0xc004_2700, 3, 0x1234_5678, 0, 3, 0],
        "every dword is now measured, not three of five"
    );
}
