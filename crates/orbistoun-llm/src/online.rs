//! Endpoints somebody else runs.
//!
//! Two wire formats, chosen by data rather than by vendor. Most hosted providers - and
//! every local model server worth naming - speak the OpenAI-shaped request, so one
//! client covers them. One does not, and this is the part worth being careful about:
//! two sibling projects both list `api.anthropic.com/v1/chat/completions` as though it
//! were OpenAI-compatible. It is not an endpoint that exists, and the failure is a
//! 404 that reads like a network problem.
//!
//! # What is deliberately not sent
//!
//! The Messages API **rejects** `temperature` on its current models rather than
//! ignoring it, so this engine does not send one and says so in [`Engine::describe`].
//! Silently dropping a caller's parameter would be worse than either sending it or
//! refusing: the caller asked for randomness, did not get it, and has no way to know.
//!
//! # No streaming
//!
//! Every reply this crate asks for is bounded and small - a proposal, a ranking, a
//! short piece of JSON. Streaming exists to keep a long generation under an HTTP
//! timeout and to show a human progress, and neither applies to a machine reading a
//! whole answer before it can act on it.

use std::time::Duration;

use serde_json::{Value, json};

use crate::Error;
use crate::catalog::{Catalog, Online, Wire};
use crate::config::Integration;
use crate::engine::{Engine, Request};

/// The version header the Messages API requires on every request.
///
/// A dated constant rather than "latest": the wire format is versioned precisely so a
/// client can pin one, and a client that follows the newest by default has an API
/// that changes without a commit.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// How long to wait for a hosted reply.
///
/// Generous, because a large hosted model thinking about a fault trace is legitimately
/// slow, and short enough that a dead endpoint falls through to the next entry in the
/// ladder rather than hanging a run.
const TIMEOUT: Duration = Duration::from_secs(120);

/// An HTTP engine.
#[derive(Debug)]
pub struct OnlineEngine {
    id: String,
    wire: Wire,
    endpoint: String,
    model: String,
    key: Option<String>,
    client: reqwest::blocking::Client,
}

impl OnlineEngine {
    /// Builds an engine for a configured entry.
    ///
    /// # Errors
    ///
    /// If the entry names no provider and supplies no endpoint of its own - there is
    /// then nowhere to send anything, and the honest report is that rather than a
    /// request to a guessed address.
    pub fn new(integration: &Integration, catalog: &Catalog) -> Result<Self, Error> {
        let provider: Option<&Online> = catalog.online(&integration.source);
        let endpoint = integration
            .endpoint
            .clone()
            .or_else(|| provider.map(|p| p.endpoint.clone()))
            .ok_or_else(|| {
                Error::Config(format!(
                    "`{}` names no known provider and no endpoint of its own",
                    integration.id
                ))
            })?;
        let model = integration
            .model
            .clone()
            .or_else(|| provider.map(|p| p.default_model.clone()))
            .unwrap_or_default();
        let wire = provider.map_or(Wire::OpenAi, |p| p.wire);
        let client = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| Error::Transport(e.to_string()))?;
        Ok(Self {
            id: integration.id.clone(),
            wire,
            endpoint,
            model,
            key: integration.key(catalog),
            client,
        })
    }

    /// The request body for this wire.
    fn body(&self, request: &Request) -> Value {
        match self.wire {
            Wire::OpenAi => {
                let mut messages = Vec::new();
                if let Some(system) = &request.system {
                    messages.push(json!({"role": "system", "content": system}));
                }
                messages.push(json!({"role": "user", "content": request.prompt}));
                let mut body = json!({
                    "model": self.model,
                    "messages": messages,
                    "max_tokens": request.max_tokens,
                    "temperature": request.temperature,
                });
                if !request.stop.is_empty() {
                    body["stop"] = json!(request.stop);
                }
                // Honoured by some servers and ignored by others, which is why it is
                // sent rather than relied upon. The Messages API has no equivalent.
                body["seed"] = json!(request.seed);
                body
            }
            Wire::Anthropic => {
                // The system message is a top-level field here, not a message with a
                // role - putting it in the array is accepted and then behaves
                // differently, which is the worst of both.
                let mut body = json!({
                    "model": self.model,
                    "max_tokens": request.max_tokens,
                    "messages": [{"role": "user", "content": request.prompt}],
                });
                if let Some(system) = &request.system {
                    body["system"] = json!(system);
                }
                if !request.stop.is_empty() {
                    body["stop_sequences"] = json!(request.stop);
                }
                body
            }
        }
    }

    /// The reply text, or an account of why there is none.
    fn text(&self, reply: &Value) -> Result<String, Error> {
        match self.wire {
            Wire::OpenAi => reply
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| Error::Protocol(shape_of(reply))),
            Wire::Anthropic => {
                // A refusal arrives as a successful response with an empty reply, so
                // checking the status code is not enough. Reporting it as a refusal
                // rather than as an empty answer is the whole difference between a
                // caller retrying and a caller silently proposing nothing.
                if reply.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
                    return Err(Error::Refused(
                        reply
                            .pointer("/stop_details/category")
                            .and_then(Value::as_str)
                            .unwrap_or("unstated")
                            .to_owned(),
                    ));
                }
                let blocks = reply
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Error::Protocol(shape_of(reply)))?;
                let text: String = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect();
                if text.is_empty() {
                    return Err(Error::Protocol(shape_of(reply)));
                }
                Ok(text)
            }
        }
    }
}

