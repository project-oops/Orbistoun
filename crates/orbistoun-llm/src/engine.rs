//! The generic ask, the generic answer, and the trait an engine implements.
//!
//! # Nothing here knows what it is being asked
//!
//! A [`Request`] carries a system message, a prompt, and limits. It does not carry a
//! work item, a finding, a trace, an unnamed import, or any other thing this project
//! happens to have. That is the point of the crate: the callers arrive later and there
//! will be several of them, and a trait shaped around the first one is a trait the
//! second one fights.
//!
//! A sibling project's equivalent trait takes `(item, allowed_tags, required,
//! hints, background)` and returns tag suggestions. It works there because there is
//! one job. Copying that shape here would put naming, diagnosis and stub-semantics
//! into one signature and make each of them slightly wrong.
//!
//! # Why the default temperature is zero, and where that stops applying
//!
//! An identical request should produce an identical answer, so a result can be
//! attributed to a change rather than to the weather. That is right for anything whose
//! output is *believed*.
//!
//! **It is wrong for a proposer, and the reason is worth carrying here** so the default
//! is not read as universal advice. A proposer's output is not believed - it is checked
//! by an oracle, so a suggestion is worth nothing until arithmetic agrees, and what it
//! needs from a second round is a *different question*. Greedy decoding does not merely
//! repeat between rounds; it repeats within one, and the first measured round returned
//! twenty suggestions of which fourteen were the same word.
//!
//! Hence [`Request::seed`]: sampling and repeatability are separable, so a caller can
//! have variety without giving up the ability to reproduce a particular answer (D219).

use std::fmt;

use crate::Error;

/// The sample taken when a caller does not ask for a particular one.
pub const DEFAULT_SEED: u64 = 1;

/// One question, in the only shape every engine understands.
#[derive(Debug, Clone)]
pub struct Request {
    /// Standing instructions. Sent as a system message where the wire has one.
    pub system: Option<String>,
    /// The question itself.
    pub prompt: String,
    /// Ceiling on the reply, in tokens.
    pub max_tokens: u32,
    /// Sampling temperature. Zero means take the most likely token every time.
    ///
    /// Some hosted models reject this parameter outright rather than ignoring it, so
    /// an engine may decline to send it. That is recorded per engine rather than
    /// hidden here, because a caller asking for randomness deserves to know it did not
    /// happen.
    pub temperature: f32,
    /// Strings that end generation. Honoured where the engine can.
    pub stop: Vec<String>,
    /// Which sample to take, when the temperature allows more than one.
    ///
    /// Fixed by default, so an identical request is an identical answer. A caller that
    /// wants a *different* sample says so by changing this - which is what makes
    /// "ask again, differently" possible without giving up reproducibility.
    ///
    /// Ignored at temperature zero, where there is only one sample to take.
    pub seed: u64,
}

impl Request {
    /// A deterministic request with the given prompt.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            system: None,
            prompt: prompt.into(),
            // Large enough for a paragraph of reasoning and a small JSON object, and
            // small enough that a runaway reply is bounded rather than billed.
            max_tokens: 1024,
            temperature: 0.0,
            stop: Vec::new(),
            seed: DEFAULT_SEED,
        }
    }

    /// Adds standing instructions.
    #[must_use]
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Sets the reply ceiling.
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Sets the sampling temperature.
    #[must_use]
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// Takes a different sample.
    ///
    /// Only meaningful above temperature zero. Two rounds at the same seed produce the
    /// same words, which is correct for reproducing a result and useless for widening
    /// a search - so a caller looping deliberately varies it.
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Adds a stop string.
    #[must_use]
    pub fn with_stop(mut self, stop: impl Into<String>) -> Self {
        self.stop.push(stop.into());
        self
    }
}

