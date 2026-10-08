//! A guest compute dispatch whose loads and stores convert by their descriptors' formats (D738),
//! run on the device.
//!
//! PPSA03416's and PPSA02664's first dispatch is a formatted buffer copy: a `buffer_load_format`
//! through the descriptor in user-data words 0-3 and a `buffer_store_format` through the one in
//! words 4-7, each element at the thread's index. This is its four-channel form, encoded by LLVM
//! 18.1.8 for gfx1013:
//!
//! ```text
//! buffer_load_format_xyzw v[1:4], v0, s[0:3], 0 idxen
//! s_waitcnt vmcnt(0)
//! buffer_store_format_xyzw v[1:4], v0, s[4:7], 0 idxen
//! s_endpgm
//! ```

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::dispatch_guest_with_buffers;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    BufferFormats, ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
};

const COPY: [u32; 7] = [
    0xe00c_2000,
    0x8000_0100,
    0xbf8c_3f70,
    0xe01c_2000,
    0x8001_0100,
    0xbf81_0000,
    0xbf9f_0000,
];

/// `SQ_BUF_RSRC_WORD3` with the identity selects (`DST_SEL_X..W` = 4..7, `0xfac`),
/// `RESOURCE_LEVEL` 1 and `OOB_SELECT` 0, structured, with the format `code` in bits 18:12.
const fn word3(code: u32) -> u32 {
    0x0100_0fac | code << 12
}
const FLOAT_32X4: u32 = 77;
const UINT_32X4: u32 = 75;
const UNORM_8X4: u32 = 56;

const SOURCE: u64 = 0x4_0a60_0000;
const DESTINATION: u64 = 0x4_1260_0000;
const ELEMENTS: u32 = 64;

/// Two descriptors of 64 sixteen-byte elements each.
fn user_data(source: u32, destination: u32) -> Vec<u32> {
    let mut block = vec![0_u32; 32];
    for (at, base, word3) in [(0, SOURCE, source), (4, DESTINATION, destination)] {
        block[at] = base as u32;
        block[at + 1] = (base >> 32) as u32 & 0xffff | 16 << 16;
        block[at + 2] = ELEMENTS;
        block[at + 3] = word3;
    }
    block
}

fn translated(source: u32, destination: u32) -> Result<Vec<u32>, String> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = COPY.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let mut formats = BufferFormats::default();
    formats.set(0, Some(source));
    formats.set(1, Some(destination));
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            first_register: 0,
            count: 8,
            block_offset: 0,
            compute: Some(ComputeInputs {
                workgroup_ids: [false; 3],
                thread_id_components: 1,
                threads: [ELEMENTS, 1, 1],
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            buffer_formats: Some(formats),
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
    .map_err(|e| e.to_string())
}

/// Each thread loads its float element and stores it through a four-word unsigned descriptor:
/// whole-word formats move their words unconverted, so the destination holds the source's bits.
#[test]
fn a_formatted_copy_moves_whole_word_elements() {
    match probe() {
        Availability::Available { properties } => println!("device: {}", properties.device),
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            return;
        }
    }
    let module = translated(word3(FLOAT_32X4), word3(UINT_32X4))
        .expect("the copy translates through its descriptors' formats");
    let source: Vec<u32> = (0..ELEMENTS * 4)
        .map(|i| (f32::from(u16::try_from(i).expect("small")) * 0.25).to_bits())
        .collect();
    let destination = vec![0xdead_beef_u32; (ELEMENTS * 4) as usize];
    let done = dispatch_guest_with_buffers(
        &module,
        &[0],
        (&user_data(word3(FLOAT_32X4), word3(UINT_32X4)), [1, 1, 1]),
        &DispatchImages::default(),
        &[source.clone(), destination],
    )
    .expect("dispatched");
    assert!(!done.escaped, "nothing left the bound buffers");
    assert_eq!(done.buffers[1], source, "the destination holds the copy");
}

