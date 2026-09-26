//! Translating guest instructions and running the result.
//!
//! Each test states what a guest instruction does, and passes only if a real device agrees;
//! `spirv-val` accepting the module is necessary, not sufficient. The translated module
//! copies the low registers into the storage buffer before returning, so `buffer[n]` is
//! vector register `n` at the end of the shader. A missing device skips loudly, since a
//! harness hides a passing test's output; `bin/orbistoun check` surfaces it.

use orbistoun_gpu_vulkan::{Availability, dispatch, probe};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_spirv::op;
use orbistoun_translate::predicated::MEMORY_WORDS as MEMORY_WORDS_U32;
use orbistoun_translate::{Fidelity, Strategy, Width, translate};

/// Guest-memory words a translated module sees.
const MEMORY_WORDS: usize = MEMORY_WORDS_U32 as usize;

/// Registers copied out of each file.
const PER_FILE: usize = 8;
/// The buffer holds the vector file then the scalar file.
const OBSERVED: usize = PER_FILE * 2;

/// The loaded encoding table, read once for the whole file.
fn encodings() -> &'static EncodingTable {
    static TABLE: std::sync::OnceLock<EncodingTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| EncodingTable::builtin().expect("encodings"))
}

/// The first word of an instruction, from its name: family bits plus opcode in place.
///
/// Every instruction in this file is built through here, so the tests follow the target
/// generation's encodings rather than hard-coding numbers.
fn head(name: &str) -> u32 {
    let table = encodings();
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

/// `s_endpgm`, which every program here ends with.
fn s_endpgm() -> u32 {
    head("s_endpgm")
}

/// `s_waitcnt` with a zero count.
fn s_waitcnt() -> u32 {
    head("s_waitcnt")
}

/// Vector register `n`, from a returned buffer.
fn vector(registers: &[u32], n: usize) -> u32 {
    registers[n]
}

/// Scalar register `n`, from a returned buffer.
fn scalar(registers: &[u32], n: usize) -> u32 {
    registers[PER_FILE + n]
}

fn device_or_skip(test: &str) -> bool {
    match probe() {
        Availability::Available { .. } => true,
        Availability::Unavailable { reason } => {
            println!();
            println!("!! SKIPPED: {test}");
            println!("!! no Vulkan device: {reason}");
            println!("!! translated shaders were NOT executed");
            println!();
            false
        }
    }
}

/// Assembles a guest instruction stream, translates it, runs it, returns the registers.
fn run(words: &[u32]) -> Vec<u32> {
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

    let decoded = decode(&bytes, &table, &operands);
    assert!(
        decoded.is_trustworthy(),
        "the guest stream must decode cleanly before translation is the thing under test"
    );

    let translated = translate(&decoded, &table, Strategy::default()).expect("translate");
    dispatch(&translated.module, OBSERVED, MEMORY_WORDS, [1, 1, 1])
        .expect("dispatch")
        .observed
}

/// `v_mov_b32_e32 vN, <inline constant>`.
///
/// Built here rather than assembled, so no toolchain is needed.
fn v_mov_inline(dst: u32, constant: u32) -> u32 {
    // The source code for a small non-negative integer is 128 + value.
    head("v_mov_b32_e32") | (dst << 17) | (128 + constant)
}

/// A move of an inline constant reaches the named vector register.
#[test]
fn a_move_of_an_inline_constant_reaches_the_register() {
    if !device_or_skip("a_move_of_an_inline_constant_reaches_the_register") {
        return;
    }

    let registers = run(&[v_mov_inline(0, 5), s_endpgm()]);
    assert_eq!(
        vector(&registers, 0),
        5,
        "v0 should hold 5; the register file came back as {registers:?}"
    );
}

/// A move writes only the register its destination field names.
#[test]
fn a_move_writes_only_the_register_it_names() {
    if !device_or_skip("a_move_writes_only_the_register_it_names") {
        return;
    }

    let registers = run(&[v_mov_inline(3, 9), s_endpgm()]);
    assert_eq!(
        vector(&registers, 3),
        9,
        "v3 should hold 9, got {registers:?}"
    );
    assert_eq!(
        vector(&registers, 0),
        0,
        "v0 was never written, got {registers:?}"
    );
    assert_eq!(
        vector(&registers, 1),
        0,
        "v1 was never written, got {registers:?}"
    );
}

/// Instructions run in order, and the last write to a register wins.
#[test]
fn later_moves_overwrite_earlier_ones() {
    if !device_or_skip("later_moves_overwrite_earlier_ones") {
        return;
    }

    let registers = run(&[
        v_mov_inline(2, 1),
        v_mov_inline(2, 7),
        v_mov_inline(4, 3),
        s_endpgm(),
    ]);
    assert_eq!(
        vector(&registers, 2),
        7,
        "the later move wins, got {registers:?}"
    );
    assert_eq!(vector(&registers, 4), 3, "got {registers:?}");
}

/// `s_mov_b32 sN, <source code>`, for a code the inline-integer helper cannot express.
fn s_mov_code(dst: u32, code: u32) -> u32 {
    head("s_mov_b32") | (dst << 16) | code
}

/// `s_mov_b32 sN, <inline constant>`.
fn s_mov_inline(dst: u32, constant: u32) -> u32 {
    // sdst at bit 16, ssrc0 in the low byte.
    head("s_mov_b32") | (dst << 16) | (128 + constant)
}

/// A scalar move reaches the scalar file.
#[test]
fn a_scalar_move_reaches_a_scalar_register() {
    if !device_or_skip("a_scalar_move_reaches_a_scalar_register") {
        return;
    }

    let registers = run(&[s_mov_inline(2, 6), s_endpgm()]);
    assert_eq!(
        scalar(&registers, 2),
        6,
        "s2 should hold 6, got {registers:?}"
    );
}

/// Writing s3 leaves v3 alone and the other way round: the files are separate.
#[test]
fn the_scalar_and_vector_files_are_separate() {
    if !device_or_skip("the_scalar_and_vector_files_are_separate") {
        return;
    }

    let registers = run(&[s_mov_inline(3, 11), v_mov_inline(3, 4), s_endpgm()]);
    assert_eq!(scalar(&registers, 3), 11, "s3, got {registers:?}");
    assert_eq!(vector(&registers, 3), 4, "v3, got {registers:?}");
}

/// The operand code the table names `m0`: past the last scalar register.
const M0_CODE: u32 = 124;

/// A value moved through m0 round-trips without touching the scalar file, in both models.
#[test]
fn a_move_through_m0_round_trips_and_leaves_the_scalar_file_alone() {
    // m0 is scalar state outside the register file, written before the first
    // interpolation by compiled pixel shaders. Aliasing it onto a scalar register would
    // corrupt that register; dropping the write would read back zero.
    if !device_or_skip("a_move_through_m0_round_trips_and_leaves_the_scalar_file_alone") {
        return;
    }

    for fidelity in [Fidelity::Lane, Fidelity::Wavefront] {
        let registers = run_at(
            fidelity,
            &[
                s_mov_inline(2, 9),
                s_mov_code(M0_CODE, 2),
                s_mov_inline(2, 1),
                s_mov_code(5, M0_CODE),
                s_endpgm(),
            ],
        );
        assert_eq!(
            scalar(&registers, 5),
            9,
            "{fidelity:?}: s5 should hold what m0 held, got {registers:?}"
        );
        assert_eq!(
            scalar(&registers, 2),
            1,
            "{fidelity:?}: s2 was overwritten after the copy into m0, got {registers:?}"
        );
    }
}

/// `s_waitcnt` translates and changes no register.
#[test]
fn a_wait_instruction_translates_and_changes_nothing() {
    // It orders memory operations and computes nothing, but most compiled shaders contain
    // it.
    if !device_or_skip("a_wait_instruction_translates_and_changes_nothing") {
        return;
    }

    let registers = run(&[v_mov_inline(1, 7), s_waitcnt(), s_endpgm()]);
    assert_eq!(vector(&registers, 1), 7, "got {registers:?}");
}

/// The supported list and the translator agree on every instruction the table names.
#[test]
fn the_supported_list_and_the_translator_agree() {
    // A worklist built on a list that drifted from the translator would rank finished work
    // or hide unfinished work. Needs no device.

    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");

    for (family, opcode, name, words) in known_encodings() {
        let (family, name) = (family.as_str(), name.as_str());
        let bytes: Vec<u8> = words
            .iter()
            .copied()
            .chain(std::iter::once(s_endpgm()))
            .flat_map(u32::to_le_bytes)
            .collect();
        let decoded = decode(&bytes, &table, &operands);
        let translated = translate(&decoded, &table, Strategy::default());

        // Asked through the loaded table: the list names instructions, and the encodings
        // decide where they live on this target.
        let listed = orbistoun_translate::model::supports_named(&table, family, opcode);

        // Whether it was refused for not being supported. A supported instruction can
        // still be refused for its operands: these encodings carry zero in every operand
        // field, which for carry arithmetic names an ordinary register pair where only the
        // condition mask is handled.
        let unsupported = matches!(
            &translated,
            Err(orbistoun_translate::TranslateError::Unsupported { detail, .. })
                if *detail == orbistoun_translate::model::NO_TRANSLATION
                    || orbistoun_translate::model::blocked(name) == Some(*detail)
        );
        // A listed instruction must not be refused as unsupported. An absent one must be
        // refused somehow; a more specific reason (an export has no operand layout) counts.
        if listed {
            assert!(
                !unsupported,
                concat!(
                    "{}:{:#x} is listed in SUPPORTED but the translator rejected ",
                    "it as unsupported: {:?}"
                ),
                family,
                opcode,
                translated.err(),
            );
        } else {
            assert!(
                translated.is_err(),
                concat!(
                    "{}:{:#x} is absent from SUPPORTED but the translator ",
                    "accepted it"
                ),
                family,
                opcode,
            );
        }
    }
}

/// Every instruction this target has a name for, as bytes that decode back to it.
///
/// Derived from the table rather than written by hand, so a checker does not share the
/// understanding it checks. Every name the table knows appears, supported or not, which
/// makes the absent direction meaningful. Operand fields are zero, a valid encoding for
/// every family here (scalar register zero).
fn known_encodings() -> Vec<(String, u32, String, Vec<u32>)> {
    let table = encodings();
    let mut out = Vec::new();
    for (family, opcode, name) in table.names() {
        // Padding is excluded: the decoder stops at it, so it never reaches the translator.
        if name == "s_code_end" {
            continue;
        }
        let Some(encoding) = table.encodings().iter().find(|e| e.name == family) else {
            continue;
        };
        let mut words = vec![encoding.value | (opcode << encoding.opcode.shift)];
        // A wide encoding needs its second word, or the appended terminator would be read
        // as it.
        words.resize((encoding.width_bytes / 4) as usize, 0);
        out.push((family.to_owned(), opcode, name.to_owned(), words));
    }
    out
}

/// Runs a stream at a chosen fidelity.
fn run_at(fidelity: Fidelity, words: &[u32]) -> Vec<u32> {
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);
    let translated = translate(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity,
            width: Width::default(),
        },
    )
    .unwrap_or_else(|e| panic!("translate at {fidelity}: {e}"));
    assert_eq!(
        translated.fidelity, fidelity,
        "the level used must be reported"
    );
    dispatch(&translated.module, OBSERVED, MEMORY_WORDS, [1, 1, 1])
        .expect("dispatch")
        .observed
}

/// Programs both levels must agree on.
fn agreement_cases() -> Vec<(&'static str, Vec<u32>)> {
    vec![
        (
            "a single vector move",
            vec![v_mov_inline(0, 10), s_endpgm()],
        ),
        ("a scalar move", vec![s_mov_inline(2, 6), s_endpgm()]),
        (
            "both files at once",
            vec![s_mov_inline(3, 11), v_mov_inline(3, 4), s_endpgm()],
        ),
        (
            "a wait between moves",
            vec![v_mov_inline(1, 7), s_waitcnt(), s_endpgm()],
        ),
    ]
}

/// The wavefront and lane models leave identical registers on the agreement programs.
#[test]
fn the_wavefront_and_lane_models_agree() {
    // Two independent models of the same machine, checked against each other with no
    // reference hardware (D100).
    if !device_or_skip("the_wavefront_and_lane_models_agree") {
        return;
    }

    for (what, program) in agreement_cases() {
        let lane = run_at(Fidelity::Lane, &program);
        let wavefront = run_at(Fidelity::Wavefront, &program);
        assert_eq!(
            lane, wavefront,
            "{what}: the two wavefront models disagree.\n  lane:      {lane:?}\n  wavefront: {wavefront:?}"
        );
    }
}

/// The wavefront model starts with every lane active.
#[test]
fn the_wavefront_model_starts_with_every_lane_active() {
    // A mask starting at zero would execute nothing and still produce a buffer of zeros.
    if !device_or_skip("the_wavefront_model_starts_with_every_lane_active") {
        return;
    }

    let registers = run_at(Fidelity::Wavefront, &[v_mov_inline(1, 12), s_endpgm()]);
    assert_eq!(
        vector(&registers, 1),
        12,
        "lane zero should have executed the move, got {registers:?}"
    );
}

/// `v_mov_b32_e32 vN, <named inline constant>`, by its operand code.
fn v_mov_code(dst: u32, code: u32) -> u32 {
    head("v_mov_b32_e32") | (dst << 17) | code
}

/// `v_add_f32_e32 vD, vA, vB` / `v_mul_f32_e32`, by VOP2 opcode.
fn vop2_vv(name: &str, dst: u32, src0: u32, src1: u32) -> u32 {
    // src0 uses the shared operand numbering, where vector registers start at 256.
    head(name) | (dst << 17) | (src1 << 9) | (256 + src0)
}

/// `v_op_f32_e32 vD, vS`, by VOP1 opcode name.
fn vop1_vv(name: &str, dst: u32, src0: u32) -> u32 {
    // src0 uses the shared operand numbering, where vector registers start at 256.
    head(name) | (dst << 17) | (256 + src0)
}

/// Operand codes for the inline floats, from `data/operands.toml`.
const INLINE_ONE: u32 = 242;
const INLINE_TWO: u32 = 244;
const INLINE_HALF: u32 = 240;

/// Float addition bitcasts register contents rather than converting them.
#[test]
fn float_addition_produces_the_right_bits() {
    // Converting would turn the bit pattern for 1.0 into the float 1065353216.0.
    if !device_or_skip("float_addition_produces_the_right_bits") {
        return;
    }

    let registers = run(&[
        v_mov_code(0, INLINE_ONE),
        v_mov_code(1, INLINE_TWO),
        vop2_vv("v_add_f32_e32", 2, 0, 1),
        s_endpgm(),
    ]);
    // Compared as bits: the values are exact and the claim is about the register's bits.
    assert_eq!(
        vector(&registers, 2),
        3.0_f32.to_bits(),
        "1.0 + 2.0; got bits {:#x}",
        vector(&registers, 2)
    );
}

/// Two floats pack into one register as halves, the first source low.
///
/// 1.0 is `0x3c00` and 2.0 is `0x4000` as halves, both exact, so the packed word is
/// `0x4000_3c00` whatever the rounding; swapped halves or conversion give a different word.
#[test]
fn two_floats_pack_into_one_register_as_halves() {
    if !device_or_skip("two_floats_pack_into_one_register_as_halves") {
        return;
    }
    let registers = run(&[
        v_mov_code(0, INLINE_ONE),
        v_mov_code(1, INLINE_TWO),
        vop2_vv("v_cvt_pkrtz_f16_f32_e32", 2, 0, 1),
        s_endpgm(),
    ]);
    assert_eq!(
        vector(&registers, 2),
        0x4000_3c00,
        "half 1.0 low, half 2.0 high; got {:#x}",
        vector(&registers, 2)
    );
}

/// Float multiplication produces the right bits.
#[test]
fn float_multiplication_produces_the_right_bits() {
    if !device_or_skip("float_multiplication_produces_the_right_bits") {
        return;
    }

    let registers = run(&[
        v_mov_code(0, INLINE_TWO),
        v_mov_code(1, INLINE_HALF),
        vop2_vv("v_mul_f32_e32", 3, 0, 1),
        s_endpgm(),
    ]);
    assert_eq!(
        vector(&registers, 3),
        1.0_f32.to_bits(),
        "2.0 * 0.5; got bits {:#x}",
        vector(&registers, 3)
    );
}

/// `v_max_f32`/`v_min_f32` reach the GLSL.std.450 `FMax`/`FMin` extended instructions.
#[test]
fn float_min_and_max_produce_the_right_bits() {
    // No core SPIR-V opcode exists for these, so this covers the set import, the extended
    // instruction number and the bitcasts around `OpExtInst`.
    if !device_or_skip("float_min_and_max_produce_the_right_bits") {
        return;
    }

    let registers = run(&[
        v_mov_code(0, INLINE_ONE),
        v_mov_code(1, INLINE_TWO),
        vop2_vv("v_max_f32_e32", 2, 0, 1),
        vop2_vv("v_min_f32_e32", 3, 0, 1),
        s_endpgm(),
    ]);
    assert_eq!(
        vector(&registers, 2),
        2.0_f32.to_bits(),
        "max(1.0, 2.0); got bits {:#x}",
        vector(&registers, 2)
    );
    assert_eq!(
        vector(&registers, 3),
        1.0_f32.to_bits(),
        "min(1.0, 2.0); got bits {:#x}",
        vector(&registers, 3)
    );
}

/// `v_sqrt_f32` and `v_rsq_f32` produce the right bits.
#[test]
fn float_unary_sqrt_and_rsq_produce_the_right_bits() {
    if !device_or_skip("float_unary_sqrt_and_rsq_produce_the_right_bits") {
        return;
    }

    // sqrt(4.0) == 2.0, rsq(4.0) == 0.5
    let mut words = Vec::new();
    words.extend_from_slice(&v_mov_literal(0, 4.0_f32.to_bits()));
    words.push(vop1_vv("v_sqrt_f32_e32", 1, 0));
    words.push(vop1_vv("v_rsq_f32_e32", 2, 0));
    words.push(s_endpgm());

    let registers = run(&words);
    assert_eq!(
        vector(&registers, 1),
        2.0_f32.to_bits(),
        "sqrt(4.0); got bits {:#x} ({})",
        vector(&registers, 1),
        f32::from_bits(vector(&registers, 1))
    );
    assert_eq!(
        vector(&registers, 2),
        0.5_f32.to_bits(),
        "rsq(4.0); got bits {:#x} ({})",
        vector(&registers, 2),
        f32::from_bits(vector(&registers, 2))
    );
}

/// `v_exp_f32` and `v_log_f32` are base two.
#[test]
fn float_unary_transcendentals_produce_the_right_bits() {
    if !device_or_skip("float_unary_transcendentals_produce_the_right_bits") {
        return;
    }

    // exp2(3.0) == 8.0, log2(8.0) == 3.0
    let mut words = Vec::new();
    words.extend_from_slice(&v_mov_literal(0, 3.0_f32.to_bits()));
    words.extend_from_slice(&v_mov_literal(1, 8.0_f32.to_bits()));
    words.push(vop1_vv("v_exp_f32_e32", 2, 0));
    words.push(vop1_vv("v_log_f32_e32", 3, 1));
    words.push(s_endpgm());

    let registers = run(&words);
    assert_eq!(
        vector(&registers, 2),
        8.0_f32.to_bits(),
        "exp2(3.0); got bits {:#x} ({})",
        vector(&registers, 2),
        f32::from_bits(vector(&registers, 2))
    );
    assert_eq!(
        vector(&registers, 3),
        3.0_f32.to_bits(),
        "log2(8.0); got bits {:#x} ({})",
        vector(&registers, 3),
        f32::from_bits(vector(&registers, 3))
    );
}

