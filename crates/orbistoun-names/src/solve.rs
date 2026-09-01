//! Searching a candidate space for names that hash to wanted values.
//!
//! # A match is proof, and a miss proves nothing
//!
//! This is the rare part of the project with a real oracle. A candidate either hashes
//! to a wanted value or it does not, and there is no judgement involved - so a reported
//! name is correct, not probable. What the search cannot do is tell you a name does not
//! exist; it only ever says "not in what was tried".
//!
//! That asymmetry is why the vocabulary is data. Extending it is the whole method.
//!
//! # Splitting the work
//!
//! Patterns are indexable, so a search is a range of integers. Threads take disjoint
//! ranges and share nothing but the target set, which is read-only. No coordination, no
//! locks, and the result is identical regardless of how many threads ran.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

use orbistoun_nid::{Derivation, Method, Nid, NidHasher};

use crate::Pattern;

/// A name that was proved to hash to a wanted value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Solved {
    /// The hash that was being looked for.
    pub nid: Nid,
    /// The name that produces it.
    pub name: String,
    /// Which of this repository's inputs produced it, and where.
    ///
    /// Recorded at the moment of discovery rather than reconstructed afterwards. A
    /// provenance record assembled later is a reconstruction; one written by the code
    /// that did the work is evidence (D073).
    pub derivation: Derivation,
}

/// What a search cost and found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Candidates hashed.
    pub tried: u64,
    /// Wanted values that were still unknown when the search began.
    pub wanted: usize,
    /// Wanted values that were named.
    pub found: usize,
}

/// The set of hashes a search is looking for.
#[derive(Debug, Clone)]
pub struct Targets {
    wanted: HashSet<u64>,
}

impl Targets {
    /// Builds a target set.
    pub fn new(nids: impl IntoIterator<Item = Nid>) -> Self {
        Self {
            wanted: nids.into_iter().map(Nid::as_raw).collect(),
        }
    }

    /// How many distinct hashes are wanted.
    pub fn len(&self) -> usize {
        self.wanted.len()
    }

    /// Whether nothing is wanted.
    pub fn is_empty(&self) -> bool {
        self.wanted.is_empty()
    }

    /// Whether this hash is one of them.
    pub fn wants(&self, nid: Nid) -> bool {
        self.wanted.contains(&nid.as_raw())
    }
}

/// How many candidates one thread takes before asking for more.
///
/// Large enough that the coordination cost disappears, small enough that threads finish
/// within a similar time of each other.
pub const CHUNK: u64 = 64 * 1024;

/// Hashes a list of names, keeping those that match.
///
/// Used for published standard-library names, where the list is fixed and short enough
/// that splitting it would cost more than it saves.
pub fn solve_names<I, S>(
    hasher: &NidHasher,
    targets: &Targets,
    names: I,
    derivation: &Derivation,
) -> (Vec<Solved>, SearchStats)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut found = BTreeMap::new();
    let mut tried = 0;
    for name in names {
        tried += 1;
        let nid = hasher.hash(name.as_ref());
        if targets.wants(nid) {
            found.entry(nid.as_raw()).or_insert_with(|| Solved {
                nid,
                name: name.as_ref().to_owned(),
                derivation: derivation.clone(),
            });
        }
    }
    let solved: Vec<Solved> = found.into_values().collect();
    let stats = SearchStats {
        tried,
        wanted: targets.len(),
        found: solved.len(),
    };
    (solved, stats)
}

