//! Translating guest instructions and running the result.
//!
//! The tests that drive the translator. Each states what a guest instruction should
//! *do*, and the only way to satisfy one is to translate it correctly and have a real
//! device agree - `spirv-val` accepting the module is necessary and nowhere near
//! sufficient.
//!
//! # How a register is observed
//!
//! The guest's registers live in a private array inside the shader, and the translated
//! module copies the low registers into the storage buffer before returning. So
//! `buffer[n]` is vector register `n` at the end of the shader.
//!
//! That copy exists for observation and nothing else. It is what makes an otherwise
//! invisible register file assertable, and therefore what makes translation testable
//! at all before anything renders.
//!
//! # A missing device skips loudly
//!
//! Same rule as the dispatch tests: a harness captures a passing test's output, so a
//! silent skip would make this file appear to pass on a machine where it never ran.
//! `bin/orbistoun check` surfaces it.

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

/// Vector register `n`, from a returned buffer.
/// The loaded encoding table, read once for the whole file.
fn encodings() -> &'static EncodingTable {
    static TABLE: std::sync::OnceLock<EncodingTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| EncodingTable::builtin().expect("encodings"))
}

/// The first word of an instruction, from its name: family bits plus opcode in place.
///
/// **Every instruction in this file is built through here.** It used to write encodings
/// down - `0xBE80_0000 | (dst << 16)` and fifty more like it - and when the target
/// architecture generation changed, sixty tests failed at once, every one of them
/// blaming the translator for a number the test itself had wrong.
///
/// A test that hard-codes what it is testing stops testing it exactly when it matters.
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
/// Built here rather than assembled, so the test states the encoding it means and does
/// not depend on a toolchain being present.
fn v_mov_inline(dst: u32, constant: u32) -> u32 {
    // VOP1, opcode 1. The source code for a small non-negative integer is 128 + value.
    head("v_mov_b32_e32") | (dst << 17) | (128 + constant)
}

/// `s_endpgm`.