/// PPSA03416's and PPSA02664's first dispatch, word for word: the thread's index is the group's
/// `s12` times 64 plus its lane (`v_lshl_add_u32`); a thread at or past the count in the constant
/// buffer at words 8-11 drops out (`v_cmpx_gt_u32`); the rest load element `index & mask`, the
/// mask the constant buffer's second word, as `BUF_FMT_32_UINT` with select X (word3 `0x14004`, as
/// the title's own descriptors carry it) and store it at their index.
const AGC_FILL: [u32; 18] = [
    0xbfa0_0001, // s_inst_prefetch 0x1
    0xd746_0000, // v_lshl_add_u32 v0, s12, 6, v0
    0x0401_0c0c,
    0xf420_1a84, // s_buffer_load_dword vcc_lo, s[8:11], null
    0xfa00_0000,
    0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
    0x7da8_006a, // v_cmpx_gt_u32_e32 vcc_lo, v0
    0xbf88_0009, // s_cbranch_execz 9
    0xf420_1a84, // s_buffer_load_dword vcc_lo, s[8:11], 0x4
    0xfa00_0004,
    0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
    0x3602_006a, // v_and_b32_e32 v1, vcc_lo, v0
    0xe000_2000, // buffer_load_format_x v1, v1, s[0:3], 0 idxen
    0x8000_0101,
    0xbf8c_3f70, // s_waitcnt vmcnt(0)
    0xe010_2000, // buffer_store_format_x v1, v0, s[4:7], 0 idxen
    0x8001_0100,
    0xbf81_0000, // s_endpgm
];

/// The title's descriptor word 3: `BUF_FMT_32_UINT` (20), `DST_SEL_X` X and the rest 0.
const TITLE_WORD3: u32 = 0x0001_4004;
/// The title's constant-buffer word 3, `0x4dfac`: identity selects and `BUF_FMT_32_32_32_32_FLOAT`,
/// through a descriptor of one sixteen-byte record - so its record count is in strides (radeonsi
/// `si_state.c:3506-3527`) and the two words the program loads are inside it.
const CONSTANTS_WORD3: u32 = 0x0004_dfac;
const CONSTANTS: u64 = 0x4_1a60_0000;

#[test]
fn the_titles_formatted_fill_runs_as_its_program_says() {
    match probe() {
        Availability::Available { properties } => println!("device: {}", properties.device),
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            return;
        }
    }
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = AGC_FILL.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let mut formats = BufferFormats::default();
    // Slots in the order the program first reaches each buffer: constants, source, destination.
    formats.set(0, Some(CONSTANTS_WORD3));
    formats.set(1, Some(TITLE_WORD3));
    formats.set(2, Some(TITLE_WORD3));
    let (module, _, _) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            first_register: 0,
            count: 12,
            block_offset: 0,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [64, 1, 1],
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            buffer_formats: Some(formats),
            ..UserData::default()
        },
    )
    .expect("the title's fill translates");

    let mut block = vec![0_u32; 32];
    for (at, base, stride, records, word3) in [
        (0, SOURCE, 4, 4, TITLE_WORD3),
        (4, DESTINATION, 4, 128, TITLE_WORD3),
        (8, CONSTANTS, 16, 1, CONSTANTS_WORD3),
    ] {
        block[at] = base as u32;
        block[at + 1] = (base >> 32) as u32 & 0xffff | stride << 16;
        block[at + 2] = records;
        block[at + 3] = word3;
    }
    let source = vec![0x1111_0000, 0x2222_0001, 0x3333_0002, 0x4444_0003];
    let done = dispatch_guest_with_buffers(
        &module,
        &[0],
        (&block, [2, 1, 1]),
        &DispatchImages::default(),
        &[vec![100, 3, 0, 0], source.clone(), vec![0xdead_beef; 128]],
    )
    .expect("dispatched");
    assert!(!done.escaped, "nothing left the bound buffers");
    for (index, &stored) in done.buffers[2].iter().enumerate() {
        let expected = if index < 100 {
            source[index & 3]
        } else {
            0xdead_beef
        };
        assert_eq!(stored, expected, "element {index}");
    }
}

/// A store whose format would need a conversion nothing has measured - eight-bit normalised - is
/// refused by name, as the packed typed stores are; and so is a store whose format lacks a
/// component it writes.
#[test]
fn a_formatted_store_needing_an_unmeasured_conversion_is_refused() {
    let refused = translated(word3(FLOAT_32X4), word3(UNORM_8X4)).expect_err("refused");
    assert!(refused.contains("conversion"), "{refused}");
    let refused = translated(word3(FLOAT_32X4), word3(64)).expect_err("refused");
    assert!(refused.contains("component"), "{refused}");
}
