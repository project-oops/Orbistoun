//! Asking a model for vocabulary, as a tool rather than a test.
//!
//! # Why this is opt-in and separate
//!
//! Everything here is slow and optional. A round takes seconds to minutes; a boot of the
//! guest takes about a tenth of a second. Nothing on the path a person actually runs -
//! `./bin/orbistoun run` - may wait on a model, so this lives behind its own binary and the
//! run report *mentions* it rather than invoking it.
//!
//! # Why a model is allowed to guess here and nowhere else
//!
//! **The space is not enumerable and the answer is checkable.** You cannot loop over every
//! plausible English noun, which is what makes proposing them worth paying for; and the NID
//! hash decides every proposal for free, so a wrong one costs a sweep and vanishes. It
//! cannot enter the database, cannot produce a false name, and cannot mislead a reader.
//!
//! Both halves are load-bearing. Where enumeration works it wins - measured, repeatedly:
//! sweeping every argument of every import beats any model asked to pick one. Where there
//! is no oracle, output is plausible and unverifiable, which is the failure this project
//! treats most seriously.
//!
//! # What it is short of, before you ask it for words
//!
//! `tests/shapes.rs` reports whether the names this project cannot spell are short of
//! *vocabulary* or short of *shapes*. When it is shapes, more words buy nothing, and the
//! measured answer today is that shapes outnumber words three to one. Ask this for words
//! when the measurement says words.

use crate::bank::Bank;
use crate::vocabulary::{Context, Round, Vocabulary};
use orbistoun_names::Grammar;
use orbistoun_nid::{Nid, NidHasher};
use std::collections::BTreeSet;
use std::path::Path;

/// The positions a run extends, and what each holds.
///
/// **Asked per position, not in general.** The first live run asked for vocabulary at large
/// over six rounds and earned one usable word. Shortest list first, because that is where
/// one word changes the most.
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
/// **Small on purpose.** Asked for forty, a model with a dozen ideas pads the rest, and the
/// padding is not inert - each one costs a place in the round's budget and a sweep.
pub const WANT: usize = 12;

/// What a run of this produced.
#[derive(Debug, Default)]
pub struct Summary {
    /// Rounds actually asked.
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
/// Vendor-shaped only: the database also holds C++ ABI symbols and POSIX names, and showing
/// `_ZNSt9exceptionD2Ev` as an example of the convention teaches the wrong one.
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
/// **Derived, not fabricated, and the difference was measurable.** This was once four
/// hardcoded strings, and every name a model has ever earned came from a library that was
/// not among them - the graphics driver, the auth library, spatial audio. A model told the
/// wrong domain is being pointed away from the answer, and domain vocabulary is the one
/// thing it is measurably good at proposing.
///
/// A name is the prefix, a module word, then the rest, and the module words are a list the
/// grammar already carries - so this reads the real segmentation rather than guessing where
/// the name divides. Longest first, because `Np` is a prefix of `NpAuth` and the longer one
/// is the real library.
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
/// A different slice of the examples each time, and the libraries follow the slice - so the
/// question narrows on its own as the window rotates, which matters because the vocabulary
/// a model is any good at proposing is exactly the vocabulary that clusters by subsystem.
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

/// Everything one run of the loop needs.
///
/// A struct rather than eight arguments, which is what it was until a lint objected and was
/// right to. It also reads better at the call site: every field is named at the point it is
/// supplied, and `rounds` stops being a bare integer between two references.
pub struct Session<'a> {
    /// What answers the questions.
    ///
    /// A trait object rather than the service, so a caller can drive the whole loop with a
    /// canned reply and no model at all - which is what makes it testable on a machine
    /// with no GPU (D212).
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
    /// Rounds per position.
    ///
    /// Measured over thirty-six: effectively all of the yield is in the first round of
    /// each, so a long run is not a better one.
    pub rounds: u64,
}

impl std::fmt::Debug for Session<'_> {
    /// Written by hand because a closure has no `Debug`.
    ///
    /// The alternative was to leave the whole struct without one, and the workspace denies
    /// that - a type nobody can print is a type nobody can put in an error message.
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
