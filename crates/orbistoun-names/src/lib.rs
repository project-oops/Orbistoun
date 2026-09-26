//! Generating candidate symbol names, so a hash can be turned back into one.
//!
//! A NID is a truncated SHA-1 and cannot be inverted, so names are generated and hashed until
//! one matches, and a match verifies itself (D068). Two sources feed it: published standards
//! (the FreeBSD-derived C library's ISO C and POSIX names, not guesses) and the vendor naming
//! convention, a regular shape of prefix, module, action and object enumerated combinatorially.
//! The vocabulary is data, so adding a word needs no rebuild. Each pattern produces its `n`th
//! name directly by reading the index as a mixed-radix number, so a search splits across
//! threads by range and a given index has a testable answer.

pub mod affix;
pub mod harvest;
pub mod solve;
pub mod strings;

use std::collections::BTreeMap;

use serde::Deserialize;

/// Vocabulary and patterns shipped with the tool: a starting point that grows as names are
/// confirmed, which a user file replaces entirely.
pub const DEFAULT_VENDOR_GRAMMAR: &str = include_str!("../data/vendor.toml");

/// Names fixed by ISO C and POSIX, which the target's C library is derived from.
pub const DEFAULT_STANDARD_NAMES: &str = include_str!("../data/standard.txt");

/// Every published name, respelled the way the vendor spells it.
///
/// The threading interface is POSIX with a vendor prefix, so the respelling is mechanical:
/// capitalise each underscore-separated part and join them, making `pthread_mutexattr_settype`
/// into `PthreadMutexattrSettype`. No combination of vendor-shaped parts spells `Mutexattr`, so
/// this adds the missing shape, "a POSIX name, whole", at one candidate per harvested name.
pub fn posix_vocabulary() -> Vec<String> {
    /// Shortest underscore part worth keeping on its own.
    ///
    /// Parts of two letters or fewer (`in`, `t`, `vm`) combine with everything and multiply every
    /// pattern's candidates for nothing.
    const SHORTEST_PART: usize = 3;

    let capitalise = |part: &str| {
        let mut chars = part.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().collect::<String>() + chars.as_str()
        })
    };

    let mut words: Vec<String> = Vec::new();
    for name in DEFAULT_STANDARD_NAMES.split_whitespace() {
        let parts: Vec<&str> = name.trim_matches('_').split('_').collect();
        // The whole name, joined: what a vendor name inherits wholesale.
        let joined: String = parts.iter().copied().map(capitalise).collect();
        if !joined.is_empty() {
            words.push(joined);
        }
        // Each part on its own as well: a vendor name can borrow a part rather than a whole standard
        // name (`pmap_unset` and `rpcb_unset` both carry `unset`).
        if parts.len() > 1 {
            words.extend(
                parts
                    .iter()
                    .filter(|part| part.len() >= SHORTEST_PART)
                    .map(|part| capitalise(part)),
            );
        }
    }
    words.sort();
    words.dedup();
    words
}

/// A grammar: named word lists, plus the shapes that combine them.
#[derive(Debug, Clone, Deserialize)]
pub struct Grammar {
    /// Word lists, by name.
    #[serde(default)]
    pub vocabulary: BTreeMap<String, Vec<String>>,
    /// Shapes to build names in.
    #[serde(default)]
    pub pattern: Vec<PatternSpec>,
}

/// One shape, as written in the grammar file.
#[derive(Debug, Clone, Deserialize)]
pub struct PatternSpec {
    /// What to call it in reports.
    pub name: String,
    /// Vocabularies to concatenate, in order.
    pub parts: Vec<String>,
    /// Why this shape is not swept, if it is not.
    ///
    /// A reason rather than a boolean, so a shape cannot be turned off without saying what it cost
    /// and what would bring it back. Kept in the file rather than deleted, since a shape too
    /// expensive today may pay once its vocabulary exists (D342).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<String>,
}

