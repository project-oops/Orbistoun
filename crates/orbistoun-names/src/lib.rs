//! Generating candidate symbol names, so a hash can be turned back into one.
//!
//! A NID is a truncated SHA-1 and is not invertible, so there is exactly one way back:
//! hash names you can think of and see which ones match. Everything here exists to
//! think of a great many names cheaply.
//!
//! # Two very different sources, and only one of them is guesswork
//!
//! - **Published standards.** The target's C library is FreeBSD-derived, so a large
//!   part of it is ISO C and POSIX with the names those standards fix. Those are not
//!   guesses at all, and they are exactly the lawful reference principle 1 points at.
//! - **Vendor naming conventions.** Everything else follows a regular shape - a prefix,
//!   a module, an action, an object - so candidates can be enumerated combinatorially.
//!   This is guesswork, but structured guesswork, and a match is self-verifying: the
//!   hash either agrees or it does not.
//!
//! # The vocabulary is data, not code
//!
//! Adding a word must never mean a rebuild (principle 5). Defaults are embedded so the
//! tool works out of the box, and any file of the same shape replaces them.
//!
//! # Why patterns are indexable rather than iterated
//!
//! Each pattern can produce its `n`th name directly, by treating the index as a
//! mixed-radix number over its vocabularies. That is what lets the search be split
//! across threads by range with no shared state and no coordination - and it makes the
//! generator testable, since a specific index has a specific answer.

pub mod harvest;
pub mod solve;
pub mod strings;

use std::collections::BTreeMap;

use serde::Deserialize;

/// Vocabulary and patterns shipped with the tool.
///
/// A starting point, not an authority - the file is expected to grow as names are
/// confirmed, and a user file replaces it entirely.
pub const DEFAULT_VENDOR_GRAMMAR: &str = include_str!("../data/vendor.toml");

/// Names fixed by ISO C and POSIX, which the target's C library is derived from.
pub const DEFAULT_STANDARD_NAMES: &str = include_str!("../data/standard.txt");

