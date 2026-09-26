//! Deriving a candidate name from a name this project already holds.
//!
//! [`crate::Grammar`] builds names from words, so it reaches `sceKernelGetAppInfo` but never
//! `sceKernelGetAppInfo2`: that needs a rule applied to a finished name. A kernel export table
//! shows one function under two names, the second being the first with an affix (D606). A seed
//! is a name already proved by hash, so `snprintf_s` rests on `snprintf`, which rests on a
//! published standard. A match is proof; a miss proves only that the tried variants failed.

use serde::Deserialize;

use orbistoun_nid::{Derivation, Method, Nid, NidHasher, today};

use crate::solve::{SearchStats, Solved, Targets};

/// The affix rules shipped with this repository.
const DEFAULT_AFFIXES: &str = include_str!("../data/affixes.toml");

/// One rewriting of a name into another name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Substitution {
    /// What to look for.
    pub from: String,
    /// What to put in its place.
    pub to: String,
}

/// Every rule that turns a held name into a candidate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Affixes {
    /// Prepended, verbatim. Includes the empty string.
    #[serde(default)]
    pub prefixes: Vec<String>,
    /// Appended, verbatim. Includes the empty string.
    #[serde(default)]
    pub suffixes: Vec<String>,
    /// Applied where the left side appears.
    #[serde(default)]
    pub substitution: Vec<Substitution>,
}

/// A rule set that could not be read.
#[derive(Debug, thiserror::Error)]
pub enum AffixError {
    /// The file is not valid TOML, or not the shape this expects.
    #[error("the affix rules could not be read: {0}")]
    Unreadable(#[from] toml::de::Error),
}

impl Affixes {
    /// Reads rules from TOML.
    ///
    /// # Errors
    ///
    /// When the text is not valid TOML or does not have the shape this expects.
    pub fn parse(text: &str) -> Result<Self, AffixError> {
        Ok(toml::from_str(text)?)
    }

    /// The rules shipped in this repository.
    ///
    /// # Errors
    ///
    /// When the shipped file has been edited into something unreadable.
    pub fn builtin() -> Result<Self, AffixError> {
        Self::parse(DEFAULT_AFFIXES)
    }

    /// How many candidates one seed produces, at most.
    #[must_use]
    pub fn per_seed(&self) -> usize {
        self.prefixes.len() * self.suffixes.len() + self.substitution.len()
    }

    /// Every candidate this rule set derives from one seed, each with the rule that made it.
    ///
    /// The identity is not offered: empty prefix and suffix reproduce the seed, which is already
    /// known and must not be recorded as derived from itself. A substitution that does not apply
    /// likewise yields nothing.
    pub fn variants_of<'a>(&'a self, seed: &'a str) -> impl Iterator<Item = (String, String)> + 'a {
        let affixed = self.prefixes.iter().flat_map(move |prefix| {
            self.suffixes
                .iter()
                .filter(move |suffix| !prefix.is_empty() || !suffix.is_empty())
                .map(move |suffix| {
                    (
                        format!("{prefix}{seed}{suffix}"),
                        rule_label(prefix, suffix),
                    )
                })
        });
        let substituted = self
            .substitution
            .iter()
            .filter(move |rule| seed.contains(&rule.from))
            .map(move |rule| {
                (
                    seed.replace(&rule.from, &rule.to),
                    format!("{}>{}", rule.from, rule.to),
                )
            });
        affixed.chain(substituted)
    }

