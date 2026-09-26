//! Does this name anything nobody had named?
//!
//! Every other test in this crate fakes the model; this asks a real one.
//!
//! ```text
//! cargo test -p orbistoun-propose --release --test live -- --ignored --nocapture
//! ```
//!
//! Release, because a debug run takes minutes. The sweep contains the whole existing
//! vocabulary as well as the new words (narrowing it corrupts the provenance index, D214), so
//! the control is computed first in the same process, and only names outside it count as
//! earned.

use std::collections::BTreeSet;

use orbistoun_llm::Llm;
use orbistoun_names::Grammar;
use orbistoun_names::solve::{Targets, solve_patterns};
use orbistoun_nid::{Nid, NidHasher};
use orbistoun_propose::bank::Bank;
use orbistoun_propose::vocabulary::{Context, Vocabulary};

/// The positions a round can extend, and what each holds.
///
/// Asked per position rather than for vocabulary at large. The suffix list is short and
/// multiplies nearly every shape, so shortest list first: one word changes the most there.
const SLOTS: &[(&str, &str)] = &[
    (
        "tail",
        concat!(
            "a short suffix that modifies the meaning of the whole name - a variant ",
            "marker, a debug marker, an asynchrony marker. Most names have none"
        ),
    ),
    (
        "verb",
        "the action the function performs, such as Create, Delete, Wait, Query",
    ),
    (
        "learned",
        "a noun naming the thing the function acts on, such as Sema, Equeue, Template",
    ),
];

/// Rounds per position, overridable at run time so a longer run needs no edit.
fn rounds_per_slot() -> u64 {
    std::env::var("ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(3)
}

/// Words asked for per round.
///
/// Small, because a model asked for many pads the list, and each padded word costs a place in
/// the round's budget and, at the `tail` position, a very large sweep.
const WANT: usize = 12;

fn hasher() -> NidHasher {
    NidHasher::new(orbistoun_nid::default_suffix())
}

/// The real unnamed hashes.
fn wanted() -> Vec<Nid> {
    let text = std::fs::read_to_string("../../symbols/wanted.txt").expect("the work list exists");
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| u64::from_str_radix(line.trim_start_matches("0x"), 16).ok())
        .map(Nid::from_raw)
        .collect()
}

/// Real confirmed names, to show the convention by example.
///
/// Vendor-shaped only: C++ ABI symbols and POSIX names would teach the wrong convention.
fn examples() -> Vec<String> {
    let text =
        std::fs::read_to_string("../../symbols/generated.json").expect("the database exists");
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let mut names: Vec<String> = parsed["names"]
        .as_array()
        .expect("a names array")
        .iter()
        .filter_map(|n| n.as_str())
        .filter(|n| n.starts_with("sce") && n.len() > 12)
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

/// What the model is told for one round.
///
/// A different slice of the examples each round, so the rounds ask different questions.
fn context_for(round_number: u64, role: &str, every_example: &[String]) -> Context {
    let window = (round_number as usize * 7) % every_example.len().max(1);
    let examples: Vec<String> = every_example
        .iter()
        .cycle()
        .skip(window)
        .take(8)
        .cloned()
        .collect();
    Context {
        // Libraries derived from this round's examples, so each round names the subsystems it is
        // looking at and the question narrows by subsystem as the window rotates.
        libraries: libraries_of(&examples),
        examples,
        theme: None,
        role: Some(role.to_owned()),
        want: WANT,
    }
}

/// The libraries a set of vendor-shaped names belongs to.
///
/// A name is `sce` + a module word + the rest, and the module words are a list the grammar
/// carries, so this reads the real segmentation. Longest first, because `Np` is a prefix of
/// `NpAuth` and the longer one is the library.
fn libraries_of(examples: &[String]) -> Vec<String> {
    let grammar = Grammar::builtin().expect("grammar");
    let mut modules: Vec<String> = grammar
        .vocabulary
        .get("module")
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| !m.is_empty())
        .collect();
    modules.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

    let mut seen = BTreeSet::new();
    for name in examples {
        let Some(rest) = name.strip_prefix("sce") else {
            continue;
        };
        if let Some(module) = modules.iter().find(|m| rest.starts_with(m.as_str())) {
            seen.insert(format!("libSce{module}"));
        }
    }
    seen.into_iter().collect()
}