/// Searches every pattern, across `threads` threads.
///
/// The result does not depend on the thread count: findings are collected into a map
/// keyed by hash, so the same names come back whatever order they were found in.
pub fn solve_patterns(
    hasher: &NidHasher,
    targets: &Targets,
    patterns: &[Pattern],
    threads: usize,
) -> (Vec<Solved>, SearchStats) {
    // Gathered into one contiguous array before the threads start. `Pattern::len` is a
    // field read now, but the patterns themselves are scattered across the heap, and the
    // scan below touches every one of them for every candidate - so the lengths are
    // worth having in a single cache line (D216).
    let lens: Vec<u64> = patterns.iter().map(Pattern::len).collect();
    let total: u64 = lens.iter().sum();
    if total == 0 || targets.is_empty() {
        return (
            Vec::new(),
            SearchStats {
                tried: 0,
                wanted: targets.len(),
                found: 0,
            },
        );
    }

    // Stamped once, outside the threads: a search is one act, and a name found at the
    // start of it did not happen on a different day from one found at the end.
    let today = orbistoun_nid::today();
    let next = std::sync::atomic::AtomicU64::new(0);
    let found: Mutex<BTreeMap<u64, Solved>> = Mutex::new(BTreeMap::new());
    let threads = threads.max(1);

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                let mut mine: Vec<Solved> = Vec::new();
                // One buffer for the whole thread. Allocating per candidate would make
                // the allocator, not SHA-1, decide how long a billion-name search takes.
                let mut name = Vec::with_capacity(64);
                loop {
                    let start = next.fetch_add(CHUNK, std::sync::atomic::Ordering::Relaxed);
                    if start >= total {
                        break;
                    }
                    let end = start.saturating_add(CHUNK).min(total);
                    for index in start..end {
                        // Locate the pattern this global index falls in. Patterns are
                        // few, so a scan costs less than the bookkeeping to avoid it.
                        let mut offset = index;
                        for (pattern, &len) in patterns.iter().zip(&lens) {
                            if offset < len {
                                if pattern.write_at(offset, &mut name) {
                                    let nid = hasher.hash_bytes(&name);
                                    if targets.wants(nid) {
                                        // Allocated only on a match, which is rare
                                        // enough to be free.
                                        mine.push(Solved {
                                            nid,
                                            name: String::from_utf8_lossy(&name).into_owned(),
                                            // Pattern and index together identify this
                                            // one candidate out of hundreds of millions,
                                            // so the claim can be rechecked in isolation.
                                            derivation: Derivation::new(
                                                Method::Generated {
                                                    pattern: pattern.name.clone(),
                                                    index: offset,
                                                },
                                                &today,
                                            ),
                                        });
                                    }
                                }
                                break;
                            }
                            offset -= len;
                        }
                    }
                }
                if !mine.is_empty() {
                    // Locked once per thread at the end rather than per match, so the
                    // hot path stays entirely local.
                    let mut shared = found
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    for solved in mine {
                        shared.entry(solved.nid.as_raw()).or_insert(solved);
                    }
                }
            });
        }
    });

    let solved: Vec<Solved> = found
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .into_values()
        .collect();
    let stats = SearchStats {
        tried: total,
        wanted: targets.len(),
        found: solved.len(),
    };
    (solved, stats)
}

/// Whether a recorded derivation actually produces the name it claims.
///
/// **The audit primitive.** A stored derivation is a claim, and this is what turns it
/// into a check. Verifying one is a single array lookup, so auditing a database of
/// thousands costs less than a millisecond - which is what makes it reasonable to
/// re-run on every commit rather than once, hopefully, before publishing (D073).
///
/// Returns `false` when the claim does not hold: the pattern does not exist in the
/// current grammar, the index is out of range, or it produces a different name. All
/// three mean the same thing to a reader - this repository, as it stands, does not
/// derive that name the way the file says it does.
pub fn verify(
    name: &str,
    derivation: &Derivation,
    patterns: &[Pattern],
    standard: &[String],
) -> bool {
    match &derivation.method {
        Method::PublishedStandard { .. } => standard.iter().any(|n| n == name),
        Method::Generated { pattern, index } => patterns
            .iter()
            .find(|p| &p.name == pattern)
            .and_then(|p| p.name_at(*index))
            .is_some_and(|produced| produced == name),
        // None of these can be rechecked from this repository alone, which is the only
        // material CI holds. That is not the same as unverifiable: a static harvest is
        // reproducible by anyone with the module, deterministically, and a runtime one by
        // anyone willing to run it. `Method::reproducible` says which, and the audit
        // prints them by tier rather than lumping them together (D213). What none of them
        // may do is count as re-derived here, because that would be the one lie this
        // whole mechanism exists to prevent.
        Method::Static { .. } | Method::Runtime { .. } | Method::Supplied { .. } => false,
    }
}

