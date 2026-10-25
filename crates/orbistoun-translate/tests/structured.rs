//! Control flow laid out as structure runs as the dispatch loop runs it (D972).
//!
//! Each program is translated both ways - nested selections and loops, and the switch on a program
//! counter inside one loop - and run on the device; the registers must agree with each other and
//! with what the program computes. Where `spirv-val` is on the path it checks the structured module
//! too, because a driver can accept a module the structured rules refuse.

use orbistoun_gpu_vulkan::{Availability, dispatch, probe};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_spirv::op;
use orbistoun_translate::predicated::MEMORY_WORDS;
use orbistoun_translate::structured::with_shape;
use orbistoun_translate::{Fidelity, Strategy, Width, translate};

/// Registers copied out of each file.
const PER_FILE: usize = 8;
const OBSERVED: usize = PER_FILE * 2;

fn encodings() -> &'static EncodingTable {
    static TABLE: std::sync::OnceLock<EncodingTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| EncodingTable::builtin().expect("encodings"))
}

/// The first word of an instruction, from its name.
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

/// `s_mov_b32 sN, <inline constant>`.
fn s_mov(dst: u32, constant: u32) -> u32 {
    head("s_mov_b32") | (dst << 16) | (128 + constant)
}

/// A SOP2 instruction on scalar registers or inline constants, by source code.
fn sop2(name: &str, dst: u32, first: u32, second: u32) -> u32 {
    head(name) | (dst << 16) | (second << 8) | first
}

/// `s_add_u32 sN, sN, <constant>`.
fn s_inc(register: u32, by: u32) -> u32 {
    sop2("s_add_u32", register, register, 128 + by)
}

/// A SOPC compare: the second source at bit 8, the first in the low byte.
fn s_cmp(name: &str, first: u32, second: u32) -> u32 {
    head(name) | (second << 8) | first
}

/// The code of an inline constant.
const fn constant(value: u32) -> u32 {
    128 + value
}

/// A SOPP branch to the instruction `to`, from the one at `at`, every instruction one word.
fn branch(name: &str, at: usize, to: usize) -> u32 {
    let offset = i16::try_from(to as isize - (at as isize + 1)).expect("a short branch");
    head(name) | u32::from(offset as u16)
}

fn s_endpgm() -> u32 {
    head("s_endpgm")
}

fn device_or_skip(test: &str) -> bool {
    match probe() {
        Availability::Available { .. } => true,
        Availability::Unavailable { reason } => {
            println!("!! SKIPPED: {test}: no Vulkan device: {reason}");
            false
        }
    }
}

/// The module `words` translates to, at `fidelity`, laid out as structure or not.
fn module(words: &[u32], fidelity: Fidelity, structured: bool) -> Vec<u32> {
    let table = encodings();
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, table, &operands);
    assert!(decoded.is_trustworthy(), "the program must decode cleanly");
    with_shape(structured, || {
        translate(
            &decoded,
            table,
            Strategy::Predicated {
                fidelity,
                width: Width::default(),
            },
        )
    })
    .unwrap_or_else(|e| panic!("translate: {e}"))
    .module
}

/// Whether the module holds an instruction with `opcode`, walking its instructions.
fn has(module: &[u32], opcode: u16) -> bool {
    let mut at = 5;
    while at < module.len() {
        let word = module[at];
        if (word & 0xFFFF) as u16 == opcode {
            return true;
        }
        at += (word >> 16).max(1) as usize;
    }
    false
}

