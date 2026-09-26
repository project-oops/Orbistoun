//! The two wavefront models, compared on generated programs.
//!
//! `execute.rs` asserts what instructions do against hand-worked values; this asserts that
//! [`Fidelity::Lane`] and [`Fidelity::Wavefront`], independent below the shared dispatch,
//! leave identical registers and memory for every generated program both accept (D100).
//! A misunderstanding shared through `model::instruction` is invisible here. Programs have
//! no branches: a generated backward branch is a hung GPU, and forward targets would need
//! patching into variable-length code. The generator is seeded and each failure prints its
//! seed.

use orbistoun_gpu_vulkan::{Availability, dispatch, probe};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::predicated::MEMORY_WORDS as MEMORY_WORDS_U32;
use orbistoun_translate::{Fidelity, Strategy, Width, translate};

const MEMORY_WORDS: usize = MEMORY_WORDS_U32 as usize;
const PER_FILE: usize = 8;
const OBSERVED: usize = PER_FILE * 2;

/// How many programs to generate.
///
/// Each runs on a real device twice, so this is a wall-clock budget; raise it when hunting.
const PROGRAMS: u32 = 48;

/// Instructions per generated program, before the prologue and terminator.
const BODY: u32 = 12;

/// A tiny seeded generator.
///
/// Written out rather than a dependency; its sequence is fixed by its seed.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        // xorshift64*.
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    const fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        (self.next() >> 33) as u32 % bound
    }
}

// Instruction encoders, restricted to what both models accept (nothing needing a lane
// mask). Every instruction is composed by name from the loaded table, so the generator
// emits this target's encoding rather than a number from another generation.

/// The first word of an instruction, from its name: family bits plus opcode in place.
///
/// Panics on a name this target lacks: skipping would shrink the comparison silently.
fn head(table: &EncodingTable, name: &str) -> u32 {
    let (family, opcode) = table
        .find_by_name(name)
        .unwrap_or_else(|| panic!("this target has no instruction named {name}"));
    let encoding = table
        .encodings()
        .iter()
        .find(|encoding| encoding.name == family)
        .unwrap_or_else(|| panic!("no encoding family named {family}"));
    encoding.value | (opcode << encoding.opcode.shift)
}

fn v_mov_inline(t: &EncodingTable, dst: u32, constant: u32) -> u32 {
    head(t, "v_mov_b32_e32") | (dst << 17) | (128 + constant)
}

fn s_mov_inline(t: &EncodingTable, dst: u32, constant: u32) -> u32 {
    head(t, "s_mov_b32") | (dst << 16) | (128 + constant)
}

/// `s_mov_b64 s[dst:dst+1], s[src:src+1]`.
fn s_mov_b64(t: &EncodingTable, dst: u32, source_code: u32) -> u32 {
    head(t, "s_mov_b64") | (dst << 16) | source_code
}

/// `v_rcp_f32_e32 vDst, src`.
fn v_rcp(t: &EncodingTable, dst: u32, source_code: u32) -> u32 {
    head(t, "v_rcp_f32_e32") | (dst << 17) | source_code
}

/// `s_load_dwordxN sDst, s[base:base+1], offset`.
fn s_load(t: &EncodingTable, name: &str, dst: u32, base: u32, offset: u32) -> [u32; 2] {
    [head(t, name) | (dst << 6) | (base / 2), offset]
}

fn v_op2(t: &EncodingTable, name: &str, dst: u32, first_code: u32, second_vgpr: u32) -> u32 {
    head(t, name) | (dst << 17) | (second_vgpr << 9) | first_code
}

fn vop3(t: &EncodingTable, name: &str, dst: u32, sources: [u32; 3]) -> [u32; 2] {
    [
        head(t, name) | dst,
        (sources[2] << 18) | (sources[1] << 9) | sources[0],
    ]
}

fn sop2(t: &EncodingTable, name: &str, dst: u32, first_code: u32, second_code: u32) -> u32 {
    head(t, name) | (dst << 16) | (second_code << 8) | first_code
}

fn sopk(t: &EncodingTable, name: &str, dst: u32, immediate: i16) -> u32 {
    head(t, name) | (dst << 16) | u32::from(immediate as u16)
}

fn s_cmp_i32(t: &EncodingTable, name: &str, first_code: u32, second_code: u32) -> u32 {
    head(t, name) | (second_code << 8) | first_code
}

/// The scalar-base field's value for "no base", `0x7d`, as the reference assembler emits it
/// for `off`.
const FLAT_NO_BASE_CODE: u32 = 0x7d;

fn flat_store(t: &EncodingTable, name: &str, vaddr: u32, data: u32) -> [u32; 2] {
    [
        head(t, name),
        vaddr | (data << 8) | (FLAT_NO_BASE_CODE << 16),
    ]
}