/// Searches the whole space for a derivation of `name`, for names with no record.
///
/// Expensive - it walks every candidate - and that is the point: it answers "could this
/// repository have produced this name at all?" with no help from the file being
/// audited. A name it cannot account for is precisely the one that needs explaining.
pub fn derive(name: &str, patterns: &[Pattern], standard: &[String]) -> Option<Derivation> {
    let today = orbistoun_nid::today();
    if standard.iter().any(|n| n == name) {
        return Some(Derivation::new(
            Method::PublishedStandard {
                list: STANDARD_LIST.to_owned(),
            },
            &today,
        ));
    }
    for pattern in patterns {
        // **Computed, not searched.** `name_at` reads the index as a mixed-radix number, so
        // recovering it is the same arithmetic backwards - and walking `0..len()` instead was a
        // linear scan of a space measured in trillions, which took five hours on thirty-three
        // names and did not finish (D304).
        if let Some(index) = pattern.index_of(name) {
            {
                return Some(Derivation::new(
                    Method::Generated {
                        pattern: pattern.name.clone(),
                        index,
                    },
                    &today,
                ));
            }
        }
    }
    None
}

/// Name of the shipped standard-library list, as recorded in a derivation.
pub const STANDARD_LIST: &str = "crates/orbistoun-names/data/standard.txt";

#[cfg(test)]
mod tests {
    use super::{Targets, solve_names, solve_patterns};
    use crate::Pattern;
    use orbistoun_nid::{Derivation, Method, NidHasher};

    /// An arbitrary suffix. The real one is a runtime input and deliberately absent
    /// from the tree (D006); nothing here depends on its value.
    fn hasher() -> NidHasher {
        NidHasher::new(vec![0x01, 0x02, 0x03, 0x04])
    }

    /// The derivation a published-standard match carries.
    fn standard() -> Derivation {
        Derivation::new(
            Method::PublishedStandard {
                list: super::STANDARD_LIST.to_owned(),
            },
            "2026-01-01",
        )
    }

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
    fn a_name_in_the_space_is_found_and_is_the_right_one() {
        // The one place in this project with a real oracle: the hash either agrees or
        // it does not, so a match is proof rather than a guess.
        let h = hasher();
        let wanted = h.hash("sceKernelOpen");
        let targets = Targets::new([wanted]);
        let patterns = [pattern(&[
            &["sce"],
            &["Kernel", "Audio"],
            &["Open", "Close", "Read"],
        ])];

        let (solved, stats) = solve_patterns(&h, &targets, &patterns, 4);
        assert_eq!(solved.len(), 1);
        assert_eq!(solved[0].name, "sceKernelOpen");
        assert_eq!(solved[0].nid, wanted);
        assert_eq!(stats.tried, 6);
        assert_eq!(stats.found, 1);
        assert_eq!(stats.wanted, 1);
    }

    #[test]
    fn the_result_is_the_same_however_many_threads_ran() {
        // Threads take disjoint ranges and merge into a map keyed by hash, so a
        // different split cannot change the answer - only how long it takes.
        let h = hasher();
        let targets = Targets::new([h.hash("sceAudioClose"), h.hash("sceKernelRead")]);
        let patterns = [pattern(&[
            &["sce"],
            &["Kernel", "Audio", "Video"],
            &["Open", "Close", "Read", "Write"],
        ])];

        let single = solve_patterns(&h, &targets, &patterns, 1).0;
        let many = solve_patterns(&h, &targets, &patterns, 8).0;
        assert_eq!(single, many);
        assert_eq!(single.len(), 2);
    }

