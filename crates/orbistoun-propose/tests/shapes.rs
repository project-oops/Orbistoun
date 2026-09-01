//! Is the next name short of a word, or short of a shape?
//!
//! ```text
//! cargo test -p orbistoun-propose --release --test shapes -- --nocapture
//! ```
//!
//! # Why this is not answered by the audit
//!
//! `orbistoun-cli audit` asks whether the grammar can re-derive every name in the
//! database, and today it can. That sounds like full coverage and is not, because of a
//! selection effect: most of those names were *found* by the grammar, so of course the
//! grammar can spell them. Measuring shape coverage against them measures the search that
//! produced them.
//!
//! The names found **without** the grammar are the ones worth measuring. A name read out
//! of a module's own strings, or seen going past in a trace, is a sample of what vendor
//! identifiers actually look like that owes nothing to the pattern list. If those need
//! shapes the grammar does not have, the pattern list is the binding constraint and buying
//! more vocabulary is spending on the wrong lever.
//!
//! # What it reports
//!
//! Every such name is split into known words, each word is mapped to the vocabulary lists
//! holding it, and the pattern list is asked whether any of its shapes can spell that
//! sequence. What comes out is a count of reachable and unreachable, and for the
//! unreachable a ranked list of the shapes that would fix them.
//!
//! A word can sit in several lists, so a name has several possible shapes and only one has
//! to match. An empty entry - how a shape says "no suffix" - lets a longer pattern spell a
//! shorter name, so the matcher may consume a part without consuming a word.

use orbistoun_names::Grammar;
use std::collections::{BTreeMap, BTreeSet};

/// Names the grammar did not find, and therefore did not select for.
const INDEPENDENT: [&str; 3] = ["static", "runtime", "published-standard"];

/// What every vendor pattern starts with, and so what this can say anything about.
const VENDOR_PREFIX: &str = "sce";

#[test]
fn is_the_next_name_short_of_a_word_or_short_of_a_shape() {
    let grammar = Grammar::builtin().expect("the shipped grammar parses");
    let words = every_word(&grammar);
    let database = database();

    let mut reachable = 0_usize;
    let mut undecomposable = Vec::new();
    let mut unreachable: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    let mut sampled = 0_usize;

    for (name, found) in &database {
        if !INDEPENDENT.contains(&found.as_str()) {
            continue;
        }
        // **Vendor-shaped only, and the first cut of this was wrong without it.** Of the
        // independently-found names, 276 of 464 are POSIX or libc - `fopen_s`, `gmtime_s`,
        // C++ mangled symbols. The vendor patterns all begin with the `sce` prefix and
        // cannot spell those, and are not meant to; counting them turned a 68% vocabulary
        // gap out of a sample two thirds of which the grammar was never aiming at.
        if !name.starts_with(VENDOR_PREFIX) {
            continue;
        }
        sampled += 1;
        let Some(parts) = orbistoun_propose::vocabulary::decompose(name, &words) else {
            // Not a shape question: the grammar does not hold the words to split it at
            // all, so this name is short of vocabulary rather than short of a pattern.
            // Which word it is short of is the useful part, so record where it stalled.
            undecomposable.push(name.clone());
            *missing.entry(stalled_on(name, &words)).or_default() += 1;
            continue;
        };
        let slots: Vec<BTreeSet<String>> =
            parts.iter().map(|w| slots_holding(&grammar, w)).collect();
        if grammar
            .pattern
            .iter()
            .any(|spec| spells(&grammar, &spec.parts, &slots, 0, 0))
        {
            reachable += 1;
        } else {
            unreachable
                .entry(signature(&slots))
                .or_default()
                .push(name.clone());
        }
    }

    report(
        &grammar,
        &database,
        sampled,
        reachable,
        &unreachable,
        &undecomposable,
        &missing,
    );

    // **The failure this guards is measuring nothing.** A decomposition that never
    // succeeds, or a sample that is empty, produces "no missing shapes" - which reads as
    // the strongest possible result and would be drawn from an experiment that did not run.
    assert!(
        sampled > 0,
        "no independently-found names to measure against"
    );
    assert!(
        reachable + unreachable.values().map(Vec::len).sum::<usize>() > 0,
        "not one sampled name could be split into known words - the measurement is empty"
    );
}

