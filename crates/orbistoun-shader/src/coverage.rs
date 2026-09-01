//! Coverage across a corpus, and the worklist that falls out of it.
//!
//! # Two different questions
//!
//! *Can we decode this instruction?* and *can we translate it?* are separate, and
//! conflating them hides the second behind the first. An instruction the table
//! recognises is still untranslatable until something knows what SPIR-V to emit for
//! it, so [`CorpusCoverage`] tracks both and reports them apart.
//!
//! # Rank by shaders blocked, not by occurrences
//!
//! This is the one judgement in the module and it decides what gets worked on.
//!
//! An instruction appearing ten thousand times inside a single shader blocks exactly
//! one shader. An instruction appearing once each in four hundred shaders blocks four
//! hundred. Ranking by raw frequency puts the first at the top of the list and would
//! have you spend a week unblocking one shader.
//!
//! So the primary key is **how many distinct shaders contain it**. Occurrence count
//! is kept as a tiebreak and as context, not as the ranking.
//!
//! # Deterministic ordering
//!
//! Everything here is a `BTreeMap`. These reports exist to be diffed between runs,
//! and hash iteration order would make every report differ from the last for no
//! reason - which trains a reader, human or otherwise, to ignore the diff.

use std::collections::{BTreeMap, BTreeSet};

use crate::decode::Decode;
use crate::encoding::EncodingTable;

/// Identifies a kind of instruction.
///
/// Ordering is derived and deliberate: it makes the maps below stable, so two runs
/// over the same corpus produce byte-identical reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpcodeKey {
    /// Index into the encoding table, or `None` for an unrecognised family.
    pub encoding: Option<u16>,
    /// Opcode within the family. For an unrecognised family this holds the top byte
    /// of the instruction word instead, so unknowns can still be grouped and counted
    /// rather than collapsing into one undifferentiated bucket.
    pub opcode: u32,
}

impl OpcodeKey {
    /// Renders the key against a table, for reports.
    pub fn describe(&self, table: &EncodingTable) -> String {
        match self
            .encoding
            .and_then(|i| table.encodings().get(usize::from(i)))
        {
            Some(encoding) => format!("{}:{:#x}", encoding.name, self.opcode),
            None => format!("<unrecognised>:{:#04x}xxxxxx", self.opcode),
        }
    }
}

/// What one shader contains.
#[derive(Debug, Clone)]
pub struct ShaderSummary {
    /// Content hash, so a shader is identified by what it is rather than by when it
    /// was seen.
    pub id: String,
    /// Instructions found.
    pub instructions: usize,
    /// Instructions whose family the table recognised.
    pub decodable: usize,
    /// Instructions a translator could also emit code for.
    pub translatable: usize,
    /// Whether the decode can be read as a measurement rather than a lower bound.
    pub trustworthy: bool,
}

impl ShaderSummary {
    /// Whether every instruction in this shader could be translated.
    ///
    /// The only status that matters for "will this shader render": partial support
    /// for a shader is no support for a shader.
    pub const fn is_complete(&self) -> bool {
        self.translatable == self.instructions && self.trustworthy
    }
}

/// Roughly what it would cost to unblock an instruction.
///
/// # Why the ranking needs this at all
///
/// Ranking purely by shaders blocked answers "what would help most" and says nothing
/// about what is *reachable*. The two came apart the first time this list had real data
/// in it: the instruction blocking the most shaders was an export, which needs a whole
/// render-target model, while the one blocking fewest was an ordinary multiply-add that
/// took twenty minutes. A single ordered list puts a week of work above a morning's and
/// offers no way to tell.
///
/// # Why two tiers and not a score
///
/// A number - blocked divided by effort - would rank them precisely and the precision
/// would be invented. Nothing here can measure effort, and a ratio built from a guess
/// reads like a measurement. Two tiers claim only what is actually known: whether the
/// work is *ordinary*, or whether it is waiting on a subsystem that does not exist.
///
/// The tier is not decided here either. This crate knows *what* blocks a shader; only the
/// translator knows *why*, and it already keeps that in its blocked-instruction table
/// with a reason attached. The caller joins the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Effort {
    /// Ordinary work: an instruction to translate, with an oracle available.
    #[default]
    Ordinary,
    /// Waiting on a subsystem that has not been built.
    ///
    /// Sorted after ordinary work, so a list read top to bottom offers what can be done
    /// now before what cannot.
    Subsystem,
}

