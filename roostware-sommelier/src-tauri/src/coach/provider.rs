//! Talking to the model providers, over plain HTTP (there's no official Rust SDK): Anthropic's
//! Messages API, and the chat-completions API that OpenAI and many other servers (OpenRouter,
//! Ollama, LM Studio, …) speak. Both are asked for a streamed reply, so the text can appear as it
//! is written and the Stop button can cut it short.
//!
//! Everything provider-specific (paths, headers, field names, how a stream ends and how errors
//! look) is in this file. The key is only ever put in a request header; it is never logged or put
//! in an error message.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::Notify;

use super::{
    error::{Error, Result},
    prompt::{Prompt, Role},
    sse::{SseEvent, SseParser},
};

pub const ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
pub const OPENAI_BASE: &str = "https://api.openai.com/v1";
/// The Claude API version this code is written against.
const ANTHROPIC_VERSION: &str = "2026-10-21";
/// What Anthropic is asked for when the user hasn't named a model.
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-5-5";

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long each stage of a request may take before it's given up on, so a provider that stalls
/// can't leave the UI "thinking" for ever. All three are generous: a model that thinks before it
/// answers, or a local one loading itself into memory, can be slow.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// From sending the request to the provider starting to answer. (Connecting has its own,
    /// shorter, limit; without this one, a provider that accepts the connection and then says
    /// nothing is waited on for ever.)
    pub start: Duration,
    /// Without a byte from the provider while it is answering: a dead connection.
    pub idle: Duration,
    /// The whole request. Providers can send keep-alive pings while they wait on something upstream,
    /// which would keep the idle limit from ever being reached.
    pub total: Duration,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        start: Duration::from_secs(120),
        idle: Duration::from_secs(150),
        total: Duration::from_secs(10 * 60),
    };
}
/// More than this much reply text is a runaway.
const MAX_REPLY_BYTES: usize = 2 * 1024 * 1024;
/// The most of an error body that's read and shown.
const MAX_ERROR_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAi,
    /// Any server that speaks OpenAI's chat API, at an address the user gives.
    Other,
}

impl Provider {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAi),
            "other" => Some(Self::Other),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Other => "other",
        }
    }
}

/// Where and how to ask.
#[derive(Debug, Clone)]
pub struct Config {
    pub provider: Provider,
    /// The full URL of the chat endpoint.
    pub endpoint: String,
    /// Absent only for a local server that doesn't ask for one.
    pub api_key: Option<String>,
    pub model: String,
}

/// What a reply cost, as the provider counted it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// What came back from a streamed request.
#[derive(Debug, Clone, Default)]
pub struct Streamed {
    pub text: String,
    pub usage: Usage,
    /// The model stopped because it ran out of room.
    pub truncated: bool,
}

/// The chat endpoint for an OpenAI-compatible base address (`…/v1`).
pub fn chat_endpoint(base: &str) -> String {
    format!("{}/chat/completions", base.trim().trim_end_matches('/'))
}

/// Models that take Anthropic's `effort` setting. (Older ones reject it.)
fn supports_effort(model: &str) -> bool {
    ["claude-opus-4-6", "claude-opus-4-7", "claude-opus-4-8", "claude-opus-5", "claude-sonnet-4-6", "claude-sonnet-5", "claude-haiku-5", "claude-fable", "claude-mythos"]
        .iter()
        .any(|p| model.starts_with(p))
}

/// Models that can be told to re-run a refused request on a fallback model.
fn supports_fallbacks(model: &str) -> bool {
    ["claude-opus-5-5", "claude-fable-5-2", "claude-fable-5-1", "claude-sonnet-5-5"].iter().any(|p| model.starts_with(p))
}

