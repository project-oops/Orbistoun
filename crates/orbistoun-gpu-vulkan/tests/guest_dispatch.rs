//! A guest compute dispatch, run on the device over guest memory.
//!
//! The shader is radeonsi's compute buffer clear as the GL port on the open toolchain compiles it
//! (Mesa's own compiler output, read out of the port's first command buffer): each thread stores
//! the sixteen-byte clear value from user data words 4-7 through the buffer descriptor in words
//! 8-11, at `(workgroup_id.x * 64 + thread_id.x) * 16`.
//!
//! ```text
//! v_lshl_add_u32 v0, s12, 6, v0
//! v_lshrrev_b64 v[4:5], 0, s[4:5]
//! v_lshrrev_b64 v[6:7], 0, s[6:7]
//! v_lshlrev_b32_e32 v0, 4, v0
//! buffer_store_dwordx4 v[4:7], v0, s[8:11], 0 offen
//! s_endpgm
//! ```

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::dispatch_guest;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
};

fn device_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

const CLEAR: [u32; 11] = [
    0xd746_0000,
    0x0401_0c0c,
    0xd700_0004,
    0x0000_0880,
    0xd700_0006,
    0x0000_0c80,
    0x3400_0084,
    0xe078_1000,
    0x8002_0400,
    0xbf81_0000,
    0xbf9f_0000,
];

/// Where the window sits, and how many words it spans.
const WINDOW: u64 = 0x4_0187_0000;
const WINDOW_WORDS: u32 = 4096;
/// What memory held before, so an untouched word is told from a cleared one.
const BEFORE: u32 = 0xdead_beef;
/// The clear value, one word per component.
const VALUE: [u32; 4] = [0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444];
/// `SQ_BUF_RSRC_WORD3` as the port's descriptor carries it: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;

fn module() -> Vec<u32> {
    translated(0).expect("the clear translates")
}

/// The clear, translated with the user-data words in `unwritten` marked as never written.
fn translated(unwritten: u32) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = CLEAR.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            first_register: 0,
            count: 12,
            block_offset: 0,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [64, 1, 1],
                unwritten_user_data: unwritten,
            }),
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
}

/// A user-data word the stream never wrote is refused only when the program reads it: the clear
/// reads words 4 to 12 and never word one.
#[test]
fn an_unwritten_user_data_word_is_refused_only_when_read() {
    assert!(translated(1 << 1).is_ok(), "word one is never read");
    assert!(translated(1 << 5).is_err(), "word five is the clear value");
    assert!(
        translated(1 << 10).is_err(),
        "word ten is the descriptor's record count"
    );
}

/// The user-data block: words 4-7 the value, 8-11 a raw descriptor at `base` of `records` bytes.
fn user_data(base: u64, records: u32) -> Vec<u32> {
    let mut block = vec![0_u32; 32];
    block[4..8].copy_from_slice(&VALUE);
    block[8] = base as u32;
    block[9] = (base >> 32) as u32 & 0xffff;
    block[10] = records;
    block[11] = WORD3;
    block
}

/// Three groups clear three kilobytes from the descriptor's base, and nothing else changes.
#[test]
fn a_dispatch_clears_the_buffer_its_descriptor_names() {
    if !device_or_skip("guest clear") {
        return;
    }
    let before = vec![BEFORE; WINDOW_WORDS as usize];
    let done = dispatch_guest(
        &module(),
        &before,
        &user_data(WINDOW + 0x100, 3 * 64 * 16),
        [3, 1, 1],
    )
    .expect("dispatched");
    assert!(!done.escaped, "every store landed in the window");
    for (index, &word) in done.memory.iter().enumerate() {
        let expected = if (0x40..0x40 + 3 * 64 * 4).contains(&index) {
            VALUE[index % 4]
        } else {
            BEFORE
        };
        assert_eq!(word, expected, "word {index}");
    }
}

/// A raw buffer's bounds hold: stores past its record count are dropped, as the hardware drops
/// them, and that is not an escape.
#[test]
fn stores_past_a_raw_buffers_records_are_dropped() {
    if !device_or_skip("guest clear, short buffer") {
        return;
    }
    let before = vec![BEFORE; WINDOW_WORDS as usize];
    // Sixty-three whole stores fit in 1008 bytes; the sixty-fourth lies wholly past them, so how a
    // store straddling the end is checked does not enter into it.
    let done = dispatch_guest(&module(), &before, &user_data(WINDOW, 1008), [1, 1, 1])
        .expect("dispatched");
    assert!(!done.escaped);
    assert!(
        done.memory[..252]
            .iter()
            .enumerate()
            .all(|(i, &w)| w == VALUE[i % 4])
    );
    assert!(
        done.memory[252..].iter().all(|&w| w == BEFORE),
        "nothing past the records"
    );
}

/// A descriptor naming memory outside the window - in another four-gigabyte span, or past its end
/// - is an escape: the dispatch says so rather than dropping or folding the stores.
#[test]
fn a_descriptor_outside_the_window_is_an_escape() {
    if !device_or_skip("guest clear, escaping") {
        return;
    }
    let before = vec![BEFORE; WINDOW_WORDS as usize];
    for base in [
        WINDOW + (1 << 32),
        WINDOW + u64::from(WINDOW_WORDS) * 4 - 0x100,
    ] {
        let done = dispatch_guest(&module(), &before, &user_data(base, 64 * 16), [1, 1, 1])
            .expect("dispatched");
        assert!(done.escaped, "{base:#x}");
    }
}