/// One reason shaders cannot be translated yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocker {
    /// The instruction kind.
    pub key: OpcodeKey,
    /// Roughly what it would cost to unblock.
    pub effort: Effort,
    /// How many distinct shaders contain it. **The ranking key, within an effort tier.**
    pub shaders_blocked: usize,
    /// How many times it appears across the corpus. Context, not ranking.
    pub occurrences: usize,
    /// Whether the encoding table recognises it at all.
    ///
    /// Separates "we do not know what this instruction is" from "we know and cannot
    /// translate it yet" - different work, and the first is usually a table fix.
    pub decodable: bool,
}

/// Every blocker treated as ordinary work.
///
/// For tests and callers that have no translator to ask. It is a named function rather
/// than a closure at each call site so the assumption is visible: a report built with
/// this one cannot distinguish reachable work from work waiting on a subsystem.
pub fn all_ordinary(_key: OpcodeKey) -> Effort {
    Effort::Ordinary
}

/// A run's coverage, reduced to what is worth comparing against the next one.
///
/// # Why a shader corpus needs a progress block at all
///
/// The import side of this project has one and it is the thing that makes the work
/// iterable: every run ends with `FURTHER`, `same` or `BACK`, so a change either moved
/// something or it did not, and nobody has to hold two numbers in their head between
/// runs. The shader side has had the same loop available all along - rank what blocks,
/// implement the top entry, run again - and no way to say whether it worked except
/// reading two figures off consecutive screens.
///
/// This is that missing half. It is deliberately the *same vocabulary*, because they are
/// the same loop pointed at different material.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    /// Shaders in the corpus every instruction of which translates.
    pub complete: usize,
    /// Shaders looked at.
    pub shaders: usize,
    /// Distinct instructions that translate.
    pub translatable: usize,
    /// Distinct instructions seen.
    pub instructions: usize,
    /// What still blocks something, by name where one is known.
    ///
    /// Kept so a run can say *which* blocker went away, not only that the count fell. A
    /// count that stays the same while the contents change is a real thing - one blocker
    /// implemented, another uncovered behind it - and it reads as no progress.
    pub blockers: Vec<String>,
}

/// How a run compares with the one before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Movement {
    /// The verdict, in the vocabulary the import side already uses.
    pub verdict: Verdict,
    /// Change in shaders that translate completely.
    pub complete_delta: i64,
    /// Change in distinct instructions that translate.
    pub translatable_delta: i64,
    /// Blockers that were there last time and are not now.
    pub cleared: Vec<String>,
    /// Blockers that were not there last time and are now.
    ///
    /// Not a regression: implementing one blocker routinely uncovers the next instruction
    /// in a shader that could not be reached past it. Reported separately so that reads
    /// as progress rather than as breakage.
    pub uncovered: Vec<String>,
}

/// What a run did, relative to the last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to compare against.
    FirstRun,
    /// More shaders translate completely, or more instructions do.
    Further,
    /// Fewer. Worth knowing immediately rather than three changes later.
    Back,
    /// Nothing moved.
    Same,
}

impl Verdict {
    /// The label a run prints beside the summary.
    ///
    /// Loud for the ones that matter and quiet for the ones that do not, matching the
    /// import side exactly - `FURTHER` is the only thing this project is trying to
    /// produce, so it should be findable by eye in a wall of output.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FirstRun => "",
            Self::Further => "FURTHER",
            Self::Back => "BACK",
            Self::Same => "same",
        }
    }
}

impl Summary {
    /// Reduces a corpus to its comparable figures.
    pub fn of(coverage: &CorpusCoverage, describe: impl Fn(OpcodeKey) -> String) -> Self {
        let blockers = coverage
            .ranked_blockers(all_ordinary)
            .into_iter()
            .map(|blocker| describe(blocker.key))
            .collect();
        let shaders = coverage.shaders();
        Self {
            complete: coverage.complete_shaders(),
            shaders: shaders.len(),
            translatable: shaders.iter().map(|s| s.translatable).sum(),
            instructions: shaders.iter().map(|s| s.instructions).sum(),
            blockers,
        }
    }