/// The request body and the headers beyond the key, for a provider.
fn request(cfg: &Config, prompt: &Prompt) -> Result<(Value, HeaderMap)> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let bad_key = || Error::NotReady("The API key contains characters that can't be sent. Enter it again in Settings.".into());
    let turns = |prompt: &Prompt| -> Vec<Value> {
        prompt
            .turns
            .iter()
            .map(|t| json!({ "role": if t.role == Role::User { "user" } else { "assistant" }, "content": t.content }))
            .collect()
    };

    match cfg.provider {
        Provider::Anthropic => {
            let key = cfg.api_key.as_deref().ok_or_else(|| Error::NotReady("Add your Anthropic API key in Settings.".into()))?;
            let mut key_value = HeaderValue::from_str(key).map_err(|_| bad_key())?;
            key_value.set_sensitive(true);
            headers.insert(HeaderName::from_static("x-api-key"), key_value);
            headers.insert(HeaderName::from_static("anthropic-version"), HeaderValue::from_static(ANTHROPIC_VERSION));
            let mut body = json!({
                "model": cfg.model,
                "max_tokens": prompt.max_tokens,
                "stream": true,
                "system": prompt.system,
                "messages": turns(prompt),
            });
            if supports_effort(&cfg.model) {
                body["output_config"] = json!({ "effort": prompt.effort.as_str() });
            }
            // If the provider's safety systems decline, have it re-run the request on a fallback
            // model rather than just stopping.
            if supports_fallbacks(&cfg.model) {
                headers.insert(HeaderName::from_static("anthropic-beta"), HeaderValue::from_static("server-side-fallback-2026-07-01"));
                body["fallbacks"] = json!("default");
            }
            Ok((body, headers))
        }
        Provider::OpenAi | Provider::Other => {
            if let Some(key) = cfg.api_key.as_deref() {
                let mut value = HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| bad_key())?;
                value.set_sensitive(true);
                headers.insert(AUTHORIZATION, value);
            } else if cfg.provider == Provider::OpenAi {
                return Err(Error::NotReady("Add your OpenAI API key in Settings.".into()));
            }
            let mut messages = vec![json!({ "role": "system", "content": prompt.system })];
            messages.extend(turns(prompt));
            let mut body = json!({
                "model": cfg.model,
                "stream": true,
                "stream_options": { "include_usage": true },
                "messages": messages,
            });
            // OpenAI's current models want `max_completion_tokens`; other servers only know `max_tokens`.
            if cfg.provider == Provider::OpenAi {
                body["max_completion_tokens"] = json!(prompt.max_tokens);
            } else {
                body["max_tokens"] = json!(prompt.max_tokens.min(8_000));
            }
            Ok((body, headers))
        }
    }
}

/// What an error object says: its message, plus whatever the provider added about who failed and
/// why. OpenRouter, for one, answers a failure on the model host's side with just "Provider
/// returned error" and puts the host's name and its own words in `metadata`, which is the part
/// that says what to do about it.
fn describe(e: &Value) -> Option<String> {
    let message = e.get("message").and_then(Value::as_str).or_else(|| e.as_str())?.trim();
    let metadata = e.get("metadata").filter(|m| m.is_object());
    let who = metadata.and_then(|m| m.get("provider_name")).and_then(Value::as_str).map(str::trim).filter(|w| !w.is_empty());
    let raw = metadata
        .and_then(|m| m.get("raw"))
        .filter(|r| !r.is_null())
        .map(|r| match r {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .map(|raw| {
            // The host's own answer is often JSON with a message in it.
            serde_json::from_str::<Value>(&raw)
                .ok()
                .and_then(|v| v.pointer("/error/message").or_else(|| v.get("message")).and_then(Value::as_str).map(str::to_owned))
                .unwrap_or(raw)
        })
        .map(|raw| raw.trim().chars().take(300).collect::<String>())
        .filter(|raw| !raw.is_empty());
    Some(match (who, raw) {
        (Some(who), Some(raw)) => format!("{message} ({who}: {raw})"),
        (Some(who), None) => format!("{message} ({who})"),
        (None, Some(raw)) => format!("{message} ({raw})"),
        (None, None) => message.to_owned(),
    })
}

/// A short, key-free description of an error response.
fn api_error(status: reqwest::StatusCode, body: &str) -> Error {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            // Anthropic: {"error":{"type","message"}}; OpenAI and most others: {"error":{"message"}}.
            describe(v.get("error")?)
        })
        .unwrap_or_else(|| {
            // JSON with no message in it says nothing useful; anything else (an HTML error page,
            // a plain-text line) is shown as far as it goes.
            let text = body.trim();
            if text.starts_with('{') || text.starts_with('[') {
                String::new()
            } else {
                text.chars().take(300).collect()
            }
        });
    let message = if message.is_empty() { format!("HTTP {}", status.as_u16()) } else { message };
    match status.as_u16() {
        401 | 403 => Error::Auth(message),
        429 => Error::RateLimited(message),
        code => Error::Api(format!("The provider returned an error (HTTP {code}): {message}")),
    }
}