/// `v_sin_f32` and `v_cos_f32` take their angle in revolutions.
#[test]
fn float_unary_trig_produces_the_right_bits() {
    if !device_or_skip("float_unary_trig_produces_the_right_bits") {
        return;
    }

    // sin/cos of 2*pi*x: a quarter turn gives 1.0 and 0.0, where dropping the scale would
    // give 0.247 and 0.969 (at zero both conventions agree). Compared approximately:
    // 2*pi is inexact and the extended sin/cos are not correctly rounded.
    let mut words = Vec::new();
    words.extend_from_slice(&v_mov_literal(0, 0.25_f32.to_bits()));
    words.push(vop1_vv("v_sin_f32_e32", 1, 0));
    words.push(vop1_vv("v_cos_f32_e32", 2, 0));
    words.push(s_endpgm());

    let registers = run(&words);
    let sin = f32::from_bits(vector(&registers, 1));
    let cos = f32::from_bits(vector(&registers, 2));
    assert!(
        (sin - 1.0).abs() < 1e-3,
        "sin(2*pi*0.25) should be 1.0; got {sin} (bits {:#x})",
        vector(&registers, 1)
    );
    assert!(
        cos.abs() < 1e-3,
        "cos(2*pi*0.25) should be 0.0; got {cos} (bits {:#x})",
        vector(&registers, 2)
    );
}

/// An inline float constant reaches a register as its bit pattern.
#[test]
fn an_inline_float_constant_reaches_a_register_unconverted() {
    // Treating the operand code as a number would store 242; converting would store
    // something enormous.
    if !device_or_skip("an_inline_float_constant_reaches_a_register_unconverted") {
        return;
    }

    let registers = run(&[v_mov_code(4, INLINE_ONE), s_endpgm()]);
    assert_eq!(
        vector(&registers, 4),
        1.0_f32.to_bits(),
        "got {:#x}",
        vector(&registers, 4)
    );
}

/// The models agree on float arithmetic, computed once or per lane.
#[test]
fn the_models_agree_on_float_arithmetic() {
    if !device_or_skip("the_models_agree_on_float_arithmetic") {
        return;
    }

    let program = [
        v_mov_code(0, INLINE_ONE),
        v_mov_code(1, INLINE_TWO),
        vop2_vv("v_add_f32_e32", 2, 0, 1),
        vop2_vv("v_mul_f32_e32", 3, 2, 1),
        s_endpgm(),
    ];
    let lane = run_at(Fidelity::Lane, &program);
    let wavefront = run_at(Fidelity::Wavefront, &program);
    assert_eq!(
        lane, wavefront,
        "\n  lane:      {lane:?}\n  wavefront: {wavefront:?}"
    );
}

/// Runs a stream and returns guest memory as well as the registers.
fn run_memory(fidelity: Fidelity, words: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);
    let translated = translate(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity,
            width: Width::default(),
        },
    )
    .unwrap_or_else(|e| panic!("translate at {fidelity}: {e}"));
    let out = dispatch(&translated.module, OBSERVED, MEMORY_WORDS, [1, 1, 1]).expect("dispatch");
    (out.observed, out.memory)
}

/// `global_store_dword vAddr, vData, off`.
///
/// The base field holds the code for "no scalar base".
fn global_store(vaddr: u32, vdata: u32) -> [u32; 2] {
    [
        head("global_store_dword"),
        vaddr | (vdata << 8) | (FLAT_NO_BASE_CODE << 16),
    ]
}

/// `global_load_dword vDst, vAddr, off`.
fn global_load(vdst: u32, vaddr: u32) -> [u32; 2] {
    [
        head("global_load_dword"),
        vaddr | (FLAT_NO_BASE_CODE << 16) | (vdst << 24),
    ]
}

/// `s_load_dwordxN sDst, s[base:base+1], offset`, from the solved layout.
///
/// Destination at bit 6, base halved at bit 0, byte offset in the second word.
fn s_load(name: &str, dst: u32, base: u32, offset: u32) -> [u32; 2] {
    [head(name) | (dst << 6) | (base / 2), offset]
}

/// A wide scalar load running past the register file is refused, not truncated.
#[test]
fn a_wide_scalar_load_past_the_register_file_is_refused() {
    // `s_load_dwordx8` into s100 would write four registers and then four special
    // registers, since the operand numbering runs on past the file. Needs no device.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");

    let mut program: Vec<u32> = Vec::new();
    program.extend(s_load("s_load_dwordx8", 100, 0, 0));
    program.push(s_endpgm());
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);

    let error = translate(&decoded, &table, Strategy::default())
        .expect_err("a load running off the register file must be refused");
    assert!(
        error
            .to_string()
            .contains("past the end of the register file"),
        "the error should say what is wrong, got: {error}"
    );
}

/// `s_load_dwordx4` fills four consecutive registers from one byte address.
#[test]
fn a_wide_scalar_load_fills_consecutive_registers() {
    if !device_or_skip("a_wide_scalar_load_fills_consecutive_registers") {
        return;
    }

    // Seed four consecutive words through stores, since guest memory starts zeroed.
    // Addresses 16, 20, 24, 28 are words 4 to 7.
    let mut program = Vec::new();
    for (i, value) in [11u32, 22, 33, 44].iter().enumerate() {
        let address = 16 + (i as u32) * 4;
        program.push(v_mov_inline(0, address));
        program.push(v_mov_inline(1, *value));
        program.extend(global_store(0, 1));
    }
    // s0 is zero, so the address is the offset alone.
    program.extend(s_load("s_load_dwordx4", 2, 0, 16));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Lane, &program);
    for (i, expected) in [11u32, 22, 33, 44].iter().enumerate() {
        assert_eq!(
            scalar(&registers, 2 + i),
            *expected,
            "s{} should hold {expected}; registers were {registers:?}",
            2 + i
        );
    }
}

/// `s_load_dwordx2` writes no register beyond its width.
#[test]
fn a_wide_scalar_load_writes_no_register_beyond_its_width() {
    if !device_or_skip("a_wide_scalar_load_writes_no_register_beyond_its_width") {
        return;
    }

    let mut program = Vec::new();
    for (i, value) in [5u32, 6, 7].iter().enumerate() {
        let address = 32 + (i as u32) * 4;
        program.push(v_mov_inline(0, address));
        program.push(v_mov_inline(1, *value));
        program.extend(global_store(0, 1));
    }
    program.extend(s_load("s_load_dwordx2", 1, 0, 32));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(scalar(&registers, 1), 5, "registers were {registers:?}");
    assert_eq!(scalar(&registers, 2), 6, "registers were {registers:?}");
    assert_eq!(
        scalar(&registers, 3),
        0,
        "a two-word load must not touch the third register; registers were {registers:?}"
    );
}

/// The models agree about a wide scalar load, written by different routes.
#[test]
fn the_models_agree_about_a_wide_scalar_load() {
    if !device_or_skip("the_models_agree_about_a_wide_scalar_load") {
        return;
    }

    let mut program = Vec::new();
    // All within 0..=64, which is what an inline integer constant can carry.
    for (i, value) in [3u32, 17, 41, 64].iter().enumerate() {
        let address = 8 + (i as u32) * 4;
        program.push(v_mov_inline(0, address));
        program.push(v_mov_inline(1, *value));
        program.extend(global_store(0, 1));
    }
    program.extend(s_load("s_load_dwordx4", 4, 0, 8));
    program.push(s_endpgm());

    let (lane, lane_memory) = run_memory(Fidelity::Lane, &program);
    let (wave, wave_memory) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(lane, wave, "the models disagree about the registers");
    assert_eq!(
        lane_memory, wave_memory,
        "the models disagree about guest memory"
    );
}

/// A store reaches guest memory at its address and nowhere else.
#[test]
fn a_store_reaches_guest_memory() {
    // Address eight is word two.
    if !device_or_skip("a_store_reaches_guest_memory") {
        return;
    }

    let mut program = vec![v_mov_inline(0, 8), v_mov_inline(1, 42)];
    program.extend(global_store(0, 1));
    program.push(s_endpgm());

    let (_, memory) = run_memory(Fidelity::Lane, &program);
    assert_eq!(memory[2], 42, "address 8 is word 2; memory was {memory:?}");
    assert_eq!(memory[0], 0, "nothing else should have moved");
    assert_eq!(memory[3], 0, "nothing else should have moved");
}

/// A value stored can be loaded back through the same address.
#[test]
fn a_value_stored_can_be_loaded_back() {
    if !device_or_skip("a_value_stored_can_be_loaded_back") {
        return;
    }

    let mut program = vec![v_mov_inline(0, 16), v_mov_inline(1, 7)];
    program.extend(global_store(0, 1));
    program.extend(global_load(2, 0));
    program.push(s_endpgm());

    let (registers, memory) = run_memory(Fidelity::Lane, &program);
    assert_eq!(memory[4], 7, "address 16 is word 4; memory was {memory:?}");
    assert_eq!(
        vector(&registers, 2),
        7,
        "loaded back into v2, got {registers:?}"
    );
}

/// The models agree about memory: one store, or sixty-four under a mask.
#[test]
fn the_models_agree_about_memory() {
    if !device_or_skip("the_models_agree_about_memory") {
        return;
    }

    let mut program = vec![v_mov_inline(0, 24), v_mov_inline(1, 19)];
    program.extend(global_store(0, 1));
    program.extend(global_load(3, 0));
    program.push(s_endpgm());

    let (lane_regs, lane_mem) = run_memory(Fidelity::Lane, &program);
    let (wave_regs, wave_mem) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(lane_mem, wave_mem, "memory differs");
    assert_eq!(lane_regs, wave_regs, "registers differ");
}

/// `s_mov_b64 s[dst:dst+1], <source code>`.
///
/// Destination at bit 16, source code at bit 0, opcode 1 at bit 8.
fn s_mov_b64(dst: u32, source_code: u32) -> u32 {
    head("s_mov_b64") | (dst << 16) | source_code
}

/// A 64-bit move sign-extends a constant: 1 zeroes the high half, -1 fills it.
#[test]
fn a_sixty_four_bit_move_extends_a_constant_rather_than_repeating_it() {
    if !device_or_skip("a_sixty_four_bit_move_extends_a_constant_rather_than_repeating_it") {
        return;
    }

    // Inline integer 1 is code 129; inline integer -1 is code 193.
    let registers = run(&[s_mov_b64(0, 129), s_mov_b64(2, 193), s_endpgm()]);

    assert_eq!(scalar(&registers, 0), 1, "registers were {registers:?}");
    assert_eq!(
        scalar(&registers, 1),
        0,
        "a positive constant leaves the high half zero; registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 2),
        u32::MAX,
        "registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 3),
        u32::MAX,
        "a negative constant fills the high half; registers were {registers:?}"
    );
}

/// A 64-bit move copies both halves of a register pair.
#[test]
fn a_sixty_four_bit_move_copies_both_halves_of_a_register_pair() {
    if !device_or_skip("a_sixty_four_bit_move_copies_both_halves_of_a_register_pair") {
        return;
    }

    // Distinct values, so copying the low half twice is visible.
    let registers = run(&[
        s_mov_inline(4, 9),
        s_mov_inline(5, 23),
        s_mov_b64(0, 4),
        s_endpgm(),
    ]);

    assert_eq!(scalar(&registers, 0), 9, "registers were {registers:?}");
    assert_eq!(scalar(&registers, 1), 23, "registers were {registers:?}");
}

/// The models agree about a 64-bit move.
#[test]
fn the_models_agree_about_a_sixty_four_bit_move() {
    if !device_or_skip("the_models_agree_about_a_sixty_four_bit_move") {
        return;
    }

    let program = [
        s_mov_inline(6, 5),
        s_mov_inline(7, 60),
        s_mov_b64(0, 6),
        s_mov_b64(2, 193),
        s_endpgm(),
    ];

    let (lane, _) = run_memory(Fidelity::Lane, &program);
    let (wave, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(lane, wave, "the models disagree about a 64-bit move");
}

/// A blocked instruction's refusal carries its reason, and every entry names a real
/// instruction.
#[test]
fn a_blocked_instruction_says_what_it_is_blocked_on() {
    // The reason separates "needs a subsystem" from unwritten work in the worklist. Needs
    // no device.
    use orbistoun_translate::model::{BLOCKED, blocked, supports};

    let table = encodings();
    for (name, reason) in BLOCKED {
        assert!(!supports(name), "{name} is in both SUPPORTED and BLOCKED");
        assert!(
            !reason.is_empty(),
            "{name} is blocked on nothing in particular"
        );
        assert_eq!(blocked(name), Some(*reason));

        // An entry for an instruction this target lacks can never be reached.
        assert!(
            table.find_by_name(name).is_some(),
            "{name} is blocked, but this target has no instruction by that name"
        );
    }

    assert_eq!(
        blocked("s_endpgm"),
        None,
        "a supported instruction is not blocked"
    );
}

/// `s_mov_b64 exec, <source code>`.
///
/// The mask's low half is register 126, and a 64-bit operand names its pair that way.
fn s_mov_exec(source_code: u32) -> u32 {
    s_mov_b64(126, source_code)
}

/// A cleared execution mask suppresses a vector write.
#[test]
fn a_cleared_execution_mask_suppresses_a_vector_write() {
    // Inline integer 0 is code 128; -1 is code 193.
    if !device_or_skip("a_cleared_execution_mask_suppresses_a_vector_write") {
        return;
    }

    let (enabled, _) = run_memory(
        Fidelity::Wavefront,
        &[s_mov_exec(193), v_mov_inline(0, 42), s_endpgm()],
    );
    assert_eq!(
        vector(&enabled, 0),
        42,
        "with every lane enabled the write lands; registers were {enabled:?}"
    );

    let (disabled, _) = run_memory(
        Fidelity::Wavefront,
        &[s_mov_exec(128), v_mov_inline(0, 42), s_endpgm()],
    );
    assert_eq!(
        vector(&disabled, 0),
        0,
        "with every lane disabled the write is suppressed; registers were {disabled:?}"
    );
}

/// The execution mask is read bit by bit, not tested against zero.
#[test]
fn the_execution_mask_is_per_lane_not_all_or_nothing() {
    // Mask 1 leaves lane 0 active; mask 2 leaves lane 1 active and lane 0 not. Both are
    // observed through lane 0, the lane the observation window reports.
    //
    // Inline integer 1 is code 129, 2 is code 130.
    if !device_or_skip("the_execution_mask_is_per_lane_not_all_or_nothing") {
        return;
    }

    let (lane_zero, _) = run_memory(
        Fidelity::Wavefront,
        &[s_mov_exec(129), v_mov_inline(0, 7), s_endpgm()],
    );
    assert_eq!(
        vector(&lane_zero, 0),
        7,
        "lane 0 is active in mask 1; registers were {lane_zero:?}"
    );

    let (lane_one, _) = run_memory(
        Fidelity::Wavefront,
        &[s_mov_exec(130), v_mov_inline(0, 7), s_endpgm()],
    );
    assert_eq!(
        vector(&lane_one, 0),
        0,
        concat!(
            "lane 0 is inactive in mask 2, so its register keeps its old value; ",
            "registers were {:?}"
        ),
        lane_one
    );
}

/// A store from an inactive lane leaves guest memory alone.
#[test]
fn a_masked_store_does_not_reach_guest_memory() {
    // Another lane would read what it wrote, which is why `write_memory` is a required
    // trait method.
    if !device_or_skip("a_masked_store_does_not_reach_guest_memory") {
        return;
    }

    let mut program = vec![v_mov_inline(0, 12), v_mov_inline(1, 33), s_mov_exec(128)];
    program.extend(global_store(0, 1));
    program.push(s_endpgm());

    let (_, memory) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        memory[3], 0,
        "address 12 is word 3, and no lane was active; memory was {memory:?}"
    );
}

/// The lane model refuses a mask write, and the wavefront model accepts it.
#[test]
fn the_lane_model_refuses_a_shader_that_writes_the_execution_mask() {
    // The lane model cannot represent an inactive lane, so it would run every lane the
    // guest disabled (D098). Needs no device.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let program = [s_mov_exec(128), s_endpgm()];
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);

    let error = translate(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        },
    )
    .expect_err("the lane model must refuse a mask write");
    assert!(
        error.to_string().contains("no execution mask"),
        "the error should name what is missing, got: {error}"
    );

    // The wavefront model accepts the same shader, so the refusal is the model's.
    translate(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::default(),
        },
    )
    .expect("the wavefront model has a mask and must accept this");
}

/// Automatic fidelity picks the lane model for a shader that leaves the mask alone and
/// the wavefront model for one that writes it.
#[test]
fn auto_fidelity_picks_the_model_the_shader_needs() {
    // Needs no device.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");

    let plain = [v_mov_inline(0, 1), s_endpgm()];
    let masked = [s_mov_exec(128), v_mov_inline(0, 1), s_endpgm()];

    for (program, expected) in [
        (&plain[..], Fidelity::Lane),
        (&masked[..], Fidelity::Wavefront),
    ] {
        let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &table, &operands);
        let translated = translate(&decoded, &table, Strategy::default()).expect("translate");
        assert_eq!(
            translated.fidelity, expected,
            "auto fidelity chose {} for this shader",
            translated.fidelity
        );
    }
}

/// `s_and_b64` / `s_or_b64` / `s_andn2_b64`, by opcode.
///
/// SOP2: destination at bit 16, first source in the low byte, second at bit 8.
fn s_logic_b64(name: &str, dst: u32, first: u32, second: u32) -> u32 {
    head(name) | (dst << 16) | (second << 8) | first
}

/// Anding the execution mask narrows which lanes write.
#[test]
fn narrowing_the_mask_narrows_which_lanes_write() {
    // Entering a conditional region: and the mask with the lanes that passed, here a
    // constant. The mask starts all ones; anding with 2 leaves lane 1 and not lane 0.
    if !device_or_skip("narrowing_the_mask_narrows_which_lanes_write") {
        return;
    }

    let excluded = [
        s_mov_exec(193),
        s_logic_b64("s_and_b64", 126, 126, 130),
        v_mov_inline(0, 55),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &excluded);
    assert_eq!(
        vector(&registers, 0),
        0,
        "lane 0 was anded out of the mask; registers were {registers:?}"
    );

    // Anding with 1 leaves lane 0, so the same write lands.
    let included = [
        s_mov_exec(193),
        s_logic_b64("s_and_b64", 126, 126, 129),
        v_mov_inline(0, 55),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &included);
    assert_eq!(
        vector(&registers, 0),
        55,
        "lane 0 survived the and; registers were {registers:?}"
    );
}

/// `s_andn2_b64` keeps the active lanes that were not taken.
#[test]
fn andn2_takes_the_lanes_the_first_branch_did_not() {
    // The else-branch: translated as a plain and, it would invert every else.
    if !device_or_skip("andn2_takes_the_lanes_the_first_branch_did_not") {
        return;
    }

    // Mask all ones, then remove lane 0: lane 0 must not write.
    let removed = [
        s_mov_exec(193),
        s_logic_b64("s_andn2_b64", 126, 126, 129),
        v_mov_inline(0, 9),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &removed);
    assert_eq!(
        vector(&registers, 0),
        0,
        "lane 0 was removed from the mask; registers were {registers:?}"
    );

    // Remove lane 1 instead: lane 0 still writes. A plain `and` gets this case wrong and
    // the previous one right.
    let kept = [
        s_mov_exec(193),
        s_logic_b64("s_andn2_b64", 126, 126, 130),
        v_mov_inline(0, 9),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &kept);
    assert_eq!(
        vector(&registers, 0),
        9,
        "only lane 1 was removed, so lane 0 still writes; registers were {registers:?}"
    );
}