fn flat_load(t: &EncodingTable, name: &str, vaddr: u32, destination: u32) -> [u32; 2] {
    [
        head(t, name),
        vaddr | (FLAT_NO_BASE_CODE << 16) | (destination << 24),
    ]
}

/// The code for a vector register in the shared source numbering.
const fn vgpr(register: u32) -> u32 {
    256 + register
}

/// Registers the generator uses.
///
/// Confined to the observation window, so every write is observed.
const REGISTERS: u32 = PER_FILE as u32;

/// Guest-memory addresses the generator uses, in bytes.
///
/// Bounded inside the module's window: what the models do out of range need not match.
const ADDRESS_LIMIT: u32 = (MEMORY_WORDS as u32 - 4) * 4;

/// One random instruction, appended to `program`.
///
/// Every form here is one both models accept. Anything needing a lane mask (the
/// comparisons, the conditional move, carry arithmetic, whole quad mode, the local data
/// share) is absent because the per-lane model refuses it.
fn emit(t: &EncodingTable, rng: &mut Rng, program: &mut Vec<u32>) {
    let dst = rng.below(REGISTERS);
    let a = rng.below(REGISTERS);
    let b = rng.below(REGISTERS);
    // Inline integers stop at sixty-four.
    let small = rng.below(65);
    // A destination low enough that a four-register write stays inside the observation
    // window.
    let wide_dst = rng.below(REGISTERS - 3);
    let wide_src = rng.below(REGISTERS - 3);

    match rng.below(26) {
        // Moves.
        0 => program.push(v_mov_inline(t, dst, small)),
        1 => program.push(s_mov_inline(t, dst, small)),
        2 => program.push(s_mov_b64(t, dst & !1, a & !1)),
        // Short-form float arithmetic on whatever bits the registers hold; a NaN is as good
        // a test as any, since the question is agreement.
        3 => program.push(v_op2(t, "v_add_f32_e32", dst, vgpr(a), b)),
        4 => program.push(v_op2(t, "v_sub_f32_e32", dst, vgpr(a), b)),
        5 => program.push(v_op2(t, "v_subrev_f32_e32", dst, vgpr(a), b)),
        6 => program.push(v_op2(t, "v_mul_f32_e32", dst, vgpr(a), b)),
        // Short-form integer. The unsigned add is spelled `v_add_nc_u32` on this generation.
        7 => program.push(v_op2(t, "v_add_nc_u32_e32", dst, vgpr(a), b)),
        8 => program.push(v_op2(t, "v_lshlrev_b32_e32", dst, vgpr(a), b)),
        // Long-form float, which reaches the modifier path even with no modifiers set.
        9 => program.extend(vop3(t, "v_add_f32_e64", dst, [vgpr(a), vgpr(b), 0])),
        10 => program.extend(vop3(t, "v_sub_f32_e64", dst, [vgpr(a), vgpr(b), 0])),
        11 => program.extend(vop3(t, "v_subrev_f32_e64", dst, [vgpr(a), vgpr(b), 0])),
        12 => program.extend(vop3(t, "v_mul_f32_e64", dst, [vgpr(a), vgpr(b), 0])),
        13 | 14 => program.extend(vop3(t, "v_fma_f32", dst, [vgpr(a), vgpr(b), vgpr(dst)])),
        15 => program.push(v_rcp(t, dst, vgpr(a))),
        // Scalar arithmetic and logic, each of which also writes the condition code.
        16 => program.push(sop2(t, "s_add_i32", dst, a, b)),
        17 => program.push(sop2(t, "s_sub_i32", dst, a, b)),
        18 => program.push(sop2(t, "s_and_b32", dst, a, b)),
        19 => program.push(sop2(t, "s_or_b32", dst, a, b)),
        20 => program.push(sop2(t, "s_xor_b32", dst, a, b)),
        // The compact forms, two of which accumulate rather than assign.
        21 => program.push(sopk(t, "s_movk_i32", dst, small as i16)),
        22 => program.push(sopk(t, "s_addk_i32", dst, small as i16)),
        23 => program.push(sopk(t, "s_mulk_i32", dst, small as i16)),
        // A scalar compare, which writes only the condition code.
        24 => program.push(s_cmp_i32(t, SCALAR_COMPARES[rng.below(6) as usize], a, b)),
        // Memory. The scalar load's base is even, since the field encodes an aligned pair,
        // and its offset small, so it is not masked into the window by coincidence.
        _ => match rng.below(6) {
            0 => program.extend(flat_store(t, "global_store_dword", a, dst)),
            1 => program.extend(flat_store(t, "global_store_dwordx2", a, wide_src)),
            2 => program.extend(flat_store(t, "global_store_dwordx4", a, wide_src)),
            3 => program.extend(flat_load(t, "global_load_dword", a, dst)),
            4 => program.extend(flat_load(t, "global_load_dwordx2", a, wide_dst)),
            _ => program.extend(s_load(
                t,
                SCALAR_LOADS[rng.below(3) as usize],
                dst,
                a & !1,
                rng.below(64) * 4,
            )),
        },
    }
}

