//! Asking a model for vocabulary, as a tool rather than a test.
//!
//! Slow and optional, so it lives behind its own binary and nothing a person runs routinely
//! waits on it. A model may guess here because the space of plausible words is not enumerable
//! and the NID hash checks every proposal for free: a wrong word costs a sweep and cannot become
//! a name. Where enumeration works, it wins. `tests/shapes.rs` reports whether unspelled names
//! are short of vocabulary or of shapes; more words help only in the first case.

use crate::bank::Bank;
use crate::vocabulary::{Context, Round, Vocabulary};
use orbistoun_names::Grammar;
use orbistoun_nid::{Nid, NidHasher};
use std::collections::BTreeSet;
use std::path::Path;

/// The positions a run extends, and what each holds.
///
/// Asked per position rather than for vocabulary at large, and shortest list first, where one
/// word changes the most.
pub const SLOTS: &[(&str, &str)] = &[
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

/// Words asked for per round.
///
/// Small, because a model asked for many pads the list, and each padded word costs a place in
/// the round's budget and a sweep.
pub const WANT: usize = 12;

/// What a run of this produced.
#[derive(Debug, Default)]
pub struct Summary {
    /// Rounds asked.
    pub rounds: usize,
    /// Words offered and accepted for sweeping.
    pub proposed: usize,
    /// Names the hash confirmed.
    pub earned: BTreeSet<String>,
    /// Words banked, which is the number that says whether anything was learned.
    pub banked: usize,
}

/// The hashes nothing can name yet, from the committed work list.
///
/// # Errors
///
/// If the work list cannot be read.
pub fn wanted(path: &Path) -> Result<Vec<Nid>, crate::Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| crate::Error::Reply(format!("reading {}: {e}", path.display())))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| u64::from_str_radix(line.trim_start_matches("0x"), 16).ok())
        .map(Nid::from_raw)
        .collect())
}

/// Confirmed names, to show the convention by example.
///
/// Vendor-shaped only: the database also holds C++ ABI symbols and POSIX names, which would
/// teach the wrong convention.
///
/// # Errors
///
/// If the database cannot be read or parsed.
pub fn examples(path: &Path) -> Result<Vec<String>, crate::Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| crate::Error::Reply(format!("reading {}: {e}", path.display())))?;
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| crate::Error::Reply(format!("parsing {}: {e}", path.display())))?;
    let mut names: Vec<String> = parsed["names"]
        .as_array()
        .ok_or_else(|| crate::Error::Reply("no names array in the database".to_owned()))?
        .iter()
        .filter_map(|n| n.as_str())
        .filter(|n| n.starts_with(VENDOR_PREFIX) && n.len() > 12)
        .map(str::to_owned)
        .collect();
    names.sort();
    Ok(names)
}

/// What every vendor name starts with.
const VENDOR_PREFIX: &str = "sce";

/// The libraries a set of vendor-shaped names belongs to.
///
/// Derived from the names, so the model is pointed at the domains the examples come from. A name
/// is the prefix, a module word, then the rest; the module words are a list the grammar carries,
/// so this reads the real segmentation. Longest first, because `Np` is a prefix of `NpAuth` and
/// the longer one is the library.
#[must_use]
pub fn libraries_of(grammar: &Grammar, examples: &[String]) -> Vec<String> {
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
        let Some(rest) = name.strip_prefix(VENDOR_PREFIX) else {
            continue;
        };
        if let Some(module) = modules.iter().find(|m| rest.starts_with(m.as_str())) {
            seen.insert(format!("libSce{module}"));
        }
    }
    seen.into_iter().collect()
}

/// What the model is told for one round.
///
/// A different slice of the examples each round, with the libraries following the slice, so
/// the question narrows by subsystem as the window rotates.
#[must_use]
pub fn context_for(grammar: &Grammar, round: u64, role: &str, every_example: &[String]) -> Context {
    let window = (round as usize * 7) % every_example.len().max(1);
    let examples: Vec<String> = every_example
        .iter()
        .cycle()
        .skip(window)
        .take(8)
        .cloned()
        .collect();
    Context {
        libraries: libraries_of(grammar, &examples),
        examples,
        theme: None,
        role: Some(role.to_owned()),
        want: WANT,
    }
}

/// Everything one run of the loop needs, with every field named at the call site.
pub struct Session<'a> {
    /// What answers the questions.
    ///
    /// A trait object, so a caller can drive the whole loop with a canned reply and no model
    /// (D212).
    pub asker: &'a dyn orbistoun_llm::Ask,
    /// The vocabulary and shapes to extend.
    pub grammar: &'a Grammar,
    /// The oracle. Every proposal is checked against it and nothing else.
    pub hasher: &'a NidHasher,
    /// The hashes worth trying to name.
    pub targets: &'a orbistoun_names::solve::Targets,
    /// Confirmed names, to show the convention by example.
    pub examples: &'a [String],
    /// Where a slot's banked words are kept.
    pub bank_for: &'a dyn Fn(&str) -> std::path::PathBuf,
    /// Rounds per position. Almost all of the yield is in each position's first round.
    pub rounds: u64,
}

impl std::fmt::Debug for Session<'_> {
    /// Written by hand because a closure has no `Debug`, and the workspace requires one.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("asker", &self.asker)
            .field("targets", &self.targets.len())
            .field("examples", &self.examples.len())
            .field("bank_for", &"<closure>")
            .field("rounds", &self.rounds)
            .finish_non_exhaustive()
    }
}

impl Session<'_> {
    /// Runs the loop, calling `watch` with each round as it lands.
    ///
    /// # Errors
    ///
    /// If a round could not be asked, or a bank could not be opened.
    pub fn run(&self, mut watch: impl FnMut(&str, u64, &Round)) -> Result<Summary, crate::Error> {
        let mut summary = Summary::default();
        for (slot, role) in SLOTS {
            let bank = Bank::open((self.bank_for)(slot))?;
            let mut proposer =
                Vocabulary::new(self.asker, self.grammar.clone(), self.hasher.clone())
                    .with_slot(*slot)
                    .with_budget(WANT)
                    .with_bank(bank);

            for round in 0..self.rounds {
                let context = context_for(self.grammar, round, role, self.examples);
                let outcome = proposer.round(self.targets, &context)?;
                summary.rounds += 1;
                summary.proposed += outcome.tried.len();
                summary
                    .earned
                    .extend(outcome.solved.iter().map(|s| s.name.clone()));
                summary.banked += outcome.banked;
                watch(slot, round, &outcome);
            }
        }
        Ok(summary)
    }
}
