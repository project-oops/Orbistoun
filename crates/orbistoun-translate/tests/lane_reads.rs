//! A guest dispatch that reads across its wave, run on the device one invocation per lane - two
//! lanes an invocation for a wave of sixty-four on a subgroup of thirty-two (D973), one for a wave
//! the subgroup holds (D884) - and held to what the host's concrete run of the same program stores
//! ([`evaluate::wave_trace`]) and to what the whole wave in one invocation stores.
//!
//! Every program forms `v2` from the thread's id in `v0`, then stores it at the thread's word:
//! `buffer_store_dword v2, v70, s[8:11], 0 offen` with `v70` four times the id. A group of 128
//! threads is two waves of sixty-four or four of thirty-two, so each form also meets a wave other
//! than the first.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::dispatch_guest;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::evaluate::{LaneProgram, TraceStop, WaveEntry, wave_trace};
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
};

/// Where the window sits, and how many words it spans.
const WINDOW: u64 = 0x4_0187_0000;
const WINDOW_WORDS: u32 = 1024;
/// What memory held before, so an untouched word is told from a stored one.
const BEFORE: u32 = 0xdead_beef;
/// The group's threads.
const THREADS: u32 = 128;
/// User-data words: 8-11 the descriptor, then the group id in `s12`.
const USER_WORDS: usize = 12;
/// `SQ_BUF_RSRC_WORD3`: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;

/// Operand codes: an inline integer `n` is `128 + n`, -1 is 193, a literal follows at 255, vector
/// registers from 256.
const fn inline(n: u32) -> u32 {
    128 + n
}
const MINUS_ONE: u32 = 193;
const LITERAL: u32 = 255;
const fn vgpr(n: u32) -> u32 {
    256 + n
}
const VCC_LO: u32 = 106;
const EXEC_LO: u32 = 126;

fn host_subgroup() -> Option<u32> {
    match probe() {
        Availability::Available { properties } => {
            println!("device: {}", properties.device);
            Some(properties.subgroup_size)
        }
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            None
        }
    }
}

/// A word of the encoding table's family for `name`, its opcode placed, with `fields` ored in.
fn encoded(name: &str, fields: u32) -> u32 {
    let encodings = EncodingTable::builtin().expect("encodings");
    let (family, opcode) = encodings
        .find_by_name(name)
        .unwrap_or_else(|| panic!("this target has no {name}"));
    let encoding = encodings
        .encodings()
        .iter()
        .find(|e| e.name == family)
        .expect("the family it was found in");
    encoding.value | (opcode << encoding.opcode.shift) | fields
}

/// A short vector form `name vD, src0, vS1`.
fn vop2(name: &str, dst: u32, src0: u32, src1: u32) -> u32 {
    encoded(name, (dst << 17) | (src1 << 9) | src0)
}

/// A short one-source vector form `name vD, src0`.
fn vop1(name: &str, dst: u32, src0: u32) -> u32 {
    encoded(name, (dst << 17) | src0)
}

/// A long vector form `name d, s0, s1, s2`.
fn vop3(name: &str, dst: u32, [s0, s1, s2]: [u32; 3]) -> [u32; 2] {
    [encoded(name, dst), (s2 << 18) | (s1 << 9) | s0]
}

/// `s_mov_b32 sD, literal`.
fn s_mov_literal(dst: u32, value: u32) -> [u32; 2] {
    [encoded("s_mov_b32", (dst << 16) | LITERAL), value]
}

/// A one-source scalar form `name sD, src`.
fn sop1(name: &str, dst: u32, src: u32) -> u32 {
    encoded(name, (dst << 16) | src)
}

/// A comparison of the short form, writing the condition mask, or the execution mask for `cmpx`.
fn vopc(name: &str, src0: u32, src1: u32) -> u32 {
    encoded(name, (src1 << 9) | src0)
}

/// `v_mov_b32_dpp v2, v0` under a DPP16 `control`, every row and bank enabled, `bound_ctrl`
/// as given.
fn mov_dpp(control: u32, bound_zero: bool) -> [u32; 2] {
    [
        vop1("v_mov_b32_e32", 2, 0xfa),
        (control << 8) | (u32::from(bound_zero) << 19) | (0xff << 24),
    ]
}