    /// Whether `name` is what applying `rule` to `seed` produces: one rule against one string,
    /// with no grammar or index to resolve.
    #[must_use]
    pub fn produces(&self, seed: &str, rule: &str, name: &str) -> bool {
        self.variants_of(seed)
            .any(|(candidate, applied)| applied == rule && candidate == name)
    }
}

/// How a rule is spelled in a derivation record.
///
/// Both ends always, so `_foo`, `foo_r` and `_foo_r` stay distinct.
fn rule_label(prefix: &str, suffix: &str) -> String {
    format!("{prefix}*{suffix}")
}

/// Hashes every variant of every seed, keeping those that match something wanted.
///
/// Seeds must be names the database holds: a guessed seed would record a proved name as built
/// from an unproved one, which the provenance audit cannot catch.
pub fn solve_affixed<I, S>(
    hasher: &NidHasher,
    targets: &Targets,
    affixes: &Affixes,
    seeds: I,
) -> (Vec<Solved>, SearchStats)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let stamp = today();
    let mut found: std::collections::BTreeMap<u64, Solved> = std::collections::BTreeMap::new();
    let mut tried = 0;
    for seed in seeds {
        let seed = seed.as_ref();
        for (candidate, rule) in affixes.variants_of(seed) {
            tried += 1;
            let nid = hasher.hash(&candidate);
            if targets.wants(nid) {
                found.entry(nid.as_raw()).or_insert_with(|| Solved {
                    nid,
                    name: candidate.clone(),
                    derivation: Derivation::new(
                        Method::Affixed {
                            seed: seed.to_owned(),
                            rule: rule.clone(),
                        },
                        &stamp,
                    ),
                });
            }
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

/// Searches every rule for one that derives `name` from a held seed.
///
/// The audit's question: could `name` have been derived from held names and shipped rules at
/// all? `None` means the name needs explaining.
#[must_use]
pub fn derive_affixed<'a>(
    name: &str,
    affixes: &Affixes,
    seeds: impl IntoIterator<Item = &'a str>,
) -> Option<Derivation> {
    let stamp = today();
    for seed in seeds {
        // A name is never its own seed; `variants_of` refuses it, and so does this, for a caller
        // handing over the whole database.
        if seed == name {
            continue;
        }
        if let Some((_, rule)) = affixes
            .variants_of(seed)
            .find(|(candidate, _)| candidate == name)
        {
            return Some(Derivation::new(
                Method::Affixed {
                    seed: seed.to_owned(),
                    rule,
                },
                &stamp,
            ));
        }
    }
    None
}

/// Whether `nid` is what `name` hashes to. For a caller checking a record it did not write.
#[must_use]
pub fn hashes_to(hasher: &NidHasher, name: &str, nid: Nid) -> bool {
    hasher.hash(name) == nid
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped rules parse and offer both prefixes and suffixes.
    #[test]
    fn the_shipped_rules_parse_and_offer_both_ends() {
        let affixes = Affixes::builtin().expect("the shipped affix rules must parse");
        assert!(
            affixes.prefixes.iter().any(String::is_empty),
            "an empty prefix must be offered, or no suffix-only variant is reachable"
        );
        assert!(
            affixes.suffixes.iter().any(String::is_empty),
            "an empty suffix must be offered, or no prefix-only variant is reachable"
        );
        assert!(!affixes.substitution.is_empty(), "and the substitutions");
    }

    /// A seed is never offered back as its own variant.
    #[test]
    fn a_seed_is_never_offered_back_as_its_own_variant() {
        // Empty prefix and suffix reproduce the seed, and a record deriving `snprintf` from itself
        // would pass every other check in this crate.
        let affixes = Affixes::builtin().expect("parses");
        assert!(
            !affixes
                .variants_of("snprintf")
                .any(|(candidate, _)| candidate == "snprintf"),
            "the identity must not be offered"
        );
    }

    /// The shipped rules reach every variant the export-table evidence shows.
    #[test]
    fn the_rules_reach_the_variants_the_export_table_showed() {
        // Each pair is either half of a doubly-exported kernel function or a name proved once the rules
        // existed: the evidence the file was written from (D606).
        let affixes = Affixes::builtin().expect("parses");
        for (seed, wanted) in [
            ("getpeername", "_getpeername"),
            ("inet_pton", "__inet_pton"),
            ("fileno", "fileno_unlocked"),
            ("snprintf", "snprintf_s"),
            ("mmap", "mmap2"),
            ("sceKernelGetAppInfo", "sceKernelGetAppInfo2"),
            ("_ZNSt14error_categoryD2Ev", "_ZNSt14error_categoryD1Ev"),
            ("_ZNSt6_WinitC1Ev", "_ZNSt6_WinitC2Ev"),
        ] {
            assert!(
                affixes
                    .variants_of(seed)
                    .any(|(candidate, _)| candidate == wanted),
                "no rule derives {wanted} from {seed}"
            );
        }
    }

    /// A rule that does not apply produces nothing rather than the seed.
    #[test]
    fn a_rule_that_does_not_apply_produces_nothing_rather_than_the_seed() {
        // A substitution whose left side is absent must not quietly yield the seed back.
        let affixes = Affixes {
            prefixes: vec![String::new()],
            suffixes: vec![String::new()],
            substitution: vec![Substitution {
                from: "C1E".to_owned(),
                to: "C2E".to_owned(),
            }],
        };
        assert_eq!(affixes.variants_of("memcpy").count(), 0);
        assert_eq!(affixes.variants_of("_ZC1Ev").count(), 1);
    }

    /// A recheck refuses a rule that does not produce the name.
    #[test]
    fn a_recheck_refuses_a_rule_that_does_not_produce_the_name() {
        // Asserting on the refusal: a `produces` that answered true for everything would pass every
        // audit.
        let affixes = Affixes::builtin().expect("parses");
        assert!(affixes.produces("snprintf", "*_s", "snprintf_s"));
        assert!(
            !affixes.produces("snprintf", "*_s", "snprintf_r"),
            "a rule that produces a different name must be refused"
        );
        assert!(
            !affixes.produces("snprintf", "*_r", "snprintf_s"),
            "and so must the right name under the wrong rule"
        );
        assert!(
            !affixes.produces("printf", "*_s", "snprintf_s"),
            "and the right name under the wrong seed"
        );
    }

    /// A solved affix carries the seed and the rule that made it.
    #[test]
    fn a_solved_affix_carries_the_seed_and_the_rule_that_made_it() {
        let hasher = NidHasher::default();
        let affixes = Affixes::builtin().expect("parses");
        let wanted = hasher.hash("snprintf_s");
        let targets = Targets::new([wanted]);
        let (solved, stats) = solve_affixed(&hasher, &targets, &affixes, ["snprintf"]);
        assert_eq!(solved.len(), 1, "one seed, one hit");
        assert_eq!(solved[0].name, "snprintf_s");
        assert_eq!(stats.found, 1);
        let Method::Affixed { seed, rule } = &solved[0].derivation.method else {
            panic!("an affixed name must record how it was affixed");
        };
        assert_eq!(seed, "snprintf");
        assert!(
            affixes.produces(seed, rule, &solved[0].name),
            "the recorded rule must reproduce the name it claims"
        );
    }

    /// Nothing is found when no rule reaches the target.
    #[test]
    fn nothing_is_found_when_no_rule_reaches_the_target() {
        let hasher = NidHasher::default();
        let affixes = Affixes::builtin().expect("parses");
        let targets = Targets::new([hasher.hash("something_no_rule_spells")]);
        let (solved, stats) = solve_affixed(&hasher, &targets, &affixes, ["snprintf", "memcpy"]);
        assert!(solved.is_empty(), "a miss must stay a miss");
        assert_eq!(stats.found, 0);
        assert!(stats.tried > 0, "and it must have actually tried");
    }

    /// The audit re-derives a name without the search that found it.
    #[test]
    fn the_audit_can_re_derive_a_name_without_the_search_that_found_it() {
        let affixes = Affixes::builtin().expect("parses");
        let held = ["snprintf", "memcpy", "getpeername"];
        let found = derive_affixed("_getpeername", &affixes, held).expect("derivable");
        let Method::Affixed { seed, rule } = &found.method else {
            panic!("wrong method");
        };
        assert_eq!(seed, "getpeername");
        assert!(affixes.produces(seed, rule, "_getpeername"));
        assert!(
            derive_affixed("sceKernelSomethingElse", &affixes, held).is_none(),
            "a name no rule reaches must come back unaccounted for"
        );
        assert!(
            derive_affixed("memcpy", &affixes, held).is_none(),
            "and a held name must not be recorded as derived from itself"
        );
    }
}
