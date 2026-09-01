//! Does this actually name anything nobody had named?
//!
//! Every other test in this crate fakes the model, which is right for pinning behaviour
//! and cannot answer the question that decides whether the proposer earns its place.
//!
//! ```text
//! cargo test -p orbistoun-propose --release --test live -- --ignored --nocapture
//! ```
//!
//! Release, not debug: in debug a run of this is twenty minutes, which is not a loop
//! anybody iterates on.
//!
//! # The control runs first, here, in the same process
//!
//! **A round's raw count means nothing on its own.** The sweep it runs contains the
//! whole existing vocabulary as well as the new words, so a name it reports may have
//! needed a model or may have been sitting there all along.
//!
//! An earlier version narrowed the swept vocabulary to only the new words, which would
//! have made anything found attributable by construction. That was reverted because it
//! corrupts the provenance index (D214) - and the guarantee was still being claimed
//! afterwards. It cost a whole run to notice.
//!
//! So the control is computed first, in this test, and only names **outside** it are
//! reported as earned. The first run scored 18 that way and had earned 2.

use std::collections::BTreeSet;

use orbistoun_llm::Llm;
use orbistoun_names::Grammar;
use orbistoun_names::solve::{Targets, solve_patterns};
use orbistoun_nid::{Nid, NidHasher};
use orbistoun_propose::bank::Bank;
use orbistoun_propose::vocabulary::{Context, Vocabulary};

/// The positions a round can extend, and what each holds.
///
/// **Asked per position, not in general.** The first live run asked for vocabulary at
/// large over six rounds and earned one usable word: `Async` - a suffix. The suffix list
/// is six entries guarding a multiplier on nearly every shape, which makes "suggest more
/// suffixes" a far better question than "suggest more words".
///
/// Shortest list first: that is where one word changes the most.
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

/// Rounds per position, overridable so a longer run needs no edit.
///
/// Three is enough to show the mechanism works, which is what this test was for. Asking
/// whether the mechanism is *worth having* is a different question and needs a longer run,
/// so the number is a runtime input rather than a recompile (principle 5).
fn rounds_per_slot() -> u64 {
    std::env::var("ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(3)
}

/// Words asked for per round.
///
/// **Small on purpose.** Asked for forty, a model that has a dozen ideas pads the rest,
/// and the padding is not inert - each one costs a place in the round's budget and, at
/// the `tail` position, several hundred million candidates to sweep. Twelve good words
/// beat forty of which thirty are counting.
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
/// Vendor-shaped only: the database also holds C++ ABI symbols and POSIX names, and
/// showing `_ZNSt9exceptionD2Ev` as an example of the convention teaches the wrong one.
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
/// A different slice of the examples each time. Without it every round asks one
/// question and gets one answer, which is the failure the first live run exhibited.
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
        // **Derived from this round's examples, not fabricated.**
        //
        // This was four hardcoded names - `libkernel`, `libSceNpManager`, `libSceAudioOut`,
        // `libSceVideoOut` - and every name the model has ever earned came from a library
        // that was *not* among them: `sceAgcDriverRegisterResource` from the graphics
        // driver, `sceNpAuthPollAsync` from the auth library, `sceAudio3dObjectReserve`
        // from spatial audio. It was succeeding despite the context rather than because
        // of it, and a model told the wrong domain is being pointed away from the answer.
        //
        // Taken from the window means each round names the subsystems whose examples it is
        // actually looking at, so the question narrows on its own as the window rotates -
        // which matters because the vendor vocabulary a model is any good at proposing is
        // exactly the vocabulary that clusters by subsystem.
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
/// already carries - so this reads the real segmentation rather than guessing at where the
/// name divides. Longest first, because `Np` is a prefix of `NpAuth` and the longer one is
/// the real library.
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

/// Prints what one round did, including why suggestions were dropped.
///
/// The refusal counts are the diagnostic that matters: a round offering seventy and
/// accepting none is a different problem from one offering three, and the totals cannot
/// tell them apart - which is exactly what the `verb` position looked like for three
/// rounds before this was printed.
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
/// **One file per position, not one for everything.** A word proves itself in the slot
/// it was swept in - `Async` earned its place as a *suffix* - and loading it into the
/// noun position would generate candidates of a shape the convention does not use. The
/// file name says which, so a reader can see what was learned where.
fn bank_path(slot: &str) -> String {
    format!("../../symbols/proposed-{slot}.txt")
}

/// What the vocabulary already names through the shapes that reach `slot`.
///
/// **Computed from the proposer's own grammar**, which already holds everything banked.
/// A control that knows only the shipped vocabulary credits a run for names its own
/// earlier runs made reachable, and the total then climbs every run while nothing is
/// learned.
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
        // Banked words go in through the proposer, which then keeps whatever this run
        // adds without the caller having to remember to. Kept beside the symbol
        // databases rather than in the run's scratch directory: the point of a bank is
        // that it outlives the run, and scratch space by definition does not.
        let bank = Bank::open(bank_path(slot)).expect("the bank opens");
        let started_with = bank.len();
        let mut proposer = Vocabulary::new(&llm, Grammar::builtin().expect("grammar"), hasher())
            .with_slot(*slot)
            .with_budget(WANT)
            .with_bank(bank);

        // Recomputed whenever a round banks a word, further down. A control computed
        // once goes stale the moment the grammar grows: round zero banks a word, rounds
        // one and two then re-find the name it unlocked, and each reports it as earned
        // again. Measured - one name was credited three times that way.
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
                // The grammar just grew, so what counts as already-reachable grew with
                // it. Anything this round unlocked is the control's from here on.
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

    // Deliberately not asserted. Whether a model is good enough at this is a fact about
    // the model and the prompt, not about this code, and a test failing on it would be
    // reporting the wrong thing. What is asserted is that the experiment ran - rounds
    // that all error out would otherwise read as a clean negative result.
    assert!(
        proposed_total > 0,
        "no round produced a usable word, so nothing was measured"
    );
}