/// The program around `body`: `v70` the thread's word, `v2` seeded `0x77`, then `body`, then
/// `v2` stored at the thread's word.
fn around(body: &[u32]) -> Vec<u32> {
    let mut program = vec![
        vop2("v_lshlrev_b32_e32", 70, inline(2), 0),
        vop1("v_mov_b32_e32", 2, LITERAL),
        0x77,
    ];
    program.extend_from_slice(body);
    program.extend([
        encoded("buffer_store_dword", 1 << 12),
        // Data `v2`, address `v70`, descriptor `s[8:11]`.
        0x8002_0000 | (2 << 8) | 0x46,
    ]);
    program.push(encoded("s_endpgm", 0));
    program
}

/// Each lane-reading form, by name, as the body [`around`] runs, for a wave of `lanes`.
#[allow(clippy::too_many_lines, reason = "one program a form")]
fn forms(lanes: u32) -> Vec<(&'static str, Vec<u32>)> {
    let mut forms = Vec::new();
    // v_readlane_b32 s2, v0, 33; v_mov_b32 v2, s2: lane 33 of each wave of sixty-four, in its
    // high half, lane 1 of each of thirty-two; a lane each wave of 100 threads has.
    let mut body = vop3("v_readlane_b32", 2, [vgpr(0), inline(33), 0]).to_vec();
    body.push(vop1("v_mov_b32_e32", 2, 2));
    forms.push(("v_readlane", body));

    // The first running lane, in each half of the mask: lane 16 with the low sixteen off, and
    // lane 32 of a wave of sixty-four with the low half off.
    for (name, low) in [
        ("v_readfirstlane, low sixteen off", 0xffff_0000),
        ("v_readfirstlane, low half off", 0),
    ] {
        // The mask's low half saved in `s20` and restored, so a lane past the group stays off.
        // `s_and_b32 exec_lo, exec_lo, low`: a lane past the group is not made to run.
        let mut body = vec![sop1("s_mov_b32", 20, EXEC_LO)];
        body.extend([
            encoded("s_and_b32", (EXEC_LO << 16) | (LITERAL << 8) | EXEC_LO),
            low,
        ]);
        body.push(vop1("v_readfirstlane_b32", 2, vgpr(0)));
        body.push(sop1("s_mov_b32", EXEC_LO, 20));
        body.push(vop1("v_mov_b32_e32", 2, 2));
        if lanes == 64 || low != 0 {
            forms.push((name, body));
        }
    }

    // v_permlane16_b32 v2, v0, s4, s5, each row reversed; v_permlanex16_b32 the other row's lane
    // at the same place.
    for (name, selects) in [
        ("v_permlane16", [0x89ab_cdef, 0x0123_4567]),
        ("v_permlanex16", [0x7654_3210, 0xfedc_ba98]),
    ] {
        let mut body = s_mov_literal(4, selects[0]).to_vec();
        body.extend(s_mov_literal(5, selects[1]));
        let instruction = if name == "v_permlane16" {
            "v_permlane16_b32"
        } else {
            "v_permlanex16_b32"
        };
        body.extend(vop3(instruction, 2, [vgpr(0), 4, 5]));
        forms.push((name, body));
    }

    // DPP16: a quad permute, a row shift with and without `bound_ctrl`, a mirror, a half mirror
    // and an xor of the row's lanes.
    for (name, control, bound) in [
        ("dpp quad_perm:[1,0,3,2]", 0xb1, false),
        ("dpp row_shr:1", 0x111, false),
        ("dpp row_shl:3 bound_ctrl", 0x103, true),
        ("dpp row_mirror", 0x140, false),
        ("dpp row_half_mirror", 0x141, false),
        ("dpp row_xmask:5", 0x165, false),
        ("dpp row_share:9", 0x159, false),
    ] {
        forms.push((name, mov_dpp(control, bound).to_vec()));
    }
    // ds_bpermute_b32 v2, v3, v0 with v3 = (lane + 37) * 4: modulo 32 in the reader's half.
    forms.push((
        "ds_bpermute",
        vec![
            vop2("v_add_nc_u32_e32", 3, inline(37), 0),
            vop2("v_lshlrev_b32_e32", 3, inline(2), 3),
            encoded("ds_bpermute_b32", 0),
            3 | (2 << 24),
        ],
    ));

    // A lane mask read whole: `v_cmp_lt_u32 vcc, 40, v0` and its count, and its halves. A wave of
    // thirty-two counts its mask copied to `s24` beside a zero `s25`.
    let mut count = vec![vopc("v_cmp_lt_u32_e32", inline(40), 0)];
    if lanes == 64 {
        count.push(sop1("s_bcnt1_i32_b64", 20, VCC_LO));
    } else {
        count.push(sop1("s_mov_b32", 24, VCC_LO));
        count.push(sop1("s_mov_b32", 25, inline(0)));
        count.push(sop1("s_bcnt1_i32_b64", 20, 24));
    }
    count.push(vop1("v_mov_b32_e32", 2, 20));
    forms.push(("a mask counted", count));
    forms.push((
        "a mask's high half",
        vec![
            vopc("v_cmp_lt_u32_e32", inline(50), 0),
            vop1("v_mov_b32_e32", 2, VCC_LO + u32::from(lanes == 64)),
        ],
    ));

    // The execution mask written: lanes past 40 run `v_mov_b32 v2, 7`, a wave with none of them
    // branching past it on `execz`, the mask restored after.
    let save = if lanes == 64 {
        "s_mov_b64"
    } else {
        "s_mov_b32"
    };
    forms.push((
        "the execution mask written",
        vec![
            sop1(save, 20, EXEC_LO),
            vopc("v_cmpx_lt_u32_e32", inline(40), 0),
            encoded("s_cbranch_execz", 1),
            vop1("v_mov_b32_e32", 2, inline(7)),
            sop1(save, EXEC_LO, 20),
        ],
    ));
    forms.push((
        "a wave with no lane running branches",
        vec![
            sop1(save, 20, EXEC_LO),
            vopc("v_cmpx_lt_u32_e32", LITERAL, 0),
            100,
            encoded("s_cbranch_execz", 1),
            vop1("v_mov_b32_e32", 2, inline(7)),
            sop1(save, EXEC_LO, 20),
        ],
    ));

    // v_writelane_b32 v2, s2, 40 over v2 = v0, then v_mbcnt of the whole mask added.
    let mut body = vec![vop1("v_mov_b32_e32", 2, vgpr(0))];
    body.extend(s_mov_literal(2, 0x1234));
    body.extend(vop3("v_writelane_b32", 2, [2, inline(40), 0]));
    body.extend(vop3("v_mbcnt_lo_u32_b32", 3, [MINUS_ONE, vgpr(2), 0]));
    body.extend(vop3("v_mbcnt_hi_u32_b32", 2, [MINUS_ONE, vgpr(3), 0]));
    forms.push(("v_writelane and v_mbcnt", body));

    // The local data share in the wave's own order, no barrier: each lane writes its id, then
    // reads the word of the lane in its wave that differs in its lowest bit and, in a wave of
    // sixty-four, in its half.
    let other = if lanes == 64 { 33 } else { 1 };
    forms.push((
        "the local data share, unbarriered",
        vec![
            encoded("ds_write_b32", 0),
            70,
            vop2("v_xor_b32_e32", 3, LITERAL, 0),
            other,
            vop2("v_lshlrev_b32_e32", 3, inline(2), 3),
            encoded("ds_read_b32", 0),
            3 | (2 << 24),
        ],
    ));
    forms
}

