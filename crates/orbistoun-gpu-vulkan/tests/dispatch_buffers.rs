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

/// Two slots bound to one buffer, the second from 256 bytes into it (D953): the copy reads the
/// buffer's first 64 words through slot 0 and stores them through slot 1 into its next 64, and both
/// slots answer the one buffer's words.
#[test]
fn a_slot_bound_into_another_s_buffer_stores_into_it() {
    both_models(|| {
        if !device_present() {
            return;
        }
        let held: Vec<u32> = (0..128).map(|i| 0x5000_0000 | i).collect();
        let done = dispatch_guest_with_buffers(
            &copy_module(),
            &[0],
            (&user_data(), [1, 1, 1]),
            &DispatchImages::default(),
            &[BufferIn::Words(&held), BufferIn::Alias(0, 256)],
        )
        .expect("dispatched");
        assert!(!done.escaped, "nothing left the bound buffers");
        let words = &done.buffers[0].words;
        assert_eq!(words[..64], held[..64], "the first half is unchanged");
        assert_eq!(words[64..], held[..64], "the second half holds the copy");
        assert_eq!(
            done.buffers[1].words, *words,
            "the alias answers the one buffer"
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

/// A traced global load reads its buffer relative to the base the push block carries (D883), and a
/// lane whose address the bound buffer does not hold - below its base or past its end - is an
/// escape, not a read of another word (D950): a range placed from a sample of a dispatch's groups
/// is kept honest so. Each of 64 threads loads the word at `s[0:1]` plus four times its id and
/// stores it through the descriptor in `s[8:11]`.
#[test]
fn a_global_load_outside_its_bound_buffer_is_an_escape() {
    // v_lshlrev_b32 v1, 2, v0; v_add_nc_u32 v1, s0, v1; v_mov_b32 v2, s1;
    // global_load_dword v3, v[1:2], off; s_waitcnt vmcnt(0); v_lshlrev_b32 v0, 2, v0;
    // buffer_store_dword v3, v0, s[8:11], 0 offen; s_endpgm.
    let short_form = |name: &str, dst: u32, src0: u32, vsrc1: u32| {
        let encodings = EncodingTable::builtin().expect("encodings");
        let (family, opcode) = encodings.find_by_name(name).expect(name);
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        encoding.value | (opcode << encoding.opcode.shift) | (dst << 17) | (vsrc1 << 9) | src0
    };
    let program = [
        short_form("v_lshlrev_b32_e32", 1, 130, 0),
        short_form("v_add_nc_u32_e32", 1, 0, 1),
        short_form("v_mov_b32_e32", 2, 1, 0),
        0xdc30_8000,
        0x037d_0001,
        0xbf8c_3f70,
        0x3400_0082,
        0xe070_1000,
        0x8002_0300,
        0xbf81_0000,
        0xbf9f_0000,
    ];
    let global = 0x4_0a60_0000_u64;
    both_models(|| {
        if !device_present() {
            return;
        }
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
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
        .expect("the load translates through its traced buffer");
        let source: Vec<u32> = (0..64).map(|i| 0x6000_0000 | i).collect();
        // The global address in s[0:1]; the store's descriptor in s[8:11], slot 0, so the global
        // loads' buffer is slot 1.
        let push = |base: u64| {
            let mut block = user_data();
            block[0] = global as u32;
            block[1] = (global >> 32) as u32;
            block.resize(
                orbistoun_translate::wavefront::USER_DATA_BLOCK_WORDS as usize,
                0,
            );
            block[orbistoun_translate::wavefront::GLOBAL_BASE_PUSH_WORD as usize] = base as u32;
            block
        };
        let run = |base: u64, held: &[u32]| {
            let destination = vec![0_u32; 64];
            dispatch_guest_with_buffers(
                &module,
                &[0],
                (&push(base), [1, 1, 1]),
                &DispatchImages::default(),
                &[BufferIn::Words(&destination), BufferIn::Words(held)],
            )
            .expect("dispatched")
        };
        let whole = run(global, &source);
        assert!(!whole.escaped, "every lane's word is held");
        assert_eq!(
            whole.buffers[0].words, source,
            "each thread copied its word"
        );
        // Half the words bound, as a sample that missed the upper threads would place it.
        assert!(run(global, &source[..32]).escaped, "past the end");
        // The base a word above the first address: lane 0 reads below it.
        assert!(run(global + 4, &source[1..]).escaped, "below the base");
    });
}

/// A buffer placed from a sample of a dispatch's groups (D951) is read relative to the base the
/// push block carries, from the descriptor the access computes, and an access the bound buffer
/// does not hold - past its end, below its base, or through a descriptor in another
/// four-gigabyte span - is an escape, not a read of another word or a robust zero. The copy's
/// source, slot 0, is so placed.
#[test]
fn a_buffer_access_outside_its_sampled_range_is_an_escape() {
    both_models(|| {
        if !device_present() {
            return;
        }
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
                    sampled_buffers: 1,
                    ..ComputeInputs::default()
                }),
                draw_buffers: true,
                per_invocation: per_invocation(),
                ..UserData::default()
            },
        )
        .expect("the copy translates through a sampled buffer");
        let source: Vec<u32> = (0..64).map(|i| 0x7000_0000 | i).collect();
        let push = |base: u64| {
            let mut block = user_data();
            block.resize(
                orbistoun_translate::wavefront::USER_DATA_BLOCK_WORDS as usize,
                0,
            );
            let at = orbistoun_translate::wavefront::SAMPLED_BASES_PUSH_WORD as usize;
            block[at] = base as u32;
            block[at + 1] = (base >> 32) as u32 & 0xffff;
            block
        };
        let run = |base: u64, held: &[u32]| {
            let destination = vec![0_u32; 64];
            dispatch_guest_with_buffers(
                &module,
                &[0],
                (&push(base), [1, 1, 1]),
                &DispatchImages::default(),
                &[BufferIn::Words(held), BufferIn::Words(&destination)],
            )
            .expect("dispatched")
        };
        let whole = run(SOURCE, &source);
        assert!(!whole.escaped, "every lane's word is held");
        assert_eq!(
            whole.buffers[1].words, source,
            "each thread copied its word"
        );
        // Bound four words below the descriptor's base, as a margin places it: the words are
        // still the descriptor's, read relative to the buffer's own base.
        let mut wider = vec![0xdead_beef_u32; 4];
        wider.extend(&source);
        let below = run(SOURCE - 16, &wider);
        assert!(!below.escaped, "the margin holds every lane's word");
        assert_eq!(
            below.buffers[1].words, source,
            "read from the descriptor's base"
        );
        // Half the words bound, as a sample that missed the upper threads would place it.
        assert!(run(SOURCE, &source[..32]).escaped, "past the end");
        // The base a word above the descriptor's: lane 0 reads below it.
        assert!(run(SOURCE + 4, &source[1..]).escaped, "below the base");
        // The buffer in another four-gigabyte span than the descriptor's.
        assert!(run(SOURCE + (1 << 32), &source).escaped, "another span");
    });
}