/// Everything the measurement found, in the order a person would want it.
///
/// Split out because the measurement should read as a measurement: the loop above decides
/// what is true and this decides how to say it, and mixing them made one function that did
/// both and was longer than the lint allows.
fn report(
    grammar: &Grammar,
    database: &[(String, String)],
    sampled: usize,
    reachable: usize,
    unreachable: &BTreeMap<String, Vec<String>>,
    undecomposable: &[String],
    missing: &BTreeMap<String, usize>,
) {
    eprintln!(
        "SHAPES  {} names in the database, {sampled} vendor-shaped and found without the grammar",
        database.len()
    );
    eprintln!("        reachable under the current pattern list : {reachable}");
    eprintln!(
        "        unreachable, needing a shape               : {}",
        unreachable.values().map(Vec::len).sum::<usize>()
    );
    eprintln!(
        "        not splittable into known words at all      : {}",
        undecomposable.len()
    );

    if unreachable.is_empty() {
        eprintln!(concat!(
            "\n  Every independently-found name is spellable. On this evidence the pattern ",
            "list is not the binding constraint, and vocabulary is where the next name ",
            "comes from."
        ));
    } else {
        eprintln!("\n  Shapes the grammar does not have, most names first:");
        let mut ranked: Vec<_> = unreachable.iter().collect();
        ranked.sort_by_key(|(_, names)| std::cmp::Reverse(names.len()));
        let whole = whole_space(grammar);
        eprintln!(
            concat!(
                "    names  cost of the cheapest pattern that would spell it, against a ",
                "current space of {0} candidates"
            ),
            whole
        );
        for (shape, names) in ranked.iter().take(12) {
            let cost = cost_of(grammar, shape);
            eprintln!(
                "    {:<3}    +{:>7}%   {shape}",
                names.len(),
                cost.saturating_mul(100) / whole.max(1)
            );
            for name in names.iter().take(2) {
                eprintln!("               {name}");
            }
        }
        eprintln!(concat!(
            "
  Cost is the cheapest list for each ambiguous position, so it is a floor. ",
            "A shape costing a large multiple of the whole space is not affordable at any ",
            "vocabulary size and says the name has to come from somewhere other than the ",
            "generator."
        ));
    }

    if !undecomposable.is_empty() {
        eprintln!(concat!(
            "\n  Short of vocabulary rather than of a shape. The fragment each name stalls ",
            "on is the word that would unblock it, most names first:"
        ));
        let mut ranked: Vec<_> = missing.iter().collect();
        ranked.sort_by_key(|(fragment, names)| (std::cmp::Reverse(**names), (*fragment).clone()));
        for (fragment, names) in ranked.iter().take(10) {
            eprintln!("    {names:<3}  {fragment}");
        }
        eprintln!(concat!(
            "\n  A fragment is where a greedy split gave up, so it is the *rest* of the ",
            "name and not the word itself - `prSubmitCommandBuffer` means the split ate an ",
            "`A` and wanted `Apr`. It still says where to look."
        ));
    }
}

/// The part of a name a greedy split could not get past.
///
/// Not the missing word - a greedy longest-first split will happily consume a short word
/// that happens to fit and then stall one character later, so `sceKernelApr...` eats the
/// `A` and stops at `pr...`. What it gives is the position, which is enough to see that
/// twelve names all stall in the same place and one word would free them all.
fn stalled_on(name: &str, words: &[String]) -> String {
    let mut at = 0;
    while at < name.len() {
        let Some(word) = words.iter().find(|w| name[at..].starts_with(w.as_str())) else {
            return name[at..].to_owned();
        };
        at += word.len();
    }
    String::new()
}

/// How many candidates the whole pattern list produces today.
fn whole_space(grammar: &Grammar) -> u128 {
    grammar
        .pattern
        .iter()
        .map(|spec| {
            spec.parts
                .iter()
                .map(|part| grammar.vocabulary.get(part).map_or(1, Vec::len) as u128)
                .product::<u128>()
        })
        .sum()
}

/// What the cheapest pattern spelling a shape would cost.
///
/// A position a word could have come from more than one list is costed at the **smallest**
/// of them, because the question is what the cheapest pattern that would work costs. That
/// makes every figure here a floor, which is the honest direction for a number used to
/// decide whether to add a shape at all.
fn cost_of(grammar: &Grammar, shape: &str) -> u128 {
    shape
        .split(" + ")
        .map(|position| {
            position
                .split('|')
                .filter_map(|slot| grammar.vocabulary.get(slot).map(Vec::len))
                .min()
                .unwrap_or(1) as u128
        })
        .product()
}

/// Whether a pattern's parts can spell a sequence of words.
///
/// Recursive because a part may be consumed without consuming a word: an empty entry is
/// how a shape says "no suffix", and it lets a five-part pattern spell a four-word name.
fn spells(
    grammar: &Grammar,
    parts: &[String],
    slots: &[BTreeSet<String>],
    part: usize,
    word: usize,
) -> bool {
    if part == parts.len() {
        return word == slots.len();
    }
    let holds_empty = grammar
        .vocabulary
        .get(&parts[part])
        .is_some_and(|list| list.iter().any(String::is_empty));
    if holds_empty && spells(grammar, parts, slots, part + 1, word) {
        return true;
    }
    word < slots.len()
        && slots[word].contains(&parts[part])
        && spells(grammar, parts, slots, part + 1, word + 1)
}

/// Which vocabulary lists hold a word.
fn slots_holding(grammar: &Grammar, word: &str) -> BTreeSet<String> {
    grammar
        .vocabulary
        .iter()
        .filter(|(_, list)| list.iter().any(|w| w == word))
        .map(|(slot, _)| slot.clone())
        .collect()
}

/// One line naming the shape a decomposition takes.
///
/// The lists each word could have come from, in order. Ambiguity is kept rather than
/// resolved - a word in two lists genuinely gives the name two shapes, and picking one
/// would invent a precision the evidence does not have.
fn signature(slots: &[BTreeSet<String>]) -> String {
    slots
        .iter()
        .map(|s| {
            if s.is_empty() {
                "?".to_owned()
            } else {
                s.iter().cloned().collect::<Vec<_>>().join("|")
            }
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Every word the grammar knows, longest first, as [`decompose`] expects.
///
/// [`decompose`]: orbistoun_propose::vocabulary::decompose
fn every_word(grammar: &Grammar) -> Vec<String> {
    let mut words: Vec<String> = grammar
        .vocabulary
        .values()
        .flatten()
        .filter(|w| !w.is_empty())
        .cloned()
        .collect();
    words.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    words.dedup();
    words
}

/// Every confirmed name, with how it was found.
fn database() -> Vec<(String, String)> {
    let text =
        std::fs::read_to_string("../../symbols/generated.json").expect("the database exists");
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    parsed["derivations"]
        .as_object()
        .expect("a derivations object")
        .iter()
        .map(|(name, derivation)| {
            let found = derivation["found"].as_str().unwrap_or_default().to_owned();
            (name.clone(), found)
        })
        .collect()
}