/// 64-bit logic computes both halves.
#[test]
fn sixty_four_bit_logic_operates_on_both_halves() {
    // Into an ordinary register pair so both halves are observable; the mask tests above
    // use no lane above thirty-one.
    if !device_or_skip("sixty_four_bit_logic_operates_on_both_halves") {
        return;
    }

    // s[0:1] = -1 (both halves all ones), s[2:3] = 5 (low 5, high 0).
    // s[4:5] = s[0:1] & ~s[2:3]  ->  low = 0xFFFF_FFFA, high = 0xFFFF_FFFF.
    let program = [
        s_mov_b64(0, 193),
        s_mov_b64(2, 128 + 5),
        s_logic_b64("s_andn2_b64", 4, 0, 2),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        scalar(&registers, 4),
        0xFFFF_FFFA,
        "registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 5),
        u32::MAX,
        "the high half must be computed too; registers were {registers:?}"
    );
}

/// `v_cmp_<op>_f32_e32 vcc, src0, vsrc1`.
///
/// VOPC: source code in the low nine bits, second vector register at bit 9, opcode at
/// bit 17. The destination is `vcc` and occupies no bits.
fn v_cmp_f32(name: &str, src0_code: u32, vsrc1: u32) -> u32 {
    head(name) | (vsrc1 << 9) | src0_code
}

/// The code for a vector register in the shared source numbering.
fn vgpr_code(register: u32) -> u32 {
    256 + register
}

/// A comparison produces a mask that, anded into `exec`, narrows which lanes write.
#[test]
fn a_comparison_produces_a_mask_that_narrows_who_writes() {
    // Every lane compares the same two registers, so the mask is all-ones or all-zero;
    // a later test gives lanes different values.
    if !device_or_skip("a_comparison_produces_a_mask_that_narrows_who_writes") {
        return;
    }

    // v0 = 1.0, v1 = 2.0, so v0 < v1 holds. Inline float 1.0 is code 242, 2.0 is 244.
    let taken = [
        v_mov_code(0, 242),
        v_mov_code(1, 244),
        v_cmp_f32("v_cmp_lt_f32_e32", vgpr_code(0), 1),
        s_logic_b64("s_and_b64", 126, 126, 106),
        v_mov_inline(2, 63),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &taken);
    assert_eq!(
        vector(&registers, 2),
        63,
        "1.0 < 2.0, so every lane stays active; registers were {registers:?}"
    );

    // Reversed, no lane survives, so a comparison that always answered true fails here.
    let not_taken = [
        v_mov_code(0, 242),
        v_mov_code(1, 244),
        v_cmp_f32("v_cmp_gt_f32_e32", vgpr_code(0), 1),
        s_logic_b64("s_and_b64", 126, 126, 106),
        v_mov_inline(2, 63),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &not_taken);
    assert_eq!(
        vector(&registers, 2),
        0,
        "1.0 > 2.0 is false, so no lane writes; registers were {registers:?}"
    );
}

/// A float comparison compares floats, not their bits.
#[test]
fn a_comparison_compares_floats_not_the_bits() {
    // An integer comparison orders negatives backwards, so -1.0 < 1.0 separates the two.
    //
    // Inline float -1.0 is code 243, 1.0 is 242.
    if !device_or_skip("a_comparison_compares_floats_not_the_bits") {
        return;
    }

    let program = [
        v_mov_code(0, 243),
        v_mov_code(1, 242),
        v_cmp_f32("v_cmp_lt_f32_e32", vgpr_code(0), 1),
        s_logic_b64("s_and_b64", 126, 126, 106),
        v_mov_inline(2, 21),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        21,
        concat!(
            "-1.0 < 1.0 holds as floats; as unsigned bits it does not. Registers were ",
            "{:?}"
        ),
        registers
    );
}

/// A comparison writes `vcc`, not `exec`.
#[test]
fn a_comparison_writes_the_condition_mask_not_the_execution_mask() {
    // The shaders above and the result into `exec` straight after, so writing `exec`
    // directly would pass them.
    if !device_or_skip("a_comparison_writes_the_condition_mask_not_the_execution_mask") {
        return;
    }

    // Compare falsely, and do *not* and it into exec. Every lane must still be active,
    // so the following write lands.
    let program = [
        v_mov_code(0, 242),
        v_mov_code(1, 244),
        v_cmp_f32("v_cmp_gt_f32_e32", vgpr_code(0), 1),
        v_mov_inline(2, 17),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        17,
        concat!(
            "a false comparison must not disable any lane by itself; registers were ",
            "{:?}"
        ),
        registers
    );
}

/// The lane model refuses a comparison, and `Auto` routes it to the wavefront model.
#[test]
fn the_lane_model_refuses_a_comparison() {
    // A comparison produces one bit per lane, which this model has nowhere to put. Needs
    // no device.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let program = [v_cmp_f32("v_cmp_lt_f32_e32", vgpr_code(0), 1), s_endpgm()];
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);

    let error = translate(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        },
    )
    .expect_err("the lane model must refuse a comparison");
    assert!(
        error.to_string().contains("condition mask"),
        "the error should name what is missing, got: {error}"
    );

    // `Auto` routes it to the model that can.
    let translated = translate(&decoded, &table, Strategy::default()).expect("auto");
    assert_eq!(translated.fidelity, Fidelity::Wavefront);
}

/// `v_mbcnt_lo_u32_b32` / `v_mbcnt_hi_u32_b32`, as VOP3.
///
/// Destination in the low byte of word 0, opcode at bit 16; the two sources sit at bits
/// 0 and 9 of word 1, both in the shared numbering.
fn v_mbcnt(name: &str, dst: u32, mask_code: u32, addend_code: u32) -> [u32; 2] {
    [head(name) | dst, mask_code | (addend_code << 9)]
}

/// `v_add_nc_u32_e32` / `v_lshlrev_b32_e32`.
///
/// VOP2: destination at bit 17, first source in the low nine bits, second vector
/// register at bit 9.
fn v_int_op(name: &str, dst: u32, first_code: u32, second_vgpr: u32) -> u32 {
    head(name) | (dst << 17) | (second_vgpr << 9) | first_code
}

/// The pair that leaves a lane's own index in `dst`.
///
/// `lo` counts the all-ones low half below this lane, `hi` counts the high half and adds
/// the first result.
fn lane_index_into(dst: u32) -> [u32; 4] {
    // Inline -1 is code 193 (an all-ones mask); inline 0 is code 128.
    let lo = v_mbcnt("v_mbcnt_lo_u32_b32", dst, 193, 128);
    let hi = v_mbcnt("v_mbcnt_hi_u32_b32", dst, 193, vgpr_code(dst));
    [lo[0], lo[1], hi[0], hi[1]]
}

/// Each lane learns its own index through the `v_mbcnt` pair.
#[test]
fn a_lane_can_learn_its_own_index() {
    // Asserted through guest memory, since the register window reports only lane 0,
    // whose index is zero. Each lane stores its index at its own address.
    if !device_or_skip("a_lane_can_learn_its_own_index") {
        return;
    }

    let mut program: Vec<u32> = lane_index_into(0).to_vec();
    // v1 = v0 << 2, the byte address of word `index`.
    program.push(v_int_op("v_lshlrev_b32_e32", 1, 128 + 2, 0));
    program.extend(global_store(1, 0));
    program.push(s_endpgm());

    // Every word, not a sample: `v_lshlrev_b32` takes its amount first, and the reversed
    // reading (`2 << index`) agrees with `index << 2` only at lane 2.
    let (_, memory) = run_memory(Fidelity::Wavefront, &program);
    for word in 0..16usize {
        assert_eq!(
            memory[word], word as u32,
            "lane {word} should have written its index to word {word}; memory was {memory:?}"
        );
    }
}

/// A per-lane comparison leaves some lanes active and others not.
#[test]
fn a_comparison_can_leave_some_lanes_active_and_others_not() {
    // Lane n compares its own index against four, so the mask is a genuine mixture and a
    // translation testing the mask against zero fails.
    if !device_or_skip("a_comparison_can_leave_some_lanes_active_and_others_not") {
        return;
    }

    let mut program: Vec<u32> = lane_index_into(0).to_vec();
    program.push(v_int_op("v_lshlrev_b32_e32", 1, 128 + 2, 0));
    // v2 = 4, then vcc = (index < 4), then narrow exec to the lanes that passed.
    program.push(v_mov_inline(2, 4));
    program.push(v_cmp_f32("v_cmp_lt_u32_e32", vgpr_code(0), 2));
    program.push(s_logic_b64("s_and_b64", 126, 126, 106));
    program.extend(global_store(1, 0));
    program.push(s_endpgm());

    let (_, memory) = run_memory(Fidelity::Wavefront, &program);
    for word in 0..4usize {
        assert_eq!(
            memory[word], word as u32,
            "lane {word} passed the comparison and must have stored; memory was {memory:?}"
        );
    }
    for word in 4..16usize {
        assert_eq!(
            memory[word], 0,
            concat!(
                "lane {} failed the comparison and must not have stored; memory was ",
                "{:?}"
            ),
            word, memory
        );
    }
}

/// Each lane stores its own index to its own word, under an execution mask set by
/// `mask_program` - the shape of a primitive shader's `s_mov_b32 exec_lo, 7`.
fn stores_under_mask(mask_program: &[u32]) -> Vec<u32> {
    let mut program: Vec<u32> = mask_program.to_vec();
    program.extend(lane_index_into(0));
    program.push(v_int_op("v_lshlrev_b32_e32", 1, 128 + 2, 0));
    program.extend(global_store(1, 0));
    program.push(s_endpgm());
    program
}

/// The translated module's length in words.
fn module_words(words: &[u32]) -> usize {
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Wavefront,
        width: Width::default(),
    };
    translate(&decoded, &table, strategy)
        .expect("translate")
        .module
        .len()
}

/// A mask known at translation emits only the lanes it runs, and they are right.
#[test]
fn a_mask_known_at_translation_emits_only_the_lanes_it_runs() {
    // `s_mov_b32 exec_lo, <constant>` leaves the mask known until the block ends, so a lane
    // whose bit is clear emits no write and one whose bit is set writes without a select.
    // The module length shows the known path ran; the device run shows lanes 0 and 2 store
    // and no other.
    let known = stores_under_mask(&[s_mov_exec(128 + 5)]);
    let via_register = stores_under_mask(&[s_mov_b64(0, 128 + 5), s_mov_exec(0)]);
    let (known_words, register_words) = (module_words(&known), module_words(&via_register));
    assert!(
        known_words * 2 < register_words,
        "a known mask should emit well under half the code: {known_words} words against {register_words}"
    );

    if !device_or_skip("a_mask_known_at_translation_emits_only_the_lanes_it_runs") {
        return;
    }
    for program in [&known, &via_register] {
        let (_, memory) = run_memory(Fidelity::Wavefront, program);
        for lane in 0..64usize {
            let expected = if 0b101u64 >> lane & 1 != 0 {
                lane as u32
            } else {
                0
            };
            assert_eq!(memory[lane], expected, "lane {lane}; memory was {memory:?}");
        }
    }
}

/// A SOPP branch: opcode at bit 16, signed dword offset in the low half.
fn branch(name: &str, offset: i16) -> u32 {
    head(name) | u32::from(offset as u16)
}

/// A taken forward branch skips the block between.
#[test]
fn a_forward_branch_skips_the_block_it_jumps_over() {
    // Predication alone suppresses skipped vector writes; scalar work runs regardless of
    // the mask, so the skipped block writes a scalar register.
    if !device_or_skip("a_forward_branch_skips_the_block_it_jumps_over") {
        return;
    }

    // exec = 0, so `s_cbranch_execz` is taken and the middle block is skipped.
    //   0x0  s_mov_b64 exec, 0
    //   0x4  s_cbranch_execz +1   -> 0xc
    //   0x8  s_mov_b32 s0, 42     (skipped)
    //   0xc  s_mov_b32 s1, 7
    //   0x10 s_endpgm
    let taken = [
        s_mov_exec(128),
        branch("s_cbranch_execz", 1),
        s_mov_inline(0, 42),
        s_mov_inline(1, 7),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &taken);
    assert_eq!(
        scalar(&registers, 0),
        0,
        concat!(
            "the skipped block must not have run - a scalar write ignores the mask, so ",
            "falling through would show here; registers were {:?}"
        ),
        registers
    );
    assert_eq!(
        scalar(&registers, 1),
        7,
        "the branch target must have run; registers were {registers:?}"
    );

    // With every lane enabled the branch is not taken and both blocks run, so a
    // translation that never branches fails.
    let not_taken = [
        s_mov_exec(193),
        branch("s_cbranch_execz", 1),
        s_mov_inline(0, 42),
        s_mov_inline(1, 7),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Wavefront, &not_taken);
    assert_eq!(
        scalar(&registers, 0),
        42,
        concat!(
            "the branch was not taken, so the middle block must have run; registers were ",
            "{:?}"
        ),
        registers
    );
    assert_eq!(scalar(&registers, 1), 7, "registers were {registers:?}");
}

/// A backward branch loops until its condition fails.
#[test]
fn a_backward_branch_loops() {
    // A shader that counts, and stops on its own:
    //
    //   0x0  v_mov_b32 v0, 0          ; the counter
    //   0x4  v_mov_b32 v1, 1          ; the step
    //   0x8  v_mov_b32 v2, 5          ; the limit
    //   0xc  v_add_u32 v0, v0, v1     <- the loop target
    //   0x10 v_cmp_lt_u32 vcc, v0, v2
    //   0x14 s_and_b64 exec, exec, vcc
    //   0x18 s_cbranch_execnz -4      -> 0xc
    //   0x1c s_endpgm
    //
    // Five passes: the counter reaches five, the comparison fails, the mask empties and the
    // branch falls through. A body run once would leave one.
    if !device_or_skip("a_backward_branch_loops") {
        return;
    }

    let program = [
        v_mov_inline(0, 0),
        v_mov_inline(1, 1),
        v_mov_inline(2, 5),
        v_int_op("v_add_nc_u32_e32", 0, vgpr_code(0), 1),
        v_cmp_f32("v_cmp_lt_u32_e32", vgpr_code(0), 2),
        s_logic_b64("s_and_b64", 126, 126, 106),
        branch("s_cbranch_execnz", -4),
        s_endpgm(),
    ];

    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 0),
        5,
        concat!(
            "the loop should have run until the counter reached the limit; registers were ",
            "{:?}"
        ),
        registers
    );
}

/// `s_cmp_<op>_i32 src0, src1`.
///
/// SOPC: opcode at bit 16, second source at bit 8, first in the low byte. The answer goes
/// to the condition code, which is not an operand.
fn s_cmp_i32(name: &str, first_code: u32, second_code: u32) -> u32 {
    head(name) | (second_code << 8) | first_code
}

/// A scalar compare sets the condition code a branch reads.
#[test]
fn a_scalar_compare_drives_a_branch() {
    if !device_or_skip("a_scalar_compare_drives_a_branch") {
        return;
    }

    // s0 = 5. 5 < 1 is false, so scc is clear and s_cbranch_scc1 falls through.
    //   0x0  s_mov_b32 s0, 5
    //   0x4  s_cmp_lt_i32 s0, 1
    //   0x8  s_cbranch_scc1 +1   -> 0x10
    //   0xc  s_mov_b32 s1, 42
    //   0x10 s_endpgm
    let not_taken = [
        s_mov_inline(0, 5),
        s_cmp_i32("s_cmp_lt_i32", 0, 128 + 1),
        branch("s_cbranch_scc1", 1),
        s_mov_inline(1, 42),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Lane, &not_taken);
    assert_eq!(
        scalar(&registers, 1),
        42,
        "5 < 1 is false, so the branch is not taken; registers were {registers:?}"
    );

    // 5 < 64 is true, so the branch is taken and the block skipped; a compare that always
    // answered false fails here.
    let taken = [
        s_mov_inline(0, 5),
        s_cmp_i32("s_cmp_lt_i32", 0, 128 + 64),
        branch("s_cbranch_scc1", 1),
        s_mov_inline(1, 42),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Lane, &taken);
    assert_eq!(
        scalar(&registers, 1),
        0,
        "5 < 64 is true, so the block is skipped; registers were {registers:?}"
    );
}

/// A scalar compare is signed.
#[test]
fn a_scalar_compare_is_signed() {
    // Signed and unsigned agree on non-negative values; -1 separates them.
    if !device_or_skip("a_scalar_compare_is_signed") {
        return;
    }

    // s0 = -1 (inline constant code 193). -1 < 0 holds signed, and does not unsigned.
    let program = [
        s_mov_code(0, 193),
        s_cmp_i32("s_cmp_lt_i32", 0, 128),
        branch("s_cbranch_scc1", 1),
        s_mov_inline(1, 42),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        scalar(&registers, 1),
        0,
        concat!(
            "-1 < 0 holds as a signed comparison, so the branch is taken and the block is ",
            "skipped; as unsigned it would not be. Registers were {:?}"
        ),
        registers,
    );
}

/// A shader using only the condition code keeps the lane model.
#[test]
fn the_condition_code_is_not_a_lane_mask() {
    // The condition code is one bit for the whole wavefront, which the per-lane model
    // represents. Needs no device.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let program = [
        s_mov_inline(0, 5),
        s_cmp_i32("s_cmp_lt_i32", 0, 128 + 1),
        branch("s_cbranch_scc1", 1),
        s_mov_inline(1, 42),
        s_endpgm(),
    ];
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);

    let translated = translate(&decoded, &table, Strategy::default()).expect("translate");
    assert_eq!(
        translated.fidelity,
        Fidelity::Lane,
        "a shader using only the condition code needs no lane mask"
    );
}

/// The compiled `control` fixture, with scalar, mask and backward branches, translates.
#[test]
fn a_compiled_shader_with_control_flow_translates() {
    // Compiler output rather than a hand-written stream. Needs no device.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("orbistoun-shader")
        .join("tests")
        .join("fixtures")
        .join("control.gcn");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    // The whole buffer: the decoder stops at the padding after a compiled shader, and this
    // shader ends its wave twice, so stopping at the first terminator would halve it.
    let decoded = decode(&bytes, &table, &operands);
    assert!(decoded.is_trustworthy(), "the fixture must decode cleanly");
    assert!(
        decoded.terminated,
        "a compiled shader should reach its terminator"
    );

    let translated = translate(&decoded, &table, Strategy::default())
        .expect("a compiled shader with control flow should translate");

    // It reads and writes the execution mask, so `Auto` must choose the model that has one.
    assert_eq!(translated.fidelity, Fidelity::Wavefront);
    assert!(
        translated.instructions >= 8,
        "most of the shader should have been translated, got {}",
        translated.instructions
    );
}

/// `v_rcp_f32_e32 vDst, src`.
fn v_rcp(dst: u32, source_code: u32) -> u32 {
    head("v_rcp_f32_e32") | (dst << 17) | source_code
}

/// A short-form float VOP2: add, subtract, reverse-subtract or multiply.
fn v_op2(name: &str, dst: u32, first_code: u32, second_vgpr: u32) -> u32 {
    head(name) | (dst << 17) | (second_vgpr << 9) | first_code
}

/// A long-form vector ALU instruction.
///
/// Word 0 carries the opcode, the per-source absolute flags and the destination; word 1
/// carries the three sources and the per-source negate flags.
fn vop3(name: &str, dst: u32, sources: [u32; 3], abs: u32, neg: u32) -> [u32; 2] {
    [
        head(name) | (abs << 8) | dst,
        (neg << 29) | (sources[2] << 18) | (sources[1] << 9) | sources[0],
    ]
}