/// Prints what one round did, including why suggestions were dropped: a round offering many
/// and accepting none is a different problem from one offering few.
fn report(round_number: u64, round: &orbistoun_propose::vocabulary::Round, new: &[&str]) {
    eprintln!(
        concat!(
            "  round {}: via={} ({}) offered={:<3} accepted={:<3} banked={} ",
            "swept={:<11} {:>6}ms  parsed={:?}"
        ),
        round_number,
        round.backend,
        round.model,
        round.offered,
        round.tried.len(),
        round.banked,
        round.stats.tried,
        round.swept_ms,
        round.parsed_as
    );
    eprintln!("      words: {:?}", round.tried);

    let mut reasons: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for refusal in &round.rejected {
        *reasons.entry(refusal.because.describe()).or_default() += 1;
    }
    if !reasons.is_empty() {
        eprintln!("      refused: {reasons:?}");
    }
    if new.is_empty() {
        eprintln!("      earned: nothing beyond the control");
    } else {
        eprintln!("      *** EARNED: {new:?}");
    }
}

/// Where a position's banked words live.
///
/// One file per position: a word proves itself in the slot it was swept in, and loading it
/// into another would generate shapes the convention does not use.
fn bank_path(slot: &str) -> String {
    format!("../../symbols/proposed-{slot}.txt")
}

/// What the vocabulary already names through the shapes that reach `slot`.
///
/// Computed from the proposer's own grammar, which holds everything banked, so a run is not
/// credited for names earlier runs made reachable.
fn control(grammar: &Grammar, slot: &str, targets: &Targets) -> BTreeSet<String> {
    let mut grammar = grammar.clone();
    grammar
        .pattern
        .retain(|spec| spec.parts.iter().any(|part| part == slot));
    if grammar.pattern.is_empty() {
        return BTreeSet::new();
    }
    let patterns = grammar.patterns().expect("patterns resolve");
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let (solved, _) = solve_patterns(&hasher(), targets, &patterns, threads);
    solved.into_iter().map(|s| s.name).collect()
}

/// Reports the names a model earns beyond the control, and that the experiment ran.
#[test]
#[ignore = "runs a model against the real work list; opt-in via --ignored"]
fn does_a_model_name_anything_nobody_had_named() {
    let dir = tempfile::tempdir().expect("temp dir");
    let llm = Llm::open(dir.path()).expect("the service opens");
    assert!(
        llm.is_available(),
        "nothing is configured here: {}",
        llm.host().summary()
    );

    let hashes = wanted();
    let every_example = examples();
    let targets = Targets::new(hashes.iter().copied());

    eprintln!(
        "LIVE  host={}\n      wanted={} examples={}",
        llm.host().summary(),
        hashes.len(),
        every_example.len()
    );

    let mut earned: BTreeSet<String> = BTreeSet::new();
    let mut proposed_total = 0_usize;
    let mut asked_rounds = 0_usize;
    let mut banked_total = 0_usize;

    for (slot, role) in SLOTS {
        // Banked words go in through the proposer, which keeps what this run adds. Kept beside the
        // symbol databases, since a bank outlives the run.
        let bank = Bank::open(bank_path(slot)).expect("the bank opens");
        let started_with = bank.len();
        let mut proposer = Vocabulary::new(&llm, Grammar::builtin().expect("grammar"), hasher())
            .with_slot(*slot)
            .with_budget(WANT)
            .with_bank(bank);

        // Recomputed whenever a round banks a word below, so a name a round unlocked is not credited
        // again by later rounds.
        let mut already = control(proposer.grammar(), slot, &targets);
        eprintln!(
            "\n[{slot}]  banked={started_with}, and the vocabulary already names {} through these shapes",
            already.len()
        );

        for round_number in 0..rounds_per_slot() {
            let outcome =
                proposer.round(&targets, &context_for(round_number, role, &every_example));

            let round = match outcome {
                Ok(round) => round,
                Err(error) => {
                    eprintln!("  round {round_number}: FAILED - {error}");
                    continue;
                }
            };

            asked_rounds += 1;
            proposed_total += round.tried.len();
            let new: Vec<&str> = round
                .solved
                .iter()
                .map(|s| s.name.as_str())
                .filter(|name| !already.contains(*name))
                .collect();

            report(round_number, &round, &new);
            earned.extend(new.iter().map(|n| (*n).to_owned()));
            banked_total += round.banked;
            if round.banked > 0 {
                // The grammar grew, so the control grows with it.
                already = control(proposer.grammar(), slot, &targets);
            }
        }
    }

    eprintln!(
        concat!(
            "\nLIVE RESULT  rounds={} words_proposed={} earned={} newly_banked={}\n",
            "  `newly_banked` is the number that says whether anything was learned. ",
            "Names earned\n  from words already held are names this run did not earn."
        ),
        asked_rounds,
        proposed_total,
        earned.len(),
        banked_total
    );
    for name in &earned {
        eprintln!("  {name}");
    }
    if earned.is_empty() {
        eprintln!("  nothing beyond what the existing vocabulary already reaches.");
    }

    // Not asserted: whether a model is good enough is a fact about the model and the prompt. That
    // the experiment ran is asserted, so all-error rounds do not read as a clean negative.
    assert!(
        proposed_total > 0,
        "no round produced a usable word, so nothing was measured"
    );
}
