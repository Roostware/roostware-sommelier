//! The AI coach's backend: asking a model for hints, debugging help, analyses and solutions.
//!
//! The frontend never sees the API key and never talks to a provider. It sends a request
//! (`prompt::ReplyRequest`: what the user asked, their code, the conversation so far) to a Tauri
//! command; this module builds the prompt, calls the provider over HTTP, and streams the text
//! back over a channel as it arrives. Keys live in the OS credential store (`keys`); the provider
//! and model are ordinary settings (`db::settings`).
//!
//! * `prompt`   what the model is told, and the product's rules (never write the solution, …)
//! * `provider` the two wire formats (Anthropic's, and OpenAI's chat API) and the stream loop
//! * `sse`      reading Server-Sent Events
//! * `keys`     the credential store
//! * `commands` the Tauri commands

pub mod commands;
mod error;
pub mod keys;
pub mod prompt;
pub mod provider;
mod sse;
#[cfg(test)]
mod test_server;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::Notify;

pub use error::{Error, Result};

use crate::db::settings::CoachSettings;
use keys::{KeyStore, KeyringKeys};
use prompt::{Analysis, Prompt, Solution, Structured};
use provider::{Config, Provider, Usage};

/// Managed Tauri state: where the keys are, the HTTP client, and the requests in flight.
pub struct Coach {
    keys: Box<dyn KeyStore>,
    http: reqwest::Client,
    cancels: Mutex<HashMap<String, Arc<Notify>>>,
}

impl Coach {
    pub fn new(keys: Box<dyn KeyStore>) -> Self {
        Self {
            keys,
            http: reqwest::Client::builder()
                .connect_timeout(provider::CONNECT_TIMEOUT)
                .build()
                .expect("a plain reqwest client always builds"),
            cancels: Mutex::default(),
        }
    }

    /// The real thing, with keys in the OS credential store under a name derived from the app's
    /// identifier (so a differently-identified build never touches the real keys).
    pub fn production(app: &AppHandle) -> Self {
        Self::new(Box::new(KeyringKeys::new(format!("{}.coach", app.config().identifier))))
    }

    pub fn keys(&self) -> &dyn KeyStore {
        &*self.keys
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Starts tracking a request so it can be cancelled; it stops being tracked when the guard drops.
    pub fn track(&self, request_id: &str) -> Tracked<'_> {
        let notify = Arc::new(Notify::new());
        self.cancels.lock().unwrap().insert(request_id.to_owned(), notify.clone());
        Tracked { coach: self, id: request_id.to_owned(), notify }
    }

    /// Stops a request in flight (the Stop button). Nothing happens if it already finished.
    pub fn cancel(&self, request_id: &str) {
        if let Some(notify) = self.cancels.lock().unwrap().get(request_id) {
            notify.notify_one();
        }
    }
}

pub struct Tracked<'a> {
    coach: &'a Coach,
    id: String,
    pub notify: Arc<Notify>,
}

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.coach.cancels.lock().unwrap().remove(&self.id);
    }
}

// ---- Is it set up? --------------------------------------------------------------------------

/// Which providers have a key saved. (Only whether: a key is never read back out.)
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KeyFlags {
    pub anthropic: bool,
    pub openai: bool,
    pub other: bool,
}

/// Whether the coach can answer, and if not, what's missing.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub ready: bool,
    /// What to do, in a sentence, when not ready.
    pub reason: Option<String>,
    /// The model that will be used (Anthropic's default when none is set).
    pub model: String,
    pub keys: KeyFlags,
}

fn effective_model(settings: &CoachSettings) -> String {
    let model = settings.model.trim();
    match Provider::parse(&settings.provider) {
        Some(Provider::Anthropic) if model.is_empty() => provider::DEFAULT_ANTHROPIC_MODEL.to_owned(),
        _ => model.to_owned(),
    }
}

/// What's missing for the coach to work, if anything.
fn missing(settings: &CoachSettings, keys: &KeyFlags) -> Option<String> {
    if !settings.enabled {
        return Some("The AI Coach is turned off in Settings.".into());
    }
    let model = effective_model(settings);
    match Provider::parse(&settings.provider)? {
        Provider::Anthropic if !keys.anthropic => Some("Add your Anthropic API key in Settings.".into()),
        Provider::OpenAi if !keys.openai => Some("Add your OpenAI API key in Settings.".into()),
        Provider::OpenAi if model.is_empty() => Some("Choose a model in Settings.".into()),
        Provider::Other if settings.base_url.trim().is_empty() => Some("Enter your server's address in Settings.".into()),
        Provider::Other if model.is_empty() => Some("Choose a model in Settings.".into()),
        _ => None,
    }
}