/// `program` translated for a group of `threads` in waves of `width`, one invocation per lane on
/// a subgroup of `subgroup` where it is given.
fn translated(
    program: &[u32],
    (width, threads): (Width, u32),
    subgroup: Option<u32>,
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    let inputs = ComputeInputs {
        workgroup_ids: [true, false, false],
        thread_id_components: 1,
        threads: [threads, 1, 1],
        ..ComputeInputs::default()
    };
    translated_with(program, width, subgroup, inputs)
}

/// [`translated`] for a group of `inputs` as given.
fn translated_with(
    program: &[u32],
    width: Width,
    subgroup: Option<u32>,
    inputs: ComputeInputs,
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        width,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            count: USER_WORDS as u32,
            compute: Some(inputs),
            per_invocation: subgroup,
            local_words: Some(1024),
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
}

/// [`translated_with`] as a legacy vertex shader's streamout pass (D861), whose wave reads its
/// first vertex from the push block.
fn translated_streamout(
    program: &[u32],
    width: Width,
    subgroup: Option<u32>,
    inputs: ComputeInputs,
) -> Result<Vec<u32>, orbistoun_translate::TranslateError> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        width,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            count: USER_WORDS as u32,
            compute: Some(inputs),
            per_invocation: subgroup,
            local_words: Some(1024),
            discard_vertex_exports: true,
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
}