/// A dispatch's 64-bit buffer atomics, each one step over a pair of words, low word first (AMD's
/// published RDNA 2 instruction set, MUBUF): Grand Theft Auto V's in-world scan publishes a group's
/// flag and sum with `buffer_atomic_swap_x2` and polls them with `buffer_atomic_or_x2` of zero.
///
/// Lane 0 swaps `(0x1111_1111, 0x2222_2222)` into the pair at byte 0, answering what it held,
/// `(0xaaaa_aaaa, 0xbbbb_bbbb)`, which it stores at byte 16. Every lane ors `1 << (lane % 32)` and
/// `1 << (lane / 2)` into the pair at byte 8, which ends all ones, and `1 << (lane / 2)` alone
/// into the word at byte 32 with the one-word `buffer_atomic_or`, which ends all ones too. Lane 0
/// then ors zero into the pair at byte 0, answering the swapped value, which it stores at byte 24.
#[test]
fn sixty_four_bit_buffer_atomics_move_both_words_at_once() {
    const LITERAL: u32 = 255;
    const EXEC: u32 = 126;
    const GLC: u32 = 1 << 14;
    let encodings = EncodingTable::builtin().expect("encodings");
    let encoded = |name: &str, fields: u32| {
        let (family, opcode) = encodings.find_by_name(name).expect("this target has it");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|e| e.name == family)
            .expect("its family");
        encoding.value | (opcode << encoding.opcode.shift) | fields
    };
    // The source codes of an inline integer and of a vector register.
    let inline = |value: u32| 128 + value;
    let vgpr = |register: u32| 256 + register;
    // `name v[data:data+1], off, s[8:11], 0 offset:offset`.
    let buffer = |name: &str, data: u32, offset: u32, flags: u32| {
        [
            encoded(name, offset | flags),
            (data << 8) | (2 << 16) | (inline(0) << 24),
        ]
    };
    // A short form `name vDst, src0, vSrc1`.
    let vop2 = |name: &str, dst: u32, src0: u32, vsrc1: u32| {
        encoded(name, src0 | (vsrc1 << 9) | (dst << 17))
    };
    let v_mov = |dst: u32, source: u32| vop2("v_mov_b32_e32", dst, source, 0);
    let mut program = vec![
        v_mov(5, inline(1)),
        // v3 = 1 << (lane & 31), v4 = 1 << (lane >> 1)
        vop2("v_and_b32_e32", 3, inline(31), 0),
        vop2("v_lshlrev_b32_e32", 3, vgpr(3), 5),
        vop2("v_lshrrev_b32_e32", 4, inline(1), 0),
        vop2("v_lshlrev_b32_e32", 4, vgpr(4), 5),
    ];
    program.extend(buffer("buffer_atomic_or_x2", 3, 8, 0));
    program.extend(buffer("buffer_atomic_or", 4, 32, 0));
    program.extend([
        encoded("s_mov_b64", (EXEC << 16) | inline(1)),
        v_mov(1, LITERAL),
        0x1111_1111,
        v_mov(2, LITERAL),
        0x2222_2222,
    ]);
    program.extend(buffer("buffer_atomic_swap_x2", 1, 0, GLC));
    program.push(encoded("s_waitcnt", 0));
    program.extend(buffer("buffer_store_dwordx2", 1, 16, 0));
    program.extend([v_mov(1, inline(0)), v_mov(2, inline(0))]);
    program.extend(buffer("buffer_atomic_or_x2", 1, 0, GLC));
    program.push(encoded("s_waitcnt", 0));
    program.extend(buffer("buffer_store_dwordx2", 1, 24, 0));
    program.push(encoded("s_endpgm", 0));
    both_models(|| {
        if !device_present() {
            return;
        }
        let module = lane_zero_module(&program);
        let mut words = vec![0_u32; 64];
        words[0] = 0xaaaa_aaaa;
        words[1] = 0xbbbb_bbbb;
        let done = dispatch_guest_with_buffers(
            &module,
            &[0],
            (&user_data(), [1, 1, 1]),
            &DispatchImages::default(),
            &[BufferIn::Words(&words)],
        )
        .expect("dispatched");
        let after = &done.buffers[0].words;
        assert_eq!(after[..2], [0x1111_1111, 0x2222_2222], "the swapped pair");
        assert_eq!(after[2..4], [u32::MAX, u32::MAX], "every lane's bits");
        assert_eq!(
            after[4..6],
            [0xaaaa_aaaa, 0xbbbb_bbbb],
            "what the swap answered"
        );
        assert_eq!(
            after[6..8],
            [0x1111_1111, 0x2222_2222],
            "what the or of zero answered"
        );
        assert_eq!(after[8], u32::MAX, "the one-word or");
    });
}