/// Inline float constant codes.
const F_1: u32 = 242;
const F_2: u32 = 244;
const F_4: u32 = 246;
const F_MINUS_2: u32 = 245;

/// Bit patterns the assertions below compare against.
const BITS_MINUS_1: u32 = 0xBF80_0000;
const BITS_1: u32 = 0x3F80_0000;
/// The bit pattern of 2.0f.
const BITS_2: u32 = 0x4000_0000;
const BITS_3: u32 = 0x4040_0000;
/// The bit pattern of 9.0f.
const BITS_9: u32 = 0x4110_0000;

/// `v_subrev_f32` reverses its operands.
#[test]
fn subtract_and_reverse_subtract_are_not_the_same() {
    // The name says so and the encoding does not; the two agree only on equal operands.
    if !device_or_skip("subtract_and_reverse_subtract_are_not_the_same") {
        return;
    }

    // v0 = 2.0; v_sub_f32 v1, 1.0, v0 gives 1.0 - 2.0 = -1.0.
    let forward = [
        v_mov_code(0, F_2),
        v_op2("v_sub_f32_e32", 1, F_1, 0),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Lane, &forward);
    assert_eq!(
        vector(&registers, 1),
        BITS_MINUS_1,
        "1.0 - 2.0 should be -1.0; registers were {registers:?}"
    );

    // The same operands through subrev: 2.0 - 1.0 = 1.0.
    let reversed = [
        v_mov_code(0, F_2),
        v_op2("v_subrev_f32_e32", 1, F_1, 0),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Lane, &reversed);
    assert_eq!(
        vector(&registers, 1),
        BITS_1,
        "subrev reverses, so this is 2.0 - 1.0 = 1.0; registers were {registers:?}"
    );
}

/// Source negate and absolute apply, absolute before negate.
#[test]
fn source_modifiers_apply_and_apply_in_order() {
    // A negate flag is one bit neither the operand layout nor the encoding table
    // describes. Three cases: negate alone, absolute alone, and both, where the order
    // decides the answer.
    if !device_or_skip("source_modifiers_apply_and_apply_in_order") {
        return;
    }

    // v0 = 2.0; v_add_f32_e64 v1, -v0, 1.0 gives -2.0 + 1.0 = -1.0. Without the negate
    // it would be 3.0.
    let mut program = vec![v_mov_code(0, F_2)];
    program.extend(vop3("v_add_f32_e64", 1, [vgpr_code(0), F_1, 0], 0, 0b001));
    program.push(s_endpgm());
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        vector(&registers, 1),
        BITS_MINUS_1,
        "the negate flag was dropped; registers were {registers:?}"
    );

    // v0 = -2.0; v_add_f32_e64 v1, |v0|, 1.0 gives 2.0 + 1.0 = 3.0. Without the absolute
    // it would be -1.0.
    let mut program = vec![v_mov_code(0, F_MINUS_2)];
    program.extend(vop3("v_add_f32_e64", 1, [vgpr_code(0), F_1, 0], 0b001, 0));
    program.push(s_endpgm());
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        vector(&registers, 1),
        BITS_3,
        "the absolute flag was dropped; registers were {registers:?}"
    );

    // v0 = -2.0; v_add_f32_e64 v1, -|v0|, 1.0.
    //
    // Absolute first, then negate: |-2.0| = 2.0, negated -2.0, plus 1.0 is -1.0. The other
    // order gives 3.0.
    let mut program = vec![v_mov_code(0, F_MINUS_2)];
    program.extend(vop3(
        "v_add_f32_e64",
        1,
        [vgpr_code(0), F_1, 0],
        0b001,
        0b001,
    ));
    program.push(s_endpgm());
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        vector(&registers, 1),
        BITS_MINUS_1,
        concat!(
            "absolute is applied before negate, so this is -1.0 and not 3.0; registers were ",
            "{:?}"
        ),
        registers
    );
}

/// The clamp flag and the output multiplier are refused by name at compute defaults.
#[test]
fn a_modifier_that_is_not_translated_is_refused() {
    // Both change the result; ignoring either computes something close to right. Needs no
    // device.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");

    for (word0_extra, word1_extra, expected) in [
        (1 << 15, 0, "clamps its result"),
        (0, 1 << 27, "output multiplier"),
    ] {
        let mut encoded = vop3("v_add_f32_e64", 1, [vgpr_code(0), F_1, 0], 0, 0);
        encoded[0] |= word0_extra;
        encoded[1] |= word1_extra;
        let program = [encoded[0], encoded[1], s_endpgm()];
        let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &table, &operands);

        let error = translate(&decoded, &table, Strategy::default())
            .expect_err("an untranslated modifier must be refused");
        assert!(
            error.to_string().contains(expected),
            "the error should name the modifier, got: {error}"
        );
    }
}

/// Runs a program through the wavefront model with a `DX10_CLAMP` mode given, as a stage's
/// `RSRC1` gives it.
fn run_with_dx10_clamp(dx10_clamp: bool, words: &[u32]) -> Vec<u32> {
    use orbistoun_translate::wavefront::{MeshPrimitive, Stage, UserData, Window};
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);
    let translated = orbistoun_translate::translate_with_user_data(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::default(),
        },
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            dx10_clamp: Some(dx10_clamp),
            ..UserData::default()
        },
    )
    .unwrap_or_else(|e| panic!("translate: {e}"));
    dispatch(&translated.module, OBSERVED, MEMORY_WORDS, [1, 1, 1])
        .expect("dispatch")
        .observed
}

/// The output clamp holds a float result to `[0, 1]`, with NaN handling set by `DX10_CLAMP`.
#[test]
fn the_output_clamp_holds_a_float_result_to_the_unit_range() {
    // `clamp(x, 0.0, 1.0)` folded into the instruction producing `x`. A NaN becomes zero
    // under DX10_CLAMP and passes through without it.
    const CLAMP: u32 = 1 << 15;
    if !device_or_skip("the_output_clamp_holds_a_float_result_to_the_unit_range") {
        return;
    }
    let clamped_add = |dst: u32, source: u32, addend: u32| {
        let mut encoded = vop3("v_add_f32_e64", dst, [vgpr_code(source), addend, 0], 0, 0);
        encoded[0] |= CLAMP;
        encoded
    };
    let mut program = vec![v_mov_code(0, F_2), v_mov_code(1, F_MINUS_2)];
    program.extend(v_mov_literal(2, 0xBF00_0000)); // -0.5
    program.extend(v_mov_literal(3, BITS_QUIET_NAN));
    program.extend(clamped_add(4, 0, F_1)); // 2 + 1 = 3 -> 1
    program.extend(clamped_add(5, 1, F_1)); // -2 + 1 = -1 -> 0
    program.extend(clamped_add(6, 2, F_1)); // -0.5 + 1 = 0.5 -> 0.5
    program.extend(clamped_add(7, 3, F_1)); // NaN + 1 = NaN
    program.push(s_endpgm());

    let with = run_with_dx10_clamp(true, &program);
    assert_eq!(vector(&with, 4), BITS_1, "above the range clamps to one");
    assert_eq!(vector(&with, 5), 0, "below the range clamps to zero");
    assert_eq!(
        vector(&with, 6),
        0x3F00_0000,
        "inside the range is untouched"
    );
    assert_eq!(vector(&with, 7), 0, "DX10_CLAMP turns a NaN into zero");

    let without = run_with_dx10_clamp(false, &program);
    assert_eq!(vector(&without, 4), BITS_1);
    let nan = vector(&without, 7);
    assert!(
        nan & 0x7F80_0000 == 0x7F80_0000 && nan & 0x007F_FFFF != 0,
        "without DX10_CLAMP a NaN passes through, got {nan:#x}"
    );
}

/// A clamp on a result it does not clamp, such as a select, is refused.
#[test]
fn a_clamp_on_a_result_it_does_not_clamp_is_refused() {
    // The clamp applies only to a 32-bit float arithmetic result; elsewhere it is refused
    // by name even with the stage's mode known.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    // v_cndmask_b32_e64 v1, v0, 1.0, vcc - vcc is scalar code 106.
    let mut encoded = vop3("v_cndmask_b32_e64", 1, [vgpr_code(0), F_1, 106], 0, 0);
    encoded[0] |= 1 << 15;
    let bytes: Vec<u8> = [encoded[0], encoded[1], s_endpgm()]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let decoded = decode(&bytes, &table, &operands);
    let error = orbistoun_translate::translate_with_user_data(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::default(),
        },
        (
            orbistoun_translate::wavefront::Stage::Compute,
            orbistoun_translate::wavefront::MeshPrimitive::default(),
        ),
        orbistoun_translate::wavefront::Window::default(),
        orbistoun_translate::wavefront::UserData {
            dx10_clamp: Some(true),
            ..orbistoun_translate::wavefront::UserData::default()
        },
    )
    .expect_err("a clamp on a select must be refused");
    assert!(error.to_string().contains("does not clamp"), "got: {error}");
}

/// `v_fma_f32` computes the product plus the addend.
#[test]
fn a_fused_multiply_add_computes_the_product_plus_the_addend() {
    if !device_or_skip("a_fused_multiply_add_computes_the_product_plus_the_addend") {
        return;
    }

    // v0 = 2.0, v1 = 4.0; v_fma_f32 v2, v0, v1, 1.0 gives 2*4 + 1 = 9.0.
    let mut program = vec![v_mov_code(0, F_2), v_mov_code(1, F_4)];
    program.extend(vop3(
        "v_fma_f32",
        2,
        [vgpr_code(0), vgpr_code(1), F_1],
        0,
        0,
    ));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        vector(&registers, 2),
        0x4110_0000,
        "2.0 * 4.0 + 1.0 should be 9.0; registers were {registers:?}"
    );
}

/// `v_rcp_f32` divides into one.
#[test]
fn a_reciprocal_divides_into_one() {
    if !device_or_skip("a_reciprocal_divides_into_one") {
        return;
    }

    let program = [v_mov_code(0, F_2), v_rcp(1, vgpr_code(0)), s_endpgm()];
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        vector(&registers, 1),
        0x3F00_0000,
        "1.0 / 2.0 should be 0.5; registers were {registers:?}"
    );
}

/// A conditional move picks the second source when the bit is set.
#[test]
fn a_conditional_move_picks_the_second_source_when_the_bit_is_set() {
    // Both directions are asserted: the reverse takes the wrong side of every ternary.
    if !device_or_skip("a_conditional_move_picks_the_second_source_when_the_bit_is_set") {
        return;
    }

    for (mask_code, expected) in [(129u32, 9u32), (128, 5)] {
        let mut program = vec![
            s_mov_b64(4, mask_code),
            v_mov_inline(1, 5),
            v_mov_inline(2, 9),
        ];
        program.extend(vop3(
            "v_cndmask_b32_e64",
            0,
            [vgpr_code(1), vgpr_code(2), 4],
            0,
            0,
        ));
        program.push(s_endpgm());

        let (registers, _) = run_memory(Fidelity::Wavefront, &program);
        assert_eq!(
            vector(&registers, 0),
            expected,
            "mask code {mask_code} should select {expected}; registers were {registers:?}"
        );
    }
}

/// A SOPK instruction: destination at bit 16, sixteen-bit immediate in the low half.
fn sopk(name: &str, dst: u32, immediate: i16) -> u32 {
    head(name) | (dst << 16) | u32::from(immediate as u16)
}

/// A SOP2 instruction: destination at bit 16, second source at bit 8, first in the low
/// byte.
fn sop2(name: &str, dst: u32, first_code: u32, second_code: u32) -> u32 {
    head(name) | (dst << 16) | (second_code << 8) | first_code
}

/// 64-bit logic writes the condition code: whether its result is non-zero.
#[test]
fn sixty_four_bit_logic_writes_the_condition_code() {
    // `s_and_b64 exec, exec, vcc` then a branch on the code is how a compiler skips a block
    // once no lane survives.
    if !device_or_skip("sixty_four_bit_logic_writes_the_condition_code") {
        return;
    }

    // Set the code with a true compare, then and two zero masks; if the and does not write
    // the code the branch is taken.
    //
    //   s_mov_b32 s0, 5
    //   s_cmp_lt_i32 s0, 64      ; true, so the code is set
    //   s_mov_b64 s[2:3], 0
    //   s_and_b64 s[4:5], s[2:3], s[2:3]   ; zero, so the code must be cleared
    //   s_cbranch_scc1 +1        ; must NOT be taken
    //   s_mov_b32 s1, 42
    //   s_endpgm
    let program = [
        s_mov_inline(0, 5),
        s_cmp_i32("s_cmp_lt_i32", 0, 128 + 64),
        s_mov_b64(2, 128),
        s_logic_b64("s_and_b64", 4, 2, 2),
        branch("s_cbranch_scc1", 1),
        s_mov_inline(1, 42),
        s_endpgm(),
    ];

    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        scalar(&registers, 1),
        42,
        concat!(
            "the and produced zero, so it must have cleared the condition code and the ",
            "branch must not have been taken; registers were {:?}"
        ),
        registers
    );
}

/// `s_movk_i32` sign-extends its immediate.
#[test]
fn a_compact_move_sign_extends_its_immediate() {
    // The decoder reports the field as encoded (the reference prints it unsigned), so the
    // sign extension happens in translation; -2 would otherwise be 65534.
    if !device_or_skip("a_compact_move_sign_extends_its_immediate") {
        return;
    }

    let program = [
        sopk("s_movk_i32", 0, -2),
        sopk("s_movk_i32", 1, 0x1234),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        scalar(&registers, 0),
        0xFFFF_FFFE,
        "-2 should sign-extend; registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 1),
        0x1234,
        "a positive immediate is unchanged; registers were {registers:?}"
    );
}

/// `s_addk_i32` and `s_mulk_i32` accumulate into their destination.
#[test]
fn the_accumulating_compact_forms_read_their_destination() {
    if !device_or_skip("the_accumulating_compact_forms_read_their_destination") {
        return;
    }

    let program = [
        s_mov_inline(0, 5),
        sopk("s_addk_i32", 0, 3),
        s_mov_inline(1, 6),
        sopk("s_mulk_i32", 1, 7),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        scalar(&registers, 0),
        8,
        "5 + 3 should be 8, not 3; registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 1),
        42,
        "6 * 7 should be 42, not 7; registers were {registers:?}"
    );
}

/// 32-bit scalar logic sets the condition code to whether its result is non-zero.
#[test]
fn scalar_logic_sets_the_condition_code_from_its_result() {
    // Two cases, so always-set and always-clear translations each fail one.
    if !device_or_skip("scalar_logic_sets_the_condition_code_from_its_result") {
        return;
    }

    for (first, second, expect_written) in [(128 + 1, 128, 0u32), (128 + 1, 128 + 1, 42)] {
        // s2 = first & second; then branch on the code, skipping the write when set.
        let program = [
            sop2("s_and_b32", 2, first, second),
            branch("s_cbranch_scc0", 1),
            s_mov_inline(3, 42),
            s_endpgm(),
        ];
        let (registers, _) = run_memory(Fidelity::Lane, &program);
        assert_eq!(
            scalar(&registers, 3),
            expect_written,
            concat!(
                "s_cbranch_scc0 is taken exactly when the and produced zero; registers were ",
                "{:?}"
            ),
            registers
        );
    }
}

/// Scalar addition sets the condition code on signed overflow, not on a non-zero result.
#[test]
fn scalar_addition_sets_the_condition_code_on_signed_overflow() {
    if !device_or_skip("scalar_addition_sets_the_condition_code_on_signed_overflow") {
        return;
    }

    // 5 + 3 does not overflow, so the code is clear and s_cbranch_scc0 is taken.
    let program = [
        sop2("s_add_i32", 0, 128 + 5, 128 + 3),
        branch("s_cbranch_scc0", 1),
        s_mov_inline(1, 42),
        s_endpgm(),
    ];
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        scalar(&registers, 0),
        8,
        "5 + 3 should be 8; registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 1),
        0,
        concat!(
            "no overflow means the code is clear and the branch is taken; registers were ",
            "{:?}"
        ),
        registers
    );
}

/// A long-form instruction of the sub-encoding with a scalar destination.
///
/// The scalar destination sits at bits 8 to 14 of the first word, the bits the other
/// sub-encoding uses for absolute-value flags.
fn vop3b(name: &str, vdst: u32, sdst: u32, sources: [u32; 3]) -> [u32; 2] {
    [
        head(name) | (sdst << 8) | vdst,
        (sources[2] << 18) | (sources[1] << 9) | sources[0],
    ]
}

/// The operand code for the condition mask.
const VCC_CODE: u32 = 106;

/// A carry out reaches the condition mask, set and clear.
#[test]
fn carry_out_reaches_the_condition_mask() {
    // 64-bit address arithmetic is built from these; a dropped carry breaks addresses
    // above four gigabytes.
    if !device_or_skip("carry_out_reaches_the_condition_mask") {
        return;
    }

    // v0 = 0xFFFFFFFF (inline -1), v1 = 1. The sum wraps to zero and carries.
    let mut program = vec![v_mov_code(0, 193), v_mov_inline(1, 1)];
    program.extend(vop3b(
        "v_add_co_u32",
        2,
        VCC_CODE,
        [vgpr_code(0), vgpr_code(1), 0],
    ));
    // Move the carry mask somewhere observable.
    program.push(s_mov_b64(0, VCC_CODE));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        0,
        "0xFFFFFFFF + 1 wraps to zero; registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 0),
        u32::MAX,
        concat!(
            "every lane carried, so every bit of the mask should be set; registers were ",
            "{:?}"
        ),
        registers
    );

    // 2 + 1 does not carry, and no lane's bit may be set.
    let mut program = vec![v_mov_inline(0, 2), v_mov_inline(1, 1)];
    program.extend(vop3b(
        "v_add_co_u32",
        2,
        VCC_CODE,
        [vgpr_code(0), vgpr_code(1), 0],
    ));
    program.push(s_mov_b64(0, VCC_CODE));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(vector(&registers, 2), 3, "registers were {registers:?}");
    assert_eq!(
        scalar(&registers, 0),
        0,
        "no lane carried; registers were {registers:?}"
    );
}

/// A scalar destination in bits 8 to 14 is not read as absolute-value flags.
#[test]
fn a_scalar_destination_is_not_read_as_absolute_value_flags() {
    // `vcc` is 106 (1101010), whose low bits would claim "the second source is absolute".
    // The second source here is negative, so a misread changes the answer.
    if !device_or_skip("a_scalar_destination_is_not_read_as_absolute_value_flags") {
        return;
    }

    // v0 = 2, v1 = 0xFFFFFFFF: the sum wraps to 1 and carries. Misread as an absolute flag,
    // the second source would be 0x7FFFFFFF and the sum 0x80000001 with no carry.
    let mut program = vec![v_mov_inline(0, 2), v_mov_code(1, 193)];
    program.extend(vop3b(
        "v_add_co_u32",
        2,
        VCC_CODE,
        [vgpr_code(0), vgpr_code(1), 0],
    ));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        1,
        concat!(
            "2 + 0xFFFFFFFF wraps to 1. Getting 0x80000001 means bits of the scalar ",
            "destination were read as absolute-value flags; registers were {:?}"
        ),
        registers
    );
}

