//! A dispatch's scratch, each wave's own private memory, run on the device (D954).
//!
//! One group of sixty-four threads, each its lane id in `v0`. Each lane stores two values of its
//! own into scratch and reads them back at other offsets and widths, then stores what it read into
//! the window, sixteen bytes a lane, and one more word a lane after those. Scratch is reached both
//! ways a program reaches it: through the scratch descriptor in user data words 0-3 with the wave's
//! offset - scalar register 13, after twelve user-data words and the group's x id - as the scalar
//! offset, and through the flat encoding's scratch segment. Both ways a dispatch runs are held to
//! it: the whole wave in one invocation, and one invocation per lane (D880).

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::{GuestDispatch, dispatch_guest};
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

const WINDOW: u64 = 0x4_0187_0000;
const WINDOW_WORDS: u32 = 4096;
/// What memory held before, so an untouched word is told from a written one.
const BEFORE: u32 = 0xdead_beef;
/// The output descriptor's fourth word: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;
/// The scratch descriptor (`aco_ir.cpp:1945-1958`, radv's base word): swizzled, every record, the
/// lane id added at an index stride of sixty-four, raw bounds.
const SCRATCH: [u32; 4] = [
    0x0000_1000,
    1 << 31,
    0xffff_ffff,
    (1 << 23) | (3 << 21) | (1 << 24) | (3 << 28),
];
/// Each wave's scratch: 2 KiB, thirty-two bytes a lane.
const WAVE_BYTES: u32 = 2048;
/// The scalar register the wave's scratch offset arrives in.
const WAVE_OFFSET: u32 = 13;
/// A free scalar register past it.
const FREE: u32 = 14;
const LITERAL: u32 = 255;
/// The source code of an inline integer.
const fn inline(value: u32) -> u32 {
    128 + value
}
/// A vector register as a source.
const fn vgpr(register: u32) -> u32 {
    256 + register
}

/// The two ways a dispatch runs: the whole wave in one invocation, and one invocation per lane.
const MODELS: [(&str, bool); 2] = [("whole wave", false), ("per lane", true)];

/// A word of the encoding table's family for `name`, its opcode placed, with `fields` ored in.
fn encoded(name: &str, fields: u32) -> u32 {
    let encodings = EncodingTable::builtin().expect("encodings");
    let (family, opcode) = encodings.find_by_name(name).expect("this target has it");
    let encoding = encodings
        .encodings()
        .iter()
        .find(|e| e.name == family)
        .expect("the family it was found in");
    encoding.value | (opcode << encoding.opcode.shift) | fields
}

/// `name vdata, vaddr, s[0:3], s13 offset:offset`, `offen` where `offen` (`aco_assembler.cpp:
/// 561-600`).
fn scratch_buffer(name: &str, (vdata, vaddr): (u32, u32), offset: u32, offen: bool) -> [u32; 2] {
    [
        encoded(name, offset | (u32::from(offen) << 12)),
        vaddr | (vdata << 8) | (WAVE_OFFSET << 24),
    ]
}

/// `name` in the flat encoding's scratch segment, its data or destination register `register`, its
/// address register `vaddr`, `SADDR` `saddr` and immediate `offset` (`aco_assembler.cpp:889-938`).
fn scratch_flat(name: &str, register: u32, (vaddr, saddr): (u32, u32), offset: u32) -> [u32; 2] {
    let first = encoded(name, offset & 0xfff);
    let first = (first & !(3 << 14)) | (1 << 14);
    let placed = if name.contains("store") {
        register << 8
    } else {
        register << 24
    };
    [first, vaddr | placed | (saddr << 16)]
}

/// The values each lane computes: `v2 = 3 * lane`, `v3 = lane ^ 0xff80`.
fn values() -> Vec<u32> {
    vec![
        // v_lshl_add_u32 v2, v0, 1, v0
        0xd746_0002,
        vgpr(0) | (inline(1) << 9) | (vgpr(0) << 18),
        // v_xor_b32 v3, 0xff80, v0
        encoded("v_xor_b32_e32", LITERAL | (3 << 17)),
        0xff80,
    ]
}