/// The signed scalar compares, which differ only in their condition.
const SCALAR_COMPARES: [&str; 6] = [
    "s_cmp_eq_i32",
    "s_cmp_lg_i32",
    "s_cmp_gt_i32",
    "s_cmp_ge_i32",
    "s_cmp_lt_i32",
    "s_cmp_le_i32",
];

/// The scalar loads, narrowest first.
const SCALAR_LOADS: [&str; 3] = ["s_load_dword", "s_load_dwordx2", "s_load_dwordx4"];

/// A program that keeps every address register inside the memory window.
///
/// Otherwise a generated store could land past the window, where the models need not agree.
fn program(t: &EncodingTable, seed: u64) -> Vec<u32> {
    let mut rng = Rng(seed | 1);
    let mut program = Vec::new();

    // Every register starts as a small in-range byte address, so any of them is safe to use
    // as one.
    for register in 0..REGISTERS {
        let address = (rng.below(ADDRESS_LIMIT / 4)) * 4;
        program.push(v_mov_inline(t, register, address.min(64)));
    }
    for _ in 0..BODY {
        emit(t, &mut rng, &mut program);
    }
    program.push(head(t, "s_endpgm"));
    program
}

fn device_or_skip(test: &str) -> bool {
    match probe() {
        Availability::Available { .. } => true,
        Availability::Unavailable { reason } => {
            println!();
            println!("!! SKIPPED: {test}");
            println!("!! no Vulkan device: {reason}");
            println!("!! the models were NOT compared");
            println!();
            false
        }
    }
}

/// Runs a program at one fidelity, returning the observed registers and memory.
fn run(words: &[u32], fidelity: Fidelity) -> Option<(Vec<u32>, Vec<u32>)> {
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);
    if !decoded.is_trustworthy() {
        return None;
    }
    let translated = translate(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity,
            width: Width::default(),
        },
    )
    .ok()?;
    let out = dispatch(&translated.module, OBSERVED, MEMORY_WORDS, [1, 1, 1]).ok()?;
    Some((out.observed, out.memory))
}

/// Both models leave identical registers and memory on every generated program.
#[test]
fn the_two_models_agree_on_generated_programs() {
    if !device_or_skip("the_two_models_agree_on_generated_programs") {
        return;
    }

    let table = EncodingTable::builtin().expect("encodings");
    let mut compared = 0u32;
    for seed in 1..=u64::from(PROGRAMS) {
        let words = program(&table, seed);

        // A program either model refuses is outside the overlap; the count below insists
        // most were compared.
        let Some(lane) = run(&words, Fidelity::Lane) else {
            continue;
        };
        let Some(wave) = run(&words, Fidelity::Wavefront) else {
            continue;
        };

        assert_eq!(
            lane.0, wave.0,
            concat!(
                "seed {}: the models disagree about the registers.\n",
                "program: {:#010x?}"
            ),
            seed, words
        );
        assert_eq!(
            lane.1, wave.1,
            concat!(
                "seed {}: the models disagree about guest memory.\n",
                "program: {:#010x?}"
            ),
            seed, words
        );
        compared += 1;
    }

    println!("{compared} of {PROGRAMS} generated programs compared");
    // Most must have run, or the test compares nothing.
    assert!(
        compared * 2 >= PROGRAMS,
        concat!(
            "only {} of {} programs were comparable - the generator is ",
            "producing something neither model accepts, so this test is checking almost ",
            "nothing"
        ),
        compared,
        PROGRAMS
    );
}

/// Every generated program decodes cleanly and translates.
#[test]
fn the_generator_produces_programs_that_translate() {
    // Separate from the comparison, so a generator fault reports as one.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");

    let mut translatable = 0u32;
    for seed in 1..=u64::from(PROGRAMS) {
        let words = program(&table, seed);
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &table, &operands);
        assert!(
            decoded.is_trustworthy(),
            concat!(
                "seed {}: the generator emitted something that does not decode cleanly.\n",
                "program: {:#010x?}"
            ),
            seed,
            words
        );
        match translate(&decoded, &table, Strategy::default()) {
            Ok(_) => translatable += 1,
            Err(e) => println!("seed {seed}: {e}"),
        }
    }
    assert_eq!(
        translatable, PROGRAMS,
        concat!(
            "every generated program should translate - the generator only emits ",
            "instructions both models accept"
        )
    );
}
