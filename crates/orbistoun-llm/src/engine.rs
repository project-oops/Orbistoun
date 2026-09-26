//! The generic ask, the generic answer, and the trait an engine implements.
//!
//! A [`Request`] carries a system message, a prompt and limits, and nothing specific to any
//! caller, so several kinds of caller can share one trait. The default temperature is zero so an
//! identical request gives an identical answer, which is right for output that is believed. A
//! proposer's output is checked by an oracle instead, and greedy decoding repeats within and
//! between rounds, so [`Request::seed`] separates sampling from repeatability: a caller gets
//! variety and can still reproduce a particular answer.

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
    /// Some hosted models reject this parameter, so an engine may decline to send it; that is
    /// recorded per engine so a caller knows.
    pub temperature: f32,
    /// Strings that end generation. Honoured where the engine can.
    pub stop: Vec<String>,
    /// Which sample to take, when the temperature allows more than one.
    ///
    /// Fixed by default, so an identical request is an identical answer; changing it asks again
    /// differently without losing reproducibility. Ignored at temperature zero.
    pub seed: u64,
}

impl Request {
    /// A deterministic request with the given prompt.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            system: None,
            prompt: prompt.into(),
            // Room for a paragraph of reasoning and a small JSON object, with a runaway reply bounded.
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
    /// Only meaningful above temperature zero. The same seed gives the same words, so a caller
    /// widening a search varies it.
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
/// Kept for success as well as failure, because which engine answered after which failed
/// changes what a proposal is worth.
#[derive(Debug, Clone)]
pub struct Attempt {
    /// The configured entry that was tried.
    pub id: String,
    /// What happened. `None` means it answered.
    pub failure: Option<String>,
}

/// One answer, and the provenance of the thing that produced it.
///
/// The fields after `text` let a proposal be attributed to a model, as a run records its
/// conditions (D181).
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
    /// True when something ahead of the answering engine failed first, so a caller comparing
    /// runs knows the answering engine moved.
    #[must_use]
    pub fn fell_back(&self) -> bool {
        self.attempts.iter().any(|a| a.failure.is_some())
    }
}

/// Removes a reasoning block from a reply.
///
/// A reasoning model narrates before it answers. Runtimes are told not to where they can
/// (`--reasoning off`, `/no_think`), but a model may emit the tags anyway, so they are stripped
/// here rather than in each reader. An unclosed block means the answer never arrived: what
/// precedes the opening tag is returned, usually nothing, never the narration.
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
/// Synchronous: inference is CPU-bound, an HTTP round trip here is one call, and the workspace
/// has no async runtime.
pub trait Engine: fmt::Debug + Send + Sync {
    /// What this is, in a few words, for a log line or a report field.
    fn describe(&self) -> String;

    /// The exact model string this will use.
    fn model(&self) -> String;

    /// Answers, or says why not.
    ///
    /// # Errors
    ///
    /// If the engine cannot be reached, refuses the request, or returns something that is not a
    /// reply. An engine never fabricates a reply to avoid an error.
    fn complete(&self, request: &Request) -> Result<String, Error>;
}

/// Something that can be asked a question and will produce a [`Reply`] or an [`Error`].
///
/// [`crate::Llm`] is the real implementation, walking the configured ladder. The trait lets a
/// caller's downstream logic - how a reply is read, what is refused, what survives - be tested
/// without real backends. `Debug` is required, as for [`Engine`], so trait-object holders
/// satisfy `missing_debug_implementations`.
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
    /// An empty reasoning block, as a model emits it with reasoning suppressed, is removed.
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

    /// The last close wins, so a model that narrates twice does not leak the first.
    #[test]
    fn the_last_block_is_the_one_that_ends_the_narration() {
        assert_eq!(
            super::without_reasoning("<think>a</think>mid<think>b</think>done"),
            "done"
        );
    }

    /// An unclosed block yields no answer rather than the narration.
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
    /// Asserted as "not above zero", the condition the engines branch on.
    #[test]
    fn a_fresh_request_is_deterministic() {
        assert!(Request::new("hello").temperature <= 0.0);
    }

    /// A request has a reply ceiling by default.
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
