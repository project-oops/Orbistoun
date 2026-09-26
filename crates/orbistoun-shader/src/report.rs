//! Rendering a corpus analysis as text.
//!
//! The report lives in the library because shims hold no logic: a CLI command and a run
//! report render the same text. Output is written to be diffed - ordering is total,
//! numbers render identically, and nothing carries a timestamp or path - and the ranked
//! list is untruncated by default so a shifted ordering never hides an entry.

use core::fmt::Write as _;

use crate::coverage::{Blocker, CorpusCoverage, Effort, OpcodeKey};
use crate::encoding::EncodingTable;
use crate::mnemonics::MnemonicTable;

/// How many blockers to list. `None` lists all of them.
///
/// A terminal wants twenty; an agent reading the whole worklist wants all of them.
pub type Limit = Option<usize>;

/// Renders the headline coverage numbers.
///
/// Leads with complete shaders: partial support for a shader renders nothing, so an
/// instruction-level percentage alone overstates coverage.
pub fn summary(coverage: &CorpusCoverage) -> String {
    let shaders = coverage.shaders();
    let complete = coverage.complete_shaders();
    let untrustworthy = coverage.untrustworthy_shaders();
    let instructions: usize = shaders.iter().map(|s| s.instructions).sum();
    let translatable: usize = shaders.iter().map(|s| s.translatable).sum();

    // The line says which question the count answers: a shader with every opcode
    // supported can still fail translation on operands, registers or stages, which an
    // opcode census cannot see.
    let attempted = shaders.iter().filter(|s| s.translated.is_some()).count();
    let basis = if attempted == shaders.len() && !shaders.is_empty() {
        "translate"
    } else {
        "have every opcode supported; no translation was attempted"
    };

    let mut out = String::new();
    let _ = writeln!(out, "shaders      {complete} of {} {basis}", shaders.len());
    let _ = writeln!(
        out,
        "instructions {translatable} of {instructions} translatable"
    );
    if untrustworthy > 0 {
        // Separate from the totals: an untrustworthy decode usually means an encoding
        // table fault, which is different work from an unimplemented instruction.
        let _ = writeln!(
            out,
            concat!(
                "suspect      {} shader(s) decoded unreliably - ",
                "likely an encoding table fault, not a missing feature"
            ),
            untrustworthy
        );
    }
    out
}

/// Renders the ranked worklist.
///
/// The top line is the instruction whose support unblocks the most shaders of those
/// workable now, so the list is not led by valuable but unreachable work.
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
        // The tiers are separated in the output as well as the order, so a reader sees
        // where the list changes meaning.
        if blocker.effort == Effort::Subsystem && !announced {
            announced = true;
            let _ = writeln!(
                out,
                concat!(
                    "\n-- waiting on a subsystem; ranked so the payoff is visible, not so ",
                    "they are next --"
                )
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
        // Truncation is stated, so a shortened list never reads as the whole list.
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
            // The family and opcode are enough to find it in the reference.
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
    /// The built-in operand table, which every decode needs.
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

    /// The summary leads with the complete-shader count and says no translation ran.
    #[test]
    fn the_summary_leads_with_complete_shaders() {
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
        assert!(
            text.contains("1 of 2"),
            "got:
{text}"
        );
        // No translator ran, so the line says the count is the opcode-level bound.
        assert!(
            text.contains("no translation was attempted"),
            "got:
{text}"
        );
    }

    /// A translation verdict overrides the opcode estimate: a shader whose every opcode
    /// is supported still counts incomplete when its translation failed.
    #[test]
    fn a_translation_verdict_overrides_the_opcode_estimate() {
        let (table, _) = parts();
        let mut coverage = CorpusCoverage::new();
        coverage.observe_translated(
            "a",
            &decode(&stream(&[vop1(1)]), &table, &operands()),
            &|_| true,
            Some(false),
        );

        let text = summary(&coverage);
        assert!(
            text.contains("0 of 1 translate"),
            "got:
{text}"
        );
    }

    /// An untrustworthy decode is reported as a probable table fault.
    #[test]
    fn an_untrustworthy_decode_is_called_out_separately() {
        let (table, _) = parts();
        let mut coverage = CorpusCoverage::new();
        coverage.observe(
            "bad",
            &decode(&stream(&[0xFFFF_FFF0]), &table, &operands()),
            &|_| true,
        );
        assert!(summary(&coverage).contains("suspect"));
    }

    /// The instruction blocking the most shaders ranks first, not the most frequent.
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

    /// An instruction in the mnemonic table is shown by name.
    #[test]
    fn a_known_instruction_is_named() {
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

    /// A truncated worklist states how many entries it left out.
    #[test]
    fn truncation_says_what_it_left_out() {
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

    /// An empty worklist says so rather than printing a bare header.
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

    /// Two renders of the same corpus are byte-identical.
    #[test]
    fn two_renders_of_the_same_corpus_are_byte_identical() {
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