#[test]
fn a_move_of_an_inline_constant_reaches_the_register() {
    // The first thing translation has to get right, and the smallest: one instruction
    // that writes a known value somewhere observable. Everything else builds on the
    // register file existing and being written to the right slot.
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

#[test]
fn a_move_writes_only_the_register_it_names() {
    // A translator that wrote every register, or the wrong one, would pass the test
    // above. This is what makes the destination field load-bearing.
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

#[test]
fn later_moves_overwrite_earlier_ones() {
    // Instructions run in order, and a register holds what was last put in it. Obvious,
    // and not free: a translator emitting stores in an arbitrary order would satisfy
    // both tests above.
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
    // SOP1, opcode 0. sdst at bit 16, ssrc0 in the low byte.
    head("s_mov_b32") | (dst << 16) | (128 + constant)
}

/// `s_waitcnt` with no counters to wait on.

#[test]
fn a_scalar_move_reaches_a_scalar_register() {
    // Scalar registers are a separate file from vector ones, and the guest addresses
    // them separately. A translator with only one file would put s2 where v2 lives and
    // corrupt whichever was written second.
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

#[test]
fn the_scalar_and_vector_files_are_separate() {
    // The property that makes two files necessary rather than tidy: writing s3 must
    // leave v3 alone, and the other way round.
    if !device_or_skip("the_scalar_and_vector_files_are_separate") {
        return;
    }

    let registers = run(&[s_mov_inline(3, 11), v_mov_inline(3, 4), s_endpgm()]);
    assert_eq!(scalar(&registers, 3), 11, "s3, got {registers:?}");
    assert_eq!(vector(&registers, 3), 4, "v3, got {registers:?}");
}

#[test]
fn a_wait_instruction_translates_and_changes_nothing() {
    // It orders memory operations rather than computing anything, so the observable
    // effect is none - but it must translate, because refusing it blocks eight of the
    // nine shaders in the fixture corpus.
    if !device_or_skip("a_wait_instruction_translates_and_changes_nothing") {
        return;
    }

    let registers = run(&[v_mov_inline(1, 7), s_waitcnt(), s_endpgm()]);
    assert_eq!(vector(&registers, 1), 7, "got {registers:?}");
}

#[test]
fn the_supported_list_and_the_translator_agree() {
    // Two things can drift: a list saying an instruction is supported, and a translator
    // that refuses it. A worklist built on the first would then rank work that is
    // already done, or hide work that is not.
    //
    // Needs no device - it never runs a shader.

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

        // Asked through the loaded table, because the supported list names
        // instructions and the encodings decide where those live on this target.
        let listed = orbistoun_translate::model::supports_named(&table, family, opcode);

        // Whether it was refused *for not being supported*, which is the only refusal
        // this test is about. A supported instruction can still be refused for its
        // operands: these encodings carry zeroes in every operand field, and for the
        // carry arithmetic that names an ordinary register pair where only the condition
        // mask is handled. That is a true answer about a synthetic encoding and not
        // drift between the list and the dispatch.
        let unsupported = matches!(
            &translated,
            Err(orbistoun_translate::TranslateError::Unsupported { detail, .. })
                if *detail == orbistoun_translate::model::NO_TRANSLATION
                    || orbistoun_translate::model::blocked(name) == Some(*detail)
        );
        // Two claims, not one claim reversed.
        //
        // A listed instruction must not be rejected *for being unsupported*; it may
        // still be refused for what these synthetic operands say. An absent one must be
        // refused *somehow*, and the reason is allowed to be more specific than the
        // list - an export is refused for having no operand layout, which is a better
        // answer than "not supported" and would be discarded by demanding one exact
        // reason.
        if listed {
            assert!(
                !unsupported,
                "{family}:{opcode:#x} is listed in SUPPORTED but the translator rejected \
                 it as unsupported: {:?}",
                translated.err(),
            );
        } else {
            assert!(
                translated.is_err(),
                "{family}:{opcode:#x} is absent from SUPPORTED but the translator \
                 accepted it",
            );
        }
    }
}

/// Every instruction this target has a name for, as bytes that decode back to it.
///
/// **Derived, not written down.** The hand-written version of this list carried its own
/// epitaph in a comment: *this entry was missing while the branches were missing from
/// `SUPPORTED`, which is exactly the drift this test exists to catch - and it did not,
/// because the list of encodings it checks is also written by hand.* A checker written
/// from the same understanding as the thing it checks agrees with it by construction.
///
/// So it asks the table. Every name the table knows appears, whether the translator
/// supports it or not, which is what makes the *absent* direction meaningful - a list of
/// supported instructions can only ever confirm that supported instructions work.
///
/// Operand fields are left zero. That is a valid encoding for every family here: zero
/// names scalar register zero, not an absent operand.
fn known_encodings() -> Vec<(String, u32, String, Vec<u32>)> {
    let table = encodings();
    let mut out = Vec::new();
    for (family, opcode, name) in table.names() {
        // Padding is excluded because the decoder stops at it: it marks the end of the
        // code, so it never reaches the translator and there is nothing to assert.
        if name == "s_code_end" {
            continue;
        }
        let Some(encoding) = table.encodings().iter().find(|e| e.name == family) else {
            continue;
        };
        let mut words = vec![encoding.value | (opcode << encoding.opcode.shift)];
        // A wide encoding needs its second word present, or the terminator appended
        // below would be read as one and the program would have no terminator at all.
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

#[test]
fn the_wavefront_and_lane_models_agree() {
    // The payoff of having more than one level, and the reason the slow one is worth
    // building: two independent models of the same machine, checked against each other
    // with no reference hardware, no console and no title.
    //
    // A disagreement means the faster model has a bug, localised to one program - which
    // is a far better starting point than a wrong pixel somewhere in a frame.
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

#[test]
fn the_wavefront_model_starts_with_every_lane_active() {
    // At entry the guest has all sixty-four lanes running - the mask is all ones. A
    // model that started it at zero would execute nothing at all while still producing
    // a valid module and a plausible buffer of zeros.
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

/// Operand codes for the inline floats, from `data/operands.toml`.
const INLINE_ONE: u32 = 242;
const INLINE_TWO: u32 = 244;
const INLINE_HALF: u32 = 240;

#[test]
fn float_addition_produces_the_right_bits() {
    // Registers hold bit patterns, not typed values - the instruction decides how to
    // read them. So this is really asserting that the translator *bitcasts* rather than
    // converts: converting would turn the bit pattern for 1.0 into the float
    // 1065353216.0 and produce a number that is wrong by nine orders of magnitude.
    if !device_or_skip("float_addition_produces_the_right_bits") {
        return;
    }

    let registers = run(&[
        v_mov_code(0, INLINE_ONE),
        v_mov_code(1, INLINE_TWO),
        vop2_vv("v_add_f32_e32", 2, 0, 1),
        s_endpgm(),
    ]);
    // Compared as bits rather than as floats. These values are exactly representable,
    // so an approximate comparison would only invite a tolerance that does not apply -
    // and the claim being made is about the bit pattern a register holds.
    assert_eq!(
        vector(&registers, 2),
        3.0_f32.to_bits(),
        "1.0 + 2.0; got bits {:#x}",
        vector(&registers, 2)
    );
}

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

#[test]
fn an_inline_float_constant_reaches_a_register_unconverted() {
    // The constant 1.0 has to arrive as its bit pattern. A translator treating the
    // operand code as a number would store 242, and a translator converting rather than
    // bitcasting would store something enormous - both plausible-looking failures.
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

#[test]
fn the_models_agree_on_float_arithmetic() {
    // Arithmetic is where the two models could most easily drift: one computes it once,
    // the other sixty-four times through a different register layout.
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
/// The base field is all ones, which is how the encoding says "no scalar base".
fn global_store(vaddr: u32, vdata: u32) -> [u32; 2] {
    [
        head("global_store_dword"),
        vaddr | (vdata << 8) | (0x7F << 16),
    ]
}

/// `global_load_dword vDst, vAddr, off`.
fn global_load(vdst: u32, vaddr: u32) -> [u32; 2] {
    [
        head("global_load_dword"),
        vaddr | (0x7F << 16) | (vdst << 24),
    ]
}

/// `s_load_dwordxN sDst, s[base:base+1], offset`, from the solved layout.
///
/// Destination at bit 6, base halved at bit 0, byte offset in the second word. The
/// opcode selects the width: 0 is one word, 1 is two, 2 is four, 3 is eight.
fn s_load(name: &str, dst: u32, base: u32, offset: u32) -> [u32; 2] {
    [head(name) | (dst << 6) | (base / 2), offset]
}

#[test]
fn a_wide_scalar_load_past_the_register_file_is_refused() {
    // `s_load_dwordx8` into s100 would write four registers and then four *specials* -
    // the shared operand numbering runs straight on past the last register, so nothing
    // downstream would see anything wrong. Refused at translation, and refused rather
    // than truncated: a load that quietly filled half its destinations would be a
    // shader computing the wrong thing while appearing to work.
    //
    // Needs no device.
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

#[test]
fn a_wide_scalar_load_fills_consecutive_registers() {
    // What separates `s_load_dwordx4` from four `s_load_dword`s: one address, four
    // destinations, and they must land in order. A translation that wrote the same
    // register four times, or counted the offset in words rather than bytes, passes
    // every single-word test and fails here.
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

#[test]
fn a_wide_scalar_load_writes_no_register_beyond_its_width() {
    // The other half of the same property. `s_load_dwordx2` fills two registers and
    // must leave the third alone - a loop bound taken from the wrong opcode would be
    // invisible in the test above, which only asserts on registers it expects written.
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

#[test]
fn the_models_agree_about_a_wide_scalar_load() {
    // The differential oracle applied to the newest instruction. The two models write
    // scalars by entirely different routes - one to a private array, the other through
    // a masked select - so agreement here is evidence rather than tautology.
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

#[test]
fn a_store_reaches_guest_memory() {
    // The first instruction that leaves the shader's own registers. Address eight is
    // word two, and nothing else in the buffer may move.
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

#[test]
fn a_value_stored_can_be_loaded_back() {
    // Store then load through the same address. Self-contained, and it fails if either
    // half computes its address differently from the other - which is the mistake most
    // likely to survive a test that only did one of them.
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

#[test]
fn the_models_agree_about_memory() {
    // Memory is where the two models differ most: one stores once, the other stores
    // sixty-four times under a mask into the same location. They must still agree.
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

#[test]
fn a_sixty_four_bit_move_extends_a_constant_rather_than_repeating_it() {
    // The distinction that makes this not two 32-bit moves. `1` fills the low register
    // and zeroes the high one; `-1` fills both. Copying the low word into both halves
    // is right for -1 and wrong for 1, so a test using only -1 would pass on a wrong
    // translation - which is why both are here.
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

#[test]
fn a_sixty_four_bit_move_copies_both_halves_of_a_register_pair() {
    if !device_or_skip("a_sixty_four_bit_move_copies_both_halves_of_a_register_pair") {
        return;
    }

    // Set s4 and s5 apart, then move the pair to s[0:1]. Distinct values, so a
    // translation copying the low half twice is visible.
    let registers = run(&[
        s_mov_inline(4, 9),
        s_mov_inline(5, 23),
        s_mov_b64(0, 4),
        s_endpgm(),
    ]);

    assert_eq!(scalar(&registers, 0), 9, "registers were {registers:?}");
    assert_eq!(scalar(&registers, 1), 23, "registers were {registers:?}");
}

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

#[test]
fn a_blocked_instruction_says_what_it_is_blocked_on() {
    // The point of BLOCKED is that the worklist can rank "needs a subsystem" apart from
    // "nobody has written it yet". If the reason does not reach the error, the two are
    // indistinguishable again and the list is decoration.
    //
    // Needs no device.
    use orbistoun_translate::model::{BLOCKED, blocked, supports};

    let table = encodings();
    for (name, reason) in BLOCKED {
        assert!(!supports(name), "{name} is in both SUPPORTED and BLOCKED");
        assert!(
            !reason.is_empty(),
            "{name} is blocked on nothing in particular"
        );
        assert_eq!(blocked(name), Some(*reason));

        // A reason for an instruction this target does not have is a reason nobody will
        // ever read. The list is a worklist, and an entry that cannot be reached is an
        // entry that quietly stopped being work.
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

#[test]
fn a_cleared_execution_mask_suppresses_a_vector_write() {
    // The first test that makes the wavefront model's masking mean anything. Until an
    // instruction could write the mask, every lane was active in every shader that
    // translated, so the select on every vector write was choosing the new value every
    // time - correct, and never exercised.
    //
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

#[test]
fn the_execution_mask_is_per_lane_not_all_or_nothing() {
    // A mask of 1 leaves lane 0 active; a mask of 2 leaves lane 1 active and lane 0 not.
    // Both are observed through lane 0, which is the only lane the observation window
    // reports - so the second case is the one that proves the mask is read bit by bit
    // rather than tested against zero.
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
        "lane 0 is inactive in mask 2, so its register keeps its old value; \
         registers were {lane_one:?}"
    );
}

#[test]
fn a_masked_store_does_not_reach_guest_memory() {
    // The same property one layer down. A store from an inactive lane must leave memory
    // alone, because another lane will read what it would otherwise have written - which
    // is why `write_memory` is a required trait method rather than a provided one.
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

#[test]
fn the_lane_model_refuses_a_shader_that_writes_the_execution_mask() {
    // Not a fallback and not a no-op. This model has one invocation per lane and no way
    // to represent an inactive one, so a shader that disables lanes would run every lane
    // regardless - plausible output, wrong answer, nothing in it to indicate the
    // problem. Refusing is what makes choosing this level safe by decision rather than
    // by the accident of no such instruction being translatable (D098).
    //
    // Needs no device.
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

    // And the wavefront model must accept the same shader, or the refusal above is just
    // the instruction being unimplemented everywhere.
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

#[test]
fn auto_fidelity_picks_the_model_the_shader_needs() {
    // A shader that leaves the mask alone gets the cheap model; one that writes it gets
    // the model that has a mask. The alternative was Auto always meaning Lane, which
    // was correct only while no instruction touching the mask could be translated -
    // safety that expired the moment one could.
    //
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

#[test]
fn narrowing_the_mask_narrows_which_lanes_write() {
    // What a guest actually does on entering a conditional region: and the mask with
    // whichever lanes passed the test. Here the second operand is a constant standing in
    // for a comparison result, because comparison instructions do not translate yet -
    // the arithmetic on the mask is the part under test.
    //
    // Mask starts all ones; anding with 2 leaves lane 1 and not lane 0.
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

#[test]
fn andn2_takes_the_lanes_the_first_branch_did_not() {
    // The else-branch. `s_andn2_b64 exec, exec, taken` leaves exactly the lanes that
    // were active and not taken - so translating it as a plain and would invert the
    // sense of every else in every shader, silently.
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

    // Remove lane 1 instead: lane 0 still writes. This is the case a plain `and` gets
    // wrong, and the previous one is the case it gets right - so both are needed.
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

#[test]
fn sixty_four_bit_logic_operates_on_both_halves() {
    // Into an ordinary register pair rather than the mask, so both halves are directly
    // observable. A translation that did the low half only would pass every mask test
    // above, because none of them uses a lane above thirty-one.
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

#[test]
fn a_comparison_produces_a_mask_that_narrows_who_writes() {
    // The whole point of comparisons here: they are where a mask comes from. Compare,
    // and the answer into `exec`, and the following write lands only in lanes that
    // passed - which is an if-branch with no branch in it.
    //
    // Every lane compares the same two registers, so the answer is all-ones or all-zero.
    // That is enough to show the mask is produced and honoured; per-lane values need a
    // lane-id source, which does not translate yet.
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

    // The same shader with the comparison reversed: no lane survives, so the write is
    // suppressed. Without this case a comparison that always answered true would pass.
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

#[test]
fn a_comparison_compares_floats_not_the_bits() {
    // A register holds thirty-two bits and this instruction is what decides they are a
    // float. Comparing the integers instead agrees on every non-negative pair and orders
    // negatives backwards - so -1.0 < 1.0 is the case that separates them, and a test
    // using positive values only would pass either way.
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
        "-1.0 < 1.0 holds as floats; as unsigned bits it does not. Registers were \
         {registers:?}"
    );
}

#[test]
fn a_comparison_writes_the_condition_mask_not_the_execution_mask() {
    // `vcc` and `exec` are different registers, and a translation that wrote the
    // comparison straight into `exec` would pass every test above - the shaders there
    // all and the result into `exec` immediately afterwards anyway.
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
        "a false comparison must not disable any lane by itself; registers were \
         {registers:?}"
    );
}

#[test]
fn the_lane_model_refuses_a_comparison() {
    // Same reasoning as the mask write. A comparison produces one bit per lane, and this
    // model has no place to put sixty-four of them - answering with the one lane it has
    // would be a mask that claims the other sixty-three all agree.
    //
    // Needs no device.
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

    // And Auto must route it to the model that can, rather than refusing.
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
/// the first result. There is no single instruction for this - a lane is not told which
/// lane it is, it counts.
fn lane_index_into(dst: u32) -> [u32; 4] {
    // Inline -1 is code 193 (an all-ones mask); inline 0 is code 128.
    let lo = v_mbcnt("v_mbcnt_lo_u32_b32", dst, 193, 128);
    let hi = v_mbcnt("v_mbcnt_hi_u32_b32", dst, 193, vgpr_code(dst));
    [lo[0], lo[1], hi[0], hi[1]]
}

#[test]
fn a_lane_can_learn_its_own_index() {
    // Asserted through guest memory rather than the register window, because the window
    // only reports lane 0 - and lane 0's index is zero, which is also what an untouched
    // register reads. Each lane stores its index at its own address, so the whole
    // wavefront is visible at once.
    //
    // This is the first test in the file where the lanes do different things.
    if !device_or_skip("a_lane_can_learn_its_own_index") {
        return;
    }

    let mut program: Vec<u32> = lane_index_into(0).to_vec();
    // v1 = v0 << 2, the byte address of word `index`.
    program.push(v_int_op("v_lshlrev_b32_e32", 1, 128 + 2, 0));
    program.extend(global_store(1, 0));
    program.push(s_endpgm());

    // Every word, not a sample. Asserting the whole window is what pins the shift:
    // `v_lshlrev_b32` takes its amount first and its value second, and the reversed
    // reading gives `2 << index` rather than `index << 2`. Those agree for lane 2 and
    // for no other lane, so one lane proves nothing and sixteen prove it outright.
    let (_, memory) = run_memory(Fidelity::Wavefront, &program);
    for word in 0..16usize {
        assert_eq!(
            memory[word], word as u32,
            "lane {word} should have written its index to word {word}; memory was {memory:?}"
        );
    }
}

#[test]
fn a_comparison_can_leave_some_lanes_active_and_others_not() {
    // What no mask test so far could show. Until a lane could learn its index, every
    // lane compared the same registers and every mask was all-ones or all-zero - so a
    // translation testing the mask against zero rather than bit by bit would have passed
    // every one of them.
    //
    // Here lane n compares its own index against four, so the mask is a genuine mixture
    // and each lane's store is kept or dropped on its own.
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
            "lane {word} failed the comparison and must not have stored; memory was \
             {memory:?}"
        );
    }
}

/// A SOPP branch: opcode at bit 16, signed dword offset in the low half.
fn branch(name: &str, offset: i16) -> u32 {
    head(name) | u32::from(offset as u16)
}

#[test]
fn a_forward_branch_skips_the_block_it_jumps_over() {
    // Predication alone gets this right by accident for *vector* work - falling through
    // with no lane active suppresses every write anyway. It does not for scalar work,
    // which runs regardless of the mask. So the block skipped here writes a scalar
    // register, and the assertion is that it did not.
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
        "the skipped block must not have run - a scalar write ignores the mask, so \
         falling through would show here; registers were {registers:?}"
    );
    assert_eq!(
        scalar(&registers, 1),
        7,
        "the branch target must have run; registers were {registers:?}"
    );

    // The same shader with every lane enabled: the branch is not taken and both blocks
    // run. Without this the test would pass on a translation that never branches at all.
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
        "the branch was not taken, so the middle block must have run; registers were \
         {registers:?}"
    );
    assert_eq!(scalar(&registers, 1), 7, "registers were {registers:?}");
}

#[test]
fn a_backward_branch_loops() {
    // The case predication cannot express at all, and the reason the dispatch loop
    // exists. A shader that counts, and stops on its own:
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
    // Five passes: the counter reaches five, the comparison fails, the mask empties and
    // the branch is not taken. Asserting on five rather than on "more than one" is what
    // makes this a loop test - a translation that ran the body once would leave one, and
    // one that never terminated would not return at all.
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
        "the loop should have run until the counter reached the limit; registers were \
         {registers:?}"
    );
}

/// `s_cmp_<op>_i32 src0, src1`.
///
/// SOPC: opcode at bit 16, second source at bit 8, first in the low byte. No
/// destination - the answer goes to the condition code, which is not an operand.
fn s_cmp_i32(name: &str, first_code: u32, second_code: u32) -> u32 {
    head(name) | (second_code << 8) | first_code
}

#[test]
fn a_scalar_compare_drives_a_branch() {
    // The pairing that makes the `control` fixture translatable: a scalar compare sets
    // the condition code and a branch reads it. Neither is any use without the other,
    // which is why the branch was refused until the compare existed.
    if !device_or_skip("a_scalar_compare_drives_a_branch") {
        return;
    }

    // s0 = 5. Compare against 1: 5 < 1 is false, so scc is clear and s_cbranch_scc1 is
    // not taken - the skipped block runs.
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

    // Compare against 64 instead: 5 < 64 is true, the branch is taken, the block is
    // skipped. Without this case a compare that always answered false would pass.
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

#[test]
fn a_scalar_compare_is_signed() {
    // These compare signed integers, and reading the same bits as unsigned agrees on
    // every pair of non-negative values. -1 is the case that separates them: as a signed
    // integer it is less than zero, and as an unsigned one it is the largest value there
    // is. A test using positive numbers passes either way.
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
        "-1 < 0 holds as a signed comparison, so the branch is taken and the block is          skipped; as unsigned it would not be. Registers were {registers:?}"
    );
}

#[test]
fn the_condition_code_is_not_a_lane_mask() {
    // The condition code is one bit for the whole wavefront, not one per lane, so the
    // per-lane model can represent it perfectly well and a shader using it must not be
    // pushed onto the slow model. Routing it there would be correct and sixty-four times
    // slower for nothing.
    //
    // Needs no device.
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

#[test]
fn a_compiled_shader_with_control_flow_translates() {
    // The first fixture that is *compiler output* rather than an instruction stream
    // written here, and it contains real control flow: a scalar compare, a branch on the
    // condition code, a branch on the condition mask, and a backward branch on the
    // execution mask. Everything before this was a shader chosen to exercise what had
    // just been built.
    //
    // Needs no device - what is being asserted is that it translates at all, which is
    // the thing that was untrue an hour ago.
    let table = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("orbistoun-shader")
        .join("tests")
        .join("fixtures")
        .join("control.gcn");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    // The whole buffer. The decoder stops itself at the padding that follows a
    // compiled shader, and this shader ends its wave twice - so asking it to stop at the
    // first terminator would cut it in half.
    let decoded = decode(&bytes, &table, &operands);
    assert!(decoded.is_trustworthy(), "the fixture must decode cleanly");
    assert!(
        decoded.terminated,
        "a compiled shader should reach its terminator"
    );

    let translated = translate(&decoded, &table, Strategy::default())
        .expect("a compiled shader with control flow should translate");

    // It reads and writes the execution mask, so Auto must have chosen the model that
    // has one. Getting Lane here would mean the shader translated by ignoring the mask.
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

#[test]
fn subtract_and_reverse_subtract_are_not_the_same() {
    // `v_subrev_f32` reverses its operands - the name says so and the encoding does not.
    // Read in written order it computes b - a where a - b was meant, and the two agree
    // exactly when the operands are equal. So both are asserted, with operands that are
    // not equal.
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

#[test]
fn source_modifiers_apply_and_apply_in_order() {
    // The single most likely way this crate could be quietly wrong at scale. A negate
    // flag is one bit that neither the operand layout nor the encoding table describes,
    // and every subtraction a compiler expressed as an addition of a negated operand
    // depends on it.
    //
    // Three assertions, because each isolates something the others cannot: negate alone
    // on a positive source, absolute alone on a negative source, and both together where
    // the order decides the answer.
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
    // Absolute first, then negate: the absolute of -2.0 is 2.0, negated is -2.0, plus
    // 1.0 is -1.0. The other order: -2.0 negated is 2.0, absolute is 2.0, plus 1.0 is
    // 3.0. Both flags are known to work individually from the two cases above, so this
    // one pins the order and nothing else.
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
        "absolute is applied before negate, so this is -1.0 and not 3.0; registers were \
         {registers:?}"
    );
}

#[test]
fn a_modifier_that_is_not_translated_is_refused() {
    // The clamp flag and the output multiplier both change the result. Ignoring one
    // produces a shader computing something close to right, which is harder to find than
    // one that refuses - so both are errors naming what they are.
    //
    // Needs no device.
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

#[test]
fn a_conditional_move_picks_the_second_source_when_the_bit_is_set() {
    // A set bit picks the *second* source. The other way round takes the wrong branch of
    // every ternary a compiler wrote, and the shader runs either way - so both
    // directions are asserted rather than one.
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

#[test]
fn sixty_four_bit_logic_writes_the_condition_code() {
    // The gap this unit was written to close. These instructions set the code to whether
    // their result is non-zero, and translating only the destination left a shader
    // branching on whatever the *previous* compare had put there.
    //
    // `s_and_b64 exec, exec, vcc` then a branch on the code is how a compiler skips a
    // block once no lane survives, so this is not an obscure corner.
    if !device_or_skip("sixty_four_bit_logic_writes_the_condition_code") {
        return;
    }

    // Set the code with a compare that is true, then and two masks to zero. If the and
    // does not write the code, it stays set from the compare and the branch is taken.
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
        "the and produced zero, so it must have cleared the condition code and the \
         branch must not have been taken; registers were {registers:?}"
    );
}

#[test]
fn a_compact_move_sign_extends_its_immediate() {
    // The immediate is signed and the reference prints it unsigned, so the decoder
    // reports the field as encoded and the sign extension happens in translation. A
    // shader loading -2 would otherwise get 65534, which is a plausible number.
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

#[test]
fn the_accumulating_compact_forms_read_their_destination() {
    // `s_addk_i32` and `s_mulk_i32` accumulate rather than assign. Treating either as a
    // plain move gives a shader computing from whatever happened to be in the register,
    // and the answer looks like a number either way.
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

#[test]
fn scalar_logic_sets_the_condition_code_from_its_result() {
    // The 32-bit logical operations set the code to whether the result is non-zero. Two
    // cases, because a translation that always set it or always cleared it would pass
    // one of them.
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
            "s_cbranch_scc0 is taken exactly when the and produced zero; registers were \
             {registers:?}"
        );
    }
}

#[test]
fn scalar_addition_sets_the_condition_code_on_signed_overflow() {
    // The arithmetic forms set the code on *signed* overflow, not on whether the result
    // is non-zero - the one place this family is not uniform. A translation that used
    // the non-zero rule here would agree whenever the sum happened to be zero and at no
    // other time.
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
        "no overflow means the code is clear and the branch is taken; registers were \
         {registers:?}"
    );
}

/// A long-form instruction of the sub-encoding with a scalar destination.
///
/// The scalar destination sits at bits 8 to 14 of the first word - the same bits the
/// other sub-encoding uses for absolute-value flags.
fn vop3b(name: &str, vdst: u32, sdst: u32, sources: [u32; 3]) -> [u32; 2] {
    [
        head(name) | (sdst << 8) | vdst,
        (sources[2] << 18) | (sources[1] << 9) | sources[0],
    ]
}

/// The operand code for the condition mask.
const VCC_CODE: u32 = 106;

#[test]
fn carry_out_reaches_the_condition_mask() {
    // Sixty-four-bit address arithmetic is built out of these, so a dropped carry gives
    // addresses that are right below four gigabytes and wrong above. Two cases, because
    // a translation that always set the carry or never set it would pass one of them.
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
        "every lane carried, so every bit of the mask should be set; registers were \
         {registers:?}"
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

#[test]
fn a_scalar_destination_is_not_read_as_absolute_value_flags() {
    // The trap this unit was written around. The two long-form sub-encodings put
    // different things in bits 8 to 14 of the first word: one has per-source
    // absolute-value flags there, the other a second destination. Reading them without
    // knowing which is which turns a carry-out register into a set of modifiers.
    //
    // `vcc` is 106, or 1101010 - so its low three bits claim "the second source is an
    // absolute value". Here the second source is negative, and an absolute value applied
    // to it would change the answer, so a translation that misread the encoding produces
    // a different number rather than the same one.
    if !device_or_skip("a_scalar_destination_is_not_read_as_absolute_value_flags") {
        return;
    }

    // v0 = 2, v1 = 0xFFFFFFFF. 2 + 0xFFFFFFFF wraps to 1 and carries. If bit 9 of the
    // first word were read as "absolute value on the second source", the second source
    // would become 0x7FFFFFFF and the sum would be 0x80000001 with no carry.
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
        "2 + 0xFFFFFFFF wraps to 1. Getting 0x80000001 means bits of the scalar \
         destination were read as absolute-value flags; registers were {registers:?}"
    );
}

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

#[test]
fn the_carry_in_form_adds_the_carry() {
    // Two additions and two carry tests. A single test would miss the case where the
    // first add did not carry and adding the carry-in did - which is exactly what this
    // asserts.
    if !device_or_skip("the_carry_in_form_adds_the_carry") {
        return;
    }

    // Set the mask to all ones so every lane has a carry in, then add 0xFFFFFFFE + 1
    // with that carry: 0xFFFFFFFE + 1 = 0xFFFFFFFF (no carry), + 1 = 0 (carry).
    let mut program = vec![
        s_mov_exec(193),
        s_mov_b64(VCC_CODE, 193),
        v_mov_code(0, 193),
        v_mov_inline(1, 1),
    ];
    // v0 = 0xFFFFFFFF, so use 0xFFFFFFFF + 0 + carry to reach the wrap.
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
        "the carry came from adding the carry-in, not from the first addition; \
         registers were {registers:?}"
    );
}

#[test]
fn the_division_sequence_is_translated_in_full() {
    // All three steps are translated now. The reference for this generation states each
    // in full, and what it leaves undefined - `Quiet`, `underflow`, `overflow`, and the
    // NaN the pre-scale emits - is IEEE-754 or unobservable downstream.
    //
    // The pre-scale was blocked longest, and not for the reason anyone expected: two of
    // its branches ask whether a quotient is subnormal, which looked like it needed the
    // host to preserve subnormals. The device this runs on does not support preserving
    // them at all. It turned out not to matter - see `exponent_is_zero` - and this test
    // exists so nothing quietly re-blocks it on the old reasoning.
    //
    // Needs no device.
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
/// These programs give every lane the same operands, so every lane reaches the same
/// branch and sets its own bit - the mask comes back as all ones rather than as one.
/// Asserting on `1` would have been asserting that the other sixty-three lanes did not
/// run, which is a different claim entirely and a false one.
const ALL_LANES: u32 = u32::MAX;

/// Exponent field `e`, mantissa zero: the float two to the power `e - 127`.
const fn power_of_two(exponent: u32) -> u32 {
    exponent << 23
}

/// `v_div_scale_f32 vDst, vcc, vS0, vDenominator, vNumerator`, run on lane zero.
///
/// Returns the scaled value and the condition mask, because the instruction produces
/// both and a test that checked only the value would miss a flag that never gets set -
/// which would leave `v_div_fmas_f32` never scaling and the division quietly wrong only
/// for the operands that needed scaling.
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

#[test]
fn the_division_pre_scale_leaves_an_ordinary_division_alone() {
    // The branch every ordinary division takes: none of them. One over two needs no
    // scaling, so the operand passes through and the flag stays clear - and if it did
    // not, every division in a shader would be scaled and then unscaled for nothing.
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

#[test]
fn the_division_pre_scale_lifts_a_tiny_numerator() {
    // The last branch: a numerator whose exponent is 23 or below is scaled up, with no
    // flag - the reciprocal is what would have lost precision, and it is not the thing
    // being scaled here.
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

#[test]
fn the_division_pre_scale_flags_a_wide_spread_and_scales_only_its_own_operand() {
    // The first branch that sets the flag, and the one that shows why the instruction
    // takes the operand to scale separately from the two it inspects: the same condition
    // scales the denominator and leaves the numerator alone, so which one moves depends
    // on which was handed in.
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

#[test]
fn the_division_pre_scale_answers_a_zero_operand_with_a_nan() {
    // The first branch of all. A zero on either side has no quotient worth scaling, so
    // the result is a NaN and the flag stays clear - the fixup replaces it afterwards.
    if !device_or_skip("the_division_pre_scale_answers_a_zero_operand_with_a_nan") {
        return;
    }

    let (value, flag) = run_scale(BITS_1, 0, BITS_1);
    assert_eq!(value, 0x7FC0_0000, "a zero denominator gives a NaN");
    assert_eq!(flag, 0, "and nothing to undo");

    let (value, _) = run_scale(BITS_1, BITS_1, 0);
    assert_eq!(value, 0x7FC0_0000, "so does a zero numerator");
}

// ---- the division sequence -------------------------------------------------------
//
// Two of the three steps are translated, and both are pure numerics, so they are worth
// executing rather than merely translating. A special-case table that emits valid SPIR-V
// and produces the wrong bits for a division by zero is exactly the failure this layer
// exists to catch.

/// Bit patterns the fixup is specified in terms of.
const BITS_QUIET_NAN: u32 = 0xFFC0_0000;
const BITS_POSITIVE_INF: u32 = 0x7F80_0000;
const BITS_NEGATIVE_INF: u32 = 0xFF80_0000;
const BITS_NEGATIVE_ZERO: u32 = 0x8000_0000;
const BITS_SIGNALLING_NAN: u32 = 0x7F80_0001;

/// `v_div_fixup_f32 vDst, vQuotient, vDenominator, vNumerator`, run on lane zero.
///
/// Loads the three operands into vector registers as raw bit patterns, so a test can
/// name an infinity or a NaN directly rather than hoping an inline constant produces one.
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

#[test]
fn the_division_fixup_replaces_the_indeterminate_forms() {
    // Zero over zero and infinity over infinity are the two the reference gives a
    // literal bit pattern for, and it is a *negative* quiet NaN - the sign is part of
    // what it specifies, so asserting on the whole word rather than "is a NaN" is the
    // point.
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

#[test]
fn the_division_fixup_gives_zero_and_infinity_their_signs() {
    // A division by zero is an infinity and a division of zero is a zero, and both carry
    // the sign of the operands rather than the sign of whatever the reciprocal sequence
    // computed. Getting this wrong renders a shader that is correct except for being
    // inside out.
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

#[test]
fn the_division_fixup_propagates_a_nan_quietly() {
    // A signalling NaN in either operand comes out quiet, and the *numerator* wins when
    // both are NaNs - which is the order the reference gives and the opposite of the
    // order the conditions are written in here.
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

#[test]
fn the_division_fixup_leaves_an_ordinary_quotient_alone() {
    // The default branch, and the one that runs for every division that is not a special
    // case at all. The magnitude is the quotient's; the sign is the operands'.
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

#[test]
fn the_division_multiply_add_scales_only_when_the_mask_says_so() {
    // `v_div_fmas_f32` reads the condition mask *implicitly* - it is not one of its
    // operands - and multiplies its result by two to the thirty-second where the mask
    // is set. That is how the pre-scale earlier in the sequence gets undone.
    //
    // Both halves are checked in one program: lane zero has the mask bit set and lane
    // one does not, so a translation that ignored the mask entirely would have to be
    // wrong about one of them.
    if !device_or_skip("the_division_multiply_add_scales_only_when_the_mask_says_so") {
        return;
    }

    // 2 * 1 + 1 = 3, which is exact, so the scaled result is exactly 3 * 2^32 and both
    // answers can be asserted to the bit.
    //
    // The mask is written with `s_mov_b64`, because that is the instruction that writes
    // a lane mask - `vcc` decodes as a named operand rather than a scalar register, so
    // `s_mov_b32 vcc, ...` is refused, correctly.
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
/// 1.5. Written out rather than computed, so the test states its own expectation.
const BITS_3_SCALED: u32 = 0x5040_0000;

// ---- thirty-two-lane shaders -----------------------------------------------------
//
// This generation runs a shader at either width, chosen when it is compiled, and the
// encodings are identical either way. What differs is which mask instructions a shader
// uses: a 32-lane shader's mask fits in one register, so it manipulates it with the
// 32-bit scalar forms rather than the 64-bit ones.
//
// No capture contains one yet, so these are built the way the reference says a compiler
// would build them. That is stated rather than hidden: what is verified here is that the
// translator does the right thing *given* such a shader, not that this is what one looks
// like in the wild.

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

/// A scalar register's own number is its operand code - the numbering starts there.
///
/// The operand code for `exec_lo`, the low half of the execution mask.
const EXEC_LO_CODE: u32 = 126;

#[test]
fn a_thirty_two_lane_shader_masks_with_the_thirty_two_bit_forms() {
    // The whole of what makes a narrow shader different: it clears its execution mask
    // with `s_mov_b32 exec_lo, 0` rather than `s_mov_b64 exec, 0`.
    //
    // Translated as an ordinary scalar move that lands in the register file, this shader
    // would run with every lane active and the vector write would happen - so the
    // assertion below fails loudly rather than subtly if the 32-bit form is not
    // recognised as touching the mask.
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

#[test]
fn a_thirty_two_lane_shader_narrows_its_mask_with_scalar_logic() {
    // `s_and_b32 exec_lo, exec_lo, sN` is how a narrow shader keeps some lanes and drops
    // the rest, and it is the form real control flow is built from. Keeping only lane
    // zero means the observed register - which reads lane zero - still updates, while a
    // mask of zero would stop it.
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

#[test]
fn the_two_widths_are_the_same_shader_with_different_lane_counts() {
    // A shader that touches no mask must produce the same observed registers at either
    // width - the observation window reads lane zero, and lane zero does the same work
    // whichever wavefront it is part of.
    //
    // The point is that width changes *how many lanes run*, not what any one of them
    // computes. A translator that got that backwards would show up here.
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

// ---- subgroup fidelity -----------------------------------------------------------

/// Runs a program with the invocations of a subgroup as its lanes.
///
/// Returns `None` when the device's subgroup is not as wide as the guest's wavefront,
/// because the module the translator produced says which width it needs and running it on
/// a device that does not match would be measuring the wrong thing.
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

#[test]
fn subgroup_fidelity_produces_a_module_a_driver_accepts() {
    // The first thing worth knowing about a new fidelity level: does the module load at
    // all. It declares two capabilities, an input built-in and a group operation, any of
    // which the driver will reject outright if they are malformed - which is a better
    // failure than a wrong answer and the reason this test is separate from the one
    // below.
    if !device_or_skip("subgroup_fidelity_produces_a_module_a_driver_accepts") {
        return;
    }

    let program = [v_mov_inline(0, 5), s_endpgm()];
    let Some(registers) = run_subgroup(Width::Wave32, &program) else {
        return;
    };
    assert_eq!(vector(&registers, 0), 5, "an unmasked write should land");
}

#[test]
fn subgroup_fidelity_masks_with_a_ballot() {
    // What the level is *for*. The per-lane model refuses masks outright; this one
    // materialises them from the subgroup, so a shader that clears its execution mask has
    // to suppress the write that follows.
    //
    // A translation that ignored the mask would return 9 here, and one that read lane
    // zero's bit for every invocation would still return 7 - so this distinguishes the
    // level working from it merely not crashing.
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

#[test]
fn the_accumulating_multiply_add_reads_its_own_destination() {
    // `v_fmac_f32` is the short form of a fused multiply-add whose third operand is the
    // destination: `D = S0 * S1 + D`. It is the only short-form instruction that reads
    // the register it writes, so a translation that treated it like the others would
    // compute `S0 * S1` and drop the accumulation - correct for a destination that
    // happened to be zero, and wrong everywhere else.
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

// ---- untyped buffer access -------------------------------------------------------
//
// A buffer access addresses memory through a resource constant held in four scalar
// registers, so the descriptor has to be built in the shader before the access runs.
// These build it with literal moves, which is exactly how the reference says a shader may
// do it: "these constants are fetched from memory using scalar memory reads prior to
// executing VM instructions, but these constants also can be generated within the shader."

/// `buffer_load_dword` / `buffer_store_dword` with the address modifiers in bits 12-13.
///
/// `vaddr` is a **plain vector register number**, not a code in the shared source
/// numbering. Passing `vgpr_code(0)` here puts 256 into an eight-bit field, whose spare
/// bit lands in the data field next door - which quietly moved a load's destination from
/// v2 to v3 and looked like the load returning zero.
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

/// `BUF_FMT_32_FLOAT`, measured rather than looked up - see D203.
const FMT_32_FLOAT: u32 = 22;
/// `BUF_FMT_32_32_32_32_FLOAT`.
const FMT_32X4_FLOAT: u32 = 77;
/// `BUF_FMT_8_UNORM`: one component, eight bits, and a conversion nothing here performs.
const FMT_8_UNORM: u32 = 1;

#[test]
fn a_typed_buffer_format_needing_conversion_is_refused_by_name() {
    // The gate that matters more than the feature.
    //
    // A narrow component has to be extracted from within a word and converted - an
    // eight-bit normalised value becomes a float by dividing by 255. None of that is
    // written. Translating it as though the word were the value produces a shader that
    // compiles, runs, draws, and is wrong only in the pixels, which is the one failure
    // this project has no cheap way to notice.
    //
    // So it is refused, and this holds that refusal. It needs no device: the refusal
    // happens during translation, before anything is submitted.
    let mut program = describe_buffer(4, 256);
    program.extend(mtbuf(
        "tbuffer_load_format_x",
        2,
        0,
        4,
        INLINE_0,
        FMT_8_UNORM,
        OFFEN,
    ));
    program.push(s_endpgm());

    let error = translate_program(Fidelity::Wavefront, &program)
        .expect_err("a format needing conversion must be refused");
    let text = error.to_string();
    assert!(
        text.contains("conversion"),
        "the refusal should say why, so a reader knows it is a gap rather than a bug: \
         {text}"
    );
}

#[test]
fn a_typed_buffer_access_whose_format_disagrees_with_its_channels_is_refused() {
    // The hardware allows this and what it does then - padding the missing channels, or
    // discarding the extra - was never measured here. Guessing costs the ability to trust
    // every shader that used one, so it is refused until somebody measures it.
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

#[test]
fn a_reserved_format_code_is_refused_rather_than_approximated() {
    // Code 90 has no meaning: the reference has no name for it and prints it back as a
    // bare number. A shader carrying one is wrong, and the nearest real format would
    // render.
    let mut program = describe_buffer(4, 256);
    program.extend(mtbuf("tbuffer_load_format_x", 2, 0, 4, INLINE_0, 90, OFFEN));
    program.push(s_endpgm());

    let error = translate_program(Fidelity::Wavefront, &program)
        .expect_err("a reserved format code must be refused");
    assert!(error.to_string().contains("no meaning"), "{error}");
}

#[test]
fn a_four_channel_typed_access_moves_four_consecutive_words() {
    // What a multi-channel access actually adds: consecutive dwords at consecutive
    // addresses, in consecutive registers. Stored from v1..v4 and read back into v5..v8.
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

    // The load's destination overlaps the store's last source, which is deliberate: only
    // v0 to v7 are copied out, so four sources and four destinations do not both fit
    // below that line. The store runs first, so memory holds what was stored, and the
    // comparison is against *memory* rather than against the source registers - which is
    // the stronger check anyway. A translation that wrote every channel to the same
    // address would pass a register-to-register comparison and fail this.
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

#[test]
fn a_single_channel_typed_access_is_an_untyped_one_with_a_format() {
    // The claim that justifies sharing the body: with a plain word format and one
    // channel, a typed access must do exactly what the untyped one does. If these ever
    // disagree, the addressing has been duplicated somewhere it should not have been.
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

#[test]
fn a_buffer_store_and_load_round_trip_through_guest_memory() {
    // The whole path: build a descriptor, store through it, read it back. Addressing is
    // base + soffset + inst_offset + voffset, and this exercises the voffset term.
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

#[test]
fn a_buffer_access_past_the_record_count_reads_zero_and_drops_its_write() {
    // The reference is explicit: out of range, "writes are ignored (dropped) and reads
    // return zero". Both halves are checked, because a bounds check that only suppresses
    // one of them is worse than none - a dropped write with a live read still returns
    // whatever was there before, which looks like data.
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

#[test]
fn a_buffer_access_inside_the_record_count_is_not_suppressed() {
    // The other side of the same check, and the one that would pass by accident if the
    // bounds test were inverted or always true. Eight bytes, accessed at byte four: the
    // last four-byte access that fits.
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

#[test]
fn a_swizzled_descriptor_is_refused_by_reading_zero() {
    // A translated shader cannot refuse at run time, so a descriptor asking for
    // addressing this does not do is forced out of bounds instead. That makes a swizzled
    // buffer read zero - visibly and consistently wrong - rather than read real-looking
    // data from the wrong offset.
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

#[test]
fn an_indexed_buffer_access_multiplies_the_index_by_the_stride() {
    // The other half of the addressing equation: `Stride * Vindex`. A structured buffer
    // addresses by record, and the stride comes from the descriptor rather than the
    // instruction - so a translation that ignored it would put every record at offset
    // zero and every write would land on the first one.
    if !device_or_skip("an_indexed_buffer_access_multiplies_the_index_by_the_stride") {
        return;
    }

    // Sixteen-byte records, four of them, bounds mode one - the raw check, `index >=
    // records`. Record two therefore starts at byte 32, which is word eight.
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

#[test]
fn a_store_past_the_memory_window_does_not_wrap_onto_the_start() {
    // The window is masked to keep the buffer index legal, and masking is not clamping:
    // before this, the word after the last one landed on the *first*, and everything
    // about it looked fine - the shader ran, memory changed, and the change was somewhere
    // the guest never asked for.
    //
    // A guest that overruns a buffer is an ordinary bug. A translator that turns the
    // overrun into a corrupted, plausible-looking start of memory is a much worse one,
    // because it makes the guest's bug unrecognisable.
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

#[test]
fn a_load_past_the_memory_window_reads_zero() {
    // The other half. Reading past the end used to alias onto the start, so a shader that
    // overran would read its own earlier writes back as though they were the data it
    // asked for.
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

#[test]
fn the_sub_encoding_is_derived_from_the_solved_operands_not_a_list() {
    // The two long-form sub-encodings differ in what bits 8-14 of the first word hold: a
    // second scalar destination, or per-source absolute-value flags. Read the wrong way,
    // `vcc` as a carry destination presents as "the second source is an absolute value",
    // and an integer addition silently loses the sign of an operand.
    //
    // This used to be a hand-written list, and nothing enforced the pairing - an opcode
    // added to SUPPORTED without also being added there read its modifiers from the wrong
    // bits. It is derived from the probe data now, so what this pins is that the *data*
    // still distinguishes them. If the solver ever stopped recording that operand, the
    // classification would collapse to "none of them have one" and every carry
    // instruction would quietly start reading modifiers from its own destination.
    //
    // Needs no device.
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

#[test]
fn the_corpus_agrees_about_hidden_side_effects() {
    // D129's difficulty: whether an instruction writes the condition code is invisible in
    // the encoding, in the operand layout, and in every test that checks destinations. It
    // has to be read out of the published instruction set, and nothing here could confirm
    // what was read.
    //
    // Something can. A compiler emitting a shader places instructions *between* one that
    // sets the condition code and one that branches on it - and every instruction it puts
    // there is one it believes does not write it, or the shader it produced would be
    // wrong. That is an observation about real compiled output rather than a restatement
    // of the document.
    //
    // So the fixtures are mined for those windows. Each is a claim about a real
    // instruction, checked against what this translator believes.
    //
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
            // Walk back to whatever last set it. Everything in between is evidence.
            let Some(setter) = names[..at].iter().rposition(|n| writes_condition_code(n)) else {
                continue;
            };
            windows += 1;
            for between in &names[setter + 1..at] {
                assert!(
                    !writes_condition_code(between),
                    "{} places {between} between {} and {name}, so the compiler believes                      it does not write the condition code - and this translator believes                      it does. One of them is wrong about the hardware, and it is not the                      compiler",
                    path.display(),
                    names[setter],
                );
                instructions_cleared.insert(between.clone());
            }
        }
    }

    // The corpus is small and this is thin evidence; it grows with the corpus. Asserting
    // that at least one window exists keeps the test honest - a fixture set that stopped
    // containing any would make this pass by examining nothing.
    assert!(
        windows > 0,
        "no compiled shader in the corpus branches on the condition code, so this test          checked nothing"
    );
    println!(
        "[hidden side effects] {windows} window(s); confirmed not to write the condition          code: {instructions_cleared:?}"
    );
}

#[test]
fn the_dispatch_loop_costs_a_measurable_amount_on_a_single_block_shader() {
    // D110 emits the dispatch loop for every shader, including ones with a single block
    // where it is pure overhead, and defers collapsing that "until there is something to
    // measure". Nothing measured it, which is how a deferral becomes permanent.
    //
    // This is the measurement. It does not argue for collapsing the loop - the reason not
    // to is that a second emission path is a second thing to keep correct, and a retarget
    // has already shown what that costs. It puts a number on what is being traded, so the
    // question can be settled by arithmetic rather than by whoever feels strongly.
    //
    // Asserted loosely on purpose: that the overhead exists and is bounded. A tight
    // assertion would fail on every unrelated change to emission and teach people to
    // update it without reading it.
    //
    // Needs no device.
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

    // The same work with nothing at all in it, to separate the module's fixed cost - the
    // types, the register file, the buffers - from what the body adds.
    let empty = translate_words(&[s_endpgm()]);

    let body = single.saturating_sub(empty);
    println!("[dispatch loop] empty module {empty} words, one block {single}, body {body}");

    assert!(
        single > empty,
        "a shader with a move in it should emit more than one with nothing"
    );

    // What the numbers say, and it is not what the decision expected: the loop is not
    // where the cost is. A module's fixed preamble - the types, two register files, two
    // storage buffers - dwarfs both the loop's scaffolding and the body. Collapsing the
    // loop would buy a fraction of one percent and cost a second emission path.
    assert!(
        empty > body * 10,
        "the fixed preamble ({empty} words) should dominate a single instruction ({body})          by a wide margin - if it no longer does, the preamble has been slimmed and the          loop's share of the cost is worth re-examining"
    );

    // Structural rather than by size, because size is the wrong instrument: a collapsed
    // loop would barely move the total, so a size assertion would not notice. This
    // notices, and it is what should fail if anyone takes D110's second path.
    let bytes: Vec<u8> = [s_endpgm()].iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, table, &operands);
    let module = translate(&decoded, table, Strategy::default())
        .expect("translates")
        .module;
    let has = |opcode: u16| module.iter().any(|word| (*word & 0xFFFF) as u16 == opcode);
    assert!(
        has(op::LOOP_MERGE) && has(op::SWITCH),
        "a single-block shader still goes through the dispatch loop. If that has changed,          it is a deliberate second emission path and D110 needs to say so"
    );
}

#[test]
fn the_condition_code_behaves_the_same_in_both_models() {
    // D115 says the condition code is "state both models hold" - one bit for the whole
    // wavefront, so the per-lane model represents it exactly. The half that was tested was
    // that a shader using it is not pushed onto the slow model. The half that was not is
    // the claim itself: that when a shader *is* on the slow model, for some other reason,
    // the condition code still behaves identically.
    //
    // Both models running the same program and disagreeing is the only way that claim
    // fails, and nothing was asking.
    if !device_or_skip("the_condition_code_behaves_the_same_in_both_models") {
        return;
    }

    // s0 = 5; if (s0 < 1) skip; s1 = 42. The compare is false, so the branch is not
    // taken and s1 is written - and a model that got the condition code wrong would skip.
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
        "the condition code is one bit for the whole wavefront, so both models must agree          about it exactly"
    );
}

#[test]
fn a_shader_mixing_condition_code_and_mask_branches_takes_the_model_with_a_mask() {
    // The case D115 said to re-check "once a real shader mixes condition-code and mask
    // branches in one block". One has, and it is a compiled fixture rather than something
    // written to make the point: `control` contains one condition-code branch and two mask
    // branches.
    //
    // The mask branches decide it, and that is right - a mask cannot be represented
    // without one, while the condition code can be represented in either. Mixing does not
    // weaken the rule that a condition-code-only shader stays on the fast model; it just
    // means something else in the same shader outvoted it.
    //
    // Needs no device.
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
        "a shader containing mask branches needs the model that has a mask, whatever else          it also contains"
    );
}

/// A flat **store** of any width: address at bit 0, data at bit 8.
fn flat_store(name: &str, vaddr: u32, data: u32) -> [u32; 2] {
    [head(name), vaddr | (data << 8) | (0x7F << 16)]
}

/// A flat **load** of any width: address at bit 0, destination at bit 24.
///
/// A load and a store do not share an operand layout - the destination of one is at bit
/// 24 and the data of the other at bit 8 - which is the whole reason operand layouts are
/// per opcode rather than per family (D096). Writing one helper for both put the
/// destination where nothing reads it, and the test that noticed was the one asserting a
/// *refusal*: it translated happily because the register it was meant to overflow was
/// never decoded.
fn flat_load(name: &str, vaddr: u32, destination: u32) -> [u32; 2] {
    [head(name), vaddr | (0x7F << 16) | (destination << 24)]
}

/// `s_wqm_b64 s[dst:dst+1], <source code>`.
fn s_wqm(dst: u32, source_code: u32) -> u32 {
    head("s_wqm_b64") | (dst << 16) | source_code
}

#[test]
fn a_wide_flat_store_writes_consecutive_words() {
    // What separates `global_store_dwordx4` from four single stores: one address, four
    // source registers, landing in order. A translation that stored the same register
    // four times, or stepped the address by one instead of by a word, passes every
    // single-word test and fails here.
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

    // The load's destination is the first operand and its address the second, so this
    // reads word 4 onwards into v0..v3 - which also overwrites the address register,
    // deliberately, since the address is consumed before the first write lands.
    let (registers, _) = run_memory(Fidelity::Lane, &program);
    for (offset, expected) in [11u32, 22, 33, 44].iter().enumerate() {
        assert_eq!(
            vector(&registers, offset),
            *expected,
            "v{offset} should hold {expected}; registers were {registers:?}"
        );
    }
}

#[test]
fn a_wide_access_past_the_register_file_is_refused() {
    // `global_load_dwordx4` into v253 would write four registers where three exist. The
    // file is an array with nothing past it, so this would be a write off the end -
    // refused rather than truncated, because a load quietly filling half its
    // destinations is a shader computing the wrong thing while appearing to work.
    //
    // Needs no device.
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

#[test]
fn whole_quad_mode_sets_every_bit_of_a_group_that_had_any() {
    // A fragment shader uses this so a derivative computed across a quad has all four
    // pixels live even where only one is covered. Getting it wrong is invisible until
    // something samples a texture with a gradient.
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

    // A bit in the second group, and nothing in the first: 0b1_0000 becomes 0b1111_0000.
    // This is the case that catches a fold spilling across a group boundary.
    let program = [s_mov_b64(0, 128 + 16), s_wqm(2, 0), s_endpgm()];
    let (registers, _) = run_memory(Fidelity::Wavefront, &program);
    assert_eq!(
        scalar(&registers, 2),
        0b1111_0000,
        "a set bit must light its own quad and no other; registers were {registers:?}"
    );
}

#[test]
fn whole_quad_mode_leaves_an_empty_mask_empty() {
    // The other direction. A fold that ored in a constant, or a spread that ignored its
    // input, would light every quad here.
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

/// `ds_write_b32 vAddr, vData offset:N`.
///
/// The opcode sits at bit 17, not 18 - the shift differs from the flat families and
/// getting it wrong produces a word that decodes as a different local-share instruction
/// entirely, which reports as a missing operand layout rather than as a wrong opcode.
fn ds_write(vaddr: u32, data: u32, offset: u32) -> [u32; 2] {
    [head("ds_write_b32") | offset, vaddr | (data << 8)]
}

/// `ds_read_b32 vDst, vAddr offset:N`.
///
/// The destination is at bit 24 and the address at bit 0 - a read and a write do not
/// share a layout, which is the mistake this file has now made twice with flat memory.
fn ds_read(dst: u32, vaddr: u32, offset: u32) -> [u32; 2] {
    [head("ds_read_b32") | offset, vaddr | (dst << 24)]
}

#[test]
fn a_value_written_to_the_local_share_can_be_read_back() {
    // Storage the lanes of a wavefront exchange values in. Write then read through the
    // same address, so it fails if either half computes its address differently - the
    // mistake most likely to survive a test that only did one of them.
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

#[test]
fn the_local_share_offset_is_not_ignored() {
    // The offset was invisible until it was probed for: the reference omits it when it
    // is zero, and every earlier probe used the zero form, so the solved layout had no
    // slot for it at all. A translator built on that would ignore every offset a
    // compiler emitted and read the wrong word - silently, because the address register
    // is still valid.
    //
    // Two values at addresses eight bytes apart, read back with an offset that must
    // reach the second.
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

#[test]
fn a_masked_write_does_not_reach_the_local_share() {
    // Sharper than the same rule for guest memory: another lane of this same wavefront
    // will read this word, so a suppressed write that lands anyway corrupts a value a
    // different lane is about to use.
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

#[test]
fn the_lane_model_refuses_the_local_share() {
    // One lane per invocation means no lanes to share between: each would get its own
    // copy and read back only what it wrote itself. That runs, and is wrong.
    //
    // Needs no device.
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

    // And Auto must route it to the model that has one.
    let translated = translate(&decoded, &table, Strategy::default()).expect("auto");
    assert_eq!(translated.fidelity, Fidelity::Wavefront);
}

#[test]
fn the_long_form_subtract_and_reverse_subtract_are_not_swapped() {
    // These are opcodes 258 and 259. The supported list said 259 and 260, taken from the
    // short form's ordering rather than read off the solved operand table - so every
    // long-form reverse-subtract computed `a - b` where the guest means `b - a`, and the
    // instruction one further along was translated as something it is not.
    //
    // Nothing caught it: the short-form test uses the short form, and the long form had
    // no test of its own. The generated-program comparison found it indirectly, by
    // producing an opcode with no operand layout.
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
        "reverse-subtract takes them the other way round, so this is -1.0; registers \
         were {registers:?}"
    );
}

#[test]
fn every_supported_name_exists_on_this_target() {
    // The test a retarget runs into first, and the reason the supported list names
    // instructions rather than numbering them.
    //
    // Opcode numbers are a property of one architecture generation and most of them move
    // between generations - the same arithmetic at a different number, in a family whose
    // identifying bits also changed. A list of numbers pointed at another generation does
    // not fail. It binds silently to whichever instructions happen to occupy those
    // numbers, and the first sign is a wrong pixel.
    //
    // Names mostly survive. The ones that do not - one generation's `v_add_u32` is
    // another's `v_add_nc_u32` - arrive here as a list of exactly what needs attention.
    //
    // Needs no device.
    use orbistoun_translate::model::unresolved;

    let table = EncodingTable::builtin().expect("encodings");
    let missing = unresolved(&table);
    assert!(
        missing.is_empty(),
        "the translator understands {} instruction(s) this target does not have under \
         those names: {missing:?}. Either the tables were generated for a different \
         generation, or these were renamed - both are real and both need a decision \
         rather than a silent rebinding",
        missing.len()
    );
}