/// `program` translated for one group of 64 threads through its buffers, as the copy is, one
/// invocation per lane numbered within the device's subgroup: the program runs parts in lane 0
/// alone, and a lane is its subgroup invocation's index.
fn lane_zero_module(program: &[u32]) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
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
            per_invocation: per_invocation().map(|_| subgroup_size()),
            ..UserData::default()
        },
    )
    .expect("the program translates through its buffers");
    module
}

/// The device's subgroup size, what one invocation per lane numbers a wave's lanes within.
fn subgroup_size() -> u32 {
    match probe() {
        Availability::Available { properties } => properties.subgroup_size,
        Availability::Unavailable { .. } => 0,
    }
}

/// The rest of a dispatch's one-word buffer atomics, each folding every thread in through a bound
/// buffer (D919) as AMD's published RDNA 2 instruction set gives them: with `v1 = lane + 1`,
/// `v2 = lane - 32` and `v3 = !(1 << (lane % 16))`, `buffer_atomic_and v3` takes 0xffff to 0,
/// `_xor v1` takes 0 to the xor of 1 to 64, 64, `_sub v1` takes 10000 to 10000 - 2080, `_umin v1`
/// takes 1000 to 1, `_umax v1` takes 0 to 64, `_smin v2` takes 0 to -32 and `_smax v2` takes -100
/// to 31, however the threads interleave.
#[test]
fn the_other_word_atomics_fold_every_thread_in() {
    const LITERAL: u32 = 255;
    let encodings = EncodingTable::builtin().expect("encodings");
    let encoded = |name: &str, fields: u32| {
        let (family, opcode) = encodings.find_by_name(name).expect("this target has it");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|e| e.name == family)
            .expect("its family");
        encoding.value | (opcode << encoding.opcode.shift) | fields
    };
    let inline = |value: u32| 128 + value;
    let vgpr = |register: u32| 256 + register;
    // `name vData, off, s[8:11], 0 offset:offset`.
    let atomic = |name: &str, data: u32, offset: u32| {
        [
            encoded(name, offset),
            (data << 8) | (2 << 16) | (inline(0) << 24),
        ]
    };
    let vop2 = |name: &str, dst: u32, src0: u32, vsrc1: u32| {
        encoded(name, src0 | (vsrc1 << 9) | (dst << 17))
    };
    let mut program = vec![
        vop2("v_add_nc_u32_e32", 1, inline(1), 0),
        vop2("v_add_nc_u32_e32", 2, LITERAL, 0),
        (-32_i32).cast_unsigned(),
        vop2("v_mov_b32_e32", 5, inline(1), 0),
        vop2("v_and_b32_e32", 3, inline(15), 0),
        vop2("v_lshlrev_b32_e32", 3, vgpr(3), 5),
        vop2("v_not_b32_e32", 3, vgpr(3), 0),
    ];
    for (name, data, offset) in [
        ("buffer_atomic_and", 3, 0),
        ("buffer_atomic_xor", 1, 4),
        ("buffer_atomic_sub", 1, 8),
        ("buffer_atomic_umin", 1, 12),
        ("buffer_atomic_umax", 1, 16),
        ("buffer_atomic_smin", 2, 20),
        ("buffer_atomic_smax", 2, 24),
    ] {
        program.extend(atomic(name, data, offset));
    }
    program.push(encoded("s_endpgm", 0));
    both_models(|| {
        if !device_present() {
            return;
        }
        let module = lane_zero_module(&program);
        let mut words = vec![0_u32; 64];
        words[..7].copy_from_slice(&[0xffff, 0, 10_000, 1000, 0, 0, (-100_i32).cast_unsigned()]);
        let done = dispatch_guest_with_buffers(
            &module,
            &[0],
            (&user_data(), [1, 1, 1]),
            &DispatchImages::default(),
            &[BufferIn::Words(&words)],
        )
        .expect("dispatched");
        let after = &done.buffers[0].words;
        assert_eq!(
            after[..7],
            [0, 64, 10_000 - 2080, 1, 64, (-32_i32).cast_unsigned(), 31]
        );
    });
}
