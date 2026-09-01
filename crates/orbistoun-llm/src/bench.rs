//! Asking every configured engine the same question, and ordering them by the answer.
//!
//! # Why the obvious measurement is the wrong one
//!
//! A benchmark that ranks by speed picks the worst engine here, and this was measured
//! rather than reasoned about. Asked for twelve words a round, a four-billion-parameter
//! model on a local accelerator produced **two**, and an installed coding assistant
//! produced twelve - while being the *slower* of the two per call. Ordering by latency
//! would have promoted the one that answers quickly and says almost nothing.
//!
//! It is also the wrong axis for a second reason. What a round actually costs is the
//! **sweep** - billions of candidates hashed against the work list - and the model is a
//! rounding error beside it. Shaving four seconds off a call that precedes seven minutes
//! of hashing buys nothing.
//!
//! So the ranking is **how much usable material came back**, and latency breaks ties.
//!
//! # The caller scores, because only the caller knows what it accepts
//!
//! [`usable_words`] here is strict - a JSON array or nothing. That was the only scorer for
//! one afternoon and it was **unfair to the engines it judged**: the proposal loop reads a
//! reply with three fallbacks, taking quoted strings and then bare tokens, so an engine
//! that scored zero here would have contributed there. A benchmark stricter than its
//! consumer measures the benchmark (D335).
//!
//! So [`measure`] takes a scoring function and the strict one is only a default. Whatever
//! a caller actually accepts is what it should rank on.
//!
//! This is deliberately not a quality judgement either way. Whether a word is *good* is
//! settled by the NID hash further down the line, and nothing here anticipates that.

use std::time::{Duration, Instant};

use crate::engine::Request;

/// Words asked for, which is also the best possible score.
///
/// The same number the proposal loop asks for, so the measurement is of the question that
/// actually gets asked rather than a smaller one that might behave differently.
pub const ASKED: usize = 12;

/// A question for a caller that has none of its own.
///
/// **Deliberately not the default, and the first version of this module got it wrong.**
/// A short, easy question does not discriminate: asked plainly for twelve nouns, a local
/// four-billion-parameter model returned twelve and tied the engine that beats it - then
/// returned *two* when asked the real question, which carries library context, decomposed
/// examples, a sample of the existing vocabulary and the constraint that none of it may be
/// repeated. A benchmark whose question is easier than the work measures nothing about the
/// work (D334).
///
/// So [`measure`] takes the request, and a caller with a real one should pass it.
#[must_use]
pub fn fallback_request() -> Request {
    Request::new(concat!(
        "Suggest 12 short single-word English nouns that could name part of a system ",
        "library function - things like Buffer, Handle, Session. Reply with only a JSON ",
        "array of strings and nothing else."
    ))
}

/// How one engine did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// The registry entry this is about.
    pub id: String,
    /// Usable words returned, out of [`ASKED`].
    ///
    /// The ranking key. Zero means it answered with something that was not a list of
    /// words, or did not answer at all.
    pub usable: usize,
    /// How long the call took.
    ///
    /// The tiebreak, never the key - see the module note.
    pub took: Duration,
    /// Why it scored nothing, when it did.
    pub failure: Option<String>,
}

impl Measurement {
    /// One line, for a person reading a list.
    #[must_use]
    pub fn summary(&self) -> String {
        match &self.failure {
            Some(why) => format!("{:<14} unusable - {why}", self.id),
            None => format!(
                "{:<14} {:>2}/{ASKED} words in {:.1}s",
                self.id,
                self.usable,
                self.took.as_secs_f64()
            ),
        }
    }
}

/// Counts the words in a reply, in the shape it was asked for.
///
/// Strict about the shape and lenient about nothing: an engine that was asked for a JSON
/// array and returned prose has not done what was asked, and scoring it on the words that
/// happen to appear in its apology would rank politeness.
#[must_use]
pub fn usable_words(text: &str) -> usize {
    novel_words(text, &std::collections::BTreeSet::new())
}

/// The same, discounting words the caller already has.
///
/// **This is the axis that actually separates engines, and finding that out took three
/// measurements.** Ranking by speed picks the one that says least. Ranking by *volume*
/// does not discriminate at all - two engines that differ six to one in the loop both
/// returned twelve of twelve here, twice. What the loop values is words it does not
/// already hold, because a proposal already in the vocabulary is refused before it costs
/// anything, and that is what this counts (D334).
///
/// `known` is compared lowercased, so casing cannot smuggle a repeat past it.
#[must_use]
pub fn novel_words(text: &str, known: &std::collections::BTreeSet<String>) -> usize {
    let trimmed = text.trim();
    // A fenced block is still an array, and every model in reach emits one sometimes.
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.rsplit_once("```").map(|(body, _)| body))
        .unwrap_or(trimmed)
        .trim();
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(inner)
    else {
        return 0;
    };
    items
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter(|word| {
            let word = word.trim();
            !word.is_empty()
                && word.chars().all(|c| c.is_ascii_alphanumeric())
                && !known.contains(&word.to_lowercase())
        })
        .count()
}

/// Puts the best first: most usable words, then quickest.
///
/// Stable, so entries that tie keep the order the registry had. That matters because a tie
/// is common - two engines that both answer fully - and reshuffling them on every run
/// would make the ladder look unstable when nothing had changed.
pub fn rank(measurements: &mut [Measurement]) {
    measurements.sort_by(|a, b| b.usable.cmp(&a.usable).then_with(|| a.took.cmp(&b.took)));
}