/// How a stream is going, filled in event by event.
#[derive(Default)]
struct State {
    streamed: Streamed,
    /// The provider declined to answer.
    refused: Option<String>,
    /// The stream said it was finished.
    finished: bool,
}

fn number(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// One event of Anthropic's stream.
fn anthropic_event(event: &SseEvent, state: &mut State, on_text: &mut impl FnMut(&str)) -> Result<()> {
    let Ok(v) = serde_json::from_str::<Value>(&event.data) else { return Ok(()) };
    match v.get("type").and_then(Value::as_str) {
        Some("message_start") => {
            let usage = &v["message"]["usage"];
            state.streamed.usage.input_tokens = number(&usage["input_tokens"])
                + number(&usage["cache_creation_input_tokens"])
                + number(&usage["cache_read_input_tokens"]);
            state.streamed.usage.output_tokens = number(&usage["output_tokens"]);
        }
        Some("content_block_delta") => {
            if let Some(text) = v["delta"]["text"].as_str().filter(|_| v["delta"]["type"] == "text_delta") {
                state.streamed.text.push_str(text);
                on_text(text);
            }
        }
        Some("message_delta") => {
            let usage = &v["usage"];
            if usage["output_tokens"].is_number() {
                state.streamed.usage.output_tokens = number(&usage["output_tokens"]);
            }
            if usage["input_tokens"].is_number() {
                state.streamed.usage.input_tokens = number(&usage["input_tokens"]);
            }
            match v["delta"]["stop_reason"].as_str() {
                Some("max_tokens") => state.streamed.truncated = true,
                Some("refusal") => {
                    let why = v["delta"]["stop_details"]["explanation"].as_str().unwrap_or("").trim().to_owned();
                    state.refused = Some(why);
                }
                _ => {}
            }
        }
        Some("message_stop") => state.finished = true,
        Some("error") => {
            let e = &v["error"];
            let message = e["message"].as_str().unwrap_or("the provider reported an error");
            return Err(match e["type"].as_str() {
                Some("authentication_error") | Some("permission_error") => Error::Auth(message.to_owned()),
                Some("rate_limit_error") => Error::RateLimited(message.to_owned()),
                _ => Error::Api(format!("The provider reported an error: {message}")),
            });
        }
        _ => {}
    }
    Ok(())
}

/// One event of an OpenAI-style stream.
fn openai_event(event: &SseEvent, state: &mut State, on_text: &mut impl FnMut(&str)) -> Result<()> {
    if event.data.trim() == "[DONE]" {
        state.finished = true;
        return Ok(());
    }
    let Ok(v) = serde_json::from_str::<Value>(&event.data) else { return Ok(()) };
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        let message = describe(e).unwrap_or_else(|| "the provider reported an error".into());
        return Err(Error::Api(format!("The provider reported an error: {message}")));
    }
    if let Some(usage) = v.get("usage").filter(|u| u.is_object()) {
        state.streamed.usage.input_tokens = number(&usage["prompt_tokens"]);
        state.streamed.usage.output_tokens = number(&usage["completion_tokens"]);
    }
    if let Some(choice) = v["choices"].get(0) {
        if let Some(text) = choice["delta"]["content"].as_str().filter(|t| !t.is_empty()) {
            state.streamed.text.push_str(text);
            on_text(text);
        }
        match choice["finish_reason"].as_str() {
            Some("length") => state.streamed.truncated = true,
            Some("content_filter") => state.refused = Some(String::new()),
            _ => {}
        }
    }
    Ok(())
}

