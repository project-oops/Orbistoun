//! A dispatch that selects each lane's buffer descriptor from a table at run time, run on the device
//! with every record of the table bound in its global buffer of pages (D966).
//!
//! The program is a waterfall over four records, as Grand Theft Auto V's program at
//! `0x74005b857200` runs one: each lane's record is its id's low two bits, and it fetches the element
//! its id's upper bits index from the buffer that record's descriptor names. The four buffers lie a
//! gigabyte apart, so no one range holds them; the module reads each through the pages that hold it.
//!
//! ```text
//! v_and_b32_e32 v1, 3, v0
//! v_lshlrev_b32_e32 v3, 2, v0
//! v_lshrrev_b32_e32 v4, 2, v0
//! s_mov_b64 s[20:21], exec
//! loop:
//! v_readfirstlane_b32 s12, v1
//! v_cmp_eq_u32_e32 vcc, s12, v1
//! s_and_saveexec_b64 s[22:23], vcc
//! s_lshl_b32 s12, s12, 4
//! s_buffer_load_dwordx4 s[8:11], s[0:3], s12
//! s_waitcnt lgkmcnt(0)
//! buffer_load_format_x v2, v4, s[8:11], 0 idxen     (or buffer_load_dword)
//! s_andn2_b64 exec, s[22:23], vcc
//! s_cbranch_execnz loop
//! s_mov_b64 exec, s[20:21]
//! s_waitcnt vmcnt(0)
//! buffer_store_dword v2, v3, s[4:7], 0 offen
//! s_endpgm
//! ```

use orbistoun_gpu::global_pages::{GlobalPages, PAGE};
use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::{BufferIn, dispatch_guest_with_buffers};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    BufferFormats, ComputeInputs, MeshPrimitive, Stage, USER_DATA_BLOCK_WORDS, UserData, Window,
    translate_with_user_data,
};

/// The waterfall, fetching with `fetch`'s two words.
fn program(fetch: [u32; 2]) -> Vec<u32> {
    let mut words = vec![
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
    ];
    words.extend_from_slice(&fetch);
    words.extend_from_slice(&[
        0x8afe_6a16,
        0xbf89_fff5,
        0xbefe_0414,
        0xbf8c_3f70,
        0xe070_1000,
        0x8001_0203,
        0xbf81_0000,
        0xbf9f_0000,
    ]);
    words
}

/// `buffer_load_format_x v2, v4, s[8:11], 0 idxen`.
const FORMAT_FETCH: [u32; 2] = [0xe000_2000, 0x8002_0204];
/// `buffer_load_dword v2, v4, s[8:11], 0 idxen`.
const WORD_FETCH: [u32; 2] = [0xe030_2000, 0x8002_0204];

/// `R32_UINT` with the identity selects, out-of-bounds mode 1: the index checked alone.
const R32_UINT: u32 = 0x1000_0000 | 20 << 12 | 0xfac;
/// `SQ_BUF_RSRC_WORD3` as radeonsi's raw descriptors carry it: `OOB_SELECT` 3.
const RAW: u32 = 0x3101_6fac;

/// Where the table lies, and the store's buffer.
const TABLE: u64 = 0x4_0a00_0000;
const DESTINATION: u64 = 0x4_0b00_0000;

/// Each record's buffer, a gigabyte and some apart, sixteen words each.
fn record_base(record: u64) -> u64 {
    0x4_0a60_0000 + record * ((1 << 30) + 0x40)
}

/// The word record `record`'s buffer holds at element `index`.
fn element(record: u64, index: u64) -> u32 {
    0x1000 * (record as u32 + 1) + index as u32
}

/// A descriptor of `base`, `stride` and `records`, its fourth word `fourth`.
fn descriptor(base: u64, stride: u32, records: u32, fourth: u32) -> [u32; 4] {
    [
        base as u32,
        (base >> 32) as u32 & 0xffff | stride << 16,
        records,
        fourth,
    ]
}

/// The table's words: four records of sixteen bytes, each a descriptor of its buffer.
fn table() -> Vec<u32> {
    (0..4)
        .flat_map(|record| descriptor(record_base(record), 4, 16, R32_UINT))
        .collect()
}