/// `v_sub_co_u32` reports a borrow.
#[test]
fn subtraction_reports_a_borrow() {
    if !device_or_skip("subtraction_reports_a_borrow") {
        return;
    }

    // 1 - 2 borrows.
    let mut program = vec![v_mov_inline(0, 1), v_mov_inline(1, 2)];
    program.extend(vop3b(
        "v_sub_co_u32",
        2,
        VCC_CODE,
        [vgpr_code(0), vgpr_code(1), 0],
    ));
    program.push(s_mov_b64(0, VCC_CODE));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        0xFFFF_FFFF,
        "1 - 2 wraps; registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 0),
        u32::MAX,
        "every lane borrowed; registers were {registers:?}"
    );
}

/// The carry-in form adds the incoming carry.
#[test]
fn the_carry_in_form_adds_the_carry() {
    // The carry must come from adding the carry-in, not from the first addition.
    if !device_or_skip("the_carry_in_form_adds_the_carry") {
        return;
    }

    // Every lane has a carry in: 0xFFFFFFFF + 0 + 1 wraps to zero and carries.
    let mut program = vec![
        s_mov_exec(193),
        s_mov_b64(VCC_CODE, 193),
        v_mov_code(0, 193),
        v_mov_inline(1, 1),
    ];
    program.extend(vop3b(
        "v_add_co_ci_u32_e64",
        2,
        VCC_CODE,
        [vgpr_code(0), 128, VCC_CODE],
    ));
    program.push(s_mov_b64(0, VCC_CODE));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        0,
        "0xFFFFFFFF + 0 + 1 wraps to zero; registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 0),
        u32::MAX,
        concat!(
            "the carry came from adding the carry-in, not from the first addition; ",
            "registers were {:?}"
        ),
        registers
    );
}

/// All three steps of the division sequence are supported and none is blocked.
#[test]
fn the_division_sequence_is_translated_in_full() {
    // The reference states each step in full; what it leaves undefined is IEEE-754 or
    // unobservable. The pre-scale's subnormal tests need no subnormal support on the host
    // (see `exponent_is_zero`). Needs no device.
    use orbistoun_translate::model::{blocked, supports};

    for step in ["v_div_scale_f32", "v_div_fixup_f32", "v_div_fmas_f32"] {
        assert!(supports(step), "{step} should be translated");
        assert_eq!(
            blocked(step),
            None,
            "{step} is translated, so it must not also be listed as blocked"
        );
    }
}

/// The condition mask's low half with every lane set.
///
/// Every lane has the same operands and sets its own bit, so the mask is all ones.
const ALL_LANES: u32 = u32::MAX;

/// Exponent field `e`, mantissa zero: the float two to the power `e - 127`.
const fn power_of_two(exponent: u32) -> u32 {
    exponent << 23
}

/// `v_div_scale_f32 vDst, vcc, vS0, vDenominator, vNumerator`, run on lane zero.
///
/// Returns the scaled value and the condition mask: an unset flag would leave
/// `v_div_fmas_f32` never scaling.
fn run_scale(scaled: u32, denominator: u32, numerator: u32) -> (u32, u32) {
    let mut program = [
        v_mov_literal(1, scaled),
        v_mov_literal(2, denominator),
        v_mov_literal(3, numerator),
    ]
    .concat();
    program.extend(vop3b(
        "v_div_scale_f32",
        0,
        VCC_CODE,
        [vgpr_code(1), vgpr_code(2), vgpr_code(3)],
    ));
    program.push(s_mov_b64(0, VCC_CODE));
    program.push(s_endpgm());

    let registers = run_at(Fidelity::Wavefront, &program);
    (vector(&registers, 0), scalar(&registers, 0))
}

/// The division pre-scale leaves an ordinary division alone and unflagged.
#[test]
fn the_division_pre_scale_leaves_an_ordinary_division_alone() {
    if !device_or_skip("the_division_pre_scale_leaves_an_ordinary_division_alone") {
        return;
    }

    let two = power_of_two(128);
    let (value, flag) = run_scale(two, two, BITS_1);
    assert_eq!(
        value, two,
        "an ordinary denominator passes through unscaled"
    );
    assert_eq!(
        flag, 0,
        "and nothing is flagged for the multiply-add to undo"
    );
}

/// A flag written to `null` is not written, and the value still is.
///
/// `null` in the flag destination is the architecture's "nothing here", a shader asking
/// for less. The value arriving shows only the write was dropped, not the arithmetic.
#[test]
fn a_pre_scale_flag_written_nowhere_is_dropped_and_the_value_is_not() {
    if !device_or_skip("a_pre_scale_flag_written_nowhere_is_dropped_and_the_value_is_not") {
        return;
    }

    // The wide-spread branch sets the flag, so dropping the whole instruction would show as
    // an unscaled value.
    let wide = power_of_two(250);
    let mut program = [
        v_mov_literal(1, wide),
        v_mov_literal(2, BITS_1),
        v_mov_literal(3, wide),
    ]
    .concat();
    program.extend(vop3b(
        "v_div_scale_f32",
        0,
        FLAT_NO_BASE_CODE,
        [vgpr_code(1), vgpr_code(2), vgpr_code(3)],
    ));
    // The condition mask, read back untouched: the instruction named no flag destination.
    program.push(s_mov_b64(0, VCC_CODE));
    program.push(s_endpgm());

    let registers = run_at(Fidelity::Wavefront, &program);
    let (value, mask) = (vector(&registers, 0), scalar(&registers, 0));

    let (expected, flagged) = run_scale(wide, BITS_1, wide);
    assert_eq!(
        value, expected,
        "the scaled operand differs from the same instruction writing its flag to vcc"
    );
    assert_ne!(
        flagged, 0,
        "this fixture must be one that sets the flag, or the assertion below proves nothing"
    );
    assert_eq!(
        mask, 0,
        "the condition mask was written, and the instruction named no destination for it"
    );
}

/// The division pre-scale lifts a tiny numerator, with no flag.
#[test]
fn the_division_pre_scale_lifts_a_tiny_numerator() {
    // A numerator with exponent 23 or below is scaled up; the reciprocal is not what is
    // scaled here, so nothing needs undoing.
    if !device_or_skip("the_division_pre_scale_lifts_a_tiny_numerator") {
        return;
    }

    let tiny = power_of_two(17);
    let (value, flag) = run_scale(tiny, BITS_1, tiny);
    assert_eq!(
        value,
        power_of_two(81),
        "two to the minus 110 scaled by two to the 64 is two to the minus 46"
    );
    assert_eq!(flag, 0, "scaling the numerator alone needs no undoing");
}

/// The division pre-scale flags a wide spread and scales only the operand handed in.
#[test]
fn the_division_pre_scale_flags_a_wide_spread_and_scales_only_its_own_operand() {
    // The same condition scales the denominator and leaves the numerator alone.
    if !device_or_skip("the_division_pre_scale_flags_a_wide_spread_and_scales_only_its_own_operand")
    {
        return;
    }

    let big = power_of_two(227);
    let small = power_of_two(27);

    let (value, flag) = run_scale(small, small, big);
    assert_eq!(
        value,
        power_of_two(91),
        "handed the denominator, it scales it up"
    );
    assert_eq!(
        flag, ALL_LANES,
        "and flags that the quotient will need scaling back"
    );

    let (value, flag) = run_scale(big, small, big);
    assert_eq!(
        value, big,
        "handed the numerator, the same condition leaves it alone"
    );
    assert_eq!(flag, ALL_LANES, "while still flagging the quotient");
}

/// The division pre-scale answers a zero operand with a NaN and no flag.
#[test]
fn the_division_pre_scale_answers_a_zero_operand_with_a_nan() {
    // The fixup replaces the NaN afterwards.
    if !device_or_skip("the_division_pre_scale_answers_a_zero_operand_with_a_nan") {
        return;
    }

    let (value, flag) = run_scale(BITS_1, 0, BITS_1);
    assert_eq!(value, 0x7FC0_0000, "a zero denominator gives a NaN");
    assert_eq!(flag, 0, "and nothing to undo");

    let (value, _) = run_scale(BITS_1, BITS_1, 0);
    assert_eq!(value, 0x7FC0_0000, "so does a zero numerator");
}

// The division sequence: pure numerics, executed rather than only translated, since a
// special-case table can emit valid SPIR-V and wrong bits.

/// Bit patterns the fixup is specified in terms of.
const BITS_QUIET_NAN: u32 = 0xFFC0_0000;
const BITS_POSITIVE_INF: u32 = 0x7F80_0000;
const BITS_NEGATIVE_INF: u32 = 0xFF80_0000;
const BITS_NEGATIVE_ZERO: u32 = 0x8000_0000;
const BITS_SIGNALLING_NAN: u32 = 0x7F80_0001;

/// `v_div_fixup_f32 vDst, vQuotient, vDenominator, vNumerator`, run on lane zero.
///
/// Loads the operands as raw bit patterns, so a test can name an infinity or a NaN.
fn run_fixup(quotient: u32, denominator: u32, numerator: u32) -> u32 {
    let mut program = [
        v_mov_literal(1, quotient),
        v_mov_literal(2, denominator),
        v_mov_literal(3, numerator),
    ]
    .concat();
    program.extend(vop3(
        "v_div_fixup_f32",
        0,
        [vgpr_code(1), vgpr_code(2), vgpr_code(3)],
        0,
        0,
    ));
    program.push(s_endpgm());
    vector(&run(&program), 0)
}

/// `v_mov_b32_e32 vN, <literal>` - two words, the literal following the instruction.
fn v_mov_literal(dst: u32, value: u32) -> [u32; 2] {
    [head("v_mov_b32_e32") | (dst << 17) | LITERAL_CODE, value]
}

/// The source code that means "a literal follows".
const LITERAL_CODE: u32 = 255;

/// The division fixup replaces 0/0 and inf/inf with the specified negative quiet NaN.
#[test]
fn the_division_fixup_replaces_the_indeterminate_forms() {
    // The reference gives a literal bit pattern, sign included, so the whole word is
    // asserted.
    if !device_or_skip("the_division_fixup_replaces_the_indeterminate_forms") {
        return;
    }

    assert_eq!(run_fixup(0, 0, 0), BITS_QUIET_NAN, "0/0");
    assert_eq!(
        run_fixup(0, BITS_POSITIVE_INF, BITS_POSITIVE_INF),
        BITS_QUIET_NAN,
        "inf/inf"
    );
    assert_eq!(
        run_fixup(0, BITS_NEGATIVE_INF, BITS_POSITIVE_INF),
        BITS_QUIET_NAN,
        "-inf/inf is the same indeterminate form, and its sign does not change it"
    );
}

/// The division fixup gives zero and infinity the operands' signs.
#[test]
fn the_division_fixup_gives_zero_and_infinity_their_signs() {
    if !device_or_skip("the_division_fixup_gives_zero_and_infinity_their_signs") {
        return;
    }

    let one = BITS_1;
    let minus_one = BITS_1 | BITS_NEGATIVE_ZERO;

    assert_eq!(run_fixup(0, 0, one), BITS_POSITIVE_INF, "1/0");
    assert_eq!(run_fixup(0, 0, minus_one), BITS_NEGATIVE_INF, "-1/0");
    assert_eq!(
        run_fixup(0, BITS_NEGATIVE_ZERO, one),
        BITS_NEGATIVE_INF,
        "1/-0 - the denominator's sign bit is read even though its value is zero"
    );

    assert_eq!(run_fixup(0, one, 0), 0, "0/1");
    assert_eq!(run_fixup(0, minus_one, 0), BITS_NEGATIVE_ZERO, "0/-1");
    assert_eq!(
        run_fixup(0, BITS_POSITIVE_INF, one),
        0,
        "1/inf vanishes to a positive zero"
    );
}

/// The division fixup quietens a NaN, and the numerator's wins over the denominator's.
#[test]
fn the_division_fixup_propagates_a_nan_quietly() {
    if !device_or_skip("the_division_fixup_propagates_a_nan_quietly") {
        return;
    }

    let quiet = BITS_SIGNALLING_NAN | 0x0040_0000;
    assert_eq!(
        run_fixup(0, BITS_1, BITS_SIGNALLING_NAN),
        quiet,
        "a signalling NaN numerator is quietened"
    );
    assert_eq!(
        run_fixup(0, BITS_SIGNALLING_NAN, BITS_1),
        quiet,
        "a signalling NaN denominator is quietened"
    );

    let other_nan = 0x7FC0_1234;
    assert_eq!(
        run_fixup(0, other_nan, BITS_SIGNALLING_NAN),
        quiet,
        "with two NaNs the numerator's is the one that propagates"
    );
}

/// The division fixup keeps an ordinary quotient's magnitude and gives it the operands'
/// sign.
#[test]
fn the_division_fixup_leaves_an_ordinary_quotient_alone() {
    if !device_or_skip("the_division_fixup_leaves_an_ordinary_quotient_alone") {
        return;
    }

    let half = 0x3F00_0000;
    // -2.0: one binade above 1.0, with the sign bit set.
    let minus_two = (BITS_1 + (1 << 23)) | BITS_NEGATIVE_ZERO;

    assert_eq!(run_fixup(half, BITS_1, BITS_1), half, "an ordinary 1/2");
    assert_eq!(
        run_fixup(half, minus_two, BITS_1),
        half | BITS_NEGATIVE_ZERO,
        "one negative operand makes the result negative, whatever sign the quotient had"
    );
    assert_eq!(
        run_fixup(half | BITS_NEGATIVE_ZERO, BITS_1, BITS_1),
        half,
        "and a negative quotient with two positive operands comes back positive"
    );
}

/// `v_div_fmas_f32` scales by two to the thirty-second only where the condition mask is set.
#[test]
fn the_division_multiply_add_scales_only_when_the_mask_says_so() {
    // It reads `vcc` implicitly; the scale undoes the pre-scale earlier in the sequence.
    if !device_or_skip("the_division_multiply_add_scales_only_when_the_mask_says_so") {
        return;
    }

    // 2 * 1 + 1 = 3 is exact, so both answers are asserted to the bit. The mask is written
    // with `s_mov_b64`: `vcc` decodes as a named operand, and `s_mov_b32 vcc, ...` is
    // refused.
    let with_mask = |code: u32| {
        let mut program = vec![s_mov_b64(VCC_CODE, code)];
        program.extend(vop3("v_div_fmas_f32", 0, [F_2, F_1, F_1], 0, 0));
        program.push(s_endpgm());
        vector(&run_at(Fidelity::Wavefront, &program), 0)
    };

    assert_eq!(
        with_mask(INLINE_1),
        BITS_3_SCALED,
        "the mask bit is set for lane zero, so its result is scaled"
    );
    assert_eq!(
        with_mask(INLINE_0),
        BITS_3,
        "with the mask clear it is an ordinary multiply-add"
    );
}

/// Inline integer constant codes: the numbering starts at zero and counts up.
const INLINE_0: u32 = 128;
const INLINE_1: u32 = 129;

/// Three times two to the thirty-second: 1.5 * 2^33, so exponent 160 and the mantissa of
/// 1.5. Written out, so the test states its own expectation.
const BITS_3_SCALED: u32 = 0x5040_0000;

// Thirty-two-lane shaders. The width is chosen at compile time with identical encodings; a
// 32-lane shader's mask fits in one register, so it uses the 32-bit scalar forms. These
// programs are built as the reference describes, verifying the translator given such a
// shader.

/// Runs a program as a shader compiled for the given width.
fn run_at_width(width: Width, words: &[u32]) -> Vec<u32> {
    let table = encodings();
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, table, &operands);
    let translated = translate(
        &decoded,
        table,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width,
        },
    )
    .unwrap_or_else(|e| panic!("translate at {width}: {e}"));
    dispatch(&translated.module, OBSERVED, MEMORY_WORDS, [1, 1, 1])
        .expect("dispatch")
        .observed
}

/// The operand code for `exec_lo`, the low half of the execution mask.
const EXEC_LO_CODE: u32 = 126;

/// A 32-lane shader clears its mask with `s_mov_b32 exec_lo, 0`, and the write after is
/// suppressed.
#[test]
fn a_thirty_two_lane_shader_masks_with_the_thirty_two_bit_forms() {
    if !device_or_skip("a_thirty_two_lane_shader_masks_with_the_thirty_two_bit_forms") {
        return;
    }

    let program = [
        v_mov_inline(0, 7),
        s_mov_code(EXEC_LO_CODE, INLINE_0),
        v_mov_inline(0, 9),
        s_endpgm(),
    ];

    let registers = run_at_width(Width::Wave32, &program);
    assert_eq!(
        vector(&registers, 0),
        7,
        "the second move happens with the mask cleared, so it must not land"
    );
}

/// A 32-lane shader narrows its mask with `s_and_b32 exec_lo`.
#[test]
fn a_thirty_two_lane_shader_narrows_its_mask_with_scalar_logic() {
    // Keeping only lane zero lets the observed register update; an empty mask stops it.
    if !device_or_skip("a_thirty_two_lane_shader_narrows_its_mask_with_scalar_logic") {
        return;
    }

    let keep_lane_zero = [
        v_mov_inline(0, 1),
        s_mov_inline(2, 1),
        sop2("s_and_b32", EXEC_LO_CODE, EXEC_LO_CODE, 2),
        v_mov_inline(0, 5),
        s_endpgm(),
    ];
    let registers = run_at_width(Width::Wave32, &keep_lane_zero);
    assert_eq!(
        vector(&registers, 0),
        5,
        "lane zero is still in the mask, so the write lands"
    );

    let keep_nothing = [
        v_mov_inline(0, 1),
        s_mov_inline(2, 0),
        sop2("s_and_b32", EXEC_LO_CODE, EXEC_LO_CODE, 2),
        v_mov_inline(0, 5),
        s_endpgm(),
    ];
    let registers = run_at_width(Width::Wave32, &keep_nothing);
    assert_eq!(
        vector(&registers, 0),
        1,
        "an empty mask stops the write, which is the same logic one bit narrower"
    );
}

/// A shader with no mask traffic computes the same registers at either width.
#[test]
fn the_two_widths_are_the_same_shader_with_different_lane_counts() {
    // Width changes how many lanes run, not what lane zero computes.
    if !device_or_skip("the_two_widths_are_the_same_shader_with_different_lane_counts") {
        return;
    }

    let program = [
        v_mov_inline(0, 3),
        v_op2("v_add_nc_u32_e32", 1, vgpr_code(0), 0),
        s_endpgm(),
    ];

    assert_eq!(
        run_at_width(Width::Wave32, &program),
        run_at_width(Width::Wave64, &program),
        "a shader with no mask traffic computes the same thing at either width"
    );
}

// Subgroup fidelity.

/// Runs a program with the invocations of a subgroup as its lanes.
///
/// Returns `None` when the device's subgroup is not as wide as the guest wavefront, which
/// the module states it needs.
fn run_subgroup(width: Width, words: &[u32]) -> Option<Vec<u32>> {
    let table = encodings();
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, table, &operands);
    let translated = translate(
        &decoded,
        table,
        Strategy::Predicated {
            fidelity: Fidelity::Subgroup,
            width,
        },
    )
    .unwrap_or_else(|e| panic!("translate at subgroup fidelity: {e}"));

    let needed = translated
        .required_subgroup
        .expect("subgroup fidelity must say what width it needs");
    let Availability::Available { properties } = probe() else {
        return None;
    };
    if properties.subgroup_size != needed {
        println!(
            "!! subgroup fidelity needs a subgroup of {needed}, this device has {}",
            properties.subgroup_size
        );
        return None;
    }

    Some(
        dispatch(&translated.module, OBSERVED, MEMORY_WORDS, [1, 1, 1])
            .expect("dispatch")
            .observed,
    )
}