impl Engine for OnlineEngine {
    fn describe(&self) -> String {
        match self.wire {
            Wire::Anthropic => format!(
                "{} - {} at {} (deterministic; this wire rejects a temperature)",
                self.id, self.model, self.endpoint
            ),
            Wire::OpenAi => format!("{} - {} at {}", self.id, self.model, self.endpoint),
        }
    }

    fn model(&self) -> String {
        self.model.clone()
    }

    fn complete(&self, request: &Request) -> Result<String, Error> {
        let mut post = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json");
        post = match (self.wire, self.key.as_deref()) {
            (Wire::Anthropic, key) => {
                let post = post.header("anthropic-version", ANTHROPIC_VERSION);
                match key {
                    Some(key) => post.header("x-api-key", key),
                    None => post,
                }
            }
            (Wire::OpenAi, Some(key)) => post.bearer_auth(key),
            (Wire::OpenAi, None) => post,
        };

        let response = post
            .json(&self.body(request))
            .send()
            .map_err(|e| Error::Transport(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .map_err(|e| Error::Transport(e.to_string()))?;
        if !status.is_success() {
            // The body carries the reason and the status alone does not, so both go in
            // - a bare "400" from a hosted endpoint is unactionable.
            return Err(Error::Rejected {
                status: status.as_u16(),
                body: truncate(&body),
            });
        }
        let reply: Value =
            serde_json::from_str(&body).map_err(|e| Error::Protocol(e.to_string()))?;
        self.text(&reply)
    }
}

/// Describes an unexpected reply by its shape rather than its content.
///
/// A hosted endpoint's error body can carry an account identifier or an echo of the
/// prompt, and this string ends up in logs and run reports.
fn shape_of(reply: &Value) -> String {
    match reply {
        Value::Object(map) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            format!("reply carried no text; fields: {}", keys.join(", "))
        }
        other => format!("reply was {} rather than an object", kind_of(other)),
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Keeps an error body to a size a report can hold.
fn truncate(body: &str) -> String {
    const LIMIT: usize = 400;
    let trimmed = body.trim();
    match trimmed.char_indices().nth(LIMIT) {
        Some((at, _)) => format!("{}...", &trimmed[..at]),
        None => trimmed.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ANTHROPIC_VERSION, OnlineEngine};
    use crate::catalog::Catalog;
    use crate::config::{Integration, Kind};
    use crate::engine::{Engine, Request};
    use crate::select::Device;

    fn integration(source: &str) -> Integration {
        Integration {
            id: source.to_owned(),
            name: source.to_owned(),
            kind: Kind::Online,
            source: source.to_owned(),
            model: None,
            endpoint: None,
            api_key: Some("test-key".to_owned()),
            device: Device::Cpu,
        }
    }

    fn engine(source: &str) -> OnlineEngine {
        OnlineEngine::new(&integration(source), &Catalog::default()).expect("builds")
    }

    /// The Messages API gets its own request shape.
    ///
    /// `system` is a top-level field, not a message with a role. Sending it as a
    /// message is accepted and then behaves differently, which is the failure mode
    /// worth pinning: no error, different answers.
    #[test]
    fn the_messages_api_puts_system_at_the_top_level() {
        let body = engine("anthropic").body(&Request::new("hi").with_system("rules"));
        assert_eq!(body["system"], json!("rules"));
        assert_eq!(body["messages"].as_array().expect("array").len(), 1);
        assert_eq!(body["messages"][0]["role"], json!("user"));
    }

    /// The Messages API is never sent a temperature.
    ///
    /// Its current models reject the parameter outright, so sending the crate's own
    /// default of zero would fail every request to a correctly configured provider.
    #[test]
    fn the_messages_api_is_never_sent_a_temperature() {
        let body = engine("anthropic").body(&Request::new("hi").with_temperature(0.7));
        assert!(body.get("temperature").is_none(), "{body}");
    }

    /// And the engine says so, rather than dropping the parameter quietly.
    #[test]
    fn dropping_the_temperature_is_stated_not_hidden() {
        assert!(engine("anthropic").describe().contains("temperature"));
    }

    /// An OpenAI-shaped request keeps the system message in the array.
    #[test]
    fn an_openai_request_puts_system_in_the_messages() {
        let body = engine("openai").body(&Request::new("hi").with_system("rules"));
        assert_eq!(body["messages"][0]["role"], json!("system"));
        assert_eq!(body["messages"][1]["role"], json!("user"));
        assert_eq!(body["temperature"], json!(0.0));
    }

    /// Stop strings use each wire's own field name.
    #[test]
    fn stop_strings_use_the_right_field_per_wire() {
        let openai = engine("openai").body(&Request::new("hi").with_stop("STOP"));
        assert_eq!(openai["stop"], json!(["STOP"]));
        let anthropic = engine("anthropic").body(&Request::new("hi").with_stop("STOP"));
        assert_eq!(anthropic["stop_sequences"], json!(["STOP"]));
    }

    /// Text is read out of each wire's own reply shape.
    #[test]
    fn text_is_read_from_each_wire() {
        let openai = engine("openai")
            .text(&json!({"choices": [{"message": {"content": "yes"}}]}))
            .expect("text");
        assert_eq!(openai, "yes");

        let anthropic = engine("anthropic")
            .text(&json!({
                "stop_reason": "end_turn",
                "content": [{"type": "text", "text": "yes"}],
            }))
            .expect("text");
        assert_eq!(anthropic, "yes");
    }

    /// A thinking block is not mistaken for the answer.
    ///
    /// The Messages API returns a content *array*, and taking element zero rather than
    /// filtering on type reads whatever happens to be first.
    #[test]
    fn only_text_blocks_become_the_answer() {
        let text = engine("anthropic")
            .text(&json!({
                "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "yes"},
                ],
            }))
            .expect("text");
        assert_eq!(text, "yes");
    }

    /// A refusal is an error, not an empty answer.
    ///
    /// It arrives as a *successful* response, so a caller checking only the status
    /// code sees a healthy request that proposed nothing - and proposing nothing is
    /// indistinguishable from having nothing to propose.
    #[test]
    fn a_refusal_is_reported_as_a_refusal() {
        let err = engine("anthropic")
            .text(&json!({
                "stop_reason": "refusal",
                "stop_details": {"type": "refusal", "category": "cyber"},
                "content": [],
            }))
            .expect_err("refused");
        assert!(err.to_string().contains("cyber"), "{err}");
    }

    /// An unrecognised reply is described by its shape, never by its content.
    ///
    /// Error bodies carry account identifiers and echoes of the prompt, and this
    /// string lands in logs and run reports.
    #[test]
    fn an_unexpected_reply_is_described_without_quoting_it() {
        let err = engine("openai")
            .text(&json!({"error": {"message": "sk-secret-in-here"}}))
            .expect_err("no text");
        let rendered = err.to_string();
        assert!(!rendered.contains("sk-secret-in-here"), "{rendered}");
        assert!(rendered.contains("error"), "{rendered}");
    }

    /// An entry with neither a known provider nor an endpoint is refused.
    ///
    /// The alternative is inventing an address, which principle 3 rules out.
    #[test]
    fn an_entry_with_nowhere_to_send_is_refused() {
        let mut entry = integration("not-a-provider");
        entry.endpoint = None;
        assert!(OnlineEngine::new(&entry, &Catalog::default()).is_err());
    }

    /// A custom endpoint works with no provider entry at all.
    #[test]
    fn a_custom_endpoint_needs_no_provider() {
        let mut entry = integration("not-a-provider");
        entry.endpoint = Some("http://localhost:8080/v1/chat/completions".to_owned());
        assert!(OnlineEngine::new(&entry, &Catalog::default()).is_ok());
    }

    /// The wire version is pinned rather than tracking whatever is newest.
    #[test]
    fn the_wire_version_is_pinned() {
        assert_eq!(ANTHROPIC_VERSION, "2023-06-01");
    }
}