/// The global buffer of pages holding the records' buffers whose numbers `held` lists, at slot 3.
fn pages(held: &[u64]) -> Vec<u32> {
    let spans = held.iter().map(|&record| (record_base(record), 16 * 4));
    let found = GlobalPages::of(3, spans).expect("pages");
    found.buffer_words(|page| {
        Some(
            (0..PAGE / 4)
                .map(|word| {
                    let address = page + word * 4;
                    (0..4)
                        .find_map(|record| {
                            let index = address.checked_sub(record_base(record))? / 4;
                            (index < 16).then(|| element(record, index))
                        })
                        .unwrap_or(0)
                })
                .collect(),
        )
    })
}

/// Whether a device is here to run on, saying so either way.
fn device_present() -> bool {
    match probe() {
        Availability::Available { properties } => {
            println!("device: {}", properties.device);
            true
        }
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            false
        }
    }
}

/// The waterfall fetching with `fetch`, translated with slot 1 - the selected record's - read
/// through the pages in slot 3, the global buffer. The whole wave runs in one invocation: a
/// waterfall reads another lane's value, which one invocation per lane cannot (D880).
fn module(fetch: [u32; 2]) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program(fetch)
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let mut formats = BufferFormats::default();
    formats.set(1, Some(R32_UINT));
    let (module, _, _) = translate_with_user_data(
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
                threads: [64, 1, 1],
                table_buffers: 1 << 1,
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            buffer_formats: Some(formats),
            ..UserData::default()
        },
    )
    .expect("the waterfall translates through its table");
    module
}

/// Runs the module over the records whose pages `held` lists, answering what the store's buffer
/// holds after and whether any lane escaped.
fn run(module: &[u32], held: &[u64]) -> (Vec<u32>, bool) {
    let mut block = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    block[..4].copy_from_slice(&descriptor(TABLE, 16, 4, RAW));
    block[4..8].copy_from_slice(&descriptor(DESTINATION, 0, 64 * 4, RAW));
    let table = table();
    let destination = vec![0_u32; 64];
    let pages = pages(held);
    let done = dispatch_guest_with_buffers(
        module,
        &[0],
        (&block, [1, 1, 1]),
        &DispatchImages::default(),
        &[
            BufferIn::Words(&table),
            // The selected record's own slot binds nothing: its records are in the pages.
            BufferIn::Words(&[0]),
            BufferIn::Words(&destination),
            BufferIn::Words(&pages),
        ],
    )
    .expect("dispatched");
    (done.buffers[2].words.clone(), done.escaped)
}

/// What each lane fetches: its record's element at its index.
fn expected() -> Vec<u32> {
    (0..64).map(|lane| element(lane & 3, lane >> 2)).collect()
}

/// Each lane fetches through the record it selected, converting by the format the records share,
/// and a lane whose record's pages are not held reads zero and is an escape: the dispatch's result
/// is then refused, never taken from another record.
#[test]
fn each_lane_formats_through_the_record_it_selected() {
    if !device_present() {
        return;
    }
    let module = module(FORMAT_FETCH);
    let (stored, escaped) = run(&module, &[0, 1, 2, 3]);
    assert!(!escaped, "every record is held");
    assert_eq!(stored, expected(), "each lane read its own record");
    let (stored, escaped) = run(&module, &[0, 1, 3]);
    assert!(escaped, "record 2's pages are not held");
    let missing: Vec<u32> = expected()
        .iter()
        .enumerate()
        .map(|(lane, &word)| if lane & 3 == 2 { 0 } else { word })
        .collect();
    assert_eq!(stored, missing, "an unheld record reads zero");
}

/// An untyped fetch through a selected record reads its words the same way.
#[test]
fn each_lane_reads_words_through_the_record_it_selected() {
    if !device_present() {
        return;
    }
    let module = module(WORD_FETCH);
    let (stored, escaped) = run(&module, &[0, 1, 2, 3]);
    assert!(!escaped, "every record is held");
    assert_eq!(stored, expected(), "each lane read its own record");
}

/// A store through a selected record is refused by name: the records are bound to be read, and
/// nothing is written back from them.
#[test]
fn a_store_through_a_selected_record_is_refused() {
    let mut words = program(WORD_FETCH);
    // The fetch becomes `buffer_store_dword v2, v4, s[8:11], 0 idxen`.
    words[11] = 0xe070_2000;
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let refused = translate_with_user_data(
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
                threads: [64, 1, 1],
                table_buffers: 1 << 1,
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            ..UserData::default()
        },
    )
    .map(|_| ())
    .expect_err("refused");
    assert!(
        refused.to_string().contains("selected from a table"),
        "{refused}"
    );
}