/// A subgroup-fidelity module loads and runs an unmasked write.
#[test]
fn subgroup_fidelity_produces_a_module_a_driver_accepts() {
    // Its capabilities, built-in input and group operation are rejected outright by a
    // driver if malformed.
    if !device_or_skip("subgroup_fidelity_produces_a_module_a_driver_accepts") {
        return;
    }

    let program = [v_mov_inline(0, 5), s_endpgm()];
    let Some(registers) = run_subgroup(Width::Wave32, &program) else {
        return;
    };
    assert_eq!(vector(&registers, 0), 5, "an unmasked write should land");
}

/// Subgroup fidelity honours a cleared mask through a ballot.
#[test]
fn subgroup_fidelity_masks_with_a_ballot() {
    // Ignoring the mask would return 9.
    if !device_or_skip("subgroup_fidelity_masks_with_a_ballot") {
        return;
    }

    let program = [
        v_mov_inline(0, 7),
        s_mov_code(EXEC_LO_CODE, INLINE_0),
        v_mov_inline(0, 9),
        s_endpgm(),
    ];
    let Some(registers) = run_subgroup(Width::Wave32, &program) else {
        return;
    };
    assert_eq!(
        vector(&registers, 0),
        7,
        "the mask was cleared, so the second write must not land"
    );
}

/// `v_fmac_f32` accumulates into its destination.
#[test]
fn the_accumulating_multiply_add_reads_its_own_destination() {
    // `D = S0 * S1 + D`: the one short-form instruction that reads the register it writes.
    if !device_or_skip("the_accumulating_multiply_add_reads_its_own_destination") {
        return;
    }

    // v1 = 3, then v1 = 2 * 4 + v1 = 11.
    let program = [
        v_mov_code(1, F_1),
        v_mov_code(2, F_2),
        v_mov_code(3, F_4),
        v_op2("v_fmac_f32_e32", 1, vgpr_code(2), 3),
        s_endpgm(),
    ];

    let registers = run(&program);
    assert_eq!(
        vector(&registers, 1),
        BITS_9,
        "2 * 4 + 1 is 9, and dropping the accumulation would give 8"
    );
}

// Untyped buffer access. A buffer access addresses memory through a resource constant in
// four scalar registers, built here with literal moves; the reference allows constants
// "generated within the shader".
// executing VM instructions, but these constants also can be generated within the shader."

/// `buffer_load_dword` / `buffer_store_dword` with the address modifiers in bits 12-13.
///
/// `vaddr` is a plain vector register number, not a code in the shared source numbering:
/// 256 in the eight-bit field spills into the data field next door.
fn mubuf(
    name: &str,
    data: u32,
    vaddr: u32,
    resource: u32,
    soffset: u32,
    modifiers: u32,
) -> [u32; 2] {
    [
        head(name) | modifiers,
        vaddr | (data << 8) | ((resource / 4) << 16) | (soffset << 24),
    ]
}

/// `offen`: take the byte offset from a vector register.
const OFFEN: u32 = 1 << 12;
/// `idxen`: take the record index from a vector register.
const IDXEN: u32 = 1 << 13;

/// Builds a raw buffer descriptor in `s[base..base+4]`.
///
/// Base address zero, no stride, `records` bytes, and out-of-bounds mode three - the raw
/// unswizzled mode, whose check is `offset + payload > records`.
fn describe_buffer(base: u32, records: u32) -> Vec<u32> {
    [
        s_mov_b32_literal(base, 0),
        s_mov_b32_literal(base + 1, 0),
        s_mov_b32_literal(base + 2, records),
        s_mov_b32_literal(base + 3, 3 << 24),
    ]
    .concat()
}

/// `s_mov_b32 sN, <literal>`.
fn s_mov_b32_literal(dst: u32, value: u32) -> [u32; 2] {
    [head("s_mov_b32") | (dst << 16) | LITERAL_CODE, value]
}

/// Translates without running, for the refusals that happen before any device is needed.
fn translate_program(
    fidelity: Fidelity,
    words: &[u32],
) -> Result<(), orbistoun_translate::TranslateError> {
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);
    translate(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity,
            width: Width::default(),
        },
    )
    .map(|_| ())
}

/// A typed buffer access. Same second word as an untyped one; the format goes in the
/// first, at bits 25:19.
fn mtbuf(
    name: &str,
    data: u32,
    vaddr: u32,
    resource: u32,
    soffset: u32,
    format: u32,
    modifiers: u32,
) -> [u32; 2] {
    [
        head(name) | modifiers | (format << 19),
        vaddr | (data << 8) | ((resource / 4) << 16) | (soffset << 24),
    ]
}

/// `BUF_FMT_32_FLOAT`, measured from the reference assembler (D097).
const FMT_32_FLOAT: u32 = 22;
/// `BUF_FMT_32_32_32_32_FLOAT`.
const FMT_32X4_FLOAT: u32 = 77;
/// `BUF_FMT_10_11_11_FLOAT`: three packed floats that are not IEEE halves.
const FMT_10_11_11_FLOAT: u32 = 36;

/// `BUF_FMT_8_8_8_8_UNORM`: four normalised bytes in one word. Loads translate; the store
/// is refused.
const FMT_8X4_UNORM_PACKED: u32 = 56;

/// A typed store to a packed format with unmeasured store rules is refused by name.
#[test]
fn a_typed_buffer_format_needing_conversion_is_refused_by_name() {
    // Packing a value down is not unpacking it backwards: a store chooses a rounding,
    // saturates, and decides the uncovered bits. The 10/11-bit float stores are measured
    // and translated; the other packed formats, such as `8_8_8_8_UNORM`, are refused. Needs
    // no device.
    let mut program = describe_buffer(4, 256);
    program.extend(mtbuf(
        "tbuffer_store_format_xyzw",
        2,
        0,
        4,
        INLINE_0,
        FMT_8X4_UNORM_PACKED,
        OFFEN,
    ));
    program.push(s_endpgm());

    let error = translate_program(Fidelity::Wavefront, &program)
        .expect_err("a format needing conversion must be refused");
    let text = error.to_string();
    assert!(
        text.contains("conversion"),
        concat!(
            "the refusal should say why, so a reader knows it is a gap rather than a bug: ",
            "{}"
        ),
        text
    );
}

/// The six words obSCEne loaded through `BUF_FMT_10_11_11_FLOAT` on hardware, and the three
/// floats each produced, as raw bit patterns (obSCEne's `typed-buffer-formats` check).
///
/// The words hit the edges (all zero, all ones, each field's lowest and highest bit, a
/// midpoint) where a packed float stops resembling an IEEE one.
const MEASURED_10_11_11: [(u32, [u32; 3]); 6] = [
    (0x0000_0000, [0x0000_0000, 0x0000_0000, 0x0000_0000]),
    (0xffff_ffff, [0x7ffe_0000, 0x7ffe_0000, 0x7ffc_0000]),
    (0x0020_0401, [0x4002_0000, 0x4000_0000, 0x0000_0000]),
    (0x8010_0200, [0x3c00_0000, 0x3c00_0000, 0x4000_0000]),
    (0x01e0_3c0f, [0x401e_0000, 0x400e_0000, 0x3760_0000]),
    (0x3c00_3c00, [0x4000_0000, 0x36e0_0000, 0x3bc0_0000]),
];

/// The same six words through `BUF_FMT_11_11_10_FLOAT`, from the same submission.
///
/// Here `x` is the ten-bit channel where in `10_11_11` it is an eleven-bit one, so the two
/// tables pin the width order from opposite directions.
const MEASURED_11_11_10: [(u32, [u32; 3]); 6] = [
    (0x0000_0000, [0x0000_0000, 0x0000_0000, 0x0000_0000]),
    (0xffff_ffff, [0x7ffc_0000, 0x7ffe_0000, 0x7ffe_0000]),
    (0x0040_0801, [0x3600_0000, 0x3600_0000, 0x3600_0000]),
    (0x8020_0400, [0x0000_0000, 0x3580_0000, 0x4002_0000]),
    (0x03c0_780f, [0x37f0_0000, 0x37f0_0000, 0x37f0_0000]),
    (0x3c00_3c00, [0x0000_0000, 0x3770_0000, 0x3bc0_0000]),
];

/// `BUF_FMT_11_11_10_FLOAT`, measured (code 43).
const FMT_11_11_10_FLOAT: u32 = 43;

/// `BUF_FMT_10_10_10_2_UINT`, measured (code 48).
const FMT_10_10_10_2_UINT: u32 = 48;

/// `BUF_FMT_2_10_10_10_UINT`, measured (code 54).
const FMT_2_10_10_10_UINT: u32 = 54;

/// `2_10_10_10` and `10_10_10_2` unpack the same word from opposite ends.
#[test]
fn the_two_ten_bit_families_unpack_from_opposite_ends() {
    // `widths` is listed highest bits first, so the last entry is the component at bit 0.
    // `UINT` is exact bit extraction, so the expected values follow from field positions.
    // The two formats are the same widths in opposite orders, so a mirrored walk swaps
    // their answers.
    const PACKED: u32 = 0xB55A_A923;

    if !device_or_skip("the_two_ten_bit_families_unpack_from_opposite_ends") {
        return;
    }

    for (format, expected) in [
        (FMT_2_10_10_10_UINT, [0x123u32, 0x2aa, 0x355, 0x2]),
        (FMT_10_10_10_2_UINT, [0x3u32, 0x248, 0x1aa, 0x2d5]),
    ] {
        let mut program = describe_buffer(4, 256);
        program.push(v_mov_inline(0, 0));
        program.extend(v_mov_literal(1, PACKED));
        program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
        program.extend(mtbuf(
            "tbuffer_load_format_xyzw",
            2,
            0,
            4,
            INLINE_0,
            format,
            OFFEN,
        ));
        program.push(s_endpgm());

        let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
        for (component, want) in expected.into_iter().enumerate() {
            assert_eq!(
                vector(&registers, 2 + component),
                want,
                concat!(
                    "format {} component {} should be its field of {:#010x}, {:#x} - a mirrored ",
                    "walk gives the other format's answer here"
                ),
                format,
                component,
                PACKED,
                want
            );
        }
    }
}

/// The packed 10/11-bit floats decode to the bits hardware produced.
#[test]
fn a_packed_float_narrower_than_a_half_decodes_to_its_measured_bits() {
    // Three floats with no sign bit and a five-bit exponent biased by 15. Asserted on bits:
    // the all-ones word gives NaN, and the Inf/NaN mantissa placement and subnormal scale
    // are exact bit facts an epsilon would hide.
    if !device_or_skip("a_packed_float_narrower_than_a_half_decodes_to_its_measured_bits") {
        return;
    }

    let cases = MEASURED_10_11_11
        .into_iter()
        .map(|(word, want)| (FMT_10_11_11_FLOAT, word, want))
        .chain(
            MEASURED_11_11_10
                .into_iter()
                .map(|(word, want)| (FMT_11_11_10_FLOAT, word, want)),
        );

    for (format, word, expected) in cases {
        let mut program = describe_buffer(4, 256);
        program.push(v_mov_inline(0, 0));
        program.extend(v_mov_literal(1, word));
        program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
        program.extend(mtbuf(
            "tbuffer_load_format_xyz",
            2,
            0,
            4,
            INLINE_0,
            format,
            OFFEN,
        ));
        program.push(s_endpgm());

        let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
        for (component, want) in expected.into_iter().enumerate() {
            let got = vector(&registers, 2 + component);
            // A NaN is compared structurally, everything else bit for bit. Vulkan does not
            // require a NaN payload to survive, and a host device may return `0x7fde0000`
            // where hardware gave `0x7ffe0000`; the measured table keeps the payload.
            let is_nan = |bits: u32| bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0;
            if is_nan(want) {
                assert!(
                    is_nan(got),
                    concat!(
                        "word {:#010x} component {} should decode to a NaN as the device ",
                        "produced ({:#010x}), got {:#010x}"
                    ),
                    word,
                    component,
                    want,
                    got
                );
                continue;
            }
            assert_eq!(
                got, want,
                concat!(
                    "word {:#010x} component {} should decode to the bits the device produced, ",
                    "{:#010x}"
                ),
                word, component, want
            );
        }
    }
}

/// Five floats stored through `BUF_FMT_10_11_11_FLOAT` and the packed word each produced.
///
/// Measured on hardware by obSCEne's `typed-buffer-formats` check. They pin the three rules a
/// store must get right: `(1, 2, 4)` is exact, `100000` saturates to the largest finite value
/// (not infinity), and `1.009375` (0.6 of a mantissa step above 1.0) truncates to `1.0` where
/// round-to-nearest would carry. The f32 bit patterns are the values obSCEne stored.
const MEASURED_10_11_11_STORE: [([u32; 3], u32); 3] = [
    ([0x3f80_0000, 0x4000_0000, 0x4080_0000], 0x8820_03c0), // (1.0, 2.0, 4.0), exact
    ([0x47c3_5000, 0x47c3_5000, 0x47c3_5000], 0xf7fd_ffbf), // (1e5 x3), saturate to max finite
    ([0x3f81_3333, 0x3f81_3333, 0x3f81_3333], 0x781e_03c0), // (1.009375 x3), truncate to 1.0
];

/// A packed 10/11-bit float store clamps to `[0, max finite]` and truncates, matching
/// hardware.
#[test]
fn a_packed_float_store_clamps_and_truncates_to_its_measured_bits() {
    // Each case stores three channels, then reads the packed word back with an untyped load,
    // so packing and memory are checked together.
    if !device_or_skip("a_packed_float_store_clamps_and_truncates_to_its_measured_bits") {
        return;
    }

    for (inputs, expected) in MEASURED_10_11_11_STORE {
        let mut program = describe_buffer(4, 256);
        program.push(v_mov_inline(0, 0));
        program.extend(v_mov_literal(2, inputs[0]));
        program.extend(v_mov_literal(3, inputs[1]));
        program.extend(v_mov_literal(4, inputs[2]));
        program.extend(mtbuf(
            "tbuffer_store_format_xyz",
            2,
            0,
            4,
            INLINE_0,
            FMT_10_11_11_FLOAT,
            OFFEN,
        ));
        program.extend(mubuf("buffer_load_dword", 5, 0, 4, INLINE_0, OFFEN));
        program.push(s_endpgm());

        let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
        let stored = vector(&registers, 5);
        assert_eq!(
            stored, expected,
            "storing {inputs:#010x?} through 10_11_11 should pack to {expected:#010x}, got {stored:#010x}"
        );
    }
}

/// A packed 10/11-bit float store translates to well-formed SPIR-V.
#[test]
fn a_packed_float_store_translates_to_well_formed_spirv() {
    // Needs no device; the builder validates its own identifiers. The values are checked
    // by the device test above.
    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    program.extend(v_mov_literal(2, 0x3f80_0000));
    program.extend(v_mov_literal(3, 0x4000_0000));
    program.extend(v_mov_literal(4, 0x4080_0000));
    program.extend(mtbuf(
        "tbuffer_store_format_xyz",
        2,
        0,
        4,
        INLINE_0,
        FMT_10_11_11_FLOAT,
        OFFEN,
    ));
    program.push(s_endpgm());

    translate_program(Fidelity::Wavefront, &program)
        .expect("a packed 10/11-bit float store now translates");
}

/// A typed access whose format's component count disagrees with its channels is refused.
#[test]
fn a_typed_buffer_access_whose_format_disagrees_with_its_channels_is_refused() {
    // What the hardware does then (padding or discarding channels) is unmeasured.
    let mut program = describe_buffer(4, 256);
    program.extend(mtbuf(
        "tbuffer_load_format_x",
        2,
        0,
        4,
        INLINE_0,
        FMT_32X4_FLOAT,
        OFFEN,
    ));
    program.push(s_endpgm());

    let error = translate_program(Fidelity::Wavefront, &program)
        .expect_err("a format naming four components for a one-channel access is refused");
    assert!(error.to_string().contains("disagree"), "{error}");
}

/// A reserved format code is refused rather than approximated.
#[test]
fn a_reserved_format_code_is_refused_rather_than_approximated() {
    // Code 90 has no name in the reference; the nearest real format would render.
    let mut program = describe_buffer(4, 256);
    program.extend(mtbuf("tbuffer_load_format_x", 2, 0, 4, INLINE_0, 90, OFFEN));
    program.push(s_endpgm());

    let error = translate_program(Fidelity::Wavefront, &program)
        .expect_err("a reserved format code must be refused");
    assert!(error.to_string().contains("no meaning"), "{error}");
}

/// A four-channel typed access moves four consecutive words into consecutive registers.
#[test]
fn a_four_channel_typed_access_moves_four_consecutive_words() {
    // Stored from v1..v4 and read back into v5..v8.
    if !device_or_skip("a_four_channel_typed_access_moves_four_consecutive_words") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    for channel in 0..4u32 {
        program.push(v_mov_inline(1 + channel, 1 + channel));
    }
    program.extend(mtbuf(
        "tbuffer_store_format_xyzw",
        1,
        0,
        4,
        INLINE_0,
        FMT_32X4_FLOAT,
        OFFEN,
    ));
    program.extend(mtbuf(
        "tbuffer_load_format_xyzw",
        4,
        0,
        4,
        INLINE_0,
        FMT_32X4_FLOAT,
        OFFEN,
    ));
    program.push(s_endpgm());

    // The load's destination overlaps the store's last source, since only v0 to v7 are
    // copied out. The store runs first, and the comparison is against memory, which a
    // translation writing every channel to one address fails.
    let (registers, memory) = run_memory(Fidelity::Wavefront, &program);
    let stored: Vec<u32> = memory[..4].to_vec();
    assert_eq!(
        stored
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
        "four channels must land in four different words, got {stored:?}"
    );
    for (channel, word) in memory[..4].iter().enumerate() {
        assert_eq!(
            vector(&registers, 4 + channel),
            *word,
            "channel {channel} should read back the word it was stored to"
        );
    }
}

/// `buffer_store_dwordx4` and `buffer_load_dwordx4` move four distinct consecutive words.
#[test]
fn a_four_word_untyped_access_moves_four_consecutive_words() {
    if !device_or_skip("a_four_word_untyped_access_moves_four_consecutive_words") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    for word in 0..4u32 {
        program.extend(v_mov_literal(1 + word, 0x1111_1111 * (word + 1)));
    }
    program.extend(mubuf("buffer_store_dwordx4", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mubuf("buffer_load_dwordx4", 4, 0, 4, INLINE_0, OFFEN));
    program.push(s_endpgm());

    // As with the typed test, the comparison is against memory, so collapsing the four
    // words onto one address fails.
    let (registers, memory) = run_memory(Fidelity::Wavefront, &program);
    let stored: Vec<u32> = memory[..4].to_vec();
    assert_eq!(
        stored
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
        "four words must land in four different addresses, got {stored:?}"
    );
    for (word, value) in memory[..4].iter().enumerate() {
        assert_eq!(
            vector(&registers, 4 + word),
            *value,
            "word {word} should read back the value stored to its address"
        );
    }
}

/// `BUF_FMT_8_8_8_8_UINT` extracts each byte into its own register, first component low.
#[test]
fn a_packed_uint_load_extracts_each_component_from_one_word() {
    // An exact bit field per component: a shift and a mask. A raw word is stored untyped,
    // then read back typed.
    /// Distinct bytes so a wrong shift or mask cannot pass: `x=1, y=2, z=3, w=4`.
    const PACKED: u32 = 0x0403_0201;
    /// `BUF_FMT_8_8_8_8_UINT`, measured (code 60).
    const FMT_8X4_UINT: u32 = 60;

    if !device_or_skip("a_packed_uint_load_extracts_each_component_from_one_word") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    program.extend(v_mov_literal(1, PACKED));
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mtbuf(
        "tbuffer_load_format_xyzw",
        2,
        0,
        4,
        INLINE_0,
        FMT_8X4_UINT,
        OFFEN,
    ));
    program.push(s_endpgm());

    let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
    for (component, expected) in [1u32, 2, 3, 4].into_iter().enumerate() {
        assert_eq!(
            vector(&registers, 2 + component),
            expected,
            "component {component} should be byte {component} of {PACKED:#010x}, low byte first"
        );
    }
}

