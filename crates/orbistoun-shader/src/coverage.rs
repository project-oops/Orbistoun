//! Coverage across a corpus, and the worklist that falls out of it.
//!
//! Decoding and translating are tracked apart: an instruction the table recognises is
//! untranslatable until something emits SPIR-V for it. Blockers rank by how many distinct
//! shaders contain them, with occurrences as a tiebreak - an instruction used ten thousand
//! times in one shader blocks one shader. Every map is a `BTreeMap` so reports diff
//! cleanly between runs.

use std::collections::{BTreeMap, BTreeSet};

use crate::decode::Decode;
use crate::encoding::EncodingTable;

/// Identifies a kind of instruction.
///
/// The derived ordering keeps the maps below stable, so two runs over the same corpus
/// produce byte-identical reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpcodeKey {
    /// Index into the encoding table, or `None` for an unrecognised family.
    pub encoding: Option<u16>,
    /// Opcode within the family. For an unrecognised family this holds the top byte of
    /// the instruction word, so unknowns group by encoding space.
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
    /// Content hash, so a shader is identified by what it is.
    pub id: String,
    /// Instructions found.
    pub instructions: usize,
    /// Instructions whose family the table recognised.
    pub decodable: usize,
    /// Instructions a translator could also emit code for.
    pub translatable: usize,
    /// Whether the decode can be read as a measurement rather than a lower bound.
    pub trustworthy: bool,
    /// What an actual translation of this shader said, when one was attempted.
    ///
    /// [`None`] means no translator ran: a pure decode census.
    pub translated: Option<bool>,
}

impl ShaderSummary {
    /// Whether this shader translates whole.
    ///
    /// Partial support for a shader is no support. A translation verdict wins over the
    /// opcode estimate, because translation refuses for reasons other than opcodes
    /// (registers outside the register file, unmeasured memory bases). With no translation
    /// attempted the estimate is a bound, and [`crate::report::summary`] says so.
    pub const fn is_complete(&self) -> bool {
        match self.translated {
            Some(verdict) => verdict,
            None => self.translatable == self.instructions && self.trustworthy,
        }
    }
}

/// Roughly what it would cost to unblock an instruction.
///
/// Ranking by shaders blocked alone says what would help most, not what is reachable:
/// an export needing a render-target model can outrank an ordinary multiply-add. Two
/// tiers claim only what is known - ordinary work, or work waiting on a subsystem -
/// where a score would invent a precision nothing measures. The translator's
/// blocked-instruction table supplies the tier; the caller joins the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Effort {
    /// Ordinary work: an instruction to translate, with an oracle available.
    #[default]
    Ordinary,
    /// Waiting on a subsystem that has not been built.
    ///
    /// Sorted after ordinary work, so the list offers what can be done now first.
    Subsystem,
}

/// One reason shaders cannot be translated yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocker {
    /// The instruction kind.
    pub key: OpcodeKey,
    /// Roughly what it would cost to unblock.
    pub effort: Effort,
    /// How many distinct shaders contain it: the ranking key within an effort tier.
    pub shaders_blocked: usize,
    /// How many times it appears across the corpus. Context, not ranking.
    pub occurrences: usize,
    /// Whether the encoding table recognises it at all.
    ///
    /// Separates an unknown instruction (usually a table fix) from a known one the
    /// translator does not handle.
    pub decodable: bool,
}

/// Every blocker treated as ordinary work.
///
/// For tests and callers with no translator to ask. A named function keeps the
/// assumption visible: such a report cannot separate reachable work from blocked work.
pub fn all_ordinary(_key: OpcodeKey) -> Effort {
    Effort::Ordinary
}