/// Runs `spirv-val` over `module` where it is on the path.
fn validate(module: &[u32], name: &str) {
    let path = std::env::temp_dir().join(format!(
        "orbistoun-structured-{name}-{}.spv",
        std::process::id()
    ));
    let bytes: Vec<u8> = module.iter().flat_map(|w| w.to_le_bytes()).collect();
    std::fs::write(&path, bytes).expect("the module was written");
    let ran = std::process::Command::new("spirv-val")
        .args(["--target-env", "vulkan1.2"])
        .arg(&path)
        .output();
    let _ = std::fs::remove_file(&path);
    match ran {
        Ok(output) => assert!(
            output.status.success(),
            "spirv-val refused the structured module for {name}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(error) => println!("[{name}] spirv-val not run: {error}"),
    }
}

/// Runs `words` both ways at both fidelities and answers the registers, checking that every run
/// agrees and that the structured module is structure, not a dispatch loop, where `nests`.
fn run_both(name: &str, words: &[u32], nests: bool) -> Vec<u32> {
    let mut seen: Option<Vec<u32>> = None;
    for fidelity in [Fidelity::Lane, Fidelity::Wavefront] {
        let structured = module(words, fidelity, true);
        let dispatched = module(words, fidelity, false);
        validate(&structured, name);
        assert!(has(&dispatched, op::SWITCH), "{name}: the dispatch loop");
        assert_eq!(
            has(&structured, op::SWITCH),
            !nests,
            "{name}: a program whose flow nests has no switch, one whose flow does not keeps it"
        );
        for module in [&structured, &dispatched] {
            let registers = dispatch(module, OBSERVED, MEMORY_WORDS as usize, [1, 1, 1])
                .expect("dispatch")
                .observed;
            if let Some(seen) = &seen {
                assert_eq!(
                    &registers, seen,
                    "{name} at {fidelity}: the shapes disagree"
                );
            }
            seen = Some(registers);
        }
    }
    seen.expect("a run")
}

fn scalar(registers: &[u32], n: usize) -> u32 {
    registers[PER_FILE + n]
}

/// A branch over an else arm and one into it meet after both: a selection.
#[test]
fn an_if_else_runs_one_arm() {
    if !device_or_skip("an_if_else_runs_one_arm") {
        return;
    }
    for (value, arm) in [(1, 2), (5, 1)] {
        let program = [
            s_mov(0, value),
            s_cmp("s_cmp_lt_u32", 0, constant(3)),
            branch("s_cbranch_scc1", 2, 5),
            s_mov(1, 1),
            branch("s_branch", 4, 6),
            s_mov(1, 2),
            sop2("s_add_u32", 2, 1, constant(10)),
            s_endpgm(),
        ];
        let registers = run_both("an_if_else_runs_one_arm", &program, true);
        assert_eq!(scalar(&registers, 1), arm, "{registers:?}");
        assert_eq!(scalar(&registers, 2), arm + 10, "{registers:?}");
    }
}

/// A loop inside a loop: each a loop of its own, the inner one's exit the outer one's latch.
#[test]
fn nested_loops_run_every_iteration() {
    if !device_or_skip("nested_loops_run_every_iteration") {
        return;
    }
    let program = [
        s_mov(0, 0),
        s_mov(2, 0),
        s_mov(1, 0), // 2: the outer loop
        s_inc(2, 1), // 3: the inner loop
        s_inc(1, 1),
        s_cmp("s_cmp_lt_u32", 1, constant(4)),
        branch("s_cbranch_scc1", 6, 3),
        s_inc(0, 1),
        s_cmp("s_cmp_lt_u32", 0, constant(3)),
        branch("s_cbranch_scc1", 9, 2),
        s_endpgm(),
    ];
    let registers = run_both("nested_loops_run_every_iteration", &program, true);
    assert_eq!(scalar(&registers, 2), 12, "{registers:?}");
    assert_eq!(scalar(&registers, 0), 3, "{registers:?}");
}

/// A loop left for the program's end from its middle, rather than at its foot: the end is routed
/// from the loop's merge, past the code after the loop.
#[test]
fn a_loop_left_for_the_end_skips_what_follows_it() {
    if !device_or_skip("a_loop_left_for_the_end_skips_what_follows_it") {
        return;
    }
    for (stop, expected) in [(5, [5, 0, 4, 9]), (50, [10, 7, 10, 0])] {
        let program = [
            s_mov(0, 0),
            s_mov(3, 0),
            s_inc(0, 1), // 2: the loop
            s_cmp("s_cmp_eq_u32", 0, constant(stop)),
            branch("s_cbranch_scc1", 4, 10),
            s_inc(3, 1),
            s_cmp("s_cmp_lt_u32", 0, constant(10)),
            branch("s_cbranch_scc1", 7, 2),
            s_mov(1, 7),
            s_endpgm(),
            s_mov(4, 9), // 10: the early end
            s_endpgm(),
        ];
        let registers = run_both(
            "a_loop_left_for_the_end_skips_what_follows_it",
            &program,
            true,
        );
        let got = [0, 1, 3, 4].map(|n| scalar(&registers, n));
        assert_eq!(got, expected, "{registers:?}");
    }
}

/// An inner loop that continues the outer one directly: the counter carries the jump out.
#[test]
fn an_inner_loop_continues_the_outer_one() {
    if !device_or_skip("an_inner_loop_continues_the_outer_one") {
        return;
    }
    let program = [
        s_mov(0, 0),
        s_mov(2, 0),
        s_cmp("s_cmp_lt_u32", 0, constant(4)), // 2: the outer loop
        branch("s_cbranch_scc0", 3, 11),
        s_inc(0, 1),
        s_mov(1, 0),
        s_inc(1, 1), // 6: the inner loop
        s_inc(2, 1),
        s_cmp("s_cmp_eq_u32", 1, 0),
        branch("s_cbranch_scc1", 9, 2),
        branch("s_branch", 10, 6),
        s_mov(3, 5), // 11
        s_endpgm(),
    ];
    let registers = run_both("an_inner_loop_continues_the_outer_one", &program, true);
    let got = [0, 2, 3].map(|n| scalar(&registers, n));
    assert_eq!(got, [4, 10, 5], "{registers:?}");
}

/// A cycle entered at two blocks has no header: it keeps the dispatch loop, and runs the same.
#[test]
fn an_irreducible_cycle_keeps_the_dispatch_loop() {
    if !device_or_skip("an_irreducible_cycle_keeps_the_dispatch_loop") {
        return;
    }
    let program = [
        s_mov(0, 0),
        s_cmp("s_cmp_eq_u32", 0, constant(0)),
        branch("s_cbranch_scc1", 2, 7),
        s_inc(0, 1), // 3: one entry
        s_cmp("s_cmp_lt_u32", 0, constant(6)),
        branch("s_cbranch_scc0", 5, 9),
        s_inc(1, 1),
        s_inc(0, 2), // 7: the other
        branch("s_branch", 8, 3),
        s_endpgm(), // 9
    ];
    let registers = run_both(
        "an_irreducible_cycle_keeps_the_dispatch_loop",
        &program,
        false,
    );
    let got = [0, 1].map(|n| scalar(&registers, n));
    assert_eq!(got, [6, 1], "{registers:?}");
}

/// Branches that each skip to the program's end, as a compiler leaves an early return: nested
/// selections that all merge at the last block.
#[test]
fn early_returns_nest_as_selections() {
    if !device_or_skip("early_returns_nest_as_selections") {
        return;
    }
    for (value, expected) in [(0, [1, 0, 0]), (1, [1, 2, 0]), (2, [1, 2, 3])] {
        let program = [
            s_mov(5, value),
            s_mov(0, 1),
            s_cmp("s_cmp_lt_u32", 5, constant(1)),
            branch("s_cbranch_scc1", 3, 9),
            s_mov(1, 2),
            s_cmp("s_cmp_lt_u32", 5, constant(2)),
            branch("s_cbranch_scc1", 6, 9),
            s_mov(2, 3),
            branch("s_branch", 8, 9),
            s_endpgm(), // 9
        ];
        let registers = run_both("early_returns_nest_as_selections", &program, true);
        let got = [0, 1, 2].map(|n| scalar(&registers, n));
        assert_eq!(got, expected, "{registers:?}");
    }
}

/// An arm that skips past where the arms meet, as a compiler leaves a `break` out of a nested
/// condition: the skip escapes through the selection's merge, and the code between the join and
/// the skip's target is held under a test of the counter. Run in a loop, so a skip in one pass
/// must not skip the join in the next.
#[test]
fn an_arm_skipping_past_the_join_skips_only_once() {
    if !device_or_skip("an_arm_skipping_past_the_join_skips_only_once") {
        return;
    }
    let program = [
        s_mov(7, 0),
        s_cmp("s_cmp_eq_u32", 7, constant(0)), // 1: the loop
        branch("s_cbranch_scc1", 2, 8),
        s_inc(1, 1),
        s_cmp("s_cmp_eq_u32", 7, constant(1)),
        branch("s_cbranch_scc1", 5, 10),
        s_inc(2, 1),
        branch("s_branch", 7, 9),
        s_inc(3, 1), // 8: the other arm
        s_inc(4, 1), // 9: the join
        s_inc(6, 1), // 10: past it
        s_inc(7, 1),
        s_cmp("s_cmp_lt_u32", 7, constant(3)),
        branch("s_cbranch_scc1", 13, 1),
        s_endpgm(),
    ];
    let registers = run_both(
        "an_arm_skipping_past_the_join_skips_only_once",
        &program,
        true,
    );
    let got = [1, 2, 3, 4, 6].map(|n| scalar(&registers, n));
    assert_eq!(got, [2, 1, 1, 2, 3], "{registers:?}");
}

/// A program with a block after its end that nothing reaches still translates, structured.
#[test]
fn an_unreachable_block_is_left_out() {
    if !device_or_skip("an_unreachable_block_is_left_out") {
        return;
    }
    let program = [
        s_mov(0, 4),
        branch("s_branch", 1, 3),
        s_mov(0, 9), // never runs
        s_endpgm(),
    ];
    let registers = run_both("an_unreachable_block_is_left_out", &program, true);
    assert_eq!(scalar(&registers, 0), 4, "{registers:?}");
}
