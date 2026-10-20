//! A guest compute dispatch through buffers of its own (D746), run on the device.
//!
//! The program is radeonsi's buffer copy at its least: each thread loads the word at its own index
//! through the descriptor in user-data words 8-11 and stores it through the one in words 12-15.
//!
//! ```text
//! v_lshlrev_b32_e32 v0, 2, v0
//! buffer_load_dword v1, v0, s[8:11], 0 offen
//! s_waitcnt vmcnt(0)
//! buffer_store_dword v1, v0, s[12:15], 0 offen
//! s_endpgm
//! ```

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::{BufferIn, dispatch_guest_with_buffers};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
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

const COPY: [u32; 8] = [
    0x3400_0082,
    0xe030_1000,
    0x8002_0100,
    0xbf8c_3f70,
    0xe070_1000,
    0x8003_0100,
    0xbf81_0000,
    0xbf9f_0000,
];

/// `SQ_BUF_RSRC_WORD3` as radeonsi's descriptors carry it: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;
/// The two buffers' bases, 128 MiB apart as STKT00001's are: no window spans both.
const SOURCE: u64 = 0x4_0a60_0000;
const DESTINATION: u64 = 0x4_1260_0000;

fn user_data() -> Vec<u32> {
    let mut block = vec![0_u32; 32];
    for (at, base) in [(8, SOURCE), (12, DESTINATION)] {
        block[at] = base as u32;
        block[at + 1] = (base >> 32) as u32 & 0xffff;
        block[at + 2] = 64 * 4;
        block[at + 3] = WORD3;
    }
    block
}

/// The copy, translated to read and write its two buffers through their slots.
fn copy_module() -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = COPY.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _, _) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            first_register: 0,
            count: 16,
            block_offset: 0,
            compute: Some(ComputeInputs {
                workgroup_ids: [false; 3],
                thread_id_components: 1,
                threads: [64, 1, 1],
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            per_invocation: per_invocation(),
            ..UserData::default()
        },
    )
    .expect("the copy translates through its buffers");
    module
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

/// Each of 64 threads copies its word from one bound buffer to the other, and the module needs no
/// window: both accesses are traced to a slot, the store included.
#[test]
fn a_dispatch_copies_between_its_own_buffers() {
    both_models(|| {
        if !device_present() {
            return;
        }
        let source: Vec<u32> = (0..64).map(|i| 0x5000_0000 | i).collect();
        let destination = vec![0xdead_beef_u32; 64];
        let done = dispatch_guest_with_buffers(
            &copy_module(),
            &[0],
            (&user_data(), [1, 1, 1]),
            &DispatchImages::default(),
            &[BufferIn::Words(&source), BufferIn::Words(&destination)],
        )
        .expect("dispatched");
        assert!(!done.escaped, "nothing left the bound buffers");
        assert_eq!(done.buffers.len(), 2);
        assert_eq!(done.buffers[0].words, source, "the source is unchanged");
        assert_eq!(
            done.buffers[1].words, source,
            "the destination holds the copy"
        );
    });
}

/// A buffer the program only stores to is handed over unread (D809): the device starts it at zero,
/// and its write mask says which words the dispatch stored - the 64 copied, not the 32 past them.
#[test]
fn an_unread_buffer_comes_back_with_the_words_stored_marked() {
    both_models(|| {
        if !device_present() {
            return;
        }
        let source: Vec<u32> = (0..64).map(|i| 0x5000_0000 | i).collect();
        let done = dispatch_guest_with_buffers(
            &copy_module(),
            &[0],
            (&user_data(), [1, 1, 1]),
            &DispatchImages::default(),
            &[BufferIn::Words(&source), BufferIn::Unread(96)],
        )
        .expect("dispatched");
        let destination = &done.buffers[1];
        assert_eq!(destination.words[..64], source[..], "the copy");
        assert_eq!(destination.words[64..], [0; 32], "started at zero");
        let stored: Vec<bool> = destination.written.iter().map(|&mark| mark != 0).collect();
        assert_eq!(stored[..64], [true; 64], "the words stored");
        assert_eq!(stored[64..], [false; 32], "and none past them");
    });
}

