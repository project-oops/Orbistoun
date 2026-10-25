//! A descriptor a dispatch selects from a table at run time is traced to the table (D966).

use super::{
    BufferSource, DescriptorReads, DescriptorWord, DrawBuffers, SelectedTable, trace_with,
};
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};

/// Traces a dispatch's program of words, its eight user-data words from `s0`.
fn dispatch(words: &[u32]) -> DrawBuffers {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode_program(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
    trace_with(&decoded, &encodings, (0, 8), (true, false, true))
}

/// The table's descriptor, user-data words 0 to 3.
const TABLE: [DescriptorWord; 4] = [
    DescriptorWord::UserData(0),
    DescriptorWord::UserData(1),
    DescriptorWord::UserData(2),
    DescriptorWord::UserData(3),
];

/// A waterfall over the lanes' records, as Grand Theft Auto V's program at `0x74005b857200` runs
/// one: each pass reads the first lane's record number, selects that record's descriptor from the
/// table in `s[0:3]` at sixteen bytes a record, and fetches through it for the lanes that hold it.
///
/// ```text
/// 0x00 v_and_b32_e32 v1, 3, v0
/// 0x04 v_lshlrev_b32_e32 v3, 2, v0
/// 0x08 v_lshrrev_b32_e32 v4, 2, v0
/// 0x0c s_mov_b64 s[20:21], exec
/// 0x10 v_readfirstlane_b32 s12, v1
/// 0x14 v_cmp_eq_u32_e32 vcc, s12, v1
/// 0x18 s_and_saveexec_b64 s[22:23], vcc
/// 0x1c s_lshl_b32 s12, s12, 4
/// 0x20 s_buffer_load_dwordx4 s[8:11], s[0:3], s12
/// 0x28 s_waitcnt lgkmcnt(0)
/// 0x2c buffer_load_format_x v2, v4, s[8:11], 0 idxen
/// 0x34 s_andn2_b64 exec, s[22:23], vcc
/// 0x38 s_cbranch_execnz 0x10
/// 0x3c s_mov_b64 exec, s[20:21]
/// 0x40 s_waitcnt vmcnt(0)
/// 0x44 buffer_store_dword v2, v3, s[4:7], 0 offen
/// 0x4c s_endpgm
/// ```
pub(crate) const WATERFALL: [u32; 21] = [
    0x3602_0083,
    0x3406_0082,
    0x2c08_0082,
    0xbe94_047e,
    0x7e18_0501,
    0x7d84_020c,
    0xbe96_246a,
    0x8f0c_840c,
    0xf428_0200,
    0x1800_0000,
    0xbf8c_c07f,
    0xe000_2000,
    0x8002_0204,
    0x8afe_6a16,
    0xbf89_fff5,
    0xbefe_0414,
    0xbf8c_3f70,
    0xe070_1000,
    0x8001_0203,
    0xbf81_0000,
    0xbf9f_0000,
];

/// The fetch through a descriptor selected from the table reads through the table, and the load
/// that selects it reads the table's whole buffer, whatever offset it computes: four slots - the
/// table's own buffer, the selected record, the store's buffer, and the global buffer of pages the
/// records are bound in.
#[test]
fn a_fetch_through_a_selected_descriptor_reads_through_its_table() {
    let traced = dispatch(&WATERFALL);
    assert_eq!(traced.sources.len(), 4, "{:#?}", traced.sources);
    assert_eq!(traced.served.get(&0x20), Some(&0), "the selecting load");
    assert_eq!(traced.served.get(&0x2c), Some(&1), "the fetch");
    assert_eq!(traced.served.get(&0x44), Some(&2), "the store");
    assert_eq!(
        traced.sources[0],
        BufferSource::Descriptor {
            words: TABLE,
            reads: DescriptorReads {
                scalar_reach: u32::MAX,
                ..DescriptorReads::default()
            },
        },
        "the table, read whole"
    );
    assert!(matches!(traced.sources[1], BufferSource::Computed(_)));
    assert!(matches!(traced.sources[3], BufferSource::Global(_)));
    assert_eq!(
        traced.tables.get(&1),
        Some(&SelectedTable {
            words: TABLE,
            element: 0,
        })
    );
    assert!(traced.several.is_empty(), "one fetch reaches the record");
}

/// A program that is not a dispatch's keeps its computed descriptor as one the host finds by
/// running the program (D753): only a dispatch's records are bound in a buffer of pages.
#[test]
fn only_a_dispatch_selects_from_a_table() {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    // The waterfall without its store, which a draw's snapshot would not see.
    let mut words = WATERFALL[..17].to_vec();
    words.extend_from_slice(&WATERFALL[19..]);
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode_program(&bytes, &encodings, &operands);
    let traced = trace_with(&decoded, &encodings, (0, 8), (false, false, false));
    assert!(traced.tables.is_empty());
    assert!(
        traced
            .sources
            .iter()
            .all(|source| !matches!(source, BufferSource::Global(_)))
    );
}

/// The selected descriptor is followed through moves and past a branch, and two fetches through
/// records of one table, in different blocks, share it and are marked as several: a run up to the
/// first need not find the record the second reads.
///
/// ```text
/// 0x00 v_readfirstlane_b32 s12, v1
/// 0x04 s_buffer_load_dwordx4 s[8:11], s[0:3], s12
/// 0x0c s_waitcnt lgkmcnt(0)
/// 0x10 buffer_load_format_x v2, v4, s[8:11], 0 idxen
/// 0x18 s_mov_b32 s20, s8
/// 0x1c s_mov_b32 s21, s9
/// 0x20 s_mov_b32 s22, s10
/// 0x24 s_mov_b32 s23, s11
/// 0x28 s_cbranch_scc0 1        (to 0x30)
/// 0x2c s_mov_b32 s5, 0
/// 0x30 buffer_load_dword v5, v4, s[20:23], 0 idxen
/// 0x38 s_endpgm
/// ```
#[test]
fn a_selected_descriptor_is_followed_through_moves_and_branches() {
    let program = [
        0x7e18_0501,
        0xf428_0200,
        0x1800_0000,
        0xbf8c_c07f,
        0xe000_2000,
        0x8002_0204,
        0xbe94_0308,
        0xbe95_0309,
        0xbe96_030a,
        0xbe97_030b,
        0xbf84_0001,
        0xbe85_0380,
        0xe030_2000,
        0x8005_0504,
        0xbf81_0000,
        0xbf9f_0000,
    ];
    let traced = dispatch(&program);
    let fetch = traced.served.get(&0x10).copied();
    assert!(fetch.is_some());
    assert_eq!(traced.served.get(&0x30).copied(), fetch, "one table");
    let slot = fetch.expect("served");
    assert_eq!(
        traced.tables.get(&slot).map(|table| table.words),
        Some(TABLE)
    );
    assert!(traced.several.contains(&slot), "two places read it");
}

/// A register of the selected descriptor written between the selection and the fetch leaves the
/// fetch's descriptor computed, not a record of the table: it is the program's own, which the host
/// finds by running the program.
#[test]
fn a_rewritten_descriptor_is_not_a_record() {
    let program = [
        0x7e18_0501, // v_readfirstlane_b32 s12, v1
        0xf428_0200, // s_buffer_load_dwordx4 s[8:11], s[0:3], s12
        0x1800_0000,
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xbe8a_0380, // s_mov_b32 s10, 0
        0xe000_2000, // buffer_load_format_x v2, v4, s[8:11], 0 idxen
        0x8002_0204,
        0xbf81_0000,
        0xbf9f_0000,
    ];
    let traced = dispatch(&program);
    let slot = traced.served.get(&0x14).copied().expect("served");
    assert!(matches!(
        traced.sources[slot as usize],
        BufferSource::Computed(_)
    ));
    assert!(traced.tables.is_empty());
}

/// A two-word scalar load forgets the two registers it writes and no more: the pair after it still
/// holds the table base the program loads a descriptor through, as Grand Theft Auto V's program at
/// `0x74005b854a00` loads `s[8:11]` from `s[4:5]` + 0x60 after `s_buffer_load_dwordx2 s[2:3]`, and
/// its fetch through the descriptor at 0x5e0 is a traced one.
///
/// ```text
/// 0x00 s_buffer_load_dwordx2 s[2:3], s[0:3], 0x180
/// 0x08 s_load_dwordx4 s[8:11], s[4:5], 0x60
/// 0x10 s_waitcnt lgkmcnt(0)
/// 0x14 buffer_load_format_x v1, v0, s[8:11], 0 idxen
/// ```
#[test]
fn a_two_word_load_leaves_the_next_pair_traced() {
    let program = [
        0xf424_0080,
        0xfa00_0180,
        0xf408_0202,
        0xfa00_0060,
        0xbf8c_c07f,
        0xe000_2000,
        0x8002_0100,
        0xbf81_0000,
        0xbf9f_0000,
    ];
    let traced = dispatch(&program);
    let slot = traced.served.get(&0x14).copied().expect("served");
    let BufferSource::Descriptor { words, .. } = traced.sources[slot as usize] else {
        panic!("{:?}", traced.sources[slot as usize]);
    };
    let table = crate::wavefront::TableBase {
        low: crate::wavefront::TableWord::UserData(4),
        high: crate::wavefront::TableWord::UserData(5),
    };
    assert_eq!(
        words,
        [0x60, 0x64, 0x68, 0x6c].map(|offset| DescriptorWord::Loaded { table, offset })
    );
}