/// What an engine that was tried did.
///
/// Kept for the successful reply as well as the failure, because "the 4B model was
/// unreachable so the 0.6B answered" changes how much a proposal is worth and is
/// invisible if only the winner is recorded.
#[derive(Debug, Clone)]
pub struct Attempt {
    /// The configured entry that was tried.
    pub id: String,
    /// What happened. `None` means it answered.
    pub failure: Option<String>,
}

/// One answer, and the provenance of the thing that produced it.
///
/// The three fields after `text` exist for the same reason D046 makes a run report
/// embed its own inputs: a proposal that cannot be attributed to a model is a
/// difference between runs that cannot be attributed to a change.
#[derive(Debug, Clone)]
pub struct Reply {
    /// What came back.
    pub text: String,
    /// The configured entry that answered.
    pub backend: String,
    /// The exact model string, as sent or as loaded.
    pub model: String,
    /// Everything tried, in order, including the one that succeeded.
    pub attempts: Vec<Attempt>,
}

impl Reply {
    /// True when something ahead of the answering engine failed first.
    ///
    /// Worth surfacing: a fallback is a quieter machine than the one that was
    /// configured, and a caller comparing two runs should know it moved.
    #[must_use]
    pub fn fell_back(&self) -> bool {
        self.attempts.iter().any(|a| a.failure.is_some())
    }
}

/// Removes a reasoning block from a reply.
///
/// **A reasoning model narrates before it answers, and the narration is not the answer.**
/// Where a runtime can be told not to, it is - the managed one passes `--reasoning off`,
/// and the in-process one appends `/no_think` to the system message for the models that
/// understand it. Neither covers everything: measured, `/no_think` suppressed the
/// *content* and the model emitted the tags anyway, so a reply arrived as
/// `<think> </think> Here is a list of...` and every parser downstream saw prose where a
/// JSON array should have been (D336).
///
/// So the tags are stripped here rather than worked around in each reader. Applied by the
/// engines that cannot configure the behaviour away; the managed one needs nothing.
///
/// **An unclosed block means the answer never arrived.** A reply cut off mid-thought is
/// all narration, and what precedes the opening tag is what there is - usually nothing.
/// Returning the narration instead would hand a caller the model's working as though it
/// were its conclusion.
#[must_use]
pub fn without_reasoning(text: &str) -> &str {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";
    if let Some(at) = text.rfind(CLOSE) {
        return text[at + CLOSE.len()..].trim();
    }
    match text.find(OPEN) {
        Some(at) => text[..at].trim(),
        None => text.trim(),
    }
}

/// Something that can answer a [`Request`].
///
/// Synchronous on purpose. Inference is CPU-bound work that an async runtime cannot
/// make faster, an HTTP round trip here is one call rather than a fan-out, and this
/// workspace has no runtime - importing one to await two things in sequence would be
/// the largest dependency in the tree paying no rent.
pub trait Engine: fmt::Debug + Send + Sync {
    /// What this is, in a few words, for a log line or a report field.
    fn describe(&self) -> String;

    /// The exact model string this will use.
    fn model(&self) -> String;

    /// Answers, or says why not.
    ///
    /// # Errors
    ///
    /// If the engine cannot be reached, refuses the request, or returns something
    /// that is not a reply. An engine never fabricates a reply to avoid an error -
    /// principle 3, and the consequence of breaking it here is a proposal nobody can
    /// tell apart from a real one.
    fn complete(&self, request: &Request) -> Result<String, Error>;
}

/// Something that can be asked a question and will produce a [`Reply`] or an [`Error`].
///
/// [`crate::Llm`] is the real implementation; it walks the configured ladder. This trait
/// exists so that a **caller** can be tested without one.
///
/// That is not a hypothetical benefit, which is the bar principle 12 sets for a seam.
/// The callers of this crate are proposers, and the interesting thing about a proposer
/// is not the model - it is everything downstream of the model: how a reply is read,
/// what is refused, and whether what survives is actually correct. All of that is
/// deterministic and worth pinning, and none of it is testable if the only way in is a
/// struct that owns real backends and downloads gigabytes.
///
/// So the seam is here rather than in each caller, and every proposer written later gets
/// it for free.
///
/// `Debug` is required for the same reason [`Engine`] requires it: a caller holding one
/// of these behind a trait object still has to satisfy the workspace's
/// `missing_debug_implementations` lint, and every real implementor is `Debug` anyway.
pub trait Ask: fmt::Debug {
    /// Answers, or says why not.
    ///
    /// # Errors
    ///
    /// If nothing could answer, or if what answered failed.
    fn ask(&self, request: &Request) -> Result<Reply, Error>;
}