/// Why a grammar could not be used.
#[derive(Debug, thiserror::Error)]
pub enum GrammarError {
    /// The file was not valid TOML, or did not match the expected shape.
    #[error("parsing the grammar: {0}")]
    Parse(#[from] toml::de::Error),
    /// A pattern referred to a vocabulary that does not exist.
    ///
    /// Refused rather than skipped: a dropped part produces plausible, systematically wrong names.
    #[error("pattern {pattern} refers to vocabulary {missing}, which is not defined")]
    UnknownVocabulary {
        /// The pattern at fault.
        pattern: String,
        /// The vocabulary it wanted.
        missing: String,
    },
    /// A pattern has more parts than the generator will decode.
    ///
    /// Refused rather than truncated: the hot path decodes into a fixed stack array, and dropping
    /// parts would generate names the grammar does not describe.
    #[error("pattern {pattern} has {parts} parts, more than the {max} supported")]
    TooManyParts {
        /// The pattern at fault.
        pattern: String,
        /// How many it declared.
        parts: usize,
        /// The ceiling.
        max: usize,
    },
}

impl Grammar {
    /// Parses a grammar file.
    ///
    /// The `posix` vocabulary is added afterwards, derived rather than written (see
    /// [`posix_vocabulary`]); a grammar that never mentions it is unaffected.
    pub fn parse(text: &str) -> Result<Self, GrammarError> {
        let mut grammar: Self = toml::from_str(text)?;
        grammar
            .vocabulary
            .insert("posix".to_owned(), posix_vocabulary());
        Ok(grammar)
    }

    /// The grammar shipped with the tool.
    pub fn builtin() -> Result<Self, GrammarError> {
        Self::parse(DEFAULT_VENDOR_GRAMMAR)
    }