    /// Compares this run with the previous one.
    ///
    /// **Completeness first, instructions second.** A run that translates one more whole
    /// shader has moved further than one that translates three more instructions across
    /// shaders that still do not run, because a shader is the unit that can be checked
    /// against hardware and an instruction is not.
    pub fn movement(&self, previous: Option<&Self>) -> Movement {
        let Some(previous) = previous else {
            return Movement {
                verdict: Verdict::FirstRun,
                complete_delta: 0,
                translatable_delta: 0,
                cleared: Vec::new(),
                uncovered: Vec::new(),
            };
        };

        let complete_delta = as_delta(self.complete, previous.complete);
        let translatable_delta = as_delta(self.translatable, previous.translatable);
        let verdict = match (complete_delta, translatable_delta) {
            (d, _) if d > 0 => Verdict::Further,
            (d, _) if d < 0 => Verdict::Back,
            (_, d) if d > 0 => Verdict::Further,
            (_, d) if d < 0 => Verdict::Back,
            _ => Verdict::Same,
        };

        Movement {
            verdict,
            complete_delta,
            translatable_delta,
            cleared: difference(&previous.blockers, &self.blockers),
            uncovered: difference(&self.blockers, &previous.blockers),
        }
    }
}

/// `left` minus `right`, in order.
fn difference(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .filter(|entry| !right.contains(entry))
        .cloned()
        .collect()
}

/// The signed change between two counts.
fn as_delta(now: usize, before: usize) -> i64 {
    i64::try_from(now).unwrap_or(i64::MAX) - i64::try_from(before).unwrap_or(i64::MAX)
}

/// Accumulates coverage over many shaders.
#[derive(Debug, Clone, Default)]
pub struct CorpusCoverage {
    per_key: BTreeMap<OpcodeKey, KeyStats>,
    shaders: Vec<ShaderSummary>,
}

#[derive(Debug, Clone, Default)]
struct KeyStats {
    occurrences: usize,
    shaders: BTreeSet<String>,
    decodable: bool,
}

impl CorpusCoverage {
    /// Creates an empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Folds one shader's decode in.
    ///
    /// `supported` answers whether a translator can emit code for a given kind.
    /// Passing a closure that always returns `false` gives a pure decode census,
    /// which is the useful shape before any translator exists.
    pub fn observe(
        &mut self,
        id: &str,
        decode: &Decode,
        supported: &impl Fn(OpcodeKey) -> bool,
    ) -> ShaderSummary {
        let mut decodable = 0;
        let mut translatable = 0;

        for instruction in &decode.instructions {
            let key = OpcodeKey {
                encoding: instruction.encoding,
                // For an unrecognised family the opcode field is meaningless, so the
                // top byte stands in - it groups unknowns by their encoding space
                // instead of collapsing them all together.
                opcode: if instruction.is_known() {
                    instruction.opcode
                } else {
                    instruction.word >> 24
                },
            };

            let is_decodable = instruction.is_known();
            let is_translatable = is_decodable && supported(key);
            if is_decodable {
                decodable += 1;
            }
            if is_translatable {
                translatable += 1;
            }

            // Only unsupported kinds are worth tracking: a supported instruction is
            // not a blocker, and recording it would bury the list under the ones that
            // already work.
            if !is_translatable {
                let stats = self.per_key.entry(key).or_default();
                stats.occurrences += 1;
                stats.shaders.insert(id.to_owned());
                stats.decodable = is_decodable;
            }
        }

        let summary = ShaderSummary {
            id: id.to_owned(),
            instructions: decode.instructions.len(),
            decodable,
            translatable,
            trustworthy: decode.is_trustworthy(),
        };
        self.shaders.push(summary.clone());
        summary
    }

    /// Blockers, cheapest-and-most-blocking first.
    ///
    /// This is the worklist. The top entry is the instruction whose support would unblock
    /// the most shaders **among those that can be worked on now** - see [`Effort`] for
    /// why that qualifier is the whole point.
    ///
    /// `effort_of` decides the tier. It is a parameter because this crate can see what
    /// blocks a shader and not why: the reason lives with the translator, which keeps it
    /// alongside the instruction it refuses.
    pub fn ranked_blockers(&self, effort_of: impl Fn(OpcodeKey) -> Effort) -> Vec<Blocker> {
        let mut blockers: Vec<Blocker> = self
            .per_key
            .iter()
            .map(|(key, stats)| Blocker {
                key: *key,
                effort: effort_of(*key),
                shaders_blocked: stats.shaders.len(),
                occurrences: stats.occurrences,
                decodable: stats.decodable,
            })
            .collect();
        // Effort first, so everything that can be done now sorts above everything that
        // cannot. Within a tier it is shaders blocked, occurrences as a tiebreak, then
        // the key itself so the order is total and two runs agree exactly.
        blockers.sort_by(|a, b| {
            a.effort
                .cmp(&b.effort)
                .then(b.shaders_blocked.cmp(&a.shaders_blocked))
                .then(b.occurrences.cmp(&a.occurrences))
                .then(a.key.cmp(&b.key))
        });
        blockers
    }

