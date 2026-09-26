//! The control for the live experiment.
//!
//! [`live`](../live.rs) sweeps with a model's words added and reports what it named. The sweep
//! also contains the whole existing vocabulary, so a reported name may not have needed a new
//! word. This runs the identical sweep with no new words; whatever it finds earns the model no
//! credit. The proposer keeps the full vocabulary in the sweep because narrowing it corrupts
//! the provenance record (D214).
//!
//! ```text
//! cargo test -p orbistoun-propose --release --test control -- --ignored --nocapture
//! ```

use orbistoun_names::Grammar;
use orbistoun_names::solve::{Targets, solve_patterns};
use orbistoun_nid::{Nid, NidHasher};

fn wanted() -> Vec<Nid> {
    let text = std::fs::read_to_string("../../symbols/wanted.txt").expect("the work list exists");
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| u64::from_str_radix(line.trim_start_matches("0x"), 16).ok())
        .map(Nid::from_raw)
        .collect()
}

/// What the shipped vocabulary already names, with nothing added.
///
/// No name found here can be credited to the model.
#[test]
#[ignore = "a full sweep of the learned shapes; opt-in via --ignored"]
fn what_the_existing_vocabulary_already_names() {
    let hashes = wanted();
    let targets = Targets::new(hashes.iter().copied());
    let mut grammar = Grammar::builtin().expect("the shipped grammar");

    // Exactly the shapes a round sweeps, so the two figures are comparable.
    grammar
        .pattern
        .retain(|spec| spec.parts.iter().any(|part| part == "learned"));
    let patterns = grammar.patterns().expect("patterns resolve");

    let nid = NidHasher::new(orbistoun_nid::default_suffix());
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let (solved, stats) = solve_patterns(&nid, &targets, &patterns, threads);

    eprintln!(
        "CONTROL  wanted={} swept={} named={}",
        hashes.len(),
        stats.tried,
        solved.len()
    );
    let mut names: Vec<&str> = solved.iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();
    for name in &names {
        eprintln!("  {name}");
    }
    eprintln!(concat!(
        "\nEvery name above is one the model must be given no credit for. ",
        "A live round that reports these and nothing else has contributed nothing."
    ));
}