    /// Resolves every pattern against the vocabulary.
    ///
    /// Shapes carrying a [`PatternSpec::disabled`] reason are left out of the sweep, but still
    /// parsed and validated (D342).
    pub fn patterns(&self) -> Result<Vec<Pattern>, GrammarError> {
        // Validated first, filtered second, so a disabled shape naming a missing vocabulary fails now
        // rather than when it is re-enabled.
        let resolved = self
            .pattern
            .iter()
            .map(|spec| {
                let parts = spec
                    .parts
                    .iter()
                    .map(|part| {
                        self.vocabulary.get(part).cloned().ok_or_else(|| {
                            GrammarError::UnknownVocabulary {
                                pattern: spec.name.clone(),
                                missing: part.clone(),
                            }
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if parts.len() > MAX_PARTS {
                    return Err(GrammarError::TooManyParts {
                        pattern: spec.name.clone(),
                        parts: parts.len(),
                        max: MAX_PARTS,
                    });
                }
                Ok((
                    spec.disabled.is_none(),
                    Pattern::new(spec.name.clone(), parts),
                ))
            })
            .collect::<Result<Vec<_>, GrammarError>>()?;

        Ok(resolved
            .into_iter()
            .filter_map(|(swept, pattern)| swept.then_some(pattern))
            .collect())
    }

    /// Every shape the grammar holds but does not sweep, and why, so a sweep covering less than the
    /// file describes says so (D342).
    #[must_use]
    pub fn disabled(&self) -> Vec<(&str, &str)> {
        self.pattern
            .iter()
            .filter_map(|spec| Some((spec.name.as_str(), spec.disabled.as_deref()?)))
            .collect()
    }
}

/// Most parts a single pattern may have.
///
/// A fixed ceiling so the hot path decodes an index into a stack array rather than a vector.
pub const MAX_PARTS: usize = 12;

/// A resolved shape: the actual word lists, ready to enumerate.
#[derive(Debug, Clone)]
pub struct Pattern {
    /// What to call it in reports.
    pub name: String,
    /// The word lists, in order.
    ///
    /// Private so it cannot fall out of step with `len`, which is derived from it.
    parts: Vec<Vec<String>>,
    /// How many names `parts` can produce, fixed when the grammar was parsed.
    len: u64,
}

impl Pattern {
    /// Builds a pattern from its vocabularies, in order.
    pub fn new(name: String, parts: Vec<Vec<String>>) -> Self {
        // Saturating, so an absurd grammar reports an enormous count rather than wrapping to a small
        // one and searching a fraction of what was asked for.
        let len = if parts.is_empty() {
            0
        } else {
            parts
                .iter()
                .try_fold(1_u64, |total, part| total.checked_mul(part.len() as u64))
                .unwrap_or(u64::MAX)
        };
        Self { name, parts, len }
    }

    /// How many names this pattern can produce: a field read, because the search asks for it on
    /// every candidate.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether it can produce nothing.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The `index`th name, or `None` past the end.
    ///
    /// The index is a mixed-radix number, least-significant part last, so consecutive indices vary
    /// the final word first and a partial search sweeps whole families of related names.
    pub fn name_at(&self, index: u64) -> Option<String> {
        if index >= self.len() {
            return None;
        }
        let mut rest = index;
        let mut chosen = vec![""; self.parts.len()];
        for (slot, part) in self.parts.iter().enumerate().rev() {
            let radix = part.len() as u64;
            chosen[slot] = &part[(rest % radix) as usize];
            rest /= radix;
        }
        Some(chosen.concat())
    }

    /// Which index would produce `name`, if any.
    ///
    /// The inverse of [`Self::name_at`]: once the name is split against the pattern's vocabularies,
    /// the index is the same arithmetic backwards, never a scan of the space. Splitting backtracks,
    /// since several words in one slot may start the remainder; the cost is bounded by vocabulary
    /// size times depth.
    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<u64> {
        let mut chosen = vec![0_usize; self.parts.len()];
        if !self.split_into(0, name, &mut chosen) {
            return None;
        }
        // Most-significant part first, mirroring the decode, which takes the last part as the least
        // significant digit; the other order yields a real index for a different name.
        let mut index: u64 = 0;
        for (slot, part) in self.parts.iter().enumerate() {
            index = index
                .checked_mul(part.len() as u64)?
                .checked_add(chosen[slot] as u64)?;
        }
        (index < self.len()).then_some(index)
    }

    /// Chooses a word from each remaining slot that spells `rest` exactly.
    ///
    /// Depth-first with backtracking: a greedy match can take a prefix that leaves a remainder no
    /// later slot can spell.
    fn split_into(&self, slot: usize, rest: &str, chosen: &mut [usize]) -> bool {
        let Some(part) = self.parts.get(slot) else {
            // Every slot filled: this is a match only if the whole name was consumed.
            return rest.is_empty();
        };
        for (position, word) in part.iter().enumerate() {
            if let Some(remainder) = rest.strip_prefix(word.as_str()) {
                if self.split_into(slot + 1, remainder, chosen) {
                    chosen[slot] = position;
                    return true;
                }
            }
        }
        false
    }

    /// Writes the `index`th name into `buffer`, replacing its contents.
    ///
    /// No allocation per candidate, so the allocator does not dominate a large search. Returns
    /// `false` past the end, leaving the buffer empty.
    pub fn write_at(&self, index: u64, buffer: &mut Vec<u8>) -> bool {
        buffer.clear();
        if index >= self.len() {
            return false;
        }
        // Decode the index from the last part backwards, then emit forwards; the digits come out
        // reversed.
        debug_assert!(
            self.parts.len() <= MAX_PARTS,
            "Grammar::patterns refuses anything longer, so this cannot happen"
        );
        let mut rest = index;
        let mut chosen: [usize; MAX_PARTS] = [0; MAX_PARTS];
        for (slot, part) in self.parts.iter().enumerate().rev() {
            let radix = part.len() as u64;
            chosen[slot] = (rest % radix) as usize;
            rest /= radix;
        }
        for (slot, part) in self.parts.iter().enumerate() {
            buffer.extend_from_slice(part[chosen[slot]].as_bytes());
        }
        true
    }

    /// Every name this pattern produces.
    pub fn iter(&self) -> impl Iterator<Item = String> + '_ {
        (0..self.len()).filter_map(|i| self.name_at(i))
    }
}

/// Reads a newline-separated word list, ignoring blanks and `#` comments.
pub fn word_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// The standard-library names shipped with the tool.
pub fn standard_names() -> Vec<String> {
    word_list(DEFAULT_STANDARD_NAMES)
}

#[cfg(test)]
mod index_tests {
    use super::Pattern;

    /// A pattern with several words per slot, and one word that is a prefix of another.
    fn pattern() -> Pattern {
        Pattern::new(
            "test".to_owned(),
            vec![
                vec!["sce".to_owned()],
                vec!["Kernel".to_owned(), "Net".to_owned()],
                // `Create` and `CreateEx` overlap on purpose: a greedy match takes the short one and leaves a
                // remainder the last slot cannot spell.
                vec!["Create".to_owned(), "CreateEx".to_owned()],
                vec!["Sema".to_owned(), "Ex".to_owned()],
            ],
        )
    }

    /// Every index round-trips through the name it produces, stated against `name_at` rather than a
    /// hand-written table.
    #[test]
    fn every_index_round_trips_through_the_name_it_produces() {
        let pattern = pattern();
        for index in 0..pattern.len() {
            let name = pattern.name_at(index).expect("in range");
            assert_eq!(
                pattern.index_of(&name),
                Some(index),
                "{name} came from {index}"
            );
        }
    }

    /// A choice that strands a later slot is backtracked: `sceKernelCreateEx` is `Create` + `Ex`
    /// only if the earlier slot gives up `CreateEx`.
    #[test]
    fn a_choice_that_strands_a_later_slot_is_backtracked() {
        let pattern = pattern();
        let index = pattern
            .index_of("sceKernelCreateEx")
            .expect("Create + Ex spells it");
        assert_eq!(pattern.name_at(index).as_deref(), Some("sceKernelCreateEx"));
    }

    /// A name outside the pattern is refused rather than mapped to something.
    #[test]
    fn a_name_this_pattern_cannot_spell_has_no_index() {
        let pattern = pattern();
        assert_eq!(pattern.index_of("sceKernelDestroySema"), None);
        assert_eq!(pattern.index_of("sceKernelCreate"), None, "short by a slot");
        assert_eq!(pattern.index_of(""), None);
    }
}

#[cfg(test)]
mod tests {
    use super::{Grammar, Pattern, standard_names, word_list};

    fn pattern(parts: &[&[&str]]) -> Pattern {
        Pattern::new(
            "test".to_owned(),
            parts
                .iter()
                .map(|p| p.iter().map(|s| (*s).to_owned()).collect())
                .collect(),
        )
    }

    /// A pattern counts the product of its parts.
    #[test]
    fn a_pattern_counts_the_product_of_its_parts() {
        // The count drives how the search is split, so an inaccurate one leaves space unsearched.
        let p = pattern(&[&["a", "b"], &["x", "y", "z"]]);
        assert_eq!(p.len(), 6);
        assert!(!p.is_empty());
    }

    /// Every index produces a distinct name, and the set is complete.
    #[test]
    fn every_index_produces_a_distinct_name_and_the_set_is_complete() {
        // A collision would shrink the search space and a gap would skip candidates.
        let p = pattern(&[&["sce", "x"], &["Kernel", "Audio"], &["Open", "Close"]]);
        let all: Vec<String> = p.iter().collect();
        assert_eq!(all.len(), 8);
        let unique: std::collections::BTreeSet<_> = all.iter().collect();
        assert_eq!(unique.len(), 8, "indices must not collide");
        assert!(all.contains(&"sceKernelOpen".to_owned()));
        assert!(all.contains(&"xAudioClose".to_owned()));
    }

    /// The last part varies fastest.
    #[test]
    fn the_last_part_varies_fastest() {
        // So a partial search sweeps whole families of related names.
        let p = pattern(&[&["a", "b"], &["1", "2", "3"]]);
        let all: Vec<String> = p.iter().collect();
        assert_eq!(all, vec!["a1", "a2", "a3", "b1", "b2", "b3"]);
    }

    /// An index past the end produces nothing rather than wrapping.
    #[test]
    fn an_index_past_the_end_produces_nothing_rather_than_wrapping() {
        // Thread ranges may overshoot the end; wrapping would re-search the beginning.
        let p = pattern(&[&["a"], &["b"]]);
        assert_eq!(p.name_at(0).as_deref(), Some("ab"));
        assert_eq!(p.name_at(1), None);
        assert_eq!(p.name_at(u64::MAX), None);
    }

    /// A pattern with no parts produces nothing, not one empty name.
    #[test]
    fn a_pattern_with_no_parts_produces_nothing_not_one_empty_name() {
        // An empty name hashes to something, which would be reported as a match.
        let p = pattern(&[]);
        assert_eq!(p.len(), 0);
        assert!(p.is_empty());
        assert_eq!(p.name_at(0), None);
    }

    /// A pattern naming an unknown vocabulary is refused, not skipped.
    #[test]
    fn a_pattern_naming_an_unknown_vocabulary_is_refused_not_skipped() {
        // A dropped part would produce plausible, systematically wrong names.
        let g = Grammar::parse(
            r#"
            [vocabulary]
            prefix = ["sce"]
            [[pattern]]
            name = "broken"
            parts = ["prefix", "nonexistent"]
            "#,
        )
        .expect("parse");
        assert!(g.patterns().is_err());
    }

    /// The builtin grammar is valid and produces names.
    #[test]
    fn the_builtin_grammar_is_valid_and_produces_names() {
        // It ships with the tool, so a typo in it breaks the feature for everyone.
        let g = Grammar::builtin().expect("the builtin grammar must parse");
        let patterns = g.patterns().expect("and resolve");
        assert!(!patterns.is_empty(), "there should be patterns");
        let total: u64 = patterns.iter().map(Pattern::len).sum();
        assert!(total > 100_000, "the search space is only {total}");
        for p in &patterns {
            assert!(
                p.name_at(0).is_some_and(|n| !n.is_empty()),
                "pattern {} produced an empty first name",
                p.name
            );
        }
    }

    /// Comments and blank lines are ignored in a word list.
    #[test]
    fn comments_and_blank_lines_are_ignored_in_a_word_list() {
        let list = word_list("# a comment\n\nalpha\n  beta  \n\n# another\ngamma\n");
        assert_eq!(list, vec!["alpha", "beta", "gamma"]);
    }

    /// The standard names cover every harvested library, system-call stubs included.
    #[test]
    fn the_standard_names_cover_every_library_harvested() {
        // Read from FreeBSD's own version scripts (D126). One name per library, because a harvest that
        // drops a whole library still reports success.
        let names = standard_names();
        assert!(names.len() > 2000, "only {} names", names.len());
        for expected in [
            "memcpy",         // libc, string
            "snprintf",       // libc, stdio
            "__cxa_atexit",   // libc, C++ runtime - reserved, and the most-called
            "pthread_create", // libthr, whose script is `pthread.map`
            "sqrt",           // msun
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "{expected} should be present"
            );
        }

        // System-call stubs, declared in `lib/libsys/Symbol.sys.map`.
        for expected in ["clock_gettime", "socket", "sched_yield"] {
            assert!(
                names.iter().any(|n| n == expected),
                "{expected} is declared in lib/libsys and should be harvested"
            );
        }
    }
    /// Writing into a buffer agrees with building a string.
    #[test]
    fn writing_into_a_buffer_agrees_with_building_a_string() {
        // The buffer path runs on every candidate, so it must agree with the string path.
        let p = pattern(&[&["sce", "x"], &["Kernel", "Audio"], &["Open", "Close"]]);
        let mut buffer = Vec::new();
        for i in 0..p.len() {
            assert!(p.write_at(i, &mut buffer));
            assert_eq!(
                std::str::from_utf8(&buffer).expect("ascii"),
                p.name_at(i).expect("in range"),
                "index {i}"
            );
        }
    }

    /// Writing past the end reports it and leaves the buffer empty.
    #[test]
    fn writing_past_the_end_reports_it_and_leaves_the_buffer_empty() {
        // An overshooting thread must not hash what the last candidate left behind.
        let p = pattern(&[&["a"], &["b"]]);
        let mut buffer = vec![0xFF; 8];
        assert!(!p.write_at(99, &mut buffer));
        assert!(buffer.is_empty());
    }

    /// A pattern with too many parts is refused rather than truncated.
    #[test]
    fn a_pattern_with_too_many_parts_is_refused_rather_than_truncated() {
        // Dropping parts past the ceiling would generate names the grammar does not describe.
        let parts: Vec<String> = (0..=super::MAX_PARTS).map(|_| "w".to_owned()).collect();
        let mut vocabulary = std::collections::BTreeMap::new();
        vocabulary.insert("w".to_owned(), vec!["a".to_owned()]);
        let g = Grammar {
            vocabulary,
            pattern: vec![super::PatternSpec {
                name: "too-long".to_owned(),
                parts,
                disabled: None,
            }],
        };
        assert!(g.patterns().is_err());
    }
    /// The POSIX shape regenerates names the vendor parts cannot spell.
    #[test]
    fn the_posix_shape_regenerates_names_the_parts_could_not_spell() {
        // Names confirmed by hash but not generable cannot be accounted for by the provenance audit
        // (D213); this rule regenerates them from the harvested list. The vendor inherited these names
        // from POSIX whole, so no module, verb and object combination spells them.
        let vocab = super::posix_vocabulary();
        for expected in [
            "PthreadMutexattrInit",
            "PthreadMutexattrSettype",
            "PthreadMutexattrSetprotocol",
            "PthreadMutexattrDestroy",
        ] {
            assert!(
                vocab.iter().any(|v| v == expected),
                "{expected} should be derivable from the harvested list"
            );
        }
    }

    /// The derived vocabulary reaches every grammar.
    #[test]
    fn the_derived_vocabulary_reaches_every_grammar() {
        // Added after parsing, so a user grammar gets it too; a grammar that never mentions `posix` is
        // unchanged.
        let g = Grammar::parse(
            "[vocabulary]
prefix = [\"sce\"]
",
        )
        .expect("parse");
        assert!(g.vocabulary.contains_key("posix"));
        assert!(!g.vocabulary["posix"].is_empty());
    }

    /// The shipped grammar uses the POSIX shape.
    #[test]
    fn the_shipped_grammar_uses_the_posix_shape() {
        // A vocabulary nothing references generates nothing.
        let g = Grammar::builtin().expect("builtin");
        assert!(
            g.pattern
                .iter()
                .any(|p| p.parts.iter().any(|x| x == "posix")),
            "some pattern must actually use it"
        );
        g.patterns().expect("and it must resolve");
    }

    /// A disabled shape is left out of the sweep but still validated (D342).
    #[test]
    fn a_disabled_shape_is_left_out_of_the_sweep_but_still_checked() {
        let mut vocabulary = std::collections::BTreeMap::new();
        vocabulary.insert("w".to_owned(), vec!["a".to_owned()]);

        let with_a_typo = Grammar {
            vocabulary: vocabulary.clone(),
            pattern: vec![super::PatternSpec {
                name: "off".to_owned(),
                parts: vec!["nosuchlist".to_owned()],
                disabled: Some("costs more than it earns".to_owned()),
            }],
        };
        assert!(
            with_a_typo.patterns().is_err(),
            "a disabled shape naming a missing vocabulary is still a broken grammar"
        );

        let sound = Grammar {
            vocabulary,
            pattern: vec![
                super::PatternSpec {
                    name: "on".to_owned(),
                    parts: vec!["w".to_owned()],
                    disabled: None,
                },
                super::PatternSpec {
                    name: "off".to_owned(),
                    parts: vec!["w".to_owned()],
                    disabled: Some("costs more than it earns".to_owned()),
                },
            ],
        };
        let swept = sound.patterns().expect("resolves");
        assert_eq!(swept.len(), 1, "only the enabled shape is swept");
        assert_eq!(swept[0].name, "on");
        assert_eq!(
            sound.disabled(),
            vec![("off", "costs more than it earns")],
            "and the one left out is reportable rather than merely absent"
        );
    }

    /// No swept shape uses the `learned` slot twice, which would make the vocabulary cost quadratic
    /// (D342).
    #[test]
    fn no_swept_shape_uses_the_learned_slot_twice() {
        let grammar = Grammar::builtin().expect("the shipped grammar parses");

        for spec in grammar.pattern.iter().filter(|s| s.disabled.is_none()) {
            let uses = spec.parts.iter().filter(|p| *p == "learned").count();
            assert!(
                uses < 2,
                "{} sweeps `learned` {uses} times - quadratic in the slot the ceiling caps",
                spec.name
            );
        }
        assert_eq!(
            grammar.disabled().len(),
            2,
            "and both are still in the file, with their reasons"
        );
    }
}
