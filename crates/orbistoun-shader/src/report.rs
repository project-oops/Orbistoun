//! Rendering a corpus analysis as text.
//!
//! # Why this is in the library rather than in the shim
//!
//! Principle 13: the shims hold no logic. What a coverage report *says* is a property
//! of the analysis, not of whichever surface asked for it, so a CLI command and a run
//! report should not be able to disagree about it. Both call this.
//!
//! # It is written to be diffed
//!
//! Ordering is total everywhere, numbers are rendered identically every time, and
//! nothing carries a timestamp or a path. Two runs over an unchanged corpus produce
//! byte-identical output, so a diff shows only what actually moved.
//!
//! That constraint is why the ranked list is not truncated by default. A top-ten that
//! silently drops the eleventh entry makes a diff lie when the ordering shifts.

use core::fmt::Write as _;

use crate::coverage::{Blocker, CorpusCoverage, Effort, OpcodeKey};
use crate::encoding::EncodingTable;
use crate::mnemonics::MnemonicTable;

/// How many blockers to list. `None` lists all of them.
///
/// A cap is presentation, so it is a parameter rather than a constant - a terminal
/// wants twenty, an agent reading the whole worklist wants all of them.
pub type Limit = Option<usize>;

/// Renders the headline coverage numbers.
///
/// Deliberately short. The number that matters is complete shaders: partial support
/// for a shader renders nothing, so an instruction-level percentage flatters progress
/// in a way that would mislead anyone tracking it.
pub fn summary(coverage: &CorpusCoverage) -> String {
    let shaders = coverage.shaders();
    let complete = coverage.complete_shaders();
    let untrustworthy = coverage.untrustworthy_shaders();
    let instructions: usize = shaders.iter().map(|s| s.instructions).sum();
    let translatable: usize = shaders.iter().map(|s| s.translatable).sum();

    let mut out = String::new();
    let _ = writeln!(out, "shaders      {complete} of {} complete", shaders.len());
    let _ = writeln!(
        out,
        "instructions {translatable} of {instructions} translatable"
    );
    if untrustworthy > 0 {
        // Called out rather than folded into the totals: an untrustworthy decode
        // usually means the encoding table is wrong, which is different work from an
        // unimplemented instruction and goes to a different file.
        let _ = writeln!(
            out,
            "suspect      {untrustworthy} shader(s) decoded unreliably - \
             likely an encoding table fault, not a missing feature"
        );
    }
    out
}

/// Renders the ranked worklist.
///
/// The top line is the instruction whose support would unblock the most shaders **of
/// those that can be worked on now**. That qualifier is the whole point: without it the
/// list is led by whatever is most valuable and least reachable, and offers no way to
/// tell the difference.
///
/// `effort_of` says which tier an instruction is in. See [`Effort`].
pub fn worklist(
    coverage: &CorpusCoverage,
    table: &EncodingTable,
    mnemonics: &MnemonicTable,
    limit: Limit,
    effort_of: impl Fn(OpcodeKey) -> Effort,
) -> String {
    let blockers = coverage.ranked_blockers(effort_of);
    let mut out = String::new();

    if blockers.is_empty() {
        let _ = writeln!(out, "no blockers - every instruction seen is supported");
        return out;
    }

    let shown = limit.unwrap_or(blockers.len()).min(blockers.len());
    let _ = writeln!(
        out,
        "{:>7}  {:>7}  {:>4}  instruction",
        "shaders", "uses", "known"
    );

    let mut announced = false;
    for blocker in &blockers[..shown] {
        // The tiers are separated in the output as well as in the order, because a
        // reader scanning for "what do I do next" should not have to know that the list
        // silently changes meaning partway down.
        if blocker.effort == Effort::Subsystem && !announced {
            announced = true;
            let _ = writeln!(
                out,
                "\n-- waiting on a subsystem; ranked so the payoff is visible, not so \
                 they are next --"
            );
        }
        let _ = writeln!(
            out,
            "{:>7}  {:>7}  {:>4}  {}",
            blocker.shaders_blocked,
            blocker.occurrences,
            if blocker.decodable { "yes" } else { "NO" },
            describe(blocker, table, mnemonics)
        );
    }

    if shown < blockers.len() {
        // Never silently truncated. A list that stops without saying so reads as the
        // whole list, and the reader concludes the work is smaller than it is.
        let _ = writeln!(
            out,
            "... {} further blocker(s) not shown",
            blockers.len() - shown
        );
    }
    out
}

/// A blocker's instruction, named where a name is known.
fn describe(blocker: &Blocker, table: &EncodingTable, mnemonics: &MnemonicTable) -> String {
    let family = blocker
        .key
        .encoding
        .and_then(|i| table.encodings().get(usize::from(i)))
        .map(|e| e.name.as_str());

    match family {
        Some(family) => match mnemonics.name(family, blocker.key.opcode) {
            Some(name) => format!("{name}  ({family}:{:#x})", blocker.key.opcode),
            // No name is not a gap worth hiding - the family and opcode are enough to
            // find it in the reference, and inventing a label would suggest otherwise.
            None => format!("{family}:{:#x}", blocker.key.opcode),
        },
        None => format!(
            "unrecognised encoding, word begins {:#04x}",
            blocker.key.opcode
        ),
    }
}

/// The whole report.
pub fn render(
    coverage: &CorpusCoverage,
    table: &EncodingTable,
    mnemonics: &MnemonicTable,
    limit: Limit,
    effort_of: impl Fn(OpcodeKey) -> Effort,
) -> String {
    let mut out = summary(coverage);
    out.push('\n');
    out.push_str(&worklist(coverage, table, mnemonics, limit, effort_of));
    out
}

