//! A whole wave in one invocation grows with its program, not with its program times its width.
//!
//! One invocation simulating a wave once emitted every vector instruction once per lane: a buffer
//! store of four words was some sixty thousand words of module, and a compute program of a few
//! hundred instructions four million, which a driver took minutes to compile. Each lane's work is
//! now emitted once, inside a loop over the lanes. The module is held to that here: a wave of
//! sixty-four lanes is about the size of one of thirty-two, and a bounded size an instruction. The
//! loop is held to what the lanes emitted one by one computed on the device, against one invocation
//! per lane (D880).
//!
//! One group of sixty-four threads, each its lane id in `v0`. The program leaves four results a lane
//! in `v[4:7]` and stores them into the window at sixteen bytes a lane through the descriptor in
//! user data words 8-11.

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
/// The source code of `exec`'s low half.
const EXEC_LO: u32 = 126;

/// The most module words a guest instruction may take, a wave in one invocation. Emitted lane by
/// lane, this program took over sixteen thousand.
const WORDS_AN_INSTRUCTION: usize = 400;

/// The source code of an inline integer.
const fn inline(value: u32) -> u32 {
    128 + value
}
/// The source code of an inline negative integer, -1 to -16.
const fn negative(value: u32) -> u32 {
    192 + value
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

/// A short vector form `name vDst, <first source code>, vSecond`.
fn vop2(name: &str, dst: u32, first: u32, second: u32) -> u32 {
    encoded(name, first | (second << 9) | (dst << 17))
}

/// A short compare `name vcc, <first source code>, vSecond` - or into `exec`, for a `v_cmpx`.
fn vopc(name: &str, first: u32, second: u32) -> u32 {
    encoded(name, first | (second << 9))
}

/// `buffer_<name> v[data..], vAddress, s[8:11], 0 offen`.
fn buffer(name: &str, data: u32, address: u32) -> [u32; 2] {
    [
        encoded(name, 1 << 12),
        address | (data << 8) | (2 << 16) | (inline(0) << 24),
    ]
}

/// The arithmetic, `times` over, each round from the lane id afresh, so every round leaves the same:
///
/// - `v4 = lane + 3`.
/// - `v5 = lane * lane`.
/// - `v6`: the smaller of the two, as `v4 > v5` picks between them.
/// - `v7 = 7` in the lanes below 32, which a compare into `exec` leaves running, and 0 elsewhere.
///
/// Then the lane's sixteen bytes are read back into `v[10:13]` and stored again from them, so each
/// round both loads and stores four words.
fn program(times: usize) -> Vec<u32> {
    let mut program = vec![vop2("v_lshlrev_b32_e32", 9, inline(4), 0)];
    for _ in 0..times {
        program.extend([
            encoded("s_mov_b64", (EXEC_LO << 16) | negative(1)),
            vop2("v_add_nc_u32_e32", 4, inline(3), 0),
            vop2("v_mul_u32_u24_e32", 5, vgpr(0), 0),
            vopc("v_cmp_gt_u32_e32", vgpr(4), 5),
            vop2("v_cndmask_b32_e32", 6, vgpr(4), 5),
            vop2("v_mov_b32_e32", 7, inline(0), 0),
            vopc("v_cmpx_gt_u32_e32", inline(32), 0),
            vop2("v_mov_b32_e32", 7, inline(7), 0),
            encoded("s_mov_b64", (EXEC_LO << 16) | negative(1)),
        ]);
        program.extend(buffer("buffer_store_dwordx4", 4, 9));
        program.extend(buffer("buffer_load_dwordx4", 10, 9));
        program.extend(buffer("buffer_store_dwordx4", 10, 9));
    }
    program.push(encoded("s_endpgm", 0));
    program
}

/// What each lane leaves: `[v4, v5, v6, v7]` as [`program`] says.
fn expected(lane: u32) -> [u32; 4] {
    let (sum, square) = (lane + 3, lane * lane);
    [
        sum,
        square,
        if sum > square { square } else { sum },
        if lane < 32 { 7 } else { 0 },
    ]
}

/// `program` translated for a dispatch of one group of sixty-four threads in waves of `width`, one
/// invocation per lane where `per_lane`, on the device's subgroup; and how many instructions it is.
fn module(program: &[u32], width: Width, per_lane: bool) -> (Vec<u32>, usize) {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
    let subgroup = per_lane.then(|| match probe() {
        Availability::Available { properties } => properties.subgroup_size,
        Availability::Unavailable { .. } => 0,
    });
    let (module, _, _) = translate_with_user_data(
        &decoded,
        &encodings,
        width,
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
                ..ComputeInputs::default()
            }),
            per_invocation: subgroup,
            ..UserData::default()
        },
    )
    .expect("the program translates");
    (module, decoded.instructions.len())
}

