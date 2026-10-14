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
use orbistoun_gpu_vulkan::{BufferIn, dispatch_guest_with_buffers};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    BufferFormats, ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
};

thread_local! {
    /// Whether the test running on this thread translates one invocation per lane (D880).
    static PER_LANE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `test` twice: the whole wave in one invocation, then one invocation per lane (D880). Each
/// run is held to the same expectations, so the two models are checked against each other.
fn both_models(test: impl Fn()) {
    for per_lane in [false, true] {
        println!("[{}]", if per_lane { "per lane" } else { "whole wave" });
        PER_LANE.with(|cell| cell.set(per_lane));
        test();
    }
}

/// What a module translated now asks of `UserData::per_invocation`.
fn per_invocation() -> Option<u32> {
    PER_LANE.with(std::cell::Cell::get).then_some(64)
}

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
/// `BUF_FMT_32_UINT`, with `DST_SEL_X` = X and the others `SQ_SEL_0`.
const ONE_UINT: u32 = 0x0100_0004 | 20 << 12;

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
            per_invocation: per_invocation(),
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
    both_models(|| {
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
            &[BufferIn::Words(&source), BufferIn::Words(&destination)],
        )
        .expect("dispatched");
        assert!(!done.escaped, "nothing left the bound buffers");
        assert_eq!(
            done.buffers[1].words, source,
            "the destination holds the copy"
        );
    });
}

/// A four-channel store through a one-component whole-word format writes the format's one component
/// and leaves the rest of the element as it was (D790), as Mesa relies on when it stores an image
/// buffer's four channels whatever the buffer's format (`ac_nir_to_llvm.c:2281-2293`). PPSA25872's
/// fill program stores `xyzw` through a `32_UINT` descriptor whose selects are `X, 0, 0, 0`.
#[test]
fn a_store_writes_only_its_formats_components() {
    both_models(|| {
        match probe() {
            Availability::Available { properties } => println!("device: {}", properties.device),
            Availability::Unavailable { reason } => {
                println!("SKIPPED - no device: {reason}");
                return;
            }
        }
        let module = translated(word3(UINT_32X4), ONE_UINT)
            .expect("a store with more channels than its format has components translates");
        let source: Vec<u32> = (0..ELEMENTS * 4).collect();
        let destination = vec![0xdead_beef_u32; (ELEMENTS * 4) as usize];
        let done = dispatch_guest_with_buffers(
            &module,
            &[0],
            (&user_data(word3(UINT_32X4), ONE_UINT), [1, 1, 1]),
            &DispatchImages::default(),
            &[BufferIn::Words(&source), BufferIn::Words(&destination)],
        )
        .expect("dispatched");
        for (element, words) in done.buffers[1].words.chunks(4).enumerate() {
            let first = u32::try_from(element * 4).expect("small");
            assert_eq!(
                words,
                [first, 0xdead_beef, 0xdead_beef, 0xdead_beef],
                "element {element}: x written, the rest untouched"
            );
        }
    });
}