/// What each lane read, into the window: `v[4:7]` at sixteen bytes a lane, `v8` at a word a lane
/// past them; then the end.
fn results() -> Vec<u32> {
    let mut program = vec![
        // v_lshlrev_b32 v9, 4, v0
        encoded("v_lshlrev_b32_e32", inline(4) | (9 << 17)),
        // v_lshlrev_b32 v10, 2, v0
        encoded("v_lshlrev_b32_e32", inline(2) | (10 << 17)),
    ];
    // buffer_store_dwordx4 v[4:7], v9, s[8:11], 0 offen
    program.extend([
        encoded("buffer_store_dwordx4", 1 << 12),
        9 | (4 << 8) | (2 << 16) | (inline(0) << 24),
    ]);
    // buffer_store_dword v8, v10, s[8:11], 0 offen offset:1024
    program.extend([
        encoded("buffer_store_dword", (1 << 12) | 0x400),
        0xa | (8 << 8) | (2 << 16) | (inline(0) << 24),
    ]);
    program.push(encoded("s_endpgm", 0));
    program
}

/// Through the scratch descriptor: `v2` at byte 8, `v3` at 4, both again as a pair at 16; read back
/// as `v4` from 8, `v5` the half at 4, `v[6:7]` the pair from 16, and `v8` from 4 past `v1 = 4`.
fn through_the_descriptor() -> Vec<u32> {
    let mut program = values();
    program.extend(scratch_buffer("buffer_store_dword", (2, 0), 8, false));
    program.extend(scratch_buffer("buffer_store_dword", (3, 0), 4, false));
    program.extend(scratch_buffer("buffer_store_dwordx2", (2, 0), 16, false));
    // v_mov_b32 v1, 4
    program.push(encoded("v_mov_b32_e32", inline(4) | (1 << 17)));
    program.extend(scratch_buffer("buffer_load_dword", (4, 0), 8, false));
    program.extend(scratch_buffer("buffer_load_ushort", (5, 0), 4, false));
    program.extend(scratch_buffer("buffer_load_dwordx2", (6, 0), 16, false));
    program.extend(scratch_buffer("buffer_load_dword", (8, 1), 4, true));
    program.extend(results());
    program
}

/// Through the scratch segment: `v2` at `v1 = 8`, `v3` at the immediate 4, `v2` at `s14 = 16`; read
/// back as `v4` from `v1`, `v5` the signed byte at 4, `v6` from `s14`, `v7` the half at 4 and `v8`
/// the word at 4.
fn through_the_segment() -> Vec<u32> {
    const NULL: u32 = 125;
    const OFF: u32 = 0x7f;
    let mut program = values();
    // v_mov_b32 v1, 8; s_mov_b32 s14, 16
    program.push(encoded("v_mov_b32_e32", inline(8) | (1 << 17)));
    program.push(encoded("s_mov_b32", (FREE << 16) | inline(16)));
    program.extend(scratch_flat("global_store_dword", 2, (1, NULL), 0));
    program.extend(scratch_flat("global_store_dword", 3, (0, OFF), 4));
    program.extend(scratch_flat("global_store_dword", 2, (0, FREE), 0));
    program.extend(scratch_flat("global_load_dword", 4, (1, NULL), 0));
    program.extend(scratch_flat("global_load_sbyte", 5, (0, OFF), 4));
    program.extend(scratch_flat("global_load_dword", 6, (0, FREE), 0));
    program.extend(scratch_flat("global_load_ushort", 7, (0, OFF), 4));
    program.extend(scratch_flat("global_load_dword", 8, (0, OFF), 4));
    program.extend(results());
    program
}

/// `program` translated for a dispatch of one group of sixty-four threads with [`WAVE_BYTES`] of
/// scratch a wave, one invocation per lane where `per_lane`, on the device's subgroup.
fn module(program: &[u32], per_lane: bool) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
    let subgroup = per_lane.then(|| match probe() {
        Availability::Available { properties } => properties.subgroup_size,
        Availability::Unavailable { .. } => 0,
    });
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
                scratch_wave_bytes: WAVE_BYTES,
                ..ComputeInputs::default()
            }),
            per_invocation: subgroup,
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
    .expect("the program translates")
}