#[cfg(test)]
mod tests {
    /// **The exact reply that was measured, and what it should have been.**
    ///
    /// `/no_think` suppressed the reasoning content and the model emitted the tags
    /// regardless. Every parser downstream then saw prose where an array should have
    /// been, and the engine scored zero on a benchmark it should have passed (D336).
    #[test]
    fn an_empty_reasoning_block_is_removed() {
        assert_eq!(
            super::without_reasoning("<think> </think> [\"One\", \"Two\"]"),
            "[\"One\", \"Two\"]"
        );
    }

    /// A block with narration in it goes too, and only what follows survives.
    #[test]
    fn a_reasoning_block_with_content_is_removed() {
        let said = "<think>The user wants nouns. Let me think.</think>
[\"One\"]";
        assert_eq!(super::without_reasoning(said), "[\"One\"]");
    }

    /// The *last* close wins, so a model that narrates twice does not leak the first.
    #[test]
    fn the_last_block_is_the_one_that_ends_the_narration() {
        assert_eq!(
            super::without_reasoning("<think>a</think>mid<think>b</think>done"),
            "done"
        );
    }

    /// **An unclosed block means the answer never arrived.**
    ///
    /// A reply cut off mid-thought is all narration. Handing that back would give a
    /// caller the model's working as though it were its conclusion, which is the same
    /// failure as a stub returning success.
    #[test]
    fn an_unclosed_block_yields_no_answer_rather_than_the_narration() {
        assert_eq!(
            super::without_reasoning("<think>still going on about it"),
            ""
        );
        assert_eq!(super::without_reasoning("before <think>and then"), "before");
    }

    /// A reply with no block is returned as it stands.
    #[test]
    fn a_reply_without_a_block_is_untouched() {
        assert_eq!(super::without_reasoning("  [\"One\"]  "), "[\"One\"]");
    }

    use super::{Attempt, Reply, Request};

    /// A fresh request is deterministic.
    ///
    /// The property the loop's attribution rests on: two identical questions must
    /// produce the same proposal, or "did that change help?" has no answer.
    ///
    /// Asserted as "not above zero" rather than "equal to zero" because that is the
    /// condition the engines actually branch on - a test comparing floats for equality
    /// would also pin a representation nothing depends on.
    #[test]
    fn a_fresh_request_is_deterministic() {
        assert!(Request::new("hello").temperature <= 0.0);
    }

    /// A request has a reply ceiling by default.
    ///
    /// Unbounded generation against a hosted endpoint is somebody's money and against
    /// a local one is somebody's afternoon.
    #[test]
    fn a_fresh_request_is_bounded() {
        assert!(Request::new("hello").max_tokens > 0);
    }

    /// A reply says whether anything failed ahead of it.
    #[test]
    fn a_reply_reports_a_fallback() {
        let plain = Reply {
            text: String::new(),
            backend: "a".to_owned(),
            model: "m".to_owned(),
            attempts: vec![Attempt {
                id: "a".to_owned(),
                failure: None,
            }],
        };
        assert!(!plain.fell_back());

        let after_failure = Reply {
            attempts: vec![
                Attempt {
                    id: "gpu".to_owned(),
                    failure: Some("no device".to_owned()),
                },
                Attempt {
                    id: "cpu".to_owned(),
                    failure: None,
                },
            ],
            ..plain
        };
        assert!(after_failure.fell_back());
    }
}