/// `BUF_FMT_8_8_8_8_SINT` sign-extends each component.
#[test]
fn a_packed_sint_load_sign_extends_each_component() {
    // The bytes 0x80, 0x01, 0x7f, 0x81 are -128, 1, 127, -127; a logical shift would leave
    // 0x80 and 0x81 unextended.
    /// Distinct signed bytes, mixing both signs: `x=-128, y=1, z=127, w=-127`.
    const PACKED: u32 = 0x817F_0180;
    /// `BUF_FMT_8_8_8_8_SINT`, measured (code 61).
    const FMT_8X4_SINT: u32 = 61;

    if !device_or_skip("a_packed_sint_load_sign_extends_each_component") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    program.extend(v_mov_literal(1, PACKED));
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mtbuf(
        "tbuffer_load_format_xyzw",
        2,
        0,
        4,
        INLINE_0,
        FMT_8X4_SINT,
        OFFEN,
    ));
    program.push(s_endpgm());

    let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
    let expected = [(-128i32) as u32, 1, 127, (-127i32) as u32];
    for (component, want) in expected.into_iter().enumerate() {
        assert_eq!(
            vector(&registers, 2 + component),
            want,
            "component {component} should be its byte sign-extended to a full word"
        );
    }
}

/// `BUF_FMT_8_8_8_8_UNORM` normalises each byte to `byte / 255`.
#[test]
fn a_packed_unorm_load_normalises_each_component() {
    // Only 0 and 255 give exactly representable results, so the bytes are 0x00, 0xff,
    // 0xff, 0x00, asserted as float bits. A bitcast or a missing divide lands nowhere near
    // 1.0.
    /// Bytes `x=0, y=255, z=255, w=0`, low byte first.
    const PACKED: u32 = 0x00FF_FF00;
    /// `BUF_FMT_8_8_8_8_UNORM`, measured (code 56).
    const FMT_8X4_UNORM: u32 = 56;

    if !device_or_skip("a_packed_unorm_load_normalises_each_component") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    program.extend(v_mov_literal(1, PACKED));
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mtbuf(
        "tbuffer_load_format_xyzw",
        2,
        0,
        4,
        INLINE_0,
        FMT_8X4_UNORM,
        OFFEN,
    ));
    program.push(s_endpgm());

    let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
    let expected = [0.0f32, 1.0, 1.0, 0.0];
    for (component, want) in expected.into_iter().enumerate() {
        assert_eq!(
            vector(&registers, 2 + component),
            want.to_bits(),
            "component {component} should be byte/255 as float bits ({want})"
        );
    }
}

/// `BUF_FMT_8_8_8_8_SNORM` normalises to `byte / 127`, clamped at -1.0.
#[test]
fn a_packed_snorm_load_normalises_each_component() {
    // -128/127 is a hair past -1.0, which the reference clamps. The bytes 127, 0, -128, -127
    // give exactly 1.0, 0.0, -1.0, -1.0; a missing clamp fails the third.
    const PACKED: u32 = 0x8180_007F;
    /// `BUF_FMT_8_8_8_8_SNORM`, measured (code 57).
    const FMT_8X4_SNORM: u32 = 57;

    if !device_or_skip("a_packed_snorm_load_normalises_each_component") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    program.extend(v_mov_literal(1, PACKED));
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mtbuf(
        "tbuffer_load_format_xyzw",
        2,
        0,
        4,
        INLINE_0,
        FMT_8X4_SNORM,
        OFFEN,
    ));
    program.push(s_endpgm());

    let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
    let expected = [1.0f32, 0.0, -1.0, -1.0];
    for (component, want) in expected.into_iter().enumerate() {
        assert_eq!(
            vector(&registers, 2 + component),
            want.to_bits(),
            "component {component} should be byte/127 clamped to >= -1.0 as float bits ({want})"
        );
    }
}

/// `BUF_FMT_16_16_FLOAT` widens each half to a single-precision float.
#[test]
fn a_packed_float16_load_widens_each_half() {
    // 0x3C00 is 1.0 and 0xC000 is -2.0, exact in both widths, packed low half first.
    const PACKED: u32 = 0xC000_3C00;
    /// `BUF_FMT_16_16_FLOAT`, measured (code 29).
    const FMT_16X2_FLOAT: u32 = 29;

    if !device_or_skip("a_packed_float16_load_widens_each_half") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    program.extend(v_mov_literal(1, PACKED));
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mtbuf(
        "tbuffer_load_format_xy",
        2,
        0,
        4,
        INLINE_0,
        FMT_16X2_FLOAT,
        OFFEN,
    ));
    program.push(s_endpgm());

    let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
    let expected = [1.0f32, -2.0];
    for (component, want) in expected.into_iter().enumerate() {
        assert_eq!(
            vector(&registers, 2 + component),
            want.to_bits(),
            "component {component} should be its half widened to a float ({want})"
        );
    }
}

/// `BUF_FMT_8_8_8_8_USCALED` converts each byte to a float without dividing.
#[test]
fn a_packed_uscaled_load_converts_without_scaling() {
    // 0, 1, 128, 255 give 0.0, 1.0, 128.0, 255.0; a kept divide fails the second.
    /// Bytes `x=0, y=1, z=128, w=255`, low byte first.
    const PACKED: u32 = 0xFF80_0100;
    /// `BUF_FMT_8_8_8_8_USCALED`, measured (code 58).
    const FMT_8X4_USCALED: u32 = 58;

    if !device_or_skip("a_packed_uscaled_load_converts_without_scaling") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    program.extend(v_mov_literal(1, PACKED));
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mtbuf(
        "tbuffer_load_format_xyzw",
        2,
        0,
        4,
        INLINE_0,
        FMT_8X4_USCALED,
        OFFEN,
    ));
    program.push(s_endpgm());

    let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
    let expected = [0.0f32, 1.0, 128.0, 255.0];
    for (component, want) in expected.into_iter().enumerate() {
        assert_eq!(
            vector(&registers, 2 + component),
            want.to_bits(),
            "component {component} should be its byte as a float, unscaled ({want})"
        );
    }
}

/// `BUF_FMT_8_8_8_8_SSCALED` keeps sign and magnitude, with no clamp.
#[test]
fn a_packed_sscaled_load_keeps_the_sign_and_the_magnitude() {
    // Without sign extension -128 reads as 128.0; with the SNORM divide it reads as -1.0.
    // An unscaled value promises no range, so nothing clamps.
    /// Bytes `x=-128, y=-1, z=1, w=127`, low byte first.
    const PACKED: u32 = 0x7F01_FF80;
    /// `BUF_FMT_8_8_8_8_SSCALED`, measured (code 59).
    const FMT_8X4_SSCALED: u32 = 59;

    if !device_or_skip("a_packed_sscaled_load_keeps_the_sign_and_the_magnitude") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    program.extend(v_mov_literal(1, PACKED));
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mtbuf(
        "tbuffer_load_format_xyzw",
        2,
        0,
        4,
        INLINE_0,
        FMT_8X4_SSCALED,
        OFFEN,
    ));
    program.push(s_endpgm());

    let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
    let expected = [-128.0f32, -1.0, 1.0, 127.0];
    for (component, want) in expected.into_iter().enumerate() {
        assert_eq!(
            vector(&registers, 2 + component),
            want.to_bits(),
            "component {component} should be its signed byte as a float, unscaled ({want})"
        );
    }
}

/// A packed element spanning two words takes each component from the right word.
#[test]
fn a_packed_load_spanning_two_words_takes_each_component_from_the_right_one() {
    // `BUF_FMT_16_16_16_16_UINT`: components 0 and 1 come from the low word and 2 and 3
    // from the high one, since the element is a little-endian byte sequence. Reading both
    // from word zero gives 1, 2, 1, 2; swapping words gives 3, 4, 1, 2.
    /// `x=1, y=2` packed low half first.
    const LOW: u32 = 0x0002_0001;
    /// `z=3, w=4`.
    const HIGH: u32 = 0x0004_0003;
    /// `BUF_FMT_16_16_16_16_UINT`, measured (code 69).
    const FMT_16X4_UINT: u32 = 69;

    if !device_or_skip("a_packed_load_spanning_two_words_takes_each_component_from_the_right_one") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.push(v_mov_inline(0, 0));
    program.extend(v_mov_literal(1, LOW));
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    // The second word goes four bytes along, by moving the address.
    program.push(v_mov_inline(0, 4));
    program.extend(v_mov_literal(1, HIGH));
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.push(v_mov_inline(0, 0));
    program.extend(mtbuf(
        "tbuffer_load_format_xyzw",
        2,
        0,
        4,
        INLINE_0,
        FMT_16X4_UINT,
        OFFEN,
    ));
    program.push(s_endpgm());

    let (registers, _memory) = run_memory(Fidelity::Wavefront, &program);
    for (component, expected) in [1u32, 2, 3, 4].into_iter().enumerate() {
        assert_eq!(
            vector(&registers, 2 + component),
            expected,
            "component {component} should be half {component} of the two-word element"
        );
    }
}

/// Whether a module's header declares a capability, by scanning its words.
///
/// `OpCapability` is opcode 17 with a word count of two and appears only in the header, so
/// the scan is exact.
fn declares_capability(module: &[u32], capability: u32) -> bool {
    let mut index = 5; // past the five-word module header
    while index + 1 < module.len() {
        let word_count = (module[index] >> 16) as usize;
        let opcode = module[index] & 0xFFFF;
        if word_count == 0 {
            break;
        }
        if opcode == 17 && module[index + 1] == capability {
            return true;
        }
        index += word_count;
    }
    false
}

/// A module declares the sixteen-bit capabilities only if it uses them.
///
/// Declaring them unconditionally asks every device for two features; omitting them where
/// a half is read makes the module invalid. Both directions are asserted. Needs no device.
#[test]
fn the_sixteen_bit_capabilities_are_declared_only_where_used() {
    /// `Float16` and `Int16`, from the SPIR-V capability enumeration.
    const FLOAT16: u32 = 9;
    const INT16: u32 = 22;
    /// `BUF_FMT_16_16_FLOAT`, the format whose channels are halves (code 29, measured).
    const FMT_16X2_FLOAT: u32 = 29;

    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Wavefront,
        width: Width::Wave64,
    };
    let translate_words = |words: &[u32]| {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &table, &operands);
        translate(&decoded, &table, strategy)
            .expect("the program translates")
            .module
    };

    // Nothing sixteen-bit.
    let plain = translate_words(&[v_mov_inline(1, 7), s_endpgm()]);
    assert!(
        !declares_capability(&plain, FLOAT16),
        "a module with no half in it declares Float16, so every device it runs on is asked for 
         a feature it does not need"
    );
    assert!(!declares_capability(&plain, INT16), "likewise Int16");

    // A typed load of a half-format channel, the one path that needs both.
    let mut program = describe_buffer(4, 256);
    program.extend(mtbuf(
        "tbuffer_load_format_xy",
        2,
        0,
        4,
        INLINE_0,
        FMT_16X2_FLOAT,
        OFFEN,
    ));
    program.push(s_endpgm());
    let halves = translate_words(&program);
    assert!(
        declares_capability(&halves, FLOAT16),
        "a module that reads a half must declare Float16, or it is invalid on every device"
    );
    assert!(
        declares_capability(&halves, INT16),
        "the half is narrowed through a sixteen-bit integer, so Int16 is needed too"
    );
}

/// A one-channel typed access with a plain word format matches the untyped access.
#[test]
fn a_single_channel_typed_access_is_an_untyped_one_with_a_format() {
    // Divergence would mean the addressing is duplicated.
    if !device_or_skip("a_single_channel_typed_access_is_an_untyped_one_with_a_format") {
        return;
    }

    let mut typed = describe_buffer(4, 256);
    typed.push(v_mov_inline(0, 8));
    typed.push(v_mov_code(1, F_2));
    typed.extend(mtbuf(
        "tbuffer_store_format_x",
        1,
        0,
        4,
        INLINE_0,
        FMT_32_FLOAT,
        OFFEN,
    ));
    typed.push(s_endpgm());

    let mut untyped = describe_buffer(4, 256);
    untyped.push(v_mov_inline(0, 8));
    untyped.push(v_mov_code(1, F_2));
    untyped.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    untyped.push(s_endpgm());

    let (_, typed_memory) = run_memory(Fidelity::Wavefront, &typed);
    let (_, untyped_memory) = run_memory(Fidelity::Wavefront, &untyped);
    assert_eq!(
        typed_memory, untyped_memory,
        "a one-channel typed store and an untyped store must be the same operation"
    );
}

/// A buffer store and load round-trip through guest memory via the `voffset` term.
#[test]
fn a_buffer_store_and_load_round_trip_through_guest_memory() {
    // Addressing is base + soffset + inst_offset + voffset.
    if !device_or_skip("a_buffer_store_and_load_round_trip_through_guest_memory") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    program.extend([v_mov_inline(0, 8), v_mov_code(1, F_2)]);
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mubuf("buffer_load_dword", 2, 0, 4, INLINE_0, OFFEN));
    program.push(s_endpgm());

    let (registers, memory) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        BITS_2,
        "what was stored through the buffer should read back through it"
    );
    assert_eq!(
        memory[2], BITS_2,
        "and it should land at byte 8, which is word two"
    );
}

/// A buffer access past the record count reads zero and drops its write.
#[test]
fn a_buffer_access_past_the_record_count_reads_zero_and_drops_its_write() {
    // The reference: out of range, "writes are ignored (dropped) and reads return zero".
    // Both halves are checked; a live read after a dropped write returns stale data.
    if !device_or_skip("a_buffer_access_past_the_record_count_reads_zero_and_drops_its_write") {
        return;
    }

    // A buffer of eight bytes, accessed at byte sixteen.
    let mut program = describe_buffer(4, 8);
    program.extend([v_mov_inline(0, 16), v_mov_code(1, F_2), v_mov_inline(3, 7)]);
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mubuf("buffer_load_dword", 3, 0, 4, INLINE_0, OFFEN));
    program.push(s_endpgm());

    let (registers, memory) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 3),
        0,
        "a read past the end returns zero, not the register's previous contents"
    );
    assert_eq!(memory[4], 0, "and the write that went with it never landed");
}

/// The last buffer access inside the record count is not suppressed.
#[test]
fn a_buffer_access_inside_the_record_count_is_not_suppressed() {
    // Eight bytes accessed at byte four, which an inverted or always-true bounds test
    // would suppress.
    if !device_or_skip("a_buffer_access_inside_the_record_count_is_not_suppressed") {
        return;
    }

    let mut program = describe_buffer(4, 8);
    program.extend([v_mov_inline(0, 4), v_mov_code(1, F_2)]);
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.push(s_endpgm());

    let (_, memory) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        memory[1], BITS_2,
        "the last access that fits must not be suppressed"
    );
}

/// A swizzled descriptor reads zero and drops its write.
#[test]
fn a_swizzled_descriptor_is_refused_by_reading_zero() {
    // A translated shader cannot refuse at run time, so unsupported addressing is forced
    // out of bounds, reading zero rather than data from the wrong offset.
    if !device_or_skip("a_swizzled_descriptor_is_refused_by_reading_zero") {
        return;
    }

    let mut program = describe_buffer(4, 256);
    // Swizzle enable is bit 63 of the descriptor: the top bit of its second word.
    program.extend(s_mov_b32_literal(5, 1 << 31));
    program.extend([v_mov_inline(0, 8), v_mov_code(1, F_2), v_mov_inline(2, 9)]);
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, OFFEN));
    program.extend(mubuf("buffer_load_dword", 2, 0, 4, INLINE_0, OFFEN));
    program.push(s_endpgm());

    let (registers, memory) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(vector(&registers, 2), 0, "a swizzled read answers zero");
    assert_eq!(memory[2], 0, "and a swizzled write does not land");
}

/// An indexed buffer access multiplies the index by the descriptor's stride.
#[test]
fn an_indexed_buffer_access_multiplies_the_index_by_the_stride() {
    // `Stride * Vindex`, with the stride from the descriptor; ignoring it would put every
    // record at offset zero.
    if !device_or_skip("an_indexed_buffer_access_multiplies_the_index_by_the_stride") {
        return;
    }

    // Sixteen-byte records, four of them, bounds mode one (`index >= records`). Record two
    // starts at byte 32, word eight.
    let mut program = [
        s_mov_b32_literal(4, 0),
        s_mov_b32_literal(5, 16 << 16),
        s_mov_b32_literal(6, 4),
        s_mov_b32_literal(7, 1 << 24),
    ]
    .concat();
    program.extend([v_mov_inline(0, 2), v_mov_code(1, F_2)]);
    program.extend(mubuf("buffer_store_dword", 1, 0, 4, INLINE_0, IDXEN));
    program.push(s_endpgm());

    let (_, memory) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        memory[8], BITS_2,
        "record two of a sixteen-byte stride starts at byte 32"
    );
    assert_eq!(memory[0], 0, "and not at the start of the buffer");
}

/// A store past the memory window lands nowhere, rather than wrapping onto the start.
#[test]
fn a_store_past_the_memory_window_does_not_wrap_onto_the_start() {
    // The window index is masked to stay legal, and masking is not clamping; an overrun
    // wrapped onto the start would make the guest's bug unrecognisable (D101).
    if !device_or_skip("a_store_past_the_memory_window_does_not_wrap_onto_the_start") {
        return;
    }

    // MEMORY_WORDS words of window, so the first byte past it is MEMORY_WORDS * 4.
    let past = MEMORY_WORDS as u32 * 4;
    let program = [
        v_mov_literal(0, past).to_vec(),
        vec![v_mov_code(1, F_2)],
        global_store(0, 1).to_vec(),
        vec![s_endpgm()],
    ]
    .concat();

    let (_, memory) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        memory[0], 0,
        "a store one word past the end must not appear at the start"
    );
    assert!(
        memory.iter().all(|word| *word == 0),
        "and must not appear anywhere: {memory:?}"
    );
}

/// A load past the memory window reads zero rather than aliasing onto the start.
#[test]
fn a_load_past_the_memory_window_reads_zero() {
    if !device_or_skip("a_load_past_the_memory_window_reads_zero") {
        return;
    }

    let past = MEMORY_WORDS as u32 * 4;
    let program = [
        vec![v_mov_code(1, F_2)],
        global_store(2, 1).to_vec(),
        v_mov_literal(0, past).to_vec(),
        global_load(3, 0).to_vec(),
        vec![s_endpgm()],
    ]
    .concat();

    let registers = run_at(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 3),
        0,
        "a read past the end answers zero rather than aliasing onto word zero"
    );
}