/// Sends a prompt and streams the reply, calling `on_text` with each piece of text as it arrives.
/// Returns early with `Cancelled` when `cancel` is notified, and with a network error when the
/// provider takes too long (see [`Limits`]).
pub async fn stream(
    http: &reqwest::Client,
    cfg: &Config,
    prompt: &Prompt,
    cancel: &Notify,
    on_text: impl FnMut(&str) + Send,
) -> Result<Streamed> {
    stream_with(http, cfg, prompt, cancel, on_text, Limits::DEFAULT).await
}

/// [`stream`] with its own time limits (so tests needn't wait minutes).
pub async fn stream_with(
    http: &reqwest::Client,
    cfg: &Config,
    prompt: &Prompt,
    cancel: &Notify,
    mut on_text: impl FnMut(&str) + Send,
    limits: Limits,
) -> Result<Streamed> {
    let started = tokio::time::Instant::now();
    let (body, headers) = request(cfg, prompt)?;
    let send = http.post(&cfg.endpoint).headers(headers).json(&body).send();
    let mut response = tokio::select! {
        biased;
        _ = cancel.notified() => return Err(Error::Cancelled),
        r = tokio::time::timeout(limits.start, send) => match r {
            Ok(sent) => sent.map_err(|e| Error::Network(e.without_url().to_string()))?,
            Err(_) => return Err(Error::Network(format!("it didn't start answering within {} seconds", limits.start.as_secs()))),
        },
    };

    let status = response.status();
    if !status.is_success() {
        // The error body is small; read what there is of it (and give up on one that never ends).
        let mut bytes = Vec::new();
        while let Ok(Ok(Some(chunk))) = tokio::time::timeout(limits.idle, response.chunk()).await {
            bytes.extend_from_slice(&chunk);
            if bytes.len() >= MAX_ERROR_BYTES {
                break;
            }
        }
        return Err(api_error(status, &String::from_utf8_lossy(&bytes)));
    }

    let mut parser = SseParser::default();
    let mut state = State::default();
    let handle = |events: Vec<SseEvent>, state: &mut State, on_text: &mut dyn FnMut(&str)| -> Result<()> {
        for event in events {
            let mut forward = |t: &str| on_text(t);
            match cfg.provider {
                Provider::Anthropic => anthropic_event(&event, state, &mut forward)?,
                Provider::OpenAi | Provider::Other => openai_event(&event, state, &mut forward)?,
            }
        }
        Ok(())
    };
    loop {
        // However steadily data arrives, the whole request has a limit too.
        let left = limits.total.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Err(Error::Network(format!("it took longer than {} minutes", limits.total.as_secs().div_ceil(60))));
        }
        let next = tokio::select! {
            biased;
            _ = cancel.notified() => return Err(Error::Cancelled),
            r = tokio::time::timeout(limits.idle.min(left), response.chunk()) => r,
        };
        match next {
            Err(_) if started.elapsed() >= limits.total => {
                return Err(Error::Network(format!("it took longer than {} minutes", limits.total.as_secs().div_ceil(60))))
            }
            Err(_) => return Err(Error::Network("it stopped sending data".into())),
            Ok(Err(e)) => return Err(Error::Network(e.without_url().to_string())),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(bytes))) => handle(parser.push(&bytes), &mut state, &mut on_text)?,
        }
        if state.streamed.text.len() > MAX_REPLY_BYTES {
            return Err(Error::Api("The reply was unreasonably long, so it was stopped.".into()));
        }
    }
    handle(parser.finish(), &mut state, &mut on_text)?;

    if let Some(why) = state.refused {
        return Err(Error::Refused(why));
    }
    Ok(state.streamed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coach::prompt::{Effort, Turn};

    fn prompt() -> Prompt {
        Prompt {
            system: "be brief".into(),
            turns: vec![
                Turn { role: Role::User, content: "hi".into() },
                Turn { role: Role::Assistant, content: "hello".into() },
                Turn { role: Role::User, content: "again".into() },
            ],
            max_tokens: 16_000,
            effort: Effort::High,
            structured: None,
        }
    }

    fn cfg(provider: Provider, model: &str, key: Option<&str>) -> Config {
        Config { provider, endpoint: "http://x".into(), api_key: key.map(str::to_owned), model: model.into() }
    }

    // ---- Requests ----

    #[test]
    fn an_anthropic_request_has_the_key_version_and_no_sampling_parameters() {
        let (body, headers) = request(&cfg(Provider::Anthropic, "claude-opus-5-5", Some("sk-ant-secret")), &prompt()).unwrap();
        assert_eq!(headers["x-api-key"], "sk-ant-secret");
        assert_eq!(headers["anthropic-version"], "2026-10-21");
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!((body["stream"].as_bool(), body["max_tokens"].as_u64()), (Some(true), Some(16_000)));
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"].as_array().unwrap().len(), 3);
        assert_eq!(body["messages"][1], json!({ "role": "assistant", "content": "hello" }));
        assert_eq!(body["output_config"], json!({ "effort": "high" }));
        // Removed on the current models: sending any of these is a 400.
        for gone in ["temperature", "top_p", "top_k", "thinking"] {
            assert!(body.get(gone).is_none(), "{gone}");
        }
    }

    #[test]
    fn refusals_are_given_a_fallback_only_on_models_that_support_it() {
        let (body, headers) = request(&cfg(Provider::Anthropic, "claude-opus-5-5", Some("k")), &prompt()).unwrap();
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(headers["anthropic-beta"], "server-side-fallback-2026-07-01");
        for model in ["claude-haiku-5-5", "claude-opus-4-6", "claude-sonnet-5", "some-future-model"] {
            let (body, headers) = request(&cfg(Provider::Anthropic, model, Some("k")), &prompt()).unwrap();
            assert!(body.get("fallbacks").is_none() && headers.get("anthropic-beta").is_none(), "{model}");
        }
    }

    #[test]
    fn effort_is_only_sent_to_models_that_know_it() {
        for (model, sent) in [("claude-opus-5-5", true), ("claude-sonnet-5-5", true), ("claude-haiku-5-5", true), ("claude-haiku-4-5", false), ("claude-sonnet-4-5", false), ("claude-3-opus", false)] {
            let (body, _) = request(&cfg(Provider::Anthropic, model, Some("k")), &prompt()).unwrap();
            assert_eq!(body.get("output_config").is_some(), sent, "{model}");
        }
    }

    #[test]
    fn an_openai_request_has_a_bearer_key_a_system_message_and_usage_reporting() {
        let (body, headers) = request(&cfg(Provider::OpenAi, "gpt-x", Some("sk-secret")), &prompt()).unwrap();
        assert_eq!(headers[AUTHORIZATION], "Bearer sk-secret");
        assert!(headers[AUTHORIZATION].is_sensitive(), "never shown in debug output");
        assert_eq!(body["messages"][0], json!({ "role": "system", "content": "be brief" }));
        assert_eq!(body["messages"].as_array().unwrap().len(), 4);
        assert_eq!(body["stream_options"], json!({ "include_usage": true }));
        assert_eq!(body["max_completion_tokens"], 16_000);
        assert!(body.get("max_tokens").is_none() && body.get("temperature").is_none());
    }

    #[test]
    fn another_server_uses_max_tokens_and_may_have_no_key() {
        let (body, headers) = request(&cfg(Provider::Other, "llama3", None), &prompt()).unwrap();
        assert!(headers.get(AUTHORIZATION).is_none());
        assert_eq!(body["max_tokens"], 8_000);
        assert!(body.get("max_completion_tokens").is_none());
        // OpenAI itself and Anthropic can't be reached without one.
        assert!(matches!(request(&cfg(Provider::OpenAi, "m", None), &prompt()), Err(Error::NotReady(_))));
        assert!(matches!(request(&cfg(Provider::Anthropic, "m", None), &prompt()), Err(Error::NotReady(_))));
    }

    #[test]
    fn a_key_that_cannot_be_sent_is_reported_without_echoing_it() {
        let err = request(&cfg(Provider::Anthropic, "m", Some("bad\nkey")), &prompt()).unwrap_err();
        assert!(matches!(err, Error::NotReady(_)));
        assert!(!err.to_string().contains("bad"));
    }

    #[test]
    fn chat_endpoints_are_built_from_the_base_address() {
        assert_eq!(chat_endpoint("https://api.openai.com/v1"), "https://api.openai.com/v1/chat/completions");
        assert_eq!(chat_endpoint(" http://localhost:11434/v1/ "), "http://localhost:11434/v1/chat/completions");
    }

    // ---- Errors ----

    #[test]
    fn api_errors_are_readable_and_sorted_by_what_the_user_can_do() {
        use reqwest::StatusCode as S;
        let anthropic = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert!(matches!(api_error(S::UNAUTHORIZED, anthropic), Error::Auth(m) if m == "invalid x-api-key"));
        assert!(matches!(api_error(S::FORBIDDEN, "{}"), Error::Auth(_)));
        let openai = r#"{"error":{"message":"Rate limit reached","type":"requests"}}"#;
        assert!(matches!(api_error(S::TOO_MANY_REQUESTS, openai), Error::RateLimited(m) if m == "Rate limit reached"));
        let e = api_error(S::BAD_REQUEST, r#"{"error":{"message":"model: not found"}}"#);
        assert!(e.to_string().contains("HTTP 400") && e.to_string().contains("model: not found"), "{e}");
        assert!(api_error(S::BAD_GATEWAY, "<html>oops</html>").to_string().contains("oops"));
        assert!(api_error(S::INTERNAL_SERVER_ERROR, "").to_string().contains("HTTP 500"));
        assert!(api_error(S::BAD_REQUEST, r#"{"error":"plain string"}"#).to_string().contains("plain string"));
    }

    #[test]
    fn a_hosts_own_reason_is_shown_when_the_provider_only_says_provider_returned_error() {
        use reqwest::StatusCode as S;
        // OpenRouter's shape for a failure on the model host's side.
        let wrapped = r#"{"error":{"message":"Provider returned error","code":429,"metadata":{"raw":"{\"error\":{\"message\":\"model is temporarily rate-limited upstream. Please retry shortly.\"}}","provider_name":"Venice"}}}"#;
        let e = api_error(S::TOO_MANY_REQUESTS, wrapped);
        assert!(matches!(e, Error::RateLimited(_)));
        let text = e.to_string();
        assert!(text.contains("Provider returned error (Venice: model is temporarily rate-limited upstream."), "{text}");
        // The raw text may be plain, and either half may be missing.
        let plain = r#"{"error":{"message":"Provider returned error","metadata":{"raw":"upstream said no","provider_name":"Acme"}}}"#;
        assert!(api_error(S::BAD_GATEWAY, plain).to_string().contains("Provider returned error (Acme: upstream said no)"));
        let name_only = r#"{"error":{"message":"Provider returned error","metadata":{"provider_name":"Acme"}}}"#;
        assert!(api_error(S::BAD_GATEWAY, name_only).to_string().contains("Provider returned error (Acme)"));
        let raw_only = r#"{"error":{"message":"Provider returned error","metadata":{"raw":{"detail":"overloaded"}}}}"#;
        assert!(api_error(S::BAD_GATEWAY, raw_only).to_string().contains("overloaded"));
        // Very long raw text is cut.
        let long = format!(r#"{{"error":{{"message":"m","metadata":{{"raw":"{}"}}}}}}"#, "x".repeat(2000));
        assert!(api_error(S::BAD_GATEWAY, &long).to_string().len() < 500);
        // The same detail shows when the error arrives inside the stream.
        let mut state = State::default();
        let event = SseEvent { event: None, data: wrapped.replace(r#"{"error":"#, r#"{"error":"#) };
        let err = openai_event(&event, &mut state, &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("Venice: model is temporarily rate-limited upstream"), "{err}");
    }

    // ---- Streams ----

    fn sse(event: &str, data: &str) -> SseEvent {
        SseEvent { event: Some(event.into()), data: data.into() }
    }

    fn run_anthropic(events: &[(&str, &str)]) -> (Result<()>, State, String) {
        let mut state = State::default();
        let mut seen = String::new();
        for (name, data) in events {
            if let Err(e) = anthropic_event(&sse(name, data), &mut state, &mut |t| seen.push_str(t)) {
                return (Err(e), state, seen);
            }
        }
        (Ok(()), state, seen)
    }

    #[test]
    fn anthropic_text_usage_and_stop_are_read_ignoring_thinking() {
        let (r, state, seen) = run_anthropic(&[
            ("message_start", r#"{"type":"message_start","message":{"usage":{"input_tokens":1200,"output_tokens":1}}}"#),
            ("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"abc"}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hel"}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"lo"}}"#),
            ("ping", r#"{"type":"ping"}"#),
            ("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]);
        r.unwrap();
        assert_eq!((state.streamed.text.as_str(), seen.as_str()), ("Hello", "Hello"), "thinking isn't shown");
        assert_eq!(state.streamed.usage, Usage { input_tokens: 1200, output_tokens: 42 });
        assert!(state.finished && !state.streamed.truncated && state.refused.is_none());
    }

    #[test]
    fn anthropic_refusals_and_truncation_are_noticed() {
        let (_, state, _) = run_anthropic(&[(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber","explanation":"Not something I can help with."}},"usage":{"output_tokens":3}}"#,
        )]);
        assert_eq!(state.refused.as_deref(), Some("Not something I can help with."));
        let (_, state, _) = run_anthropic(&[("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":16000}}"#)]);
        assert!(state.streamed.truncated);
    }

    #[test]
    fn anthropic_stream_errors_become_errors() {
        let overloaded = (("error"), r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#);
        let (r, _, _) = run_anthropic(&[overloaded]);
        assert!(matches!(r, Err(Error::Api(m)) if m.contains("Overloaded")));
        let (r, _, _) = run_anthropic(&[("error", r#"{"type":"error","error":{"type":"authentication_error","message":"bad key"}}"#)]);
        assert!(matches!(r, Err(Error::Auth(_))));
        // Garbage between events is skipped, not fatal.
        let (r, state, _) = run_anthropic(&[("content_block_delta", "not json"), ("content_block_delta", r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"ok"}}"#)]);
        r.unwrap();
        assert_eq!(state.streamed.text, "ok");
    }

    fn run_openai(datas: &[&str]) -> (Result<()>, State, String) {
        let mut state = State::default();
        let mut seen = String::new();
        for data in datas {
            let event = SseEvent { event: None, data: (*data).into() };
            if let Err(e) = openai_event(&event, &mut state, &mut |t| seen.push_str(t)) {
                return (Err(e), state, seen);
            }
        }
        (Ok(()), state, seen)
    }

    #[test]
    fn openai_text_usage_and_done_are_read() {
        let (r, state, seen) = run_openai(&[
            r#"{"choices":[{"delta":{"role":"assistant","content":""}}]}"#,
            r#"{"choices":[{"delta":{"content":"Hel"}}]}"#,
            r#"{"choices":[{"delta":{"content":"lo"},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":321,"completion_tokens":17}}"#,
            "[DONE]",
        ]);
        r.unwrap();
        assert_eq!((state.streamed.text.as_str(), seen.as_str()), ("Hello", "Hello"));
        assert_eq!(state.streamed.usage, Usage { input_tokens: 321, output_tokens: 17 });
        assert!(state.finished && !state.streamed.truncated);
    }

    #[test]
    fn openai_length_filter_and_error_events() {
        let (_, state, _) = run_openai(&[r#"{"choices":[{"delta":{"content":"x"},"finish_reason":"length"}]}"#]);
        assert!(state.streamed.truncated);
        let (_, state, _) = run_openai(&[r#"{"choices":[{"delta":{},"finish_reason":"content_filter"}]}"#]);
        assert!(state.refused.is_some());
        let (r, _, _) = run_openai(&[r#"{"error":{"message":"context length exceeded"}}"#]);
        assert!(matches!(r, Err(Error::Api(m)) if m.contains("context length exceeded")));
        // Reasoning-style extras and a null content are ignored.
        let (_, state, _) = run_openai(&[r#"{"choices":[{"delta":{"reasoning_content":"thinking","content":null}}]}"#]);
        assert_eq!(state.streamed.text, "");
    }
}
