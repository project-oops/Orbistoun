//! Ask a local model for candidate vocabulary, and keep what the hash confirms.
//!
//! ```text
//! orbistoun-suggest [rounds]
//! ```
//!
//! # Why this is its own binary
//!
//! **Nothing on the path a person actually runs may wait on a model.** A round here takes
//! seconds to minutes; a boot of the guest takes about a tenth of a second. Putting this
//! behind `orbistoun-cli` would make every user of `./bin/orbistoun run` carry the model
//! runtime for a branch almost none of them reach, on the one command that has to stay
//! fast. So it is separate, opt-in, and the run report mentions it rather than invoking it.
//!
//! # What it can and cannot do wrong
//!
//! It proposes words. The NID hash decides every one, so a wrong proposal costs a sweep and
//! vanishes - it cannot enter the database, cannot produce a false name, and cannot mislead
//! anybody reading the output later. That property is why a model is allowed to guess here
//! and nowhere else in this project.
//!
//! Words the hash confirms are written to `symbols/proposed-<slot>.txt`. **Promoting one
//! into the grammar is a separate and deliberate act**, because it changes what every
//! future search enumerates.
//!
//! # Before running it
//!
//! `cargo test -p orbistoun-propose --release --test shapes -- --nocapture` says whether the
//! names this project cannot spell are short of *vocabulary* or short of *shapes*. When the
//! answer is shapes, more words buy nothing. Ask for words when the measurement says words.

use orbistoun_names::Grammar;
use orbistoun_names::solve::Targets;
use orbistoun_propose::suggest;

/// Where the work list and the database live, relative to the repository root.
const WANTED: &str = "symbols/wanted.txt";
const DATABASE: &str = "symbols/generated.json";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let first = std::env::args().nth(1);
    let benchmarking = first.as_deref() == Some("benchmark");
    // Three per position by default. Measured over thirty-six rounds, effectively all of
    // the yield was in the first round of each - a model re-proposes its own ideas rather
    // than finding new ones - so a long run is not a better one.
    let rounds: u64 = match &first {
        Some(a) if !benchmarking => a
            .parse()
            .map_err(|e| format!("rounds must be a number, or the word `benchmark`: {e}"))?,
        _ => 3,
    };

    // **The one path resolver, not a second guess at it.** This invented its own default
    // of `.orbistoun` in the working directory, which is the repository when run from
    // here - so the first run downloaded a model runtime into the tree and tripped the
    // provenance guard. `Paths::resolve` already knows where data belongs, honours the
    // portable-mode and data-directory settings, and is what every other entry point uses.
    // An optional second argument names the entry to ask first - `claude-code`,
    // `managed`, an id from the registry. Not persisted: choosing once for one run should
    // not quietly rewrite what every later run does.
    let prefer = std::env::args().nth(2);

    let paths = orbistoun_paths::Paths::resolve();
    let mut llm = orbistoun_llm::Llm::open(paths.data_root())?;
    if let Some(id) = &prefer {
        // **A registry written before an engine existed does not know about it.** The
        // list is seeded once and then kept, so a machine that gained a command-line
        // model since - or gained the command itself - has a registry that predates it.
        // Re-seeding is safe exactly when nobody has customised the list, which is what
        // `retune` checks, so it is tried once before giving up rather than telling
        // somebody an entry does not exist when what is stale is the file.
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
        // **Which engine answered, on every round.** There is now more than one kind -
        // a downloaded local model, a server on this machine, and an installed command
        // that answers over somebody else's account - and they differ in where the
        // prompt goes. A person running this is entitled to know which one it was
        // without reading a configuration file to work it out.
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
    // **The number that says whether anything was learned is `banked`, not `confirmed`.**
    // A name confirmed from words the grammar already had teaches nothing - it was already
    // reachable, and the search would have found it. Only a banked word is new.
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
///
/// Its own function because it is a different job from proposing words, and because
/// `main` doing both was longer than the lint allows - which is the lint being right.
fn benchmark(llm: &mut orbistoun_llm::Llm) -> Result<(), Box<dyn std::error::Error>> {
    // **The real question, not an easier one.** Asked plainly for twelve nouns, a
    // local model returned twelve and tied the engine that beats it six to one in the
    // loop. What discriminates is the actual prompt: library context, decomposed
    // examples, a sample of the vocabulary, and the requirement that none of it be
    // repeated. Built here rather than in the engine crate because it is this
    // caller's question, and it is the one worth being ranked on (D334).
    let grammar = Grammar::builtin()?;
    let examples = suggest::examples(std::path::Path::new(DATABASE))?;
    let (slot, role) = suggest::SLOTS[0];
    let context = suggest::context_for(&grammar, 0, role, &examples);
    // Sampled exactly as the loop samples. The first version asked at the default
    // temperature of zero while the loop asks at 0.9, which is not the same question
    // - and a benchmark that asks a different question ranks on a different thing.
    let request = orbistoun_llm::engine::Request::new(orbistoun_propose::vocabulary::prompt(
        &context, &grammar, slot,
    ))
    .with_temperature(orbistoun_propose::vocabulary::DEFAULT_TEMPERATURE);

    // What the loop would refuse before it cost anything, so the score is words this
    // machine does not already hold rather than words returned.
    let known: std::collections::BTreeSet<String> = grammar
        .vocabulary
        .values()
        .flatten()
        .map(|w| w.to_lowercase())
        .collect();

    // **Scored the way the loop reads, not more strictly.** A reply is parsed with the
    // same three fallbacks the proposal loop uses - a JSON array, then quoted strings,
    // then bare tokens - because an engine that would have contributed there must not
    // score zero here. Measured: a strict JSON-only scorer failed an engine that answers
    // coherently in prose, which is a fact about the scorer (D335).
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

    // Every entry, not just the one that answers today - an engine nobody ever runs
    // is one nobody can find out about.
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
