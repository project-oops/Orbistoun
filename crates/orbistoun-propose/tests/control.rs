//! The control the live experiment needs to mean anything.
//!
//! [`live`](../live.rs) sweeps with a model's words added and reports what it named.
//! That figure is worthless on its own, because the sweep it runs also contains the
//! **whole existing vocabulary** - so a name it reports may have needed a new word, or
//! may have been sitting there all along.
//!
//! This runs the identical sweep with **no new words at all**. Whatever it finds is
//! what the model must be given no credit for.
//!
//! ```text
//! cargo test -p orbistoun-propose --release --test control -- --ignored --nocapture
//! ```
//!
//! # Why this was not obvious
//!
//! An earlier version of the proposer narrowed the swept vocabulary to only the new
//! words, which *would* have made the control unnecessary - anything found would have
//! needed one by construction. That narrowing was reverted because it corrupts the
//! provenance record (D214), and the guarantee went with it. The guarantee was still
//! being claimed afterwards, which is the mistake this file exists to stop repeating.

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
/// Run this before believing any figure the live experiment reports. Every name here is
/// one the model cannot be credited with.
#[test]
#[ignore = "a full sweep of the learned shapes; opt-in via --ignored"]
fn what_the_existing_vocabulary_already_names() {
    let hashes = wanted();
    let targets = Targets::new(hashes.iter().copied());
    let mut grammar = Grammar::builtin().expect("the shipped grammar");

    // Exactly the shapes a round sweeps - no more, no fewer - so the two figures are
    // comparable rather than merely both large.
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