/// A load through a null descriptor - a fourth word of zero, as radv writes for a null vertex buffer
/// and PPSA02664 binds for a stream it leaves unused - reads zero in every channel: its `DST_SEL_X..W`
/// are all `SQ_SEL_0`, so no format is consulted, and the copy stores zeros.
#[test]
fn a_load_through_a_null_descriptor_reads_zero() {
    both_models(|| {
        match probe() {
            Availability::Available { properties } => println!("device: {}", properties.device),
            Availability::Unavailable { reason } => {
                println!("SKIPPED - no device: {reason}");
                return;
            }
        }
        let module = translated(0, word3(UINT_32X4)).expect("a null source translates");
        let source = vec![0x1234_5678_u32; (ELEMENTS * 4) as usize];
        let destination = vec![0xdead_beef_u32; (ELEMENTS * 4) as usize];
        let done = dispatch_guest_with_buffers(
            &module,
            &[0],
            (&user_data(0, word3(UINT_32X4)), [1, 1, 1]),
            &DispatchImages::default(),
            &[BufferIn::Words(&source), BufferIn::Words(&destination)],
        )
        .expect("dispatched");
        assert_eq!(
            done.buffers[1].words,
            vec![0; (ELEMENTS * 4) as usize],
            "zeros stored"
        );
    });
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

/// PPSA25872's buffer copy, word for word: the thread's index is `s12` times 64 plus its lane; a
/// thread at or past the count in the constant buffer's third word drops out (`v_cmpx_gt_u32`, with
/// `s_cbranch_execz` past the copy); the rest load element `index` plus the constant buffer's second
/// word and store it at `index` plus its first.
const OFFSET_COPY: [u32; 19] = [
    0xbfa0_0001, // s_inst_prefetch 0x1
    0xd746_0001, // v_lshl_add_u32 v1, s12, 6, v0
    0x0401_0c0c,
    0xf420_1a84, // s_buffer_load_dword vcc_lo, s[8:11], 0x8
    0xfa00_0008,
    0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
    0x7da8_026a, // v_cmpx_gt_u32_e32 vcc_lo, v1
    0xbf88_000a, // s_cbranch_execz 10
    0xf424_1a84, // s_buffer_load_dwordx2 vcc, s[8:11], null
    0xfa00_0000,
    0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
    0x4a00_026b, // v_add_nc_u32_e32 v0, vcc_hi, v1
    0x4a02_026a, // v_add_nc_u32_e32 v1, vcc_lo, v1
    0xe000_2000, // buffer_load_format_x v0, v0, s[0:3], 0 idxen
    0x8000_0000,
    0xbf8c_3f70, // s_waitcnt vmcnt(0)
    0xe010_2000, // buffer_store_format_x v0, v1, s[4:7], 0 idxen
    0x8001_0001,
    0xbf81_0000, // s_endpgm
];

/// The destination a title's program over a constant buffer, a source and a destination of 128
/// elements leaves, run over two groups of 64: the whole wave in one invocation, or one invocation
/// per lane on the device's subgroup (D884). `None` where there is no device.
fn run_titles_program(
    program: &[u32],
    (constants, source): (&[u32], &[u32]),
    per_lane: bool,
) -> Option<Vec<u32>> {
    let subgroup = match probe() {
        Availability::Available { properties } => {
            println!("device: {}", properties.device);
            properties.subgroup_size
        }
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            return None;
        }
    };
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
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
            per_invocation: per_lane.then_some(subgroup),
            ..UserData::default()
        },
    )
    .expect("the title's program translates");

    let records = u32::try_from(source.len()).expect("small");
    let mut block = vec![0_u32; 32];
    for (at, base, stride, records, word3) in [
        (0, SOURCE, 4, records, TITLE_WORD3),
        (4, DESTINATION, 4, 128, TITLE_WORD3),
        (8, CONSTANTS, 16, 1, CONSTANTS_WORD3),
    ] {
        block[at] = base as u32;
        block[at + 1] = (base >> 32) as u32 & 0xffff | stride << 16;
        block[at + 2] = records;
        block[at + 3] = word3;
    }
    let done = dispatch_guest_with_buffers(
        &module,
        &[0],
        (&block, [2, 1, 1]),
        &DispatchImages::default(),
        &[
            BufferIn::Words(constants),
            BufferIn::Words(source),
            BufferIn::Words(&[0xdead_beef; 128]),
        ],
    )
    .expect("dispatched");
    assert!(!done.escaped, "nothing left the bound buffers");
    Some(done.buffers[2].words.clone())
}

/// The fill reads `vcc_lo` as a value and writes the execution mask by `v_cmpx`: a lane mask
/// whole, which one invocation per lane holds by exchanging its lanes' bits (D884). Both ways
/// store the same elements.
#[test]
fn the_titles_formatted_fill_runs_as_its_program_says() {
    let source = [0x1111_0000, 0x2222_0001, 0x3333_0002, 0x4444_0003];
    for per_lane in [false, true] {
        let Some(stored) = run_titles_program(&AGC_FILL, (&[100, 3, 0, 0], &source), per_lane)
        else {
            return;
        };
        for (index, &word) in stored.iter().enumerate() {
            let expected = if index < 100 {
                source[index & 3]
            } else {
                0xdead_beef
            };
            assert_eq!(word, expected, "per lane {per_lane}: element {index}");
        }
    }
}

/// PPSA25872's copy moves `count` elements from the source at an offset to the destination at
/// another, the same both ways (D884): 100 elements from 3 to 5.
#[test]
fn the_titles_offset_copy_runs_as_its_program_says() {
    let source: Vec<u32> = (0..128).map(|i| 0x7000_0000 | (i * 7)).collect();
    for per_lane in [false, true] {
        let Some(stored) = run_titles_program(&OFFSET_COPY, (&[5, 3, 100, 0], &source), per_lane)
        else {
            return;
        };
        for (index, &word) in stored.iter().enumerate() {
            let expected = if (5..105).contains(&index) {
                source[index - 2]
            } else {
                0xdead_beef
            };
            assert_eq!(word, expected, "per lane {per_lane}: element {index}");
        }
    }
}

/// A store whose format would need a conversion nothing has measured - eight-bit normalised - is
/// refused by name, as the packed typed stores are; and so is a store whose format lacks a
/// component it writes.
#[test]
fn a_formatted_store_needing_an_unmeasured_conversion_is_refused() {
    both_models(|| {
        let refused = translated(word3(FLOAT_32X4), word3(UNORM_8X4)).expect_err("refused");
        assert!(refused.contains("conversion"), "{refused}");
        let refused = translated(word3(FLOAT_32X4), word3(64)).expect_err("refused");
        assert!(refused.contains("component"), "{refused}");
    });
}
