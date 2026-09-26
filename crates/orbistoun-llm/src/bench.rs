//! Asking every configured engine the same question, and ordering them by the answer.
//!
//! Engines are ranked by how much usable material came back, with latency only breaking ties:
//! the sweep that follows each round dominates its cost, and the fastest engine can be the one
//! that says least. [`measure`] takes the caller's scoring function, because only the caller
//! knows what it accepts; the strict [`usable_words`] is a default. Whether a word is good is
//! settled later by the NID hash, not here.

use std::time::{Duration, Instant};

use crate::engine::Request;

/// Words asked for, which is also the best possible score; the same number the proposal loop
/// asks for.
pub const ASKED: usize = 12;

/// A question for a caller that has none of its own.
///
/// Not the real one: a short, easy question does not discriminate between engines. The real
/// question carries library context, decomposed examples, a vocabulary sample and the rule
/// that none may be repeated, and a caller with it should pass it to [`measure`] (D334).
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
    /// Usable words returned, out of [`ASKED`]: the ranking key. Zero means the reply was not a
    /// list of words, or there was none.
    pub usable: usize,
    /// How long the call took: the tiebreak, never the key.
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
/// Strict: an engine asked for a JSON array that returned prose has not done what was asked.
#[must_use]
pub fn usable_words(text: &str) -> usize {
    novel_words(text, &std::collections::BTreeSet::new())
}

/// The same, discounting words the caller already has.
///
/// The axis that separates engines: a proposal already in the vocabulary is refused before it
/// costs anything (D334). `known` is compared lowercased, so casing cannot pass a repeat.
#[must_use]
pub fn novel_words(text: &str, known: &std::collections::BTreeSet<String>) -> usize {
    let trimmed = text.trim();
    // A fenced block is still an array, and models emit one.
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
/// Stable, so ties keep the registry's order and the ladder does not reshuffle between runs.
pub fn rank(measurements: &mut [Measurement]) {
    measurements.sort_by(|a, b| b.usable.cmp(&a.usable).then_with(|| a.took.cmp(&b.took)));
}

/// Times one call and scores what came back.
///
/// Takes the engine and the request, so scoring is testable against a canned reply with no
/// model, and the question can be the caller's real one.
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
                // Two different nothings - a reply in the wrong shape, or of words already held - reported
                // with a quote of what the engine said, so a reader can tell a format failure from a broken
                // engine.
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

/// As much of a reply as fits on one line, with newlines flattened, for a summary line.
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

    /// More usable words outranks a faster answer.
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

    /// A fenced block is still an array.
    #[test]
    fn a_fenced_array_is_still_an_array() {
        assert_eq!(usable_words("```json\n[\"One\", \"Two\"]\n```"), 2);
        assert_eq!(usable_words("```\n[\"One\"]\n```"), 1);
    }

    /// Prose scores nothing, however helpful it is.
    #[test]
    fn prose_scores_nothing() {
        assert_eq!(
            usable_words("Certainly! Here are some words: Buffer, Handle."),
            0
        );
        assert_eq!(usable_words(""), 0);
    }

    /// Words the caller already has score nothing.
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

    /// A wrong-shaped reply is quoted, so a reader can tell why it was wrong.
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