/// The user-data block: words 8-11 a raw descriptor over the window's start.
fn user_data() -> Vec<u32> {
    let mut block = vec![0_u32; 32];
    block[8] = WINDOW as u32;
    block[9] = (WINDOW >> 32) as u32 & 0xffff;
    block[10] = THREADS * 4;
    block[11] = WORD3;
    block
}

/// What the host's concrete run of `program` stores, word by word over the window: `None` for a
/// word stored with a value the run does not know.
fn oracle(program: &[u32], lanes: u32, threads: u32) -> Vec<Option<u32>> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let waves: Vec<WaveEntry> = (0..threads.div_ceil(lanes))
        .map(|wave| WaveEntry {
            group_ids: vec![0],
            // A lane past the group's last thread has none, and does not run.
            threads: (wave * lanes..(wave + 1) * lanes)
                .map(|t| (t < threads).then_some([t, 0, 0]))
                .collect(),
            components: 1,
        })
        .collect();
    let block = user_data();
    let mut read = |address: u64| {
        let word = address.checked_sub(WINDOW)? / 4;
        (word < u64::from(WINDOW_WORDS)).then_some(BEFORE)
    };
    let run = wave_trace(
        &LaneProgram::new(decoded, &encodings, &[]),
        &encodings,
        &block[..USER_WORDS],
        (lanes, &waves),
        (0, 0, 0),
        &mut read,
    );
    assert!(
        matches!(run.stop, TraceStop::Ended),
        "the host runs it to its end: {:?}",
        run.stop
    );
    let mut memory = vec![Some(BEFORE); WINDOW_WORDS as usize];
    for (address, (value, _)) in run.written {
        let word = usize::try_from((address - WINDOW) / 4).expect("a word");
        memory[word] = value;
    }
    memory
}

