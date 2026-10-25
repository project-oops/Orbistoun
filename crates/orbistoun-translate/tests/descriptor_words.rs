//! An image descriptor a dispatch loads from its table names the image the host binds, and its
//! words are the guest's too: a program may read them as values (D970).

use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, UserData, Window, draw_buffers_for,
    translate_with_user_data,
};

/// A dispatch whose table is user data `s[12:13]`, as `llvm-mc` assembles it for this target:
///
/// ```text
/// s_load_dwordx8 s[16:23], s[12:13], 0x20       an image descriptor from the table
/// s_waitcnt lgkmcnt(0)
/// image_load v2, v[0:1], s[16:23] dmask:0x1 dim:SQ_RSRC_IMG_2D
/// s_cmp_lg_u32 0, s20                           a word of it, read as a value
/// s_cselect_b32 s24, s21, 0
/// v_mov_b32 v3, s24
/// s_waitcnt vmcnt(0)
/// s_endpgm
/// ```
///
/// Grand Theft Auto V's programs at `0x74005b7e8600` (word 5 of a sampled image's descriptor) and
/// `0x74005b809c00` read registers a descriptor load filled.
const READS_A_DESCRIPTOR_WORD: [u32; 10] = [
    0xf40c_0406,
    0xfa00_0020,
    0xbf8c_c07f,
    0xf000_0108,
    0x0004_0200,
    0xbf07_1480,
    0x8518_8015,
    0x7e06_0218,
    0xbf8c_3f70,
    0xbf81_0000,
];

/// The table's words are bound as the buffer the tracer gives the load, and the load reads them
/// through it while the image instruction binds the image the load names: the dispatch translates,
/// where a load taken for the image's name alone was refused for the value read.
#[test]
fn a_dispatch_reads_the_words_of_an_image_descriptor_it_loaded() {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = READS_A_DESCRIPTOR_WORD
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
    let user_data = UserData {
        count: 14,
        compute: Some(ComputeInputs {
            thread_id_components: 2,
            threads: [8, 8, 1],
            ..ComputeInputs::default()
        }),
        draw_buffers: true,
        ..UserData::default()
    };
    let traced = draw_buffers_for(&decoded, &encodings, Stage::Compute, user_data)
        .expect("the program traces");
    assert!(
        traced.served.contains_key(&0),
        "the descriptor load reads the table through a buffer: {:?}",
        traced.served
    );
    let (_, _, (textures, _)) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        user_data,
    )
    .expect("a descriptor word read as a value translates");
    assert_eq!(
        textures.first().and_then(|texture| texture.table_offset),
        Some(0x20),
        "the fetch binds the image the load names"
    );
}

/// `words` translated as a dispatch whose table is user data `s[12:13]`, with no buffers bound: an
/// image descriptor it loads is then taken as the image's name alone.
fn translated_unbound(
    words: &[u32],
) -> Result<
    (
        Vec<u32>,
        usize,
        orbistoun_translate::wavefront::ImageSources,
    ),
    orbistoun_translate::TranslateError,
> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 14,
            compute: Some(ComputeInputs {
                thread_id_components: 2,
                threads: [8, 8, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
}

/// The load lies between the write and the read in the program's bytes, but on the other path:
///
/// ```text
/// 0x00 s_cmp_eq_u32 s0, 0
/// 0x04 s_cbranch_scc1 load
/// 0x08 s_mov_b32 s20, 5
/// 0x0c s_branch read
/// 0x10 load: s_load_dwordx8 s[16:23], s[12:13], 0x20
/// 0x18 s_waitcnt lgkmcnt(0)
/// 0x1c image_load v2, v[0:1], s[16:23] dmask:0x1 dim:SQ_RSRC_IMG_2D
/// 0x24 s_endpgm
/// 0x28 read: s_add_u32 s24, s20, 1
/// 0x2c v_mov_b32 v3, s24
/// 0x30 s_endpgm
/// ```
const LOAD_ON_THE_OTHER_PATH: [u32; 13] = [
    0xbf06_8000,
    0xbf85_0002,
    0xbe94_0385,
    0xbf82_0006,
    0xf40c_0406,
    0xfa00_0020,
    0xbf8c_c07f,
    0xf000_0108,
    0x0004_0200,
    0xbf81_0000,
    0x8018_8114,
    0x7e06_0218,
    0xbf81_0000,
];

/// The read lies before the load in the program's bytes, after a write, but the load reaches it
/// through the jump back:
///
/// ```text
/// 0x00 s_cmp_eq_u32 s0, 0
/// 0x04 s_mov_b32 s20, 5
/// 0x08 s_cbranch_scc1 load
/// 0x0c read: s_add_u32 s24, s20, 1
/// 0x10 v_mov_b32 v3, s24
/// 0x14 s_endpgm
/// 0x18 load: s_load_dwordx8 s[16:23], s[12:13], 0x20
/// 0x20 s_waitcnt lgkmcnt(0)
/// 0x24 image_load v2, v[0:1], s[16:23] dmask:0x1 dim:SQ_RSRC_IMG_2D
/// 0x2c s_branch read
/// ```
const LOAD_REACHES_THE_READ_BACKWARD: [u32; 12] = [
    0xbf06_8000,
    0xbe94_0385,
    0xbf85_0003,
    0x8018_8114,
    0x7e06_0218,
    0xbf81_0000,
    0xf40c_0406,
    0xfa00_0020,
    0xbf8c_c07f,
    0xf000_0108,
    0x0004_0200,
    0xbf82_fff7,
];

/// A write lies between the load and the read both in the program's bytes and in the order the
/// translation emits blocks - a branch's taken block, then the rest - but on another path: the way
/// that falls through jumps from the load straight to the read.
///
/// ```text
/// 0x00 s_cmp_eq_u32 s0, 0
/// 0x04 s_load_dwordx8 s[16:23], s[12:13], 0x20
/// 0x0c s_waitcnt lgkmcnt(0)
/// 0x10 image_load v2, v[0:1], s[16:23] dmask:0x1 dim:SQ_RSRC_IMG_2D
/// 0x18 s_cbranch_scc1 write
/// 0x1c s_branch read
/// 0x20 write: s_mov_b32 s20, 5
/// 0x24 s_endpgm
/// 0x28 read: s_add_u32 s24, s20, 1
/// 0x2c v_mov_b32 v3, s24
/// 0x30 s_endpgm
/// ```
const WRITE_ON_A_PATH_PAST_THE_LOAD: [u32; 13] = [
    0xbf06_8000,
    0xf40c_0406,
    0xfa00_0020,
    0xbf8c_c07f,
    0xf000_0108,
    0x0004_0200,
    0xbf85_0001,
    0xbf82_0002,
    0xbe94_0385,
    0xbf81_0000,
    0x8018_8114,
    0x7e06_0218,
    0xbf81_0000,
];

/// A write the translation emits between a name-only load and a read, on a path that does not lead
/// to the read, does not clear the name: the read is refused (D970).
#[test]
fn a_write_on_another_path_does_not_clear_a_name() {
    match translated_unbound(&WRITE_ON_A_PATH_PAST_THE_LOAD) {
        Err(orbistoun_translate::TranslateError::Unsupported { offset, detail }) => {
            assert_eq!(offset, 0x28, "named at the read: {detail}");
            assert!(detail.contains("as a value"), "{detail}");
        }
        other => panic!("the read the load reaches is refused, not {other:?}"),
    }
}

/// Registers holding only an image's name are followed along the program's paths, not in the
/// order the translation emits its blocks (D970): a read on the path the load is not on is the
/// program's own value, and translates.
#[test]
fn a_name_only_load_on_another_path_is_no_value_read() {
    let (_, _, (textures, _)) =
        translated_unbound(&LOAD_ON_THE_OTHER_PATH).expect("the read on the other path translates");
    assert_eq!(
        textures.first().and_then(|texture| texture.table_offset),
        Some(0x20),
        "the fetch binds the image the load names"
    );
}

/// A read a name-only load reaches through a jump back is refused, at the read, though a write
/// comes before it in the program's bytes.
#[test]
fn a_name_only_load_reaching_a_read_backward_is_refused_at_the_read() {
    match translated_unbound(&LOAD_REACHES_THE_READ_BACKWARD) {
        Err(orbistoun_translate::TranslateError::Unsupported { offset, detail }) => {
            assert_eq!(offset, 0xc, "named at the read: {detail}");
            assert!(detail.contains("as a value"), "{detail}");
        }
        other => panic!("the read the load reaches is refused, not {other:?}"),
    }
}