/// A run's coverage, reduced to what is worth comparing against the next one.
///
/// Gives the shader corpus the same `FURTHER`, `same` or `BACK` verdict as the import
/// side (D129), so each change to the translator reads as moved or not.
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
    /// Whether `complete` counts shaders a translator was actually run over.
    ///
    /// Runs measured differently are not compared: a count of opcode-supported shaders
    /// and a count of translated shaders differ without the translator changing. A
    /// stored record without the field reads `false`, which is what it measured.
    #[serde(default)]
    pub attempted: bool,
    /// What still blocks something, by name where one is known.
    ///
    /// Names which blocker went away: an unchanged count can hide one blocker implemented
    /// and another uncovered behind it.
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
    /// in a shader, and it is reported separately so it reads as progress.
    pub uncovered: Vec<String>,
}

/// What a run did, relative to the last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to compare against.
    FirstRun,
    /// More shaders translate completely, or more instructions do.
    Further,
    /// Fewer, reported on the run that caused it.
    Back,
    /// Nothing moved.
    Same,
}

impl Verdict {
    /// The label a run prints beside the summary.
    ///
    /// Matches the import side's labels; `FURTHER` is loud so it stands out in output.
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
            attempted: !shaders.is_empty() && shaders.iter().all(|s| s.translated.is_some()),
            shaders: shaders.len(),
            translatable: shaders.iter().map(|s| s.translatable).sum(),
            instructions: shaders.iter().map(|s| s.instructions).sum(),
            blockers,
        }
    }

    /// Compares this run with the previous one.
    ///
    /// Completeness first, instructions second: a whole shader is the unit that can be
    /// checked against hardware.
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

        // Two runs that measured different things are not compared; a delta between
        // them names a movement neither observed.
        if self.attempted != previous.attempted {
            return Movement {
                verdict: Verdict::FirstRun,
                complete_delta: 0,
                translatable_delta: 0,
                cleared: Vec::new(),
                uncovered: Vec::new(),
            };
        }

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

    /// Folds one shader's decode in, with no translation attempted.
    ///
    /// `supported` answers whether a translator can emit code for a given kind; one
    /// that always returns `false` gives a pure decode census. Prefer
    /// [`observe_translated`](Self::observe_translated) wherever a translator can run,
    /// since this reports only a bound on a whole shader.
    pub fn observe(
        &mut self,
        id: &str,
        decode: &Decode,
        supported: &impl Fn(OpcodeKey) -> bool,
    ) -> ShaderSummary {
        self.observe_translated(id, decode, supported, None)
    }

    /// Folds one shader's decode in, carrying what a translation of it actually did.
    ///
    /// `translated` is [`Some`] when a translator was run over this shader: `true` if it
    /// produced a module, `false` if it refused. That verdict decides
    /// [`ShaderSummary::is_complete`]; the per-opcode census still feeds the blocker
    /// ranking, because a refusal names one instruction and the ranking needs all of them.
    pub fn observe_translated(
        &mut self,
        id: &str,
        decode: &Decode,
        supported: &impl Fn(OpcodeKey) -> bool,
        translated: Option<bool>,
    ) -> ShaderSummary {
        let mut decodable = 0;
        let mut translatable = 0;

        for instruction in &decode.instructions {
            let key = OpcodeKey {
                encoding: instruction.encoding,
                // For an unrecognised family the opcode field is meaningless, so the
                // top byte groups unknowns by encoding space.
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

            // Only unsupported kinds are tracked: a supported instruction is not a
            // blocker.
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
            translated,
        };
        self.shaders.push(summary.clone());
        summary
    }

    /// Blockers, cheapest-and-most-blocking first.
    ///
    /// The top entry is the instruction whose support unblocks the most shaders among
    /// those workable now (see [`Effort`]). `effort_of` decides the tier, because the
    /// reason an instruction is refused lives with the translator.
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
        // Effort first, then shaders blocked, occurrences, and the key itself, so the
        // order is total and two runs agree exactly.
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
    /// The headline number: partial support for a shader renders nothing.
    pub fn complete_shaders(&self) -> usize {
        self.shaders.iter().filter(|s| s.is_complete()).count()
    }

    /// Shaders whose decode could not be trusted.
    ///
    /// Reported separately because it usually indicates a table fault rather than an
    /// unsupported instruction.
    pub fn untrustworthy_shaders(&self) -> usize {
        self.shaders.iter().filter(|s| !s.trustworthy).count()
    }
}