    #[test]
    fn a_name_outside_the_space_is_simply_not_found() {
        // A miss proves nothing at all - only that it was not in what was tried. That
        // asymmetry is why the vocabulary is data rather than code.
        let h = hasher();
        let targets = Targets::new([h.hash("sceSomethingNobodyGuessed")]);
        let patterns = [pattern(&[&["sce"], &["Kernel"], &["Open"]])];

        let (solved, stats) = solve_patterns(&h, &targets, &patterns, 2);
        assert!(solved.is_empty());
        assert_eq!(stats.found, 0);
        assert_eq!(stats.wanted, 1, "still reported as wanted");
    }

    #[test]
    fn an_empty_target_set_searches_nothing_rather_than_everything() {
        // Hashing millions of candidates against nothing is pure waste, and it takes
        // long enough to look like a hang.
        let h = hasher();
        let patterns = [pattern(&[&["a", "b"], &["c", "d"]])];
        let (solved, stats) = solve_patterns(&h, &Targets::new([]), &patterns, 4);
        assert!(solved.is_empty());
        assert_eq!(stats.tried, 0);
    }

    #[test]
    fn a_plain_name_list_resolves_too() {
        // Published standard-library names are not guesses, so they get searched
        // directly rather than generated.
        let h = hasher();
        let targets = Targets::new([h.hash("memcpy")]);
        let (solved, stats) = solve_names(&h, &targets, ["malloc", "memcpy", "free"], &standard());
        assert_eq!(solved.len(), 1);
        assert_eq!(solved[0].name, "memcpy");
        assert_eq!(stats.tried, 3);
    }

    #[test]
    fn each_hash_is_reported_once_even_if_two_names_collide() {
        // A truncated hash can collide. Reporting both would put two names on one
        // import and make the database ambiguous.
        let h = hasher();
        let targets = Targets::new([h.hash("dup")]);
        let (solved, _) = solve_names(&h, &targets, ["dup", "dup", "dup"], &standard());
        assert_eq!(solved.len(), 1);
    }
    #[test]
    fn a_solved_name_records_where_it_came_from() {
        // Written by the code that did the work, not reconstructed later. A provenance
        // record assembled afterwards is a reconstruction; this is evidence.
        let h = hasher();
        let targets = Targets::new([h.hash("sceKernelOpen")]);
        let patterns = [pattern(&[&["sce"], &["Kernel"], &["Open", "Close"]])];
        let (solved, _) = solve_patterns(&h, &targets, &patterns, 2);

        match &solved[0].derivation.method {
            Method::Generated { pattern, index } => {
                assert_eq!(pattern, "test");
                // The recorded index must actually produce the recorded name.
                assert_eq!(
                    patterns[0].name_at(*index).as_deref(),
                    Some("sceKernelOpen")
                );
            }
            other => panic!("expected a generated derivation, got {other:?}"),
        }
    }

    #[test]
    fn a_recorded_derivation_verifies_against_the_grammar() {
        // The audit primitive: a claim becomes a check.
        let patterns = [pattern(&[&["sce"], &["Kernel"], &["Open", "Close"]])];
        let standard = vec!["memcpy".to_owned()];

        let good = Derivation::new(
            Method::Generated {
                pattern: "test".to_owned(),
                index: 1,
            },
            "2026-01-01",
        );
        assert!(super::verify("sceKernelClose", &good, &patterns, &standard));
        assert!(
            !super::verify("sceKernelOpen", &good, &patterns, &standard),
            "index 1 is Close, not Open"
        );
    }