/// A dispatch's buffer atomics (D919): each of 64 threads folds `i - 32.0` into a float minimum and
/// maximum and 1 into a sum, through a bound buffer, as Grand Theft Auto V's loading-screen
/// reduction does with `buffer_atomic_fmin` and `buffer_atomic_fmax` (MUBUF 63, 64). Words starting
/// at 0.5, 0.5 and 100 end at -32.0, 31.0 and 164, however the threads interleave.
#[test]
fn buffer_atomics_fold_every_thread_in() {
    // v_cvt_f32_i32 v1, v0; v_add_f32 v1, -32.0, v1; v_mov_b32 v3, 1;
    // buffer_atomic_fmin v1, off, s[8:11], 0; buffer_atomic_fmax v1, off, s[8:11], 0 offset:4;
    // buffer_atomic_add v3, off, s[8:11], 0 offset:8; s_endpgm.
    const PROGRAM: [u32; 13] = [
        0x7e02_0b00,
        0x0602_02ff,
        0xc200_0000,
        0x7e06_0281,
        0xe0fc_0000,
        0x8002_0100,
        0xe100_0004,
        0x8002_0100,
        0xe0c8_0008,
        0x8002_0300,
        0xbf81_0000,
        0xbf9f_0000,
        0xbf9f_0000,
    ];
    both_models(|| {
        if !device_present() {
            return;
        }
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = PROGRAM.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let (module, _, _) = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::default(),
            UserData {
                first_register: 0,
                count: 16,
                block_offset: 0,
                compute: Some(ComputeInputs {
                    workgroup_ids: [false; 3],
                    thread_id_components: 1,
                    threads: [64, 1, 1],
                    ..ComputeInputs::default()
                }),
                draw_buffers: true,
                per_invocation: per_invocation(),
                ..UserData::default()
            },
        )
        .expect("the atomics translate through their buffer");
        let words = [0.5f32.to_bits(), 0.5f32.to_bits(), 100, 0];
        let done = dispatch_guest_with_buffers(
            &module,
            &[0],
            (&user_data(), [1, 1, 1]),
            &DispatchImages::default(),
            &[BufferIn::Words(&words)],
        )
        .expect("dispatched");
        let folded = &done.buffers[0].words;
        assert_eq!(folded[0], (-32.0f32).to_bits(), "the float minimum");
        assert_eq!(folded[1], 31.0f32.to_bits(), "the float maximum");
        assert_eq!(folded[2], 164, "the sum");
    });
}

/// A sixteen-bit buffer load (MUBUF 10) reads the half word at its own byte, zero-extended: the
/// two halves of 0x1234_5678 are 0x5678 at byte 0 and 0x1234 at byte 2. Grand Theft Auto V's
/// loading-screen programs read with `buffer_load_ushort`.
#[test]
fn a_sixteen_bit_buffer_load_reads_its_half_word() {
    // v_mov_b32 v1, 0; v_mov_b32 v2, 2; buffer_load_ushort v3, v1, s[8:11], 0 offen;
    // buffer_load_ushort v4, v2, s[8:11], 0 offen; s_waitcnt vmcnt(0);
    // buffer_store_dword v3, off, s[12:15], 0; buffer_store_dword v4, off, s[12:15], 0 offset:4.
    const PROGRAM: [u32; 14] = [
        0x7e02_0280,
        0x7e04_0282,
        0xe028_1000,
        0x8002_0301,
        0xe028_1000,
        0x8002_0402,
        0xbf8c_3f70,
        0xe070_0000,
        0x8003_0300,
        0xe070_0004,
        0x8003_0400,
        0xbf81_0000,
        0xbf9f_0000,
        0xbf9f_0000,
    ];
    both_models(|| {
        if !device_present() {
            return;
        }
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = PROGRAM.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let (module, _, _) = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::default(),
            UserData {
                first_register: 0,
                count: 16,
                block_offset: 0,
                compute: Some(ComputeInputs {
                    workgroup_ids: [false; 3],
                    thread_id_components: 1,
                    threads: [64, 1, 1],
                    ..ComputeInputs::default()
                }),
                draw_buffers: true,
                per_invocation: per_invocation(),
                ..UserData::default()
            },
        )
        .expect("the loads translate through their buffer");
        let source = [0x1234_5678_u32, 0];
        let destination = [0_u32; 2];
        let done = dispatch_guest_with_buffers(
            &module,
            &[0],
            (&user_data(), [1, 1, 1]),
            &DispatchImages::default(),
            &[BufferIn::Words(&source), BufferIn::Words(&destination)],
        )
        .expect("dispatched");
        assert_eq!(done.buffers[1].words, [0x5678, 0x1234]);
    });
}
