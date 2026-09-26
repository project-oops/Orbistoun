//! Asks a local model for candidate vocabulary, and keeps what the hash confirms.
//!
//! ```text
//! orbistoun-suggest [rounds]
//! ```
//!
//! Its own binary so nothing a person runs routinely waits on a model or carries its runtime
//! (D265). The NID hash decides every proposed word, so a wrong one costs a sweep and cannot
//! produce a false name. Confirmed words go to `symbols/proposed-<slot>.txt`; promoting one into
//! the grammar is a separate, deliberate change. The `shapes` test in this crate says whether
//! unspelled names are short of vocabulary or of shapes; words help only in the first case.

use orbistoun_names::Grammar;
use orbistoun_names::solve::Targets;
use orbistoun_propose::suggest;

/// Where the work list and the database live, relative to the repository root.
const WANTED: &str = "symbols/wanted.txt";
const DATABASE: &str = "symbols/generated.json";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let first = std::env::args().nth(1);
    let benchmarking = first.as_deref() == Some("benchmark");
    // Three rounds per position by default: the yield is almost all in each position's first
    // round, because a model re-proposes its own ideas.
    let rounds: u64 = match &first {
        Some(a) if !benchmarking => a
            .parse()
            .map_err(|e| format!("rounds must be a number, or the word `benchmark`: {e}"))?,
        _ => 3,
    };

    // The data root comes from `Paths::resolve`, as for every entry point, never the working
    // directory. An optional second argument names the entry to ask first (`claude-code`,
    // `managed`, a registry id); it applies to this run only and is not persisted.
    let prefer = std::env::args().nth(2);

    let paths = orbistoun_paths::Paths::resolve();
    let mut llm = orbistoun_llm::Llm::open(paths.data_root())?;
    if let Some(id) = &prefer {
        // A registry seeded before an engine existed does not list it. Re-seeding is safe when nobody
        // has customised the list, which `retune` checks, so it is tried once before reporting the
        // entry missing.
        if !llm.prefer(id) && llm.retune()? {
            println!("  (re-checked this machine: the registry predated {id})");
            llm.prefer(id);
        }
    }
    if let Some(id) = &prefer {
        if !llm.config().integrations.iter().any(|i| &i.id == id) {
            let known: Vec<&str> = llm
                .config()
                .integrations
                .iter()
                .map(|i| i.id.as_str())
                .collect();
            return Err(format!(
                "no entry called {id:?}. This machine has: {}",
                known.join(", ")
            )
            .into());
        }
    }
    if !llm.is_available() {
        eprintln!(
            "no model is configured or reachable: {}",
            llm.host().summary()
        );
        eprintln!("this is optional - naming works without it, just with fewer words");
        return Ok(());
    }

    if benchmarking {
        return benchmark(&mut llm);
    }

    let grammar = Grammar::builtin()?;
    let hasher = orbistoun_nid::NidHasher::new(orbistoun_nid::default_suffix());
    let unnamed = suggest::wanted(std::path::Path::new(WANTED))?;
    let examples = suggest::examples(std::path::Path::new(DATABASE))?;
    let targets = Targets::new(unnamed.iter().copied());

    println!(
        "suggest  host={}\n         unnamed={} examples={} rounds={rounds} per position",
        llm.host().summary(),
        unnamed.len(),
        examples.len()
    );

    let session = suggest::Session {
        asker: &llm,
        grammar: &grammar,
        hasher: &hasher,
        targets: &targets,
        examples: &examples,
        bank_for: &|slot| std::path::PathBuf::from(format!("symbols/proposed-{slot}.txt")),
        rounds,
    };
    let summary = session.run(|slot, round, outcome| {
        // Which engine answered, every round: engines differ in where the prompt goes.
        println!(
            "  [{slot}] round {round}: {} ({}) offered {} accepted {} banked {} swept {}",
            outcome.backend,
            outcome.model,
            outcome.offered,
            outcome.tried.len(),
            outcome.banked,
            outcome.stats.tried
        );
        if !outcome.solved.is_empty() {
            for solved in &outcome.solved {
                println!("      *** {}", solved.name);
            }
        }
    })?;

    println!(
        "\n{} rounds, {} words tried, {} names confirmed, {} words banked",
        summary.rounds,
        summary.proposed,
        summary.earned.len(),
        summary.banked
    );
    // `banked`, not `confirmed`, says whether anything was learned: a name confirmed from words the
    // grammar already had was reachable anyway.
    if summary.banked == 0 {
        println!("nothing new. The words it proposed were ones the grammar already holds.");
    } else {
        println!(concat!(
            "New words are in symbols/proposed-*.txt. Promoting one into ",
            "crates/orbistoun-names/data/vendor.toml is deliberate and manual - put a noun ",
            "in `object` or `learned`, a suffix in `tail` - then run `./bin/orbistoun names`."
        ));
    }
    Ok(())
}

/// Measures every configured entry and reorders the ladder by what came back.
fn benchmark(llm: &mut orbistoun_llm::Llm) -> Result<(), Box<dyn std::error::Error>> {
    // Ranked on the real prompt - library context, decomposed examples, a vocabulary sample, and
    // the rule that none of it be repeated - because a plain request for nouns does not
    // discriminate between engines. Built here because it is this caller's question (D334).
    let grammar = Grammar::builtin()?;
    let examples = suggest::examples(std::path::Path::new(DATABASE))?;
    let (slot, role) = suggest::SLOTS[0];
    let context = suggest::context_for(&grammar, 0, role, &examples);
    // Sampled at the loop's temperature, so the benchmark asks the loop's question.
    let request = orbistoun_llm::engine::Request::new(orbistoun_propose::vocabulary::prompt(
        &context, &grammar, slot,
    ))
    .with_temperature(orbistoun_propose::vocabulary::DEFAULT_TEMPERATURE);

    // Words the loop would refuse, so the score counts words this machine does not already hold.
    let known: std::collections::BTreeSet<String> = grammar
        .vocabulary
        .values()
        .flatten()
        .map(|w| w.to_lowercase())
        .collect();

    // Parsed with the loop's own fallbacks (a JSON array, then quoted strings, then bare tokens),
    // so an engine that would contribute there does not score zero here.
    let score = |text: &str| {
        orbistoun_propose::vocabulary::read_words(text).map_or(0, |(words, _)| {
            words
                .iter()
                .filter(|w| {
                    let w = w.trim();
                    !w.is_empty()
                        && w.chars().all(|c| c.is_ascii_alphanumeric())
                        && !known.contains(&w.to_lowercase())
                })
                .count()
        })
    };

    // Every entry, including ones not in use, so each can be measured.
    println!(
        "benchmarking every entry on the real `{slot}` question, scoring words this machine does not already hold"
    );
    for measurement in llm.benchmark(&request, &score)? {
        println!("  {}", measurement.summary());
    }
    println!(concat!(
        "
Ordered by usable words returned, with time as a tiebreak - not by time. ",
        "Ranking by speed picks the engine that answers quickest and says least, which ",
        "is measurably the wrong one, and the model's time is a rounding error beside ",
        "the sweep that follows it. Written to the registry."
    ));
    Ok(())
}