    #[test]
    fn a_derivation_naming_a_pattern_that_no_longer_exists_fails() {
        // Grammars change. A name whose derivation stopped being reproducible is
        // exactly what an audit exists to surface, rather than quietly passing.
        let patterns = [pattern(&[&["a"], &["b"]])];
        let stale = Derivation::new(
            Method::Generated {
                pattern: "deleted-pattern".to_owned(),
                index: 0,
            },
            "2026-01-01",
        );
        assert!(!super::verify("ab", &stale, &patterns, &[]));
    }

    #[test]
    fn an_out_of_range_index_fails_rather_than_wrapping() {
        let patterns = [pattern(&[&["a"], &["b"]])];
        let bad = Derivation::new(
            Method::Generated {
                pattern: "test".to_owned(),
                index: 9_999,
            },
            "2026-01-01",
        );
        assert!(!super::verify("ab", &bad, &patterns, &[]));
    }

    #[test]
    fn nothing_outside_this_repository_verifies_mechanically() {
        // All three are legitimate; none can be rechecked from this repository alone,
        // which is the only material CI holds. Calling any of them re-derived would be
        // the one lie this whole mechanism exists to prevent.
        let harvested = Derivation::new(
            Method::Static {
                by: orbistoun_nid::StaticSource::ModuleStrings,
                from: "titles/EXAMPLE/eboot.bin".to_owned(),
            },
            "2026-01-01",
        );
        let ran = Derivation::new(
            Method::Runtime {
                by: orbistoun_nid::RuntimeSource::CallTrace,
                how: "seen while debugging a title".to_owned(),
            },
            "2026-01-01",
        );
        let supplied = Derivation::new(
            Method::Supplied {
                source: "somewhere".to_owned(),
            },
            "2026-01-01",
        );
        for d in [&harvested, &ran, &supplied] {
            assert!(!super::verify("anything", d, &[], &[]));
            assert!(!d.method.is_mechanically_checkable());
        }

        // But they are not the same thing, and the audit must not treat them alike.
        assert!(harvested.method.is_our_own_work());
        assert!(ran.method.is_our_own_work());
        assert!(!supplied.method.is_our_own_work());
    }

    #[test]
    fn the_tiers_separate_static_harvesting_from_running_something() {
        // The distinction the old single `observed` value could not make. 137 of the 154
        // names carrying it had never run anything, while its own documentation said
        // "watching something run" (D213).
        use orbistoun_nid::{Evidence, Reproducible};

        let harvested = Method::Static {
            by: orbistoun_nid::StaticSource::ModuleStrings,
            from: "titles/EXAMPLE/eboot.bin".to_owned(),
        };
        let ran = Method::Runtime {
            by: orbistoun_nid::RuntimeSource::ArgumentDump,
            how: "a run of EXAMPLE".to_owned(),
        };

        assert_eq!(harvested.evidence(), Evidence::Static);
        assert_eq!(ran.evidence(), Evidence::Runtime);
        assert_ne!(harvested.evidence(), ran.evidence());

        // And the tiers are ordered by what somebody else has to hold, which is what the
        // audit sorts by.
        assert!(harvested.reproducible() < ran.reproducible());
        assert!(Reproducible::FromRepository < harvested.reproducible());
        assert!(
            ran.reproducible()
                < Method::Supplied {
                    source: "elsewhere".to_owned()
                }
                .reproducible()
        );
    }

    #[test]
    fn an_unrecorded_name_can_be_re_derived_from_scratch() {
        // Answers "could this repository have produced this name at all?" with no help
        // from the file being audited.
        let patterns = [pattern(&[&["sce"], &["Kernel"], &["Open", "Close"]])];
        let standard = vec!["memcpy".to_owned()];

        assert!(super::derive("sceKernelClose", &patterns, &standard).is_some());
        assert!(super::derive("memcpy", &patterns, &standard).is_some());
        assert!(
            super::derive("sceSomethingElse", &patterns, &standard).is_none(),
            "a name outside the space must not be accounted for"
        );
    }
}