/// Every published name, respelled the way the vendor spells it.
///
/// # One rule, and it is a derivation rather than a guess
///
/// The target's C library is FreeBSD-derived and its threading interface is POSIX with a
/// vendor prefix. The respelling is mechanical: take a harvested name, capitalise each
/// underscore-separated part, and join them. `pthread_mutexattr_settype` becomes
/// `PthreadMutexattrSettype`, and with the prefix that is the exact symbol a real title
/// imports.
///
/// **Checked against names the generator could not previously reach.** Two titles printed
/// four of these themselves once `printf` existed to carry the message (D187); this rule
/// regenerates all four from the harvested list alone, which is what turns them from
/// *observed* into *derivable* and lets the provenance audit account for them.
///
/// Why it was missing: the vocabulary was built from vendor-shaped parts - a module, a
/// verb, an object - and no combination of those spells `Mutexattr`. The gap was never a
/// missing word. It was a missing *shape*, and the shape is "a POSIX name, whole" (D189).
///
/// Cheap: one candidate per harvested name, against millions from the compositional
/// patterns.
pub fn posix_vocabulary() -> Vec<String> {
    /// Shortest underscore part worth keeping on its own.
    ///
    /// Two letters and under are `in`, `t`, `vm` - they combine with everything and buy
    /// nothing, and each one multiplies a pattern's candidates by the size of the list.
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
        // The whole name, joined - what a vendor name inherits wholesale, and what this
        // function produced on its own until now.
        let joined: String = parts.iter().copied().map(capitalise).collect();
        if !joined.is_empty() {
            words.push(joined);
        }
        // **And each part on its own.** A vendor name borrows a *morpheme*, not always a
        // whole standard name: `pmap_unset` and `rpcb_unset` both carry `unset`, and
        // joining them produced `PmapUnset` and `RpcbUnset` while `Unset` - the piece a
        // vendor name actually reuses - was never offered to the generator at all.
        //
        // This is the same shape as the gap that kept `sceKernelUsleep` out of reach: the
        // material was present and the form it was presented in could not be used (D258).
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
    /// # Why a reason rather than a boolean
    ///
    /// **Presence is the disabling**, so a shape cannot be turned off without saying what it
    /// cost and what would bring it back. The same rule `CompatEntry::reason` already
    /// carries: an entry without one is how a file becomes a graveyard of unexplained
    /// exceptions - and a shape switched off by a bare `false` is exactly that, six months
    /// later, to somebody deciding whether to switch it on again.
    ///
    /// Kept in the file rather than deleted, because a shape that costs more than it earns
    /// *today* may be the right shape once the vocabulary it needs exists. Deleting loses the
    /// measurement; this keeps it where the next person will find it (D342).
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
    /// Refused rather than skipped: a silently dropped part produces names that look
    /// plausible and are systematically wrong, and the search then reports nothing with
    /// no indication why.
    #[error("pattern {pattern} refers to vocabulary {missing}, which is not defined")]
    UnknownVocabulary {
        /// The pattern at fault.
        pattern: String,
        /// The vocabulary it wanted.
        missing: String,
    },
    /// A pattern has more parts than the generator will decode.
    ///
    /// Refused rather than truncated. The hot path decodes an index into a fixed stack
    /// array, and quietly dropping the parts past the end would generate names that are
    /// not the ones the grammar describes.
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
    /// The `posix` vocabulary is added afterwards, derived rather than written - see
    /// [`posix_vocabulary`]. A grammar that never mentions it is unaffected.
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
    /// **Shapes carrying a [`PatternSpec::disabled`] reason are left out**, so a disabled
    /// shape costs a sweep nothing rather than being skipped by whoever remembers to. It is
    /// still parsed, still validated against the vocabulary, and still in the file with its
    /// reason attached - only unswept (D342).
    pub fn patterns(&self) -> Result<Vec<Pattern>, GrammarError> {
        // **Validated first, filtered second.** Skipping a disabled shape before resolving it
        // would let one carry a vocabulary name that does not exist - and the error would
        // surface only when somebody re-enabled it, which is the moment they have least
        // context for it.
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

    /// Every shape the grammar holds but does not sweep, and why.
    ///
    /// **So a disabled shape is reportable rather than merely absent.** A sweep that silently
    /// covered less than the file describes would be the same failure as a report that
    /// counted its silent diagnostics: the thing that is not happening is the thing worth
    /// saying (D331, D342).
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
/// A fixed ceiling so the hot path can decode an index into a stack array rather than a
/// vector. Generous - a name built from more than this many pieces is not a naming
/// convention, it is a sentence.
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
        // Saturating, so an absurd grammar reports an enormous number rather than
        // wrapping to a small one - which would silently search a fraction of what was
        // asked for.
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

    /// How many names this pattern can produce.
    ///
    /// A field read. The search asks every pattern this question for every candidate it
    /// tries, so computing it walked the whole vocabulary list - one heap allocation per
    /// part - billions of times, to arrive at a number that was fixed before the search
    /// began (D216).
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether it can produce nothing.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The `index`th name, or `None` past the end.
    ///
    /// The index is read as a mixed-radix number, least-significant part **last**, so
    /// consecutive indices vary the final word first. That ordering matters for a
    /// partial search: it sweeps whole families of related names rather than one word
    /// from each of many.
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
    /// **The inverse of [`Self::name_at`], and it exists because searching for it was costing
    /// hours.** `name_at` reads the index as a mixed-radix number; recovering it is the same
    /// arithmetic backwards, once the name has been split against the vocabularies that
    /// produced it. `solve::derive` was walking `0..len()` instead - a linear scan of a space
    /// this project measures in trillions, for something a division answers (D304).
    ///
    /// Splitting needs backtracking: several words in one slot may start the remainder, and
    /// only some of those choices leave a suffix the later slots can spell. Bounded by
    /// vocabulary size times depth rather than by their product.
    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<u64> {
        let mut chosen = vec![0_usize; self.parts.len()];
        if !self.split_into(0, name, &mut chosen) {
            return None;
        }
        // Most-significant part first, mirroring the decode - which takes the *last* part as
        // the least significant digit. Assembling it the other way round yields a real index
        // for a different name, which is the failure mode worth being explicit about.
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
    /// Depth-first with backtracking. A greedy longest-first match is not enough: a short word
    /// can consume a prefix that leaves a remainder no later slot can spell, and the name is
    /// then reported as ungenerable when it is not.
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
    /// The form a large search wants: no allocation per candidate. Testing billions of
    /// names means the allocator, not SHA-1, decides how long the search takes unless
    /// the buffer is reused.
    ///
    /// Returns `false` past the end, leaving the buffer empty.
    pub fn write_at(&self, index: u64, buffer: &mut Vec<u8>) -> bool {
        buffer.clear();
        if index >= self.len() {
            return false;
        }
        // Decode the mixed-radix index from the last part backwards, then emit forwards -
        // the digits come out in reverse order, and a name assembled in that order is a
        // different name that happens to hash to something.
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
                // `Create` and `CreateEx` overlap on purpose: a greedy match takes the short
                // one and leaves a remainder the last slot cannot spell.
                vec!["Create".to_owned(), "CreateEx".to_owned()],
                vec!["Sema".to_owned(), "Ex".to_owned()],
            ],
        )
    }

    /// **The property, stated against `name_at` rather than against a table.**
    ///
    /// Every index this pattern can produce round-trips. A hand-written expectation would be a
    /// second implementation of the encoding, and the two would drift.
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

    /// A greedy split would give up here; backtracking does not.
    ///
    /// `sceKernelCreateEx` can be spelled `Create` + `Ex`, but only if the earlier slot gives
    /// up `CreateEx` first. Taking the longest match and stopping reports a name the pattern
    /// *can* produce as one it cannot (D304).
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

    #[test]
    fn a_pattern_counts_the_product_of_its_parts() {
        // The count drives how the search is split across threads, so an inaccurate one
        // means part of the space is never looked at.
        let p = pattern(&[&["a", "b"], &["x", "y", "z"]]);
        assert_eq!(p.len(), 6);
        assert!(!p.is_empty());
    }

    #[test]
    fn every_index_produces_a_distinct_name_and_the_set_is_complete() {
        // A collision here would silently shrink the search space; a gap would skip
        // candidates. Both are invisible without checking directly.
        let p = pattern(&[&["sce", "x"], &["Kernel", "Audio"], &["Open", "Close"]]);
        let all: Vec<String> = p.iter().collect();
        assert_eq!(all.len(), 8);
        let unique: std::collections::BTreeSet<_> = all.iter().collect();
        assert_eq!(unique.len(), 8, "indices must not collide");
        assert!(all.contains(&"sceKernelOpen".to_owned()));
        assert!(all.contains(&"xAudioClose".to_owned()));
    }

    #[test]
    fn the_last_part_varies_fastest() {
        // So a partial search sweeps whole families of related names rather than one
        // word from each of many.
        let p = pattern(&[&["a", "b"], &["1", "2", "3"]]);
        let all: Vec<String> = p.iter().collect();
        assert_eq!(all, vec!["a1", "a2", "a3", "b1", "b2", "b3"]);
    }

    #[test]
    fn an_index_past_the_end_produces_nothing_rather_than_wrapping() {
        // Threads take ranges that may overshoot the end; wrapping would make them
        // re-search the beginning and report duplicates.
        let p = pattern(&[&["a"], &["b"]]);
        assert_eq!(p.name_at(0).as_deref(), Some("ab"));
        assert_eq!(p.name_at(1), None);
        assert_eq!(p.name_at(u64::MAX), None);
    }

    #[test]
    fn a_pattern_with_no_parts_produces_nothing_not_one_empty_name() {
        // An empty name hashes to something, and that something would be reported as a
        // match for whatever it collided with.
        let p = pattern(&[]);
        assert_eq!(p.len(), 0);
        assert!(p.is_empty());
        assert_eq!(p.name_at(0), None);
    }

    #[test]
    fn a_pattern_naming_an_unknown_vocabulary_is_refused_not_skipped() {
        // Dropping the part silently would produce names that look plausible and are
        // systematically wrong, and the search would report nothing with no hint why.
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

    #[test]
    fn comments_and_blank_lines_are_ignored_in_a_word_list() {
        let list = word_list("# a comment\n\nalpha\n  beta  \n\n# another\ngamma\n");
        assert_eq!(list, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn the_standard_names_cover_every_library_harvested() {
        // Not guesses - read from FreeBSD's own version scripts (D126). One name per
        // library, because a harvest that silently drops a whole library still reports
        // success: `libthr` declares its exports in `pthread.map`, and a harvester
        // looking only for `Symbol.map` lost every `pthread_*` name while announcing
        // 2,497 of them (D127).
        let names = standard_names();
        assert!(names.len() > 2000, "only {} names", names.len());
        for expected in [
            "memcpy",         // libc, string
            "snprintf",       // libc, stdio
            "__cxa_atexit",   // libc, C++ runtime - reserved, and the most-called
            "pthread_create", // libthr, which lives in a differently-named script
            "sqrt",           // msun
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "{expected} should be present"
            );
        }

        // Syscall stubs, which this asserted the *absence* of until 2026-08-22.
        //
        // The note here read: FreeBSD generates these from `syscalls.master` at build
        // time, so no version script declares them and no harvest can find them. That was
        // stated as a fact about the world and was an inference from a search that missed
        // - `lib/libsys/Symbol.sys.map` declares every one of them, and the harvest was
        // skipping the file because its walker tested for the name `Symbol.map` (D191).
        //
        // The assertion is kept, inverted. It was written to fail if the belief ever
        // stopped holding, and that is exactly what it did - which is the whole argument
        // for asserting a known limitation rather than writing it in a comment.
        for expected in ["clock_gettime", "socket", "sched_yield"] {
            assert!(
                names.iter().any(|n| n == expected),
                "{expected} is declared in lib/libsys and should be harvested"
            );
        }
    }
    #[test]
    fn writing_into_a_buffer_agrees_with_building_a_string() {
        // Two ways to produce the same name, and the fast one is the one that runs
        // billions of times - so it is the one that must not drift.
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

    #[test]
    fn writing_past_the_end_reports_it_and_leaves_the_buffer_empty() {
        // A thread whose range overshoots must not hash whatever the last candidate
        // left behind, which would be reported as a match for the wrong index.
        let p = pattern(&[&["a"], &["b"]]);
        let mut buffer = vec![0xFF; 8];
        assert!(!p.write_at(99, &mut buffer));
        assert!(buffer.is_empty());
    }

    #[test]
    fn a_pattern_with_too_many_parts_is_refused_rather_than_truncated() {
        // Dropping the parts past the ceiling would generate names the grammar does not
        // describe, and the search would report nothing with no hint why.
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
    #[test]
    fn the_posix_shape_regenerates_names_the_parts_could_not_spell() {
        // **The check that makes this a derivation rather than a story.** Two titles
        // printed these four names themselves, and the hash confirmed them - but a name
        // the generator cannot produce is one the provenance audit cannot account for
        // (D119). This rule regenerates all four from the harvested list alone.
        //
        // It also pins what the gap actually was. No combination of module, verb and
        // object spells `Mutexattr`, because the vendor did not compose that name - it
        // inherited it from POSIX whole. The vocabulary was never missing a word.
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

    #[test]
    fn the_derived_vocabulary_reaches_every_grammar() {
        // Added after parsing, so a user grammar gets it too - and a grammar that never
        // mentions `posix` is unchanged, which is why adding it silently is safe.
        let g = Grammar::parse(
            "[vocabulary]
prefix = [\"sce\"]
",
        )
        .expect("parse");
        assert!(g.vocabulary.contains_key("posix"));
        assert!(!g.vocabulary["posix"].is_empty());
    }

    #[test]
    fn the_shipped_grammar_uses_the_posix_shape() {
        // A vocabulary nothing references generates nothing, and would look wired up.
        let g = Grammar::builtin().expect("builtin");
        assert!(
            g.pattern
                .iter()
                .any(|p| p.parts.iter().any(|x| x == "posix")),
            "some pattern must actually use it"
        );
        g.patterns().expect("and it must resolve");
    }

    /// **A disabled shape is not swept, and is still validated.**
    ///
    /// Both halves matter. Skipping it before resolving would let one carry a vocabulary name
    /// that does not exist, and the error would surface only when somebody re-enabled it -
    /// the moment they have least context for it (D342).
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

    /// The shipped grammar turns off the two shapes that made the vocabulary quadratic.
    ///
    /// Pinned because the whole argument for the ceiling depends on it: `learned` appearing
    /// twice in a shape caps the list at 483 words against 16,042, and 6,966 corpus names are
    /// blocked on vocabulary against 1,025 on shapes (D342).
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