/// Every lane-reading form stores, run one invocation per lane, what the host's run and the whole
/// wave store, in waves of sixty-four and of thirty-two, in a group of whole waves and in one whose
/// last wave is partial.
#[test]
fn each_lane_read_runs_per_lane_as_the_host_runs_it() {
    let Some(subgroup) = host_subgroup() else {
        return;
    };
    let mut wrong = Vec::new();
    for (width, threads) in [
        (Width::Wave64, THREADS),
        (Width::Wave64, 100),
        (Width::Wave32, THREADS),
        (Width::Wave32, 100),
    ] {
        let lanes = width.lanes();
        for (name, body) in forms(lanes) {
            // A partial wave's lanes past the group read share words no lane wrote, which hold
            // nothing defined.
            if threads % lanes != 0 && name.starts_with("the local data share") {
                continue;
            }
            let name = format!("{name}, {threads} threads");
            let program = around(&body);
            let host = oracle(&program, lanes, threads);
            let known = host.iter().take(THREADS as usize).flatten().count();
            println!("{name}, {lanes} lanes: the host knows {known} of {THREADS} words");
            let mut whole: Option<Vec<u32>> = None;
            for (model, per_lane) in [("whole wave", None), ("per lane", Some(subgroup))] {
                let module = match translated(&program, (width, threads), per_lane) {
                    Ok(module) => module,
                    Err(why) => {
                        wrong.push(format!("{name}, {lanes} lanes, {model}: refused: {why}"));
                        continue;
                    }
                };
                let before = vec![BEFORE; WINDOW_WORDS as usize];
                let done = dispatch_guest(
                    &module,
                    &before,
                    (&user_data(), [1, 1, 1]),
                    &DispatchImages::default(),
                )
                .expect("dispatched");
                // Held to the host where it knows the word, and to the whole wave everywhere. A
                // permute reads its source lane whether it runs or not, and a lane past a partial
                // group's end holds what the host does not model, so there only the whole wave.
                let undefined = threads % lanes != 0 && name.starts_with("v_permlane");
                let differing: Vec<String> = (0..THREADS as usize)
                    .filter(|&t| {
                        host[t].is_some_and(|word| !undefined && word != done.memory[t])
                            || whole.as_ref().is_some_and(|w| w[t] != done.memory[t])
                    })
                    .take(4)
                    .map(|t| {
                        format!(
                            "thread {t}: {:#x}, host {:x?}, whole wave {:x?}",
                            done.memory[t],
                            host[t],
                            whole.as_ref().map(|w| w[t])
                        )
                    })
                    .collect();
                if !differing.is_empty() {
                    wrong.push(format!(
                        "{name}, {lanes} lanes, {model}: {}",
                        differing.join("; ")
                    ));
                }
                if per_lane.is_none() {
                    whole = Some(done.memory);
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A streamout pass's wave of a larger draw (D861) holds in `v0` the draw's vertex id: its lane
/// plus the vertex its first lane is (`first_thread`, read from the push block), however its lanes
/// are held - the whole wave in one invocation, one lane an invocation (D880), or two (D973). Each
/// lane stores its `v0` at its own word, found from `v_mbcnt` rather than from `v0`, in a partial
/// wave of 20 vertices.
#[test]
fn a_streamout_wave_s_lanes_hold_the_draw_s_vertex_ids() {
    const FIRST: u32 = 1000;
    const VERTICES: u32 = 20;
    let Some(subgroup) = host_subgroup() else {
        return;
    };
    let mut program = vop3("v_mbcnt_lo_u32_b32", 3, [MINUS_ONE, inline(0), 0]).to_vec();
    program.extend(vop3("v_mbcnt_hi_u32_b32", 3, [MINUS_ONE, vgpr(3), 0]));
    program.push(vop2("v_lshlrev_b32_e32", 70, inline(2), 3));
    // buffer_store_dword v0, v70, s[8:11], 0 offen
    program.extend([encoded("buffer_store_dword", 1 << 12), 0x8002_0046]);
    program.push(encoded("s_endpgm", 0));
    let mut wrong = Vec::new();
    for width in [Width::Wave64, Width::Wave32] {
        let lanes = width.lanes();
        let partial = orbistoun_translate::wavefront::PartialGroups {
            last: [0, 0, 0],
            threads: [VERTICES, 1, 1],
        };
        let inputs = ComputeInputs {
            thread_id_components: 1,
            threads: [lanes, 1, 1],
            partial: Some(partial),
            first_thread: FIRST,
            ..ComputeInputs::default()
        };
        // The user data, then the partial shape and the first vertex where the module reads them.
        let mut push = user_data();
        push.resize(
            orbistoun_translate::wavefront::PARTIAL_PUSH_WORD as usize,
            0,
        );
        push.extend(partial.push_words());
        push.resize(
            orbistoun_translate::wavefront::USER_DATA_BLOCK_WORDS as usize,
            0,
        );
        push[orbistoun_translate::wavefront::FIRST_THREAD_PUSH_WORD as usize] = FIRST;
        for (model, per_lane) in [("whole wave", None), ("per lane", Some(subgroup))] {
            let module =
                translated_streamout(&program, width, per_lane, inputs).expect("translates");
            let done = dispatch_guest(
                &module,
                &vec![BEFORE; WINDOW_WORDS as usize],
                (&push, [1, 1, 1]),
                &DispatchImages::default(),
            )
            .expect("dispatched");
            let stored = &done.memory[..lanes as usize];
            let expected: Vec<u32> = (0..lanes)
                .map(|lane| {
                    if lane < VERTICES {
                        FIRST + lane
                    } else {
                        BEFORE
                    }
                })
                .collect();
            if stored != expected {
                wrong.push(format!(
                    "{lanes} lanes, {model}: {:?}",
                    &stored[..=VERTICES as usize]
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