pub fn status(settings: &CoachSettings, keys: &dyn KeyStore) -> Result<Status> {
    let has = |p: Provider| keys.get(p.name()).map(|k| k.is_some_and(|k| !k.trim().is_empty()));
    let flags = KeyFlags { anthropic: has(Provider::Anthropic)?, openai: has(Provider::OpenAi)?, other: has(Provider::Other)? };
    let reason = missing(settings, &flags);
    Ok(Status { ready: reason.is_none(), reason, model: effective_model(settings), keys: flags })
}

/// Where and how to ask, from the settings and the saved key. Fails with what's missing.
pub fn resolve(settings: &CoachSettings, keys: &dyn KeyStore) -> Result<Config> {
    let status = status(settings, keys)?;
    if let Some(reason) = status.reason {
        return Err(Error::NotReady(reason));
    }
    let provider = Provider::parse(&settings.provider).ok_or_else(|| Error::NotReady("Choose a provider in Settings.".into()))?;
    let api_key = keys.get(provider.name())?.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty());
    let endpoint = match provider {
        Provider::Anthropic => provider::ANTHROPIC_ENDPOINT.to_owned(),
        Provider::OpenAi => provider::chat_endpoint(provider::OPENAI_BASE),
        Provider::Other => provider::chat_endpoint(&settings.base_url),
    };
    Ok(Config { provider, endpoint, api_key, model: status.model })
}

// ---- Asking ---------------------------------------------------------------------------------

/// What came back, in the shapes the frontend's cards take. Mirrors `CoachReply` in
/// `src/lib/coachProvider.ts`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Reply {
    Text { text: String },
    Analysis { analysis: Analysis },
    Solution { solution: Solution },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReplyResult {
    pub reply: Reply,
    pub usage: Usage,
}

/// Asks the model and turns its answer into a reply. `on_text` hears plain-text answers as they
/// arrive (answers that become cards aren't streamed: they're only usable whole).
pub async fn ask(
    http: &reqwest::Client,
    cfg: &Config,
    prompt: &Prompt,
    cancel: &Notify,
    mut on_text: impl FnMut(&str) + Send,
) -> Result<ReplyResult> {
    let streamed = provider::stream(http, cfg, prompt, cancel, |piece| {
        if prompt.structured.is_none() {
            on_text(piece);
        }
    })
    .await?;
    let usage = streamed.usage;
    let text = streamed.text.trim();

    let reply = match prompt.structured {
        None => {
            if text.is_empty() {
                return Err(if streamed.truncated { Error::Truncated } else { Error::Api("The model sent an empty reply. Try again.".into()) });
            }
            // A text answer that ran out of room is still worth showing as far as it got.
            Reply::Text { text: text.to_owned() }
        }
        Some(kind) => {
            if streamed.truncated {
                return Err(Error::Truncated);
            }
            match kind {
                Structured::Analysis => Reply::Analysis { analysis: prompt::parse_analysis(text)? },
                Structured::Solution => Reply::Solution { solution: prompt::parse_solution(text)? },
            }
        }
    };
    Ok(ReplyResult { reply, usage })
}

/// What a connection test found out.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionReport {
    pub model: String,
    pub usage: Usage,
}