/// Times one call and scores what came back.
///
/// Takes the engine and the request rather than finding either, so the scoring is testable
/// against a canned reply and the whole of this module can be exercised with no model
/// present - and so the question can be the caller's real one.
#[must_use]
pub fn measure(
    id: &str,
    engine: &dyn crate::engine::Engine,
    request: &Request,
    score: &dyn Fn(&str) -> usize,
) -> Measurement {
    let started = Instant::now();
    let outcome = engine.complete(request);
    let took = started.elapsed();
    match outcome {
        Ok(text) => {
            let usable = score(&text);
            Measurement {
                id: id.to_owned(),
                usable,
                took,
                // Two different nothings, and a person reading the list wants to know
                // which: a reply in the wrong shape, or a reply of things already held.
                // **With what it said.** "Nothing usable" names the shape and withholds
                // the evidence, and the evidence is the whole of what a reader needs to
                // tell a model that cannot follow a format from an engine that is broken.
                // Measured: an in-process model scored zero and the quote showed why - it
                // answered coherently, wrapped in prose, behind a `<think>` block the
                // managed path suppresses and this one cannot (D335).
                failure: (usable == 0)
                    .then(|| format!("nothing usable - it said: {}", glimpse(&text))),
            }
        }
        Err(e) => Measurement {
            id: id.to_owned(),
            usable: 0,
            took,
            failure: Some(e.to_string()),
        },
    }
}

/// As much of a reply as fits on one line, with newlines flattened.
///
/// Short on purpose: this goes in a summary line beside four others, and a model that
/// answered with three paragraphs has already said what a reader needs to know by the end
/// of the first clause.
fn glimpse(text: &str) -> String {
    const ROOM: usize = 90;
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return "(nothing)".to_owned();
    }
    match flat.char_indices().nth(ROOM) {
        Some((at, _)) => format!("{}...", &flat[..at]),
        None => flat,
    }
}

#[cfg(test)]
mod tests {
    use super::{Measurement, rank, usable_words};
    use std::time::Duration;

    /// One measurement, for the ordering tests.
    fn scored(id: &str, usable: usize, secs: u64) -> Measurement {
        Measurement {
            id: id.to_owned(),
            usable,
            took: Duration::from_secs(secs),
            failure: None,
        }
    }

    /// **More words beats faster, and this is the whole point of the module.**
    ///
    /// Measured on real engines: a local model answered quicker and offered two words
    /// where an installed command offered twelve. A benchmark ordered by latency promotes
    /// the first of those, which is the wrong engine and would be chosen automatically.
    #[test]
    fn more_usable_words_outranks_a_faster_answer() {
        let mut all = vec![
            scored("quick-and-terse", 2, 3),
            scored("slow-and-full", 12, 9),
        ];
        rank(&mut all);
        assert_eq!(all[0].id, "slow-and-full");
    }

    /// Latency separates engines that answered equally well.
    #[test]
    fn latency_breaks_a_tie_and_only_a_tie() {
        let mut all = vec![scored("slower", 12, 9), scored("quicker", 12, 2)];
        rank(&mut all);
        assert_eq!(all[0].id, "quicker");
    }

    /// An engine that could not answer goes last, however fast it failed.
    #[test]
    fn failing_instantly_does_not_win() {
        let mut all = vec![scored("works", 8, 30), scored("broken", 0, 0)];
        rank(&mut all);
        assert_eq!(all[0].id, "works");
    }

    /// A tie keeps the order it had, so a ladder does not shuffle for no reason.
    #[test]
    fn an_exact_tie_is_left_alone() {
        let mut all = vec![scored("first", 12, 5), scored("second", 12, 5)];
        rank(&mut all);
        assert_eq!(
            all.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    /// The shape that was asked for is counted.
    #[test]
    fn a_json_array_of_words_is_counted() {
        assert_eq!(usable_words(r#"["One", "Two", "Three"]"#), 3);
    }

    /// A fenced block is still an array - every model in reach emits one sometimes.
    #[test]
    fn a_fenced_array_is_still_an_array() {
        assert_eq!(usable_words("```json\n[\"One\", \"Two\"]\n```"), 2);
        assert_eq!(usable_words("```\n[\"One\"]\n```"), 1);
    }

    /// **Prose scores nothing, however helpful it is.**
    ///
    /// An engine asked for a JSON array that returns a sentence has not done what was
    /// asked, and counting the words in its apology would rank politeness.
    #[test]
    fn prose_scores_nothing() {
        assert_eq!(
            usable_words("Certainly! Here are some words: Buffer, Handle."),
            0
        );
        assert_eq!(usable_words(""), 0);
    }

    /// **Words the caller already has score nothing.**
    ///
    /// The axis that separates engines in the loop, where a proposal already in the
    /// vocabulary is refused before it costs anything.
    #[test]
    fn words_already_held_do_not_count() {
        let known = ["buffer", "handle"]
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        assert_eq!(
            super::novel_words(r#"["Buffer", "Handle", "Lantern"]"#, &known),
            1,
            "casing let a repeat through, or a novel word was discounted"
        );
    }

    /// **A wrong-shaped reply is quoted, so a reader can tell why it was wrong.**
    #[test]
    fn a_reply_in_the_wrong_shape_is_quoted() {
        let said = super::glimpse(
            "Certainly!

Here are some words:
- Buffer",
        );
        assert_eq!(said, "Certainly! Here are some words: - Buffer");
        assert_eq!(super::glimpse("   "), "(nothing)");
    }

    /// A long reply is cut rather than filling the summary.
    #[test]
    fn a_long_reply_is_cut() {
        let said = super::glimpse(&"word ".repeat(200));
        assert!(said.len() < 120, "{} chars is not one line", said.len());
        assert!(said.ends_with("..."), "{said}");
    }

    /// Entries that are not words do not count towards the score.
    #[test]
    fn only_word_shaped_entries_count() {
        assert_eq!(usable_words(r#"["Fine", "", "  ", "not one", "Ok2"]"#), 2);
    }
}