#[cfg(test)]
mod tests {

    /// A summary with the figures a test cares about and defaults elsewhere.
    ///
    /// Measured by translation; the test comparing across a change of basis builds its
    /// own.
    fn summary(complete: usize, translatable: usize, blockers: &[&str]) -> super::Summary {
        super::Summary {
            complete,
            attempted: true,
            shaders: 10,
            translatable,
            instructions: 127,
            blockers: blockers.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// Two runs that measured different things report no comparison, not a regression.
    #[test]
    fn a_run_measured_differently_from_the_last_is_not_compared_with_it() {
        let estimated = super::Summary {
            attempted: false,
            ..summary(2, 196, &["s_sendmsg"])
        };
        let translated = summary(0, 196, &["s_sendmsg"]);

        let movement = translated.movement(Some(&estimated));
        assert_eq!(movement.verdict, super::Verdict::FirstRun);
        assert_eq!(movement.complete_delta, 0, "no delta is claimed either");

        // And the comparison is restored as soon as both sides measure the same way.
        let later = summary(1, 198, &["s_sendmsg"]);
        assert_eq!(
            later.movement(Some(&translated)).verdict,
            super::Verdict::Further
        );
    }

    /// A first run reports `FirstRun`, not `same`.
    #[test]
    fn a_run_with_nothing_to_compare_against_says_so() {
        let now = summary(7, 112, &["exp"]);
        assert_eq!(now.movement(None).verdict, super::Verdict::FirstRun);
    }

    /// One more whole shader is `FURTHER`, and the cleared blocker is named.
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

    /// More translatable instructions without a whole shader is still `FURTHER`.
    #[test]
    fn more_instructions_without_a_whole_shader_is_still_further() {
        let before = summary(6, 110, &[]);
        let now = summary(6, 113, &[]);
        assert_eq!(now.movement(Some(&before)).verdict, super::Verdict::Further);
    }

    /// Fewer complete shaders is `BACK`.
    #[test]
    fn losing_ground_is_reported_immediately() {
        let before = summary(7, 112, &[]);
        let now = summary(6, 110, &[]);
        let movement = now.movement(Some(&before));

        assert_eq!(movement.verdict, super::Verdict::Back);
        assert_eq!(movement.complete_delta, -1);
    }

    /// A blocker uncovered behind a cleared one reads as progress, with both named.
    #[test]
    fn a_blocker_uncovered_behind_another_is_not_a_regression() {
        let before = summary(6, 110, &["v_fmac_f32_e32"]);
        let now = summary(6, 111, &["image_sample"]);
        let movement = now.movement(Some(&before));

        assert_eq!(movement.verdict, super::Verdict::Further);
        assert_eq!(movement.cleared, ["v_fmac_f32_e32"]);
        assert_eq!(movement.uncovered, ["image_sample"]);
    }
    use super::all_ordinary;
    /// The built-in operand table, which every decode needs.
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

    /// Blockers rank by shaders blocked, not raw frequency.
    #[test]
    fn ranking_is_by_shaders_blocked_not_by_raw_frequency() {
        // Opcode 1 appears fifty times in one shader; opcode 2 once in each of three.
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

    /// A supported instruction is not a blocker.
    #[test]
    fn a_supported_instruction_is_not_a_blocker() {
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

    /// Undecodable and untranslatable blockers are reported apart.
    #[test]
    fn undecodable_and_untranslatable_are_reported_apart() {
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

    /// A shader is complete only when every instruction translates.
    #[test]
    fn a_shader_counts_as_complete_only_when_every_instruction_translates() {
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

    /// A desynchronised decode never counts as complete, since it may have missed
    /// instructions.
    #[test]
    fn an_untrustworthy_decode_never_counts_as_complete() {
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

    /// Unrecognised instructions group by encoding space rather than one bucket.
    #[test]
    fn unrecognised_instructions_group_by_encoding_space_rather_than_collapsing() {
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