/// The long-form sub-encoding is derived from the solved operand layouts.
#[test]
fn the_sub_encoding_is_derived_from_the_solved_operands_not_a_list() {
    // Bits 8-14 of the first word hold a second scalar destination in one sub-encoding and
    // per-source absolute flags in the other. The classification comes from the probe
    // data, so this pins that the data still records the scalar destination. Needs no
    // device.
    let table = encodings();
    let field = |name: &str| {
        let (family, opcode) = table
            .find_by_name(name)
            .unwrap_or_else(|| panic!("this target has no instruction named {name}"));
        table
            .operands_for(family, opcode)
            .unwrap_or_else(|| panic!("{name} has no solved operand layout"))
            .iter()
            .any(|slot| slot.word == 0 && slot.shift == 8)
    };

    for carries in ["v_add_co_u32", "v_sub_co_u32", "v_div_scale_f32"] {
        assert!(
            field(carries),
            "{carries} writes a scalar destination, so its layout must record one"
        );
    }
    for plain in ["v_add_f32_e64", "v_mul_f32_e64", "v_fma_f32"] {
        assert!(
            !field(plain),
            "{plain} has no scalar destination - those bits are its modifier flags"
        );
    }
}

/// Instructions compilers place between a condition-code write and its read do not write
/// the condition code here either.
#[test]
fn the_corpus_agrees_about_hidden_side_effects() {
    // Whether an instruction writes the condition code is invisible in the encoding and
    // operand layout. A compiler only places instructions it believes do not write it
    // between a setter and a branch on it, so the fixtures are mined for those windows.
    // Needs no device.
    use orbistoun_translate::model::{reads_condition_code, writes_condition_code};

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("orbistoun-shader")
        .join("tests")
        .join("fixtures");

    let mut windows = 0usize;
    let mut instructions_cleared = std::collections::BTreeSet::new();

    for entry in std::fs::read_dir(&dir).expect("fixtures") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("fixture");
        let names: Vec<String> = text
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .filter_map(|line| line.split_whitespace().nth(2).map(str::to_owned))
            .collect();

        for (at, name) in names.iter().enumerate() {
            if !reads_condition_code(name) {
                continue;
            }
            // Walk back to whatever last set it; everything in between is evidence.
            let Some(setter) = names[..at].iter().rposition(|n| writes_condition_code(n)) else {
                continue;
            };
            windows += 1;
            for between in &names[setter + 1..at] {
                assert!(
                    !writes_condition_code(between),
                    concat!(
                        "{} places {} between {} and {}, so the compiler believes it does not ",
                        "write the condition code - and this translator believes it does. One of ",
                        "them is wrong about the hardware, and it is not the compiler"
                    ),
                    path.display(),
                    between,
                    names[setter],
                    name,
                );
                instructions_cleared.insert(between.clone());
            }
        }
    }

    // At least one window must exist, or the test examines nothing.
    assert!(
        windows > 0,
        concat!(
            "no compiled shader in the corpus branches on the condition code, so this test ",
            "checked nothing"
        )
    );
    println!(
        concat!(
            "[hidden side effects] {} window(s); confirmed not to write the condition ",
            "code: {:?}"
        ),
        windows, instructions_cleared
    );
}

/// The dispatch loop's cost on a single-block shader is bounded and structural.
#[test]
fn the_dispatch_loop_costs_a_measurable_amount_on_a_single_block_shader() {
    // Every shader goes through the dispatch loop (D098), including single-block ones.
    // This records the trade: the fixed preamble dwarfs the loop and the body. Asserted
    // loosely, so unrelated emission changes do not break it. Needs no device.
    let table = encodings();
    let operands = OperandTable::builtin().expect("operands");

    let translate_words = |program: &[u32]| {
        let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, table, &operands);
        translate(&decoded, table, Strategy::default())
            .expect("translates")
            .module
            .len()
    };

    // One block: a move and a terminator, no branches anywhere.
    let single = translate_words(&[v_mov_inline(0, 7), s_endpgm()]);

    // An empty program separates the module's fixed cost (types, register file, buffers)
    // from the body's.
    let empty = translate_words(&[s_endpgm()]);

    let body = single.saturating_sub(empty);
    println!("[dispatch loop] empty module {empty} words, one block {single}, body {body}");

    assert!(
        single > empty,
        "a shader with a move in it should emit more than one with nothing"
    );

    // The fixed preamble (types, two register files, two storage buffers) dwarfs the loop
    // scaffolding and the body, so collapsing the loop would save little.
    assert!(
        empty > body * 10,
        concat!(
            "the fixed preamble ({} words) should dominate a single instruction ({}) by a ",
            "wide margin - if it no longer does, the preamble has been slimmed and the ",
            "loop's share of the cost is worth re-examining"
        ),
        empty,
        body,
    );

    // Structural rather than by size, since collapsing the loop would barely move the
    // total.
    let bytes: Vec<u8> = [s_endpgm()].iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, table, &operands);
    let module = translate(&decoded, table, Strategy::default())
        .expect("translates")
        .module;
    let has = |opcode: u16| module.iter().any(|word| (*word & 0xFFFF) as u16 == opcode);
    assert!(
        has(op::LOOP_MERGE) && has(op::SWITCH),
        concat!(
            "a single-block shader still goes through the dispatch loop. If that has changed, ",
            "it is a deliberate second emission path and D110 needs to say so"
        )
    );
}

/// The condition code behaves identically in both models.
#[test]
fn the_condition_code_behaves_the_same_in_both_models() {
    // It is one bit for the whole wavefront, so the per-lane model represents it exactly.
    if !device_or_skip("the_condition_code_behaves_the_same_in_both_models") {
        return;
    }

    // s0 = 5; if (s0 < 1) skip; s1 = 42. The compare is false, so s1 is written.
    let program = [
        s_mov_inline(0, 5),
        s_cmp_i32("s_cmp_lt_i32", 0, 128 + 1),
        branch("s_cbranch_scc1", 1),
        s_mov_inline(1, 42),
        s_endpgm(),
    ];

    let lane = run_at(Fidelity::Lane, &program);
    let wavefront = run_at(Fidelity::Wavefront, &program);
    assert_eq!(
        scalar(&lane, 1),
        42,
        "the branch should not be taken: 5 is not less than 1"
    );
    assert_eq!(
        lane, wavefront,
        concat!(
            "the condition code is one bit for the whole wavefront, so both models must agree ",
            "about it exactly"
        )
    );
}

/// The compiled `control` fixture, mixing condition-code and mask branches, takes the
/// wavefront model.
#[test]
fn a_shader_mixing_condition_code_and_mask_branches_takes_the_model_with_a_mask() {
    // The mask branches decide it: a mask needs the model that has one, while the
    // condition code works in either. Needs no device.
    let table = encodings();
    let operands = OperandTable::builtin().expect("operands");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("orbistoun-shader")
        .join("tests")
        .join("fixtures")
        .join("control.gcn");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    let decoded = decode(&bytes, table, &operands);
    let translated = translate(&decoded, table, Strategy::default()).expect("translates");
    assert_eq!(
        translated.fidelity,
        Fidelity::Wavefront,
        concat!(
            "a shader containing mask branches needs the model that has a mask, whatever else ",
            "it also contains"
        )
    );
}

/// The scalar-base field's value for "there is no base; the address is the register pair".
///
/// `0x7d`, as the reference assembler emits for `off` and compiled shaders carry, not the
/// top of the field.
const FLAT_NO_BASE_CODE: u32 = 0x7d;

/// A flat store of any width: address at bit 0, data at bit 8.
fn flat_store(name: &str, vaddr: u32, data: u32) -> [u32; 2] {
    [head(name), vaddr | (data << 8) | (FLAT_NO_BASE_CODE << 16)]
}

/// A flat load of any width: address at bit 0, destination at bit 24.
///
/// A load and a store do not share an operand layout (destination at bit 24, data at bit 8),
/// which is why operand layouts are per opcode rather than per family (D097).
fn flat_load(name: &str, vaddr: u32, destination: u32) -> [u32; 2] {
    [
        head(name),
        vaddr | (FLAT_NO_BASE_CODE << 16) | (destination << 24),
    ]
}

/// `s_wqm_b64 s[dst:dst+1], <source code>`.
fn s_wqm(dst: u32, source_code: u32) -> u32 {
    head("s_wqm_b64") | (dst << 16) | source_code
}

/// `global_store_dwordx4` writes four consecutive words from one address.
#[test]
fn a_wide_flat_store_writes_consecutive_words() {
    // A translation storing one register four times, or stepping the address by one
    // instead of a word, fails here.
    if !device_or_skip("a_wide_flat_store_writes_consecutive_words") {
        return;
    }

    // v0 = 16 (the byte address), v4..v7 = 11, 22, 33, 44.
    let mut program = vec![
        v_mov_inline(0, 16),
        v_mov_inline(4, 11),
        v_mov_inline(5, 22),
        v_mov_inline(6, 33),
        v_mov_inline(7, 44),
    ];
    program.extend(flat_store("global_store_dwordx4", 0, 4));
    program.push(s_endpgm());

    let (_, memory) = run_memory(Fidelity::Lane, &program);
    for (offset, expected) in [11u32, 22, 33, 44].iter().enumerate() {
        assert_eq!(
            memory[4 + offset],
            *expected,
            "address 16 is word 4, so word {} should hold {expected}; memory was {memory:?}",
            4 + offset
        );
    }
    assert_eq!(memory[8], 0, "and nothing past it; memory was {memory:?}");
}

/// `global_load_dwordx4` fills four consecutive registers.
#[test]
fn a_wide_flat_load_fills_consecutive_registers() {
    if !device_or_skip("a_wide_flat_load_fills_consecutive_registers") {
        return;
    }

    // Seed four words through a wide store, then read them back into a different range.
    let mut program = vec![
        v_mov_inline(0, 16),
        v_mov_inline(4, 11),
        v_mov_inline(5, 22),
        v_mov_inline(6, 33),
        v_mov_inline(7, 44),
    ];
    program.extend(flat_store("global_store_dwordx4", 0, 4));
    program.extend(flat_load("global_load_dwordx4", 0, 0));
    program.push(s_endpgm());

    // The load reads word 4 onwards into v0..v3, overwriting the address register; the
    // address is consumed before the first write lands.
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    for (offset, expected) in [11u32, 22, 33, 44].iter().enumerate() {
        assert_eq!(
            vector(&registers, offset),
            *expected,
            "v{offset} should hold {expected}; registers were {registers:?}"
        );
    }
}

/// A wide flat load running past the vector register file is refused, not truncated.
#[test]
fn a_wide_access_past_the_register_file_is_refused() {
    // `global_load_dwordx4` into v253 would write four registers where three exist. Needs
    // no device.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");

    let mut program: Vec<u32> = Vec::new();
    program.extend(flat_load("global_load_dwordx4", 0, 253));
    program.push(s_endpgm());
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);

    let error = translate(&decoded, &table, Strategy::default())
        .expect_err("a load running off the register file must be refused");
    assert!(
        error
            .to_string()
            .contains("past the end of the vector register file"),
        "the error should say what is wrong, got: {error}"
    );
}

/// `s_wqm_b64` sets every bit of a quad that had any, and no other quad.
#[test]
fn whole_quad_mode_sets_every_bit_of_a_group_that_had_any() {
    // A fragment shader uses this so a derivative across a quad has all four pixels live.
    if !device_or_skip("whole_quad_mode_sets_every_bit_of_a_group_that_had_any") {
        return;
    }

    // s[0:1] = 1: only bit 0 set, so the first quad becomes 0b1111 and nothing else.
    let program = [s_mov_b64(0, 128 + 1), s_wqm(2, 0), s_endpgm()];
    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        scalar(&registers, 2),
        0b1111,
        "one bit in a quad lights the whole quad; registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 3),
        0,
        "and the high half is untouched; registers were {registers:?}"
    );

    // A bit in the second group and nothing in the first: 0b1_0000 becomes 0b1111_0000,
    // which catches a fold spilling across a group boundary.
    let program = [s_mov_b64(0, 128 + 16), s_wqm(2, 0), s_endpgm()];
    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        scalar(&registers, 2),
        0b1111_0000,
        "a set bit must light its own quad and no other; registers were {registers:?}"
    );
}

/// `s_wqm_b64` leaves an empty mask empty and a full mask full.
#[test]
fn whole_quad_mode_leaves_an_empty_mask_empty() {
    // A fold that ored in a constant, or a spread ignoring its input, lights every quad.
    if !device_or_skip("whole_quad_mode_leaves_an_empty_mask_empty") {
        return;
    }

    let program = [s_mov_b64(0, 128), s_wqm(2, 0), s_endpgm()];
    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(scalar(&registers, 2), 0, "registers were {registers:?}");
    assert_eq!(scalar(&registers, 3), 0, "registers were {registers:?}");

    // And an all-ones mask stays all ones.
    let program = [s_mov_b64(0, 193), s_wqm(2, 0), s_endpgm()];
    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        scalar(&registers, 2),
        u32::MAX,
        "registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 3),
        u32::MAX,
        "registers were {registers:?}"
    );
}

/// `s_wqm_b32 sDst, <source code>`.
fn s_wqm32(dst: u32, source_code: u32) -> u32 {
    head("s_wqm_b32") | (dst << 16) | source_code
}

/// The 32-lane form lights whole quads in one register.
///
/// Textured pixel shaders enter whole-quad mode with `s_wqm_b32 exec_lo, exec_lo`. Same three
/// cases as the 64-bit form: one bit lights its quad, a bit in the second quad lights only
/// that quad, and nothing stays nothing.
#[test]
fn whole_quad_mode_32_lights_the_quad_of_every_set_bit_and_nothing_else() {
    if !device_or_skip("whole_quad_mode_32_lights_the_quad_of_every_set_bit_and_nothing_else") {
        return;
    }
    for (seed, expected) in [(1_u32, 0b1111_u32), (16, 0b1111_0000), (0, 0)] {
        let program = [s_mov_b64(0, 128 + seed), s_wqm32(2, 0), s_endpgm()];
        let (registers, _) = run_memory(Fidelity::Wavefront, &program);
        assert_eq!(
            scalar(&registers, 2),
            expected,
            "seed {seed:#b}; registers were {registers:?}"
        );
    }
}

/// `ds_write_b32 vAddr, vData offset:N`.
///
/// The opcode sits at bit 17, not 18 as in the flat families; the wrong shift decodes as a
/// different local-share instruction.
fn ds_write(vaddr: u32, data: u32, offset: u32) -> [u32; 2] {
    [head("ds_write_b32") | offset, vaddr | (data << 8)]
}

/// `ds_read_b32 vDst, vAddr offset:N`.
///
/// The destination is at bit 24 and the address at bit 0; a read and a write do not share
/// a layout.
fn ds_read(dst: u32, vaddr: u32, offset: u32) -> [u32; 2] {
    [head("ds_read_b32") | offset, vaddr | (dst << 24)]
}

/// A value written to the local share reads back through the same address.
#[test]
fn a_value_written_to_the_local_share_can_be_read_back() {
    // Storage the lanes of a wavefront exchange values in.
    if !device_or_skip("a_value_written_to_the_local_share_can_be_read_back") {
        return;
    }

    let mut program = vec![v_mov_inline(0, 8), v_mov_inline(1, 37)];
    program.extend(ds_write(0, 1, 0));
    program.extend(ds_read(2, 0, 0));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        37,
        "written and read back through address 8; registers were {registers:?}"
    );
}

/// The local-share instruction offset is applied.
#[test]
fn the_local_share_offset_is_not_ignored() {
    // The reference omits a zero offset. Two values eight bytes apart, read back with an
    // offset that must reach the second.
    if !device_or_skip("the_local_share_offset_is_not_ignored") {
        return;
    }

    let mut program = vec![v_mov_inline(0, 8), v_mov_inline(1, 11), v_mov_inline(3, 22)];
    program.extend(ds_write(0, 1, 0));
    program.extend(ds_write(0, 3, 8));
    // Read address 8 with offset 8: that is the second value, not the first.
    program.extend(ds_read(2, 0, 8));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        22,
        "an ignored offset would read 11 here; registers were {registers:?}"
    );
}

/// A masked write does not reach the local share.
#[test]
fn a_masked_write_does_not_reach_the_local_share() {
    // Another lane of the same wavefront reads this word.
    if !device_or_skip("a_masked_write_does_not_reach_the_local_share") {
        return;
    }

    let mut program = vec![v_mov_inline(0, 12), v_mov_inline(1, 55)];
    program.extend(ds_write(0, 1, 0));
    // Now disable every lane and try to overwrite it.
    program.push(s_mov_exec(128));
    // Within 0..=64, which is all an inline integer constant can carry.
    program.push(v_mov_inline(3, 33));
    program.extend(ds_write(0, 3, 0));
    // Re-enable, and read back.
    program.push(s_mov_exec(193));
    program.extend(ds_read(2, 0, 0));
    program.push(s_endpgm());

    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        vector(&registers, 2),
        55,
        "the masked write must not have landed; registers were {registers:?}"
    );
}

/// The lane model refuses the local share, and `Auto` routes it to the wavefront model.
#[test]
fn the_lane_model_refuses_the_local_share() {
    // Each invocation would get its own copy and read back only what it wrote. Needs no
    // device.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let mut program: Vec<u32> = Vec::new();
    program.extend(ds_read(2, 0, 0));
    program.push(s_endpgm());
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &table, &operands);

    let error = translate(
        &decoded,
        &table,
        Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        },
    )
    .expect_err("the lane model must refuse the local data share");
    assert!(
        error.to_string().contains("no local data share"),
        "the error should name what is missing, got: {error}"
    );

    // `Auto` routes it to the model that has one.
    let translated = translate(&decoded, &table, Strategy::default()).expect("auto");
    assert_eq!(translated.fidelity, Fidelity::Wavefront);
}

/// The long-form subtract and reverse-subtract compute `a - b` and `b - a`.
#[test]
fn the_long_form_subtract_and_reverse_subtract_are_not_swapped() {
    // Opcodes 258 and 259 on this generation; the short-form test does not cover them.
    if !device_or_skip("the_long_form_subtract_and_reverse_subtract_are_not_swapped") {
        return;
    }

    // v0 = 2.0, v1 = 1.0.
    let setup = [v_mov_code(0, F_2), v_mov_code(1, F_1)];

    // v_sub_f32_e64 v2, v0, v1 -> 2.0 - 1.0 = 1.0.
    let mut program = setup.to_vec();
    program.extend(vop3(
        "v_sub_f32_e64",
        2,
        [vgpr_code(0), vgpr_code(1), 0],
        0,
        0,
    ));
    program.push(s_endpgm());
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        vector(&registers, 2),
        BITS_1,
        "2.0 - 1.0 should be 1.0; registers were {registers:?}"
    );

    // v_subrev_f32_e64 v2, v0, v1 -> 1.0 - 2.0 = -1.0. The same operands, the other way.
    let mut program = setup.to_vec();
    program.extend(vop3(
        "v_subrev_f32_e64",
        2,
        [vgpr_code(0), vgpr_code(1), 0],
        0,
        0,
    ));
    program.push(s_endpgm());
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    assert_eq!(
        vector(&registers, 2),
        BITS_MINUS_1,
        concat!(
            "reverse-subtract takes them the other way round, so this is -1.0; registers ",
            "were {:?}"
        ),
        registers
    );
}

/// Every instruction name the translator supports exists on this target.
#[test]
fn every_supported_name_exists_on_this_target() {
    // Opcode numbers move between generations and names mostly do not (D139); a renamed
    // instruction (`v_add_u32` to `v_add_nc_u32`) arrives here by name. Needs no device.
    use orbistoun_translate::model::unresolved;

    let table = EncodingTable::builtin().expect("encodings");
    let missing = unresolved(&table);
    assert!(
        missing.is_empty(),
        concat!(
            "the translator understands {} instruction(s) this target does not have under ",
            "those names: {:?}. Either the tables were generated for a different ",
            "generation, or these were renamed - both are real and both need a decision ",
            "rather than a silent rebinding"
        ),
        missing.len(),
        missing
    );
}