/// Sends the smallest request there is, to prove the key, model and address work.
pub async fn test_connection(http: &reqwest::Client, cfg: &Config, cancel: &Notify) -> Result<ConnectionReport> {
    let streamed = provider::stream(http, cfg, &prompt::connection_test(), cancel, |_| {}).await?;
    Ok(ConnectionReport { model: cfg.model.clone(), usage: streamed.usage })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::coach::{
        keys::MemoryKeys,
        prompt::{self, Action, ReplyRequest},
        test_server::{serve, Chunk},
    };

    fn settings(provider: &str, model: &str) -> CoachSettings {
        CoachSettings {
            enabled: true,
            provider: provider.into(),
            model: model.into(),
            base_url: String::new(),
            price_input: None,
            price_output: None,
        }
    }

    fn keys_with(entries: &[(&str, &str)]) -> MemoryKeys {
        let keys = MemoryKeys::default();
        for (p, k) in entries {
            keys.set(p, k).unwrap();
        }
        keys
    }

    // ---- Readiness ----

    #[test]
    fn nothing_is_ready_until_the_provider_has_what_it_needs() {
        let reason = |s: &CoachSettings, keys: &MemoryKeys| status(s, keys).unwrap().reason;
        let none = MemoryKeys::default();

        assert_eq!(reason(&settings("anthropic", ""), &none).as_deref(), Some("Add your Anthropic API key in Settings."));
        let anthropic = keys_with(&[("anthropic", "sk-ant")]);
        assert_eq!(reason(&settings("anthropic", ""), &anthropic), None, "Anthropic has a default model");
        assert_eq!(status(&settings("anthropic", ""), &anthropic).unwrap().model, "claude-opus-5-5");
        assert_eq!(status(&settings("anthropic", " claude-haiku-5-5 "), &anthropic).unwrap().model, "claude-haiku-5-5");

        assert_eq!(reason(&settings("openai", "m"), &none).as_deref(), Some("Add your OpenAI API key in Settings."));
        let openai = keys_with(&[("openai", "sk")]);
        assert_eq!(reason(&settings("openai", ""), &openai).as_deref(), Some("Choose a model in Settings."));
        assert_eq!(reason(&settings("openai", "gpt-x"), &openai), None);

        let mut other = settings("other", "llama3");
        assert_eq!(reason(&other, &none).as_deref(), Some("Enter your server's address in Settings."));
        other.base_url = "http://localhost:11434/v1".into();
        assert_eq!(reason(&other, &none), None, "a local server needs no key");
        other.model.clear();
        assert_eq!(reason(&other, &none).as_deref(), Some("Choose a model in Settings."));
    }

    #[test]
    fn switching_the_coach_off_makes_it_not_ready_and_flags_show_which_keys_exist() {
        let keys = keys_with(&[("anthropic", "sk-ant"), ("other", "   ")]);
        let off = CoachSettings { enabled: false, ..settings("anthropic", "") };
        let s = status(&off, &keys).unwrap();
        assert!(!s.ready);
        assert_eq!(s.reason.as_deref(), Some("The AI Coach is turned off in Settings."));
        assert_eq!(s.keys, KeyFlags { anthropic: true, openai: false, other: false }, "a blank key doesn't count");
    }

    #[test]
    fn the_status_serializes_without_any_key_in_it() {
        let keys = keys_with(&[("anthropic", "sk-ant-very-secret")]);
        let json = serde_json::to_string(&status(&settings("anthropic", ""), &keys).unwrap()).unwrap();
        assert!(!json.contains("secret"), "{json}");
        assert!(json.contains("\"ready\":true") && json.contains("\"keys\":{\"anthropic\":true"));
    }

    #[test]
    fn resolving_builds_the_endpoint_for_each_provider() {
        let keys = keys_with(&[("anthropic", " sk-ant "), ("openai", "sk-o")]);
        let a = resolve(&settings("anthropic", ""), &keys).unwrap();
        assert_eq!((a.endpoint.as_str(), a.api_key.as_deref(), a.model.as_str()), ("https://api.anthropic.com/v1/messages", Some("sk-ant"), "claude-opus-5-5"));
        let o = resolve(&settings("openai", "gpt-x"), &keys).unwrap();
        assert_eq!(o.endpoint, "https://api.openai.com/v1/chat/completions");
        let mut other = settings("other", "llama3");
        other.base_url = "http://localhost:11434/v1/".into();
        let c = resolve(&other, &keys).unwrap();
        assert_eq!((c.endpoint.as_str(), c.api_key), ("http://localhost:11434/v1/chat/completions", None));
        assert!(matches!(resolve(&settings("openai", ""), &keys), Err(Error::NotReady(_))));
    }

    // ---- Asking, against a stand-in server ----

    fn request(action: Action) -> ReplyRequest {
        ReplyRequest {
            request_id: "r".into(),
            action,
            problem_id: 1,
            lang: "python3".into(),
            code: Some("def f(): pass".into()),
            submission: None,
            level: 1,
            message: Some("hello".into()),
            turn: 0,
            failed_attempts: 0,
            solution_allowed: false,
            history: vec![],
        }
    }

    fn built(action: Action) -> Prompt {
        let problem = prompt::ProblemText { title: "Two Sum".into(), difficulty: "Easy".into(), statement: "Find two numbers.".into() };
        prompt::build(&request(action), &problem)
    }

    fn config(provider: Provider, url: &str) -> Config {
        Config { provider, endpoint: url.into(), api_key: Some("sk-test-key".into()), model: "claude-opus-5-5".into() }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    /// The start of an Anthropic stream, up to and including the given pieces of text.
    fn anthropic_head(texts: &[&str]) -> String {
        let mut body = String::from("event: message_start
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":900,\"output_tokens\":1}}}

");
        for t in texts {
            body.push_str(&format!("event: content_block_delta
data: {}

", serde_json::json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": t } })));
        }
        body
    }

    fn anthropic_stream(texts: &[&str], stop: &str, output_tokens: u64) -> Vec<Chunk> {
        let mut body = anthropic_head(texts);
        body.push_str(&format!("event: message_delta
data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{stop}\"}},\"usage\":{{\"output_tokens\":{output_tokens}}}}}

"));
        body.push_str("event: message_stop
data: {\"type\":\"message_stop\"}

");
        vec![Chunk::now(body)]
    }

    #[tokio::test]
    async fn a_hint_streams_from_anthropic_with_the_key_and_the_prompt_in_the_request() {
        let server = serve(200, anthropic_stream(&["Think about ", "what you've seen."], "end_turn", 25));
        let mut heard = String::new();
        let result = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Hint), &Notify::new(), |t| heard.push_str(t)).await.unwrap();

        assert_eq!(result.reply, Reply::Text { text: "Think about what you've seen.".into() });
        assert_eq!(heard, "Think about what you've seen.");
        assert_eq!(result.usage, Usage { input_tokens: 900, output_tokens: 25 });

        let sent = server.requests();
        assert_eq!(sent.len(), 1);
        let head = sent[0].to_ascii_lowercase();
        assert!(head.contains("x-api-key: sk-test-key") && head.contains("anthropic-version: 2026-10-21"), "{head}");
        let body: serde_json::Value = serde_json::from_str(sent[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["stream"], true);
        assert!(body["system"].as_str().unwrap().contains("Never write a complete solution"));
        let user = body["messages"][0]["content"].as_str().unwrap();
        assert!(user.contains("<problem title=\"Two Sum\"") && user.contains("  1 | def f(): pass"), "{user}");
    }

    #[tokio::test]
    async fn an_openai_style_stream_works_too_and_reports_usage() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hi \"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"there\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":50,\"completion_tokens\":4}}\n\ndata: [DONE]\n\n";
        let server = serve(200, vec![Chunk::now(body)]);
        let mut cfg = config(Provider::OpenAi, &server.url);
        cfg.model = "gpt-x".into();
        let result = ask(&client(), &cfg, &built(Action::Chat), &Notify::new(), |_| {}).await.unwrap();
        assert_eq!(result.reply, Reply::Text { text: "Hi there".into() });
        assert_eq!(result.usage, Usage { input_tokens: 50, output_tokens: 4 });
        let sent = server.requests();
        assert!(sent[0].to_ascii_lowercase().contains("authorization: bearer sk-test-key"));
        let json: serde_json::Value = serde_json::from_str(sent[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(json["model"], "gpt-x");
        assert_eq!(json["messages"][0]["role"], "system");
    }

    #[tokio::test]
    async fn an_analysis_arrives_as_a_card_and_is_not_streamed_as_text() {
        let json = r#"{"time":{"big":"O(n)","why":"one pass"},"space":{"big":"O(n)","why":"a map"},"optimal":{"time":"O(n)","space":"O(n)","note":"Same."},"wentWell":["Clear"],"improve":["Line [L2] repeats work"],"takeaways":["Maps"]}"#;
        let chunks = anthropic_stream(&["```json\n", &json[..40], &json[40..], "\n```"], "end_turn", 120);
        let server = serve(200, chunks);
        let mut heard = String::new();
        let result = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Analyze), &Notify::new(), |t| heard.push_str(t)).await.unwrap();
        assert_eq!(heard, "", "JSON isn't shown as it arrives");
        let Reply::Analysis { analysis } = result.reply else { panic!("{:?}", result.reply) };
        assert_eq!(analysis.time.big, "O(n)");
        assert_eq!(analysis.improve, ["Line [L2] repeats work"]);
    }

    // ---- A provider that stalls must not leave the request waiting for ever ----

    /// Limits small enough to hit within a test.
    fn quick(start_ms: u64, idle_ms: u64, total_ms: u64) -> provider::Limits {
        provider::Limits { start: Duration::from_millis(start_ms), idle: Duration::from_millis(idle_ms), total: Duration::from_millis(total_ms) }
    }

    async fn stream_with_limits(url: &str, limits: provider::Limits) -> Result<provider::Streamed> {
        provider::stream_with(&client(), &config(Provider::Anthropic, url), &built(Action::Analyze), &Notify::new(), |_| {}, limits).await
    }

    #[tokio::test]
    async fn a_provider_that_takes_the_request_and_never_answers_is_given_up_on() {
        let server = crate::coach::test_server::serve_silent(Duration::from_secs(20));
        let began = std::time::Instant::now();
        let err = stream_with_limits(&server.url, quick(300, 5_000, 20_000)).await.unwrap_err();
        assert!(matches!(&err, Error::Network(m) if m.contains("start answering")), "{err}");
        assert!(began.elapsed() < Duration::from_secs(5), "gave up after {:?}", began.elapsed());
    }

    #[tokio::test]
    async fn a_provider_that_goes_quiet_in_the_middle_is_given_up_on() {
        // The start of an answer, and then nothing for far longer than the idle limit.
        let head = anthropic_head(&["{\"time\":"]);
        let server = serve(200, vec![Chunk::now(head), Chunk::after(Duration::from_secs(10), "x")]);
        let err = stream_with_limits(&server.url, quick(5_000, 300, 20_000)).await.unwrap_err();
        assert!(matches!(&err, Error::Network(m) if m.contains("stopped sending")), "{err}");
    }

    #[tokio::test]
    async fn keep_alive_pings_do_not_keep_a_request_alive_for_ever() {
        // A comment every 100ms: never idle for long, never saying anything either.
        let pings = (0..60).map(|_| Chunk::after(Duration::from_millis(100), ": keep-alive\n\n")).collect();
        let server = serve(200, pings);
        let began = std::time::Instant::now();
        let err = stream_with_limits(&server.url, quick(5_000, 2_000, 600)).await.unwrap_err();
        assert!(matches!(&err, Error::Network(m) if m.contains("took longer")), "{err}");
        assert!(began.elapsed() < Duration::from_secs(4), "gave up after {:?}", began.elapsed());
    }

    #[tokio::test]
    async fn the_limits_do_not_get_in_the_way_of_a_normal_answer() {
        let json = r#"{"time":{"big":"O(n)","why":"x"},"space":{"big":"O(n)","why":"y"},"optimal":{"time":"O(n)","space":"O(n)","note":"n"},"wentWell":["a"],"improve":[],"takeaways":["t"]}"#;
        let server = serve(200, anthropic_stream(&[json], "end_turn", 50));
        let streamed = stream_with_limits(&server.url, quick(2_000, 2_000, 10_000)).await.unwrap();
        assert!(streamed.text.contains("\"takeaways\""));
        assert!(!streamed.truncated);
    }

    #[tokio::test]
    async fn a_solution_card_is_read_and_a_garbled_one_is_an_error() {
        let ok = r#"{"intuition":"Remember.","steps":["a"],"code":"x = 1","complexity":"O(n)","divergence":null,"takeaways":["t"]}"#;
        let server = serve(200, anthropic_stream(&[ok], "end_turn", 50));
        let result = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Solution), &Notify::new(), |_| {}).await.unwrap();
        assert!(matches!(result.reply, Reply::Solution { ref solution } if solution.code == "x = 1"));

        let server = serve(200, anthropic_stream(&["Sorry, I can't format that."], "end_turn", 5));
        let err = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Solution), &Notify::new(), |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::Unreadable), "{err}");
    }

    #[tokio::test]
    async fn running_out_of_room_is_an_error_for_cards_and_a_partial_answer_for_text() {
        let server = serve(200, anthropic_stream(&["{\"time\":"], "max_tokens", 16000));
        let err = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Analyze), &Notify::new(), |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::Truncated));

        let server = serve(200, anthropic_stream(&["Part of a long hint"], "max_tokens", 16000));
        let result = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Hint), &Notify::new(), |_| {}).await.unwrap();
        assert_eq!(result.reply, Reply::Text { text: "Part of a long hint".into() });

        let server = serve(200, anthropic_stream(&[], "max_tokens", 16000));
        let err = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Hint), &Notify::new(), |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::Truncated), "all of it went on thinking: {err}");
    }

    #[tokio::test]
    async fn a_refusal_and_an_empty_reply_are_errors() {
        let refused = "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\",\"stop_details\":{\"explanation\":\"Declined.\"}},\"usage\":{\"output_tokens\":2}}\n\n";
        let server = serve(200, vec![Chunk::now(refused)]);
        let err = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Hint), &Notify::new(), |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::Refused(ref why) if why == "Declined."), "{err}");

        let server = serve(200, anthropic_stream(&[], "end_turn", 0));
        let err = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Hint), &Notify::new(), |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::Api(ref m) if m.contains("empty reply")), "{err}");
    }

    #[tokio::test]
    async fn the_providers_own_errors_come_through_without_the_key() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let server = serve(401, vec![Chunk::now(body)]);
        let err = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Hint), &Notify::new(), |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::Auth(_)));
        assert!(err.to_string().contains("Check it in Settings") && !err.to_string().contains("sk-test-key"));

        let server = serve(429, vec![Chunk::now(r#"{"error":{"message":"Slow down"}}"#)]);
        let err = ask(&client(), &config(Provider::OpenAi, &server.url), &built(Action::Hint), &Notify::new(), |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::RateLimited(_)) && err.to_string().contains("Slow down"));

        let server = serve(400, vec![Chunk::now(r#"{"error":{"message":"model: bogus not found"}}"#)]);
        let err = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Hint), &Notify::new(), |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("HTTP 400") && err.to_string().contains("bogus"), "{err}");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_network_error_that_does_not_leak_the_address_details() {
        let err = ask(&client(), &config(Provider::Anthropic, "http://127.0.0.1:9/v1/messages"), &built(Action::Hint), &Notify::new(), |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::Network(_)), "{err}");
        assert!(!err.to_string().contains("sk-test-key"));
    }

    #[tokio::test]
    async fn stop_ends_a_reply_in_the_middle_and_before_it_starts() {
        // The server sends one piece, then keeps the stream open.
        let chunks = vec![
            Chunk::now(anthropic_head(&["Starting to say"])),
            Chunk::after(Duration::from_secs(30), "event: message_stop
data: {\"type\":\"message_stop\"}

"),
        ];
        let server = serve(200, chunks);

        let notify = Arc::new(Notify::new());
        let canceller = notify.clone();
        let heard = Arc::new(Mutex::new(String::new()));
        let sink = heard.clone();
        let cfg = config(Provider::Anthropic, &server.url);
        let prompt = built(Action::Hint);
        let http = client();
        let task = tokio::spawn(async move { ask(&http, &cfg, &prompt, &notify, |t| sink.lock().unwrap().push_str(t)).await });

        // Wait until the first words are in, then press Stop.
        for _ in 0..100 {
            if heard.lock().unwrap().contains("Starting") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(heard.lock().unwrap().contains("Starting"), "the first piece arrived");
        canceller.notify_one();
        let started = std::time::Instant::now();
        let err = task.await.unwrap().unwrap_err();
        assert!(matches!(err, Error::Cancelled), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5), "it didn't wait for the server");

        // A Stop pressed before the request even goes out also works (the permit is kept).
        let notify = Notify::new();
        notify.notify_one();
        let server = serve(200, anthropic_stream(&["x"], "end_turn", 1));
        let err = ask(&client(), &config(Provider::Anthropic, &server.url), &built(Action::Hint), &notify, |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::Cancelled));
    }

    #[test]
    fn requests_can_be_tracked_and_cancelled_by_id() {
        let coach = Coach::new(Box::new(MemoryKeys::default()));
        let tracked = coach.track("abc");
        coach.cancel("nope"); // nothing in flight: fine
        coach.cancel("abc");
        let told = block_on(async { tokio::time::timeout(Duration::from_millis(200), tracked.notify.notified()).await.is_ok() });
        assert!(told, "the request was told to stop");
        drop(tracked);
        coach.cancel("abc"); // gone now: fine
        assert!(coach.cancels.lock().unwrap().is_empty());
    }

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(f)
    }

    #[tokio::test]
    async fn the_connection_test_reports_the_model_and_what_it_cost() {
        let server = serve(200, anthropic_stream(&["ok"], "end_turn", 2));
        let report = test_connection(&client(), &config(Provider::Anthropic, &server.url), &Notify::new()).await.unwrap();
        assert_eq!(report, ConnectionReport { model: "claude-opus-5-5".into(), usage: Usage { input_tokens: 900, output_tokens: 2 } });

        let server = serve(401, vec![Chunk::now(r#"{"error":{"message":"bad"}}"#)]);
        assert!(matches!(test_connection(&client(), &config(Provider::Anthropic, &server.url), &Notify::new()).await, Err(Error::Auth(_))));
    }
}