/// Runs `program` as one group over a window that held [`BEFORE`] everywhere.
fn run(program: &[u32], per_lane: bool) -> GuestDispatch {
    let mut block = vec![0_u32; 32];
    block[0..4].copy_from_slice(&SCRATCH);
    block[8] = WINDOW as u32;
    block[9] = (WINDOW >> 32) as u32 & 0xffff;
    block[10] = WINDOW_WORDS * 4;
    block[11] = WORD3;
    dispatch_guest(
        &module(program, per_lane),
        &vec![BEFORE; WINDOW_WORDS as usize],
        (&block, [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("dispatched, not the device lost")
}

/// Asserts each lane's five results: `v[4:7]` at its sixteen bytes, `v8` at its word past them.
fn assert_results(memory: &[u32], model: &str, expected: impl Fn(u32) -> [u32; 5]) {
    for lane in 0..64_u32 {
        let words = expected(lane);
        let at = (lane * 4) as usize;
        assert_eq!(&memory[at..at + 4], &words[..4], "{model}: lane {lane}");
        assert_eq!(
            memory[256 + lane as usize],
            words[4],
            "{model}: lane {lane}'s fifth"
        );
    }
}

/// Through the scratch descriptor, each lane reads back its own words: the word stored at 8, the low
/// half of the word at 4, the pair at 16, and the word at 8 again through its address register.
#[test]
fn scratch_through_its_descriptor_is_each_lanes_own() {
    if !device_or_skip("scratch through its descriptor") {
        return;
    }
    for (model, per_lane) in MODELS {
        let done = run(&through_the_descriptor(), per_lane);
        assert!(!done.escaped, "{model}: nothing left the lane's scratch");
        assert_results(&done.memory, model, |lane| {
            let (tripled, flipped) = (3 * lane, lane ^ 0xff80);
            [tripled, flipped & 0xffff, tripled, flipped, tripled]
        });
    }
}

/// Through the scratch segment, each lane reads back its own words, by its address register, its
/// immediate and a scalar register, and a signed byte and a half of one.
#[test]
fn scratch_through_its_segment_is_each_lanes_own() {
    if !device_or_skip("scratch through its segment") {
        return;
    }
    for (model, per_lane) in MODELS {
        let done = run(&through_the_segment(), per_lane);
        assert!(!done.escaped, "{model}: nothing left the lane's scratch");
        assert_results(&done.memory, model, |lane| {
            let (tripled, flipped) = (3 * lane, lane ^ 0xff80);
            // The low byte, 0x80 + lane, is negative.
            let byte = 0xffff_ff00 | (flipped & 0xff);
            [tripled, byte, tripled, flipped & 0xffff, flipped]
        });
    }
}

/// A word past a lane's share of the wave's scratch is not its own: the access reads zero and the
/// dispatch says it escaped, rather than reading another lane's word.
#[test]
fn scratch_past_a_lanes_share_escapes() {
    if !device_or_skip("scratch past a lane's share") {
        return;
    }
    let mut program = values();
    program.extend(scratch_buffer(
        "buffer_load_dword",
        (4, 0),
        WAVE_BYTES / 64,
        false,
    ));
    program.extend(results());
    for (model, per_lane) in MODELS {
        let done = run(&program, per_lane);
        assert!(done.escaped, "{model}: the read left the wave's scratch");
    }
}

/// A program that writes the register its wave's scratch offset arrives in is refused: its scratch
/// accesses would add what it wrote, which the module does not carry.
#[test]
fn a_program_writing_its_scratch_offset_is_refused() {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let mut program = vec![encoded("s_mov_b32", (WAVE_OFFSET << 16) | inline(4))];
    program.extend(scratch_buffer("buffer_store_dword", (0, 0), 0, false));
    program.push(encoded("s_endpgm", 0));
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let refused = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            count: 12,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [64, 1, 1],
                scratch_wave_bytes: WAVE_BYTES,
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect_err("refused");
    assert!(refused.to_string().contains("scratch offset"), "{refused}");
}