    /// Every shader observed.
    pub fn shaders(&self) -> &[ShaderSummary] {
        &self.shaders
    }

    /// Shaders that could be translated in full.
    ///
    /// The headline number, and the only one that maps onto something visible: a
    /// shader is either translatable or it is not, and partial credit renders nothing.
    pub fn complete_shaders(&self) -> usize {
        self.shaders.iter().filter(|s| s.is_complete()).count()
    }

    /// Shaders whose decode could not be trusted.
    ///
    /// Reported separately because it usually indicates a table problem rather than
    /// an unsupported instruction, and the two need different work.
    pub fn untrustworthy_shaders(&self) -> usize {
        self.shaders.iter().filter(|s| !s.trustworthy).count()
    }
}

#[cfg(test)]
mod tests {

    /// A summary with the figures a test cares about and defaults elsewhere.
    fn summary(complete: usize, translatable: usize, blockers: &[&str]) -> super::Summary {
        super::Summary {
            complete,
            shaders: 10,
            translatable,
            instructions: 127,
            blockers: blockers.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn a_run_with_nothing_to_compare_against_says_so() {
        // Rather than reporting `same`, which would claim a comparison that did not
        // happen and read as "your change did nothing".
        let now = summary(7, 112, &["exp"]);
        assert_eq!(now.movement(None).verdict, super::Verdict::FirstRun);
    }

    #[test]
    fn one_more_whole_shader_is_further() {
        let before = summary(6, 110, &["exp", "v_fmac_f32_e32"]);
        let now = summary(7, 112, &["exp"]);
        let movement = now.movement(Some(&before));

        assert_eq!(movement.verdict, super::Verdict::Further);
        assert_eq!(movement.complete_delta, 1);
        assert_eq!(movement.translatable_delta, 2);
        assert_eq!(movement.cleared, ["v_fmac_f32_e32"]);
        assert!(movement.uncovered.is_empty());
    }

    #[test]
    fn more_instructions_without_a_whole_shader_is_still_further() {
        // Partial credit renders nothing, so completeness leads - but a run that
        // translated three more instructions has still moved, and calling that `same`
        // would make a real change look like a wasted one.
        let before = summary(6, 110, &[]);
        let now = summary(6, 113, &[]);
        assert_eq!(now.movement(Some(&before)).verdict, super::Verdict::Further);
    }

    #[test]
    fn losing_ground_is_reported_immediately() {
        // `BACK` exists so a regression surfaces on the run that caused it rather than
        // three changes later, when it is attributable to nothing in particular.
        let before = summary(7, 112, &[]);
        let now = summary(6, 110, &[]);
        let movement = now.movement(Some(&before));

        assert_eq!(movement.verdict, super::Verdict::Back);
        assert_eq!(movement.complete_delta, -1);
    }

    #[test]
    fn a_blocker_uncovered_behind_another_is_not_a_regression() {
        // Implementing one blocker routinely reveals the next instruction in a shader
        // that could not be reached past it. The blocker count is unchanged and the work
        // moved, so the two lists are reported apart - a count alone would read as
        // nothing having happened.
        let before = summary(6, 110, &["v_fmac_f32_e32"]);
        let now = summary(6, 111, &["image_sample"]);
        let movement = now.movement(Some(&before));

        assert_eq!(movement.verdict, super::Verdict::Further);
        assert_eq!(movement.cleared, ["v_fmac_f32_e32"]);
        assert_eq!(movement.uncovered, ["image_sample"]);
    }
    use super::all_ordinary;
    /// The built-in operand table. Every decode needs one now that operands are read.
    fn operands() -> crate::operand::OperandTable {
        crate::operand::OperandTable::builtin().expect("built-in operand table")
    }

    use super::{CorpusCoverage, OpcodeKey};
    use crate::decode::decode;
    use crate::encoding::EncodingTable;

    fn table() -> EncodingTable {
        EncodingTable::load(
            r#"
            [[encoding]]
            name = "ALPHA"
            mask = "0xFE000000"
            value = "0x7E000000"
            opcode = { shift = 9, width = 8 }
            width_bytes = 4
            "#,
        )
        .expect("table")
    }

    fn stream(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// Builds an instruction of family ALPHA with the given opcode.
    fn alpha(opcode: u32) -> u32 {
        0x7E00_0000 | (opcode << 9)
    }

    #[test]
    fn ranking_is_by_shaders_blocked_not_by_raw_frequency() {
        // The judgement the module exists to encode. Opcode 1 appears fifty times but
        // in a single shader; opcode 2 appears three times across three shaders.
        // Ranking by frequency would put opcode 1 first and have you spend the effort
        // unblocking exactly one shader.
        let table = table();
        let mut coverage = CorpusCoverage::new();
        let never = |_| false;

        let many_in_one: Vec<u32> = core::iter::repeat_n(alpha(1), 50).collect();
        coverage.observe(
            "shader-a",
            &decode(&stream(&many_in_one), &table, &operands()),
            &never,
        );

        for name in ["shader-b", "shader-c", "shader-d"] {
            coverage.observe(
                name,
                &decode(&stream(&[alpha(2)]), &table, &operands()),
                &never,
            );
        }

        let ranked = coverage.ranked_blockers(all_ordinary);
        assert_eq!(ranked[0].key.opcode, 2, "three shaders beats fifty uses");
        assert_eq!(ranked[0].shaders_blocked, 3);
        assert_eq!(ranked[1].key.opcode, 1);
        assert_eq!(ranked[1].occurrences, 50, "frequency is kept as context");
    }

    #[test]
    fn a_supported_instruction_is_not_a_blocker() {
        // Otherwise the worklist fills with things that already work and the real
        // blockers sink out of sight.
        let table = table();
        let mut coverage = CorpusCoverage::new();
        let supported = |key: OpcodeKey| key.opcode == 1;

        let summary = coverage.observe(
            "shader-a",
            &decode(&stream(&[alpha(1), alpha(2)]), &table, &operands()),
            &supported,
        );
        assert_eq!(summary.translatable, 1);
        let ranked = coverage.ranked_blockers(all_ordinary);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].key.opcode, 2);
    }

    #[test]
    fn undecodable_and_untranslatable_are_reported_apart() {
        // Different work: the first is usually a table fix, the second is a translator
        // feature. Collapsing them would send someone to the wrong file.
        let table = table();
        let mut coverage = CorpusCoverage::new();
        coverage.observe(
            "shader-a",
            &decode(&stream(&[alpha(1), 0xFFFF_FFFF]), &table, &operands()),
            &|_| false,
        );
        let ranked = coverage.ranked_blockers(all_ordinary);
        let decodable: Vec<bool> = ranked.iter().map(|b| b.decodable).collect();
        assert!(decodable.contains(&true), "the known-but-unsupported one");
        assert!(decodable.contains(&false), "the unrecognised one");
    }

    #[test]
    fn a_shader_counts_as_complete_only_when_every_instruction_translates() {
        // Partial support for a shader renders nothing, so partial credit would be
        // actively misleading about how close anything is.
        let table = table();
        let mut coverage = CorpusCoverage::new();
        let only_one = |key: OpcodeKey| key.opcode == 1;

        coverage.observe(
            "all-good",
            &decode(&stream(&[alpha(1)]), &table, &operands()),
            &only_one,
        );
        coverage.observe(
            "one-missing",
            &decode(&stream(&[alpha(1), alpha(9)]), &table, &operands()),
            &only_one,
        );
        assert_eq!(coverage.complete_shaders(), 1);
    }

    #[test]
    fn an_untrustworthy_decode_never_counts_as_complete() {
        // A desynchronised decode might have missed instructions entirely, so
        // "everything I saw was supported" is not the same claim as "this shader is
        // supported".
        let table = table();
        let mut coverage = CorpusCoverage::new();
        // Unrecognised word desynchronises; supported() says yes to everything.
        coverage.observe(
            "suspect",
            &decode(&stream(&[0xFFFF_FFFF]), &table, &operands()),
            &|_| true,
        );
        assert_eq!(coverage.complete_shaders(), 0);
        assert_eq!(coverage.untrustworthy_shaders(), 1);
    }

    #[test]
    fn unrecognised_instructions_group_by_encoding_space_rather_than_collapsing() {
        // All unknowns sharing one bucket would say "there are unknowns" and nothing
        // about how many distinct kinds, which is what decides how much work is left.
        let table = table();
        let mut coverage = CorpusCoverage::new();
        coverage.observe(
            "shader-a",
            &decode(&stream(&[0xFF00_0000, 0xFE00_0000]), &table, &operands()),
            &|_| false,
        );
        assert_eq!(
            coverage.ranked_blockers(all_ordinary).len(),
            2,
            "distinct unknown spaces"
        );
    }
}