#[cfg(test)]
mod tests {
    use crate::coverage::all_ordinary;
    /// The built-in operand table. Every decode needs one now that operands are read.
    fn operands() -> crate::operand::OperandTable {
        crate::operand::OperandTable::builtin().expect("built-in operand table")
    }

    use super::{render, summary, worklist};
    use crate::coverage::{CorpusCoverage, OpcodeKey};
    use crate::decode::decode;
    use crate::encoding::EncodingTable;
    use crate::mnemonics::MnemonicTable;

    fn parts() -> (EncodingTable, MnemonicTable) {
        (
            EncodingTable::builtin().expect("table"),
            MnemonicTable::builtin().expect("mnemonics"),
        )
    }

    /// A VOP1 instruction with the given opcode, per the built-in table.
    fn vop1(opcode: u32) -> u32 {
        0x7E00_0000 | (opcode << 9)
    }

    fn stream(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn the_summary_leads_with_complete_shaders() {
        // Partial support for a shader renders nothing, so an instruction percentage
        // alone would flatter progress to anyone tracking it.
        let (table, _) = parts();
        let mut coverage = CorpusCoverage::new();
        coverage.observe(
            "a",
            &decode(&stream(&[vop1(1)]), &table, &operands()),
            &|_| true,
        );
        coverage.observe(
            "b",
            &decode(&stream(&[vop1(2)]), &table, &operands()),
            &|_| false,
        );

        let text = summary(&coverage);
        assert!(text.contains("1 of 2 complete"), "got:\n{text}");
    }

    #[test]
    fn an_untrustworthy_decode_is_called_out_separately() {
        // It means a probable table fault, which is different work from a missing
        // feature and belongs in a different file.
        let (table, _) = parts();
        let mut coverage = CorpusCoverage::new();
        coverage.observe(
            "bad",
            &decode(&stream(&[0xFFFF_FFF0]), &table, &operands()),
            &|_| true,
        );
        assert!(summary(&coverage).contains("suspect"));
    }

    #[test]
    fn the_worklist_puts_the_most_blocking_instruction_first() {
        let (table, mnemonics) = parts();
        let mut coverage = CorpusCoverage::new();
        let none = |_: OpcodeKey| false;
        // Opcode 3 blocks three shaders; opcode 9 blocks one, many times over.
        for name in ["a", "b", "c"] {
            coverage.observe(
                name,
                &decode(&stream(&[vop1(3)]), &table, &operands()),
                &none,
            );
        }
        let many: Vec<u32> = core::iter::repeat_n(vop1(9), 40).collect();
        coverage.observe("d", &decode(&stream(&many), &table, &operands()), &none);

        let text = worklist(&coverage, &table, &mnemonics, None, all_ordinary);
        let first = text.lines().nth(1).expect("a row after the header");
        assert!(first.contains("VOP1:0x3"), "got:\n{text}");
    }

    #[test]
    fn a_known_instruction_is_named() {
        // v_mov_b32 is in the generated mnemonic table, so the report should say so
        // rather than making the reader look up an opcode number.
        let (table, mnemonics) = parts();
        let mut coverage = CorpusCoverage::new();
        // 0x7E000280 is v_mov_b32_e32 as emitted by a real compiler.
        coverage.observe(
            "a",
            &decode(&stream(&[0x7E00_0280]), &table, &operands()),
            &|_| false,
        );
        let text = worklist(&coverage, &table, &mnemonics, None, all_ordinary);
        assert!(text.contains("v_mov_b32"), "got:\n{text}");
    }

    #[test]
    fn truncation_says_what_it_left_out() {
        // A list that stops without saying so reads as the whole list, and the reader
        // concludes there is less work than there is.
        let (table, mnemonics) = parts();
        let mut coverage = CorpusCoverage::new();
        let words: Vec<u32> = (1..=10).map(vop1).collect();
        coverage.observe("a", &decode(&stream(&words), &table, &operands()), &|_| {
            false
        });

        let text = worklist(&coverage, &table, &mnemonics, Some(3), all_ordinary);
        assert!(
            text.contains("7 further blocker(s) not shown"),
            "got:\n{text}"
        );
    }

    #[test]
    fn an_empty_worklist_says_so_rather_than_printing_a_bare_header() {
        let (table, mnemonics) = parts();
        let mut coverage = CorpusCoverage::new();
        coverage.observe(
            "a",
            &decode(&stream(&[vop1(1)]), &table, &operands()),
            &|_| true,
        );
        assert!(
            worklist(&coverage, &table, &mnemonics, None, all_ordinary).contains("no blockers")
        );
    }

    #[test]
    fn two_renders_of_the_same_corpus_are_byte_identical() {
        // The property that makes a diff between runs meaningful. Any hash ordering
        // or timestamp leaking in would break it.
        let (table, mnemonics) = parts();
        let mut coverage = CorpusCoverage::new();
        for name in ["a", "b", "c"] {
            coverage.observe(
                name,
                &decode(&stream(&[vop1(1), vop1(2), vop1(3)]), &table, &operands()),
                &|_| false,
            );
        }
        let first = render(&coverage, &table, &mnemonics, None, all_ordinary);
        let second = render(&coverage, &table, &mnemonics, None, all_ordinary);
        assert_eq!(first, second);
    }
}
