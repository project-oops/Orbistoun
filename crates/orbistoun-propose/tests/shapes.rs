//! Is the next name short of a word, or short of a shape?
//!
//! ```text
//! cargo test -p orbistoun-propose --release --test shapes -- --nocapture
//! ```
//!
//! Most database names were found by the grammar, so measuring shape coverage against them
//! measures the search that produced them. This uses only names found without the grammar
//! (module strings, traces, published standards): each is split into known words, each word
//! mapped to the lists holding it, and the pattern list asked whether any shape spells the
//! sequence. A word in several lists gives several candidate shapes, and an empty entry (no
//! suffix) lets a longer pattern spell a shorter name. The output counts reachable and
//! unreachable names and ranks the shapes that would fix the rest.

use orbistoun_names::Grammar;
use std::collections::{BTreeMap, BTreeSet};

/// Names the grammar did not find, and therefore did not select for.
const INDEPENDENT: [&str; 3] = ["static", "runtime", "published-standard"];

/// What every vendor pattern starts with, and so what this can say anything about.
const VENDOR_PREFIX: &str = "sce";

/// Reports whether independently found vendor names are short of vocabulary or of shapes.
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
        // Vendor-shaped only: most independently found names are POSIX, libc or C++ mangled
        // symbols, which the `sce`-prefixed vendor patterns are not meant to spell.
        if !name.starts_with(VENDOR_PREFIX) {
            continue;
        }
        sampled += 1;
        let Some(parts) = orbistoun_propose::vocabulary::decompose(name, &words) else {
            // Not a shape question: the grammar lacks the words to split this name, so it is short of
            // vocabulary. Where it stalled says which word.
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

    // An empty sample or a decomposition that never succeeds would report "no missing shapes",
    // the strongest-looking result from an experiment that did not run.
    assert!(
        sampled > 0,
        "no independently-found names to measure against"
    );
    assert!(
        reachable + unreachable.values().map(Vec::len).sum::<usize>() > 0,
        "not one sampled name could be split into known words - the measurement is empty"
    );
}

/// Everything the measurement found, in the order a person would want it; the loop decides
/// what is true and this decides how to say it.
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
/// Not the missing word: a longest-first split can consume a short word that fits and stall
/// just after it. The position is enough to see many names stalling in the same place.
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

/// How many candidates the whole pattern list produces.
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
/// A position whose word could come from several lists is costed at the smallest, so every
/// figure is a floor.
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
/// Recursive because an empty entry consumes a part without a word, letting a five-part
/// pattern spell a four-word name.
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

/// One line naming the shape a decomposition takes: the lists each word could have come
/// from, in order. A word in two lists gives the name two shapes, and both are kept.
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