/// Runs `program` as one group over a window that held [`BEFORE`] everywhere.
fn run(program: &[u32], per_lane: bool) -> GuestDispatch {
    let mut block = vec![0_u32; 32];
    block[8] = WINDOW as u32;
    block[9] = (WINDOW >> 32) as u32 & 0xffff;
    block[10] = WINDOW_WORDS * 4;
    block[11] = WORD3;
    dispatch_guest(
        &module(program, Width::Wave64, per_lane).0,
        &vec![BEFORE; WINDOW_WORDS as usize],
        (&block, [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("dispatched, not the device lost")
}

/// A whole wave's module is about the size of half a wave's, and bounded an instruction: the lanes
/// are not each emitted.
#[test]
fn a_whole_wave_s_module_does_not_grow_with_its_width() {
    let program = program(8);
    let (wide, instructions) = module(&program, Width::Wave64, false);
    let (narrow, _) = module(&program, Width::Wave32, false);
    println!(
        "{instructions} instructions: {} words in waves of 64, {} in waves of 32",
        wide.len(),
        narrow.len()
    );
    assert!(
        wide.len() * 4 <= narrow.len() * 5,
        "a wave of 64 lanes takes {} words, one of 32 {}: the module grows with the lanes",
        wide.len(),
        narrow.len()
    );
    assert!(
        wide.len() <= instructions * WORDS_AN_INSTRUCTION,
        "{} words for {instructions} instructions, more than {WORDS_AN_INSTRUCTION} each",
        wide.len()
    );
}

/// A lane loop asks the driver not to unroll it (D956): unrolled, its 64 passes are the module that
/// grew with the lanes again, inside the driver's compiler. Grand Theft Auto V's program at
/// `image+0x5b764200`, 60,000 words with 133 lane loops, held the driver's compiler for minutes.
#[test]
fn a_lane_loop_is_not_unrolled() {
    const DONT_UNROLL: u32 = 0x2;
    let (wide, _) = module(&program(8), Width::Wave64, false);
    let mut merges = Vec::new();
    let mut at = 5;
    while at < wide.len() {
        let (count, opcode) = (wide[at] >> 16, wide[at] & 0xffff);
        if u16::try_from(opcode).is_ok_and(|opcode| opcode == orbistoun_spirv::op::LOOP_MERGE) {
            merges.push(wide[at + 3]);
        }
        at += usize::try_from(count.max(1)).expect("fits");
    }
    let unrollable = merges
        .iter()
        .filter(|&&control| control & DONT_UNROLL == 0)
        .count();
    assert!(merges.len() > 1, "the program's lanes are looped");
    // The program's own dispatch loop is the one merge that may leave the driver to choose.
    assert!(
        unrollable <= 1,
        "{unrollable} of {} loops leave the driver free to unroll them",
        merges.len()
    );
}

/// The guest's own programs are held to the same bound, where a run names a directory of them in
/// `ORBISTOUN_SIZE_PROGRAMS`: each `.bin` there a compute program whose user data is four words,
/// translated a whole wave of sixty-four lanes in one invocation, or of thirty-two where it is a
/// program for that width. A guest's code is never kept in the tree, so without the variable this
/// is skipped.
#[test]
fn guest_programs_do_not_grow_with_their_width() {
    let Some(directory) = std::env::var_os("ORBISTOUN_SIZE_PROGRAMS") else {
        println!("[guest programs] SKIPPED - ORBISTOUN_SIZE_PROGRAMS names no directory");
        return;
    };
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .expect("the directory reads")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "bin"))
        .collect();
    paths.sort();
    for path in paths {
        let bytes = std::fs::read(&path).expect("the program reads");
        let decoded = decode(&bytes, &encodings, &operands);
        let translated = [Width::Wave64, Width::Wave32]
            .into_iter()
            .find_map(|width| {
                translate_with_user_data(
                    &decoded,
                    &encodings,
                    width,
                    (Stage::Compute, MeshPrimitive::default()),
                    Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
                    UserData {
                        first_register: 0,
                        count: 4,
                        block_offset: 0,
                        compute: Some(ComputeInputs {
                            workgroup_ids: [true, false, false],
                            thread_id_components: 1,
                            threads: [64, 1, 1],
                            ..ComputeInputs::default()
                        }),
                        draw_buffers: true,
                        program_address: Some(WINDOW),
                        ..UserData::default()
                    },
                )
                .ok()
            });
        let Some((module, _, _)) = translated else {
            println!("{}: refused at either width", path.display());
            continue;
        };
        let instructions = decoded.instructions.len();
        println!(
            "{}: {instructions} instructions, {} words",
            path.display(),
            module.len()
        );
        assert!(
            module.len() <= instructions * WORDS_AN_INSTRUCTION,
            "{}: {} words for {instructions} instructions",
            path.display(),
            module.len()
        );
    }
}

/// The program leaves what it computes in every lane, the whole wave in one invocation as one
/// invocation per lane.
#[test]
fn a_whole_wave_computes_what_each_lane_does() {
    if !device_or_skip("a whole wave computes what each lane does") {
        return;
    }
    let program = program(2);
    for (model, per_lane) in MODELS {
        let done = run(&program, per_lane);
        for lane in 0..64_u32 {
            let at = (lane * 4) as usize;
            assert_eq!(
                &done.memory[at..at + 4],
                &expected(lane),
                "{model}: lane {lane}"
            );
        }
    }
}
