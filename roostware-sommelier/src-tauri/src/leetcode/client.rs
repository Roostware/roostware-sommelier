//! A small client for LeetCode's unofficial run/submit API: the same endpoints leetcode.com's
//! own pages call. Everything unofficial about it (paths, field names, how errors look) is in
//! this one file, so when LeetCode changes something this is where to fix it.
//!
//! Run and Submit are both two steps: POST the code to get an id, then poll the "check"
//! endpoint with that id until LeetCode has a verdict.

use std::time::Duration;

use reqwest::{
    header::{HeaderMap, HeaderValue, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, USER_AGENT},
    StatusCode,
};
use serde_json::{json, Value};

use super::{auth::Credentials, error::Error, error::Result};

pub const DEFAULT_BASE: &str = "https://leetcode.com";

/// LeetCode's bot protection is friendlier to browser-looking requests.
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

pub struct Client {
    http: reqwest::Client,
    base: String,
    poll_interval: Duration,
    max_polls: u32,
}

impl Client {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("a plain reqwest client always builds"),
            base: base.into().trim_end_matches('/').to_owned(),
            poll_interval: Duration::from_millis(1000),
            // About two minutes: LeetCode queues judging, and big submissions take a while.
            max_polls: 120,
        }
    }

    /// Changes how often and how long to wait for a verdict (tests use tiny values).
    pub fn with_polling(mut self, interval: Duration, max_polls: u32) -> Self {
        self.poll_interval = interval;
        self.max_polls = max_polls;
        self
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn headers(&self, creds: &Credentials, slug: Option<&str>) -> Result<HeaderMap> {
        let invalid = |_| Error::Credentials("the stored LeetCode login contains characters that can't be sent".into());
        let mut h = HeaderMap::new();
        h.insert(USER_AGENT, HeaderValue::from_static(BROWSER_USER_AGENT));
        h.insert(ORIGIN, HeaderValue::from_str(&self.base).map_err(invalid)?);
        let referer = match slug {
            Some(slug) => format!("{}/problems/{slug}/", self.base),
            None => format!("{}/", self.base),
        };
        h.insert(REFERER, HeaderValue::from_str(&referer).map_err(invalid)?);
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert("x-csrftoken", HeaderValue::from_str(&creds.csrf).map_err(invalid)?);
        h.insert(
            COOKIE,
            HeaderValue::from_str(&format!("LEETCODE_SESSION={}; csrftoken={}", creds.session, creds.csrf)).map_err(invalid)?,
        );
        Ok(h)
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let response = request.send().await.map_err(|e| Error::Network(e.without_url().to_string()))?;
        let status = response.status();
        let challenged = response.headers().contains_key("cf-mitigated");
        let body = response.text().await.map_err(|e| Error::Network(e.without_url().to_string()))?;
        interpret(status, challenged, &body)
    }

    /// The signed-in user's name, or `None` if these credentials aren't (or are no longer) a
    /// signed-in session.
    pub async fn user_status(&self, creds: &Credentials) -> Result<Option<String>> {
        let body = json!({ "query": "query { userStatus { isSignedIn username } }" });
        let request = self.http.post(format!("{}/graphql", self.base)).headers(self.headers(creds, None)?).json(&body);
        let v = match self.send(request).await {
            Ok(v) => v,
            Err(Error::SessionExpired) => return Ok(None),
            Err(e) => return Err(e),
        };
        let status = &v["data"]["userStatus"];
        Ok(match (status["isSignedIn"].as_bool(), status["username"].as_str()) {
            (Some(true), Some(name)) if !name.is_empty() => Some(name.to_owned()),
            _ => None,
        })
    }

    /// Runs code against `data_input` (one line per argument, cases one after another) without
    /// submitting it. Returns LeetCode's final response.
    pub async fn run(
        &self,
        creds: &Credentials,
        slug: &str,
        question_id: i64,
        lang: &str,
        code: &str,
        data_input: &str,
    ) -> Result<Value> {
        let body = json!({
            "lang": lang, "question_id": question_id.to_string(), "typed_code": code, "data_input": data_input,
        });
        let request = self
            .http
            .post(format!("{}/problems/{slug}/interpret_solution/", self.base))
            .headers(self.headers(creds, Some(slug))?)
            .json(&body);
        let started = self.send(request).await?;
        let id = started["interpret_id"]
            .as_str()
            .ok_or_else(|| Error::Unexpected("the run didn't return an id".into()))?
            .to_owned();
        self.wait_for_verdict(creds, slug, &id).await
    }

    /// Submits code for judging against all of LeetCode's tests. Returns LeetCode's id for the
    /// submission and its final response.
    pub async fn submit(
        &self,
        creds: &Credentials,
        slug: &str,
        question_id: i64,
        lang: &str,
        code: &str,
    ) -> Result<(String, Value)> {
        let body = json!({ "lang": lang, "question_id": question_id.to_string(), "typed_code": code });
        let request = self
            .http
            .post(format!("{}/problems/{slug}/submit/", self.base))
            .headers(self.headers(creds, Some(slug))?)
            .json(&body);
        let started = self.send(request).await?;
        let id = match &started["submission_id"] {
            Value::Number(n) => n.to_string(),
            Value::String(s) if !s.is_empty() => s.clone(),
            _ => return Err(Error::Unexpected("the submission didn't return an id".into())),
        };
        let result = self.wait_for_verdict(creds, slug, &id).await?;
        Ok((id, result))
    }

    async fn wait_for_verdict(&self, creds: &Credentials, slug: &str, id: &str) -> Result<Value> {
        for _ in 0..self.max_polls {
            tokio::time::sleep(self.poll_interval).await;
            let request = self
                .http
                .get(format!("{}/submissions/detail/{id}/check/", self.base))
                .headers(self.headers(creds, Some(slug))?);
            let v = self.send(request).await?;
            // PENDING and STARTED mean "keep waiting"; SUCCESS is a finished verdict (a wrong
            // answer is still SUCCESS: the *judging* succeeded).
            match v["state"].as_str() {
                Some("SUCCESS") => return Ok(v),
                Some("PENDING" | "STARTED") | None => {}
                Some(other) => return Err(Error::Unexpected(format!("judging ended in state {other}"))),
            }
        }
        Err(Error::Other(
            "LeetCode is taking too long to judge this. Check leetcode.com for the result.".into(),
        ))
    }
}

/// Maps an HTTP response to the JSON body or the right [`Error`]. Pure, so it is unit tested
/// without a network.
pub(crate) fn interpret(status: StatusCode, bot_challenge: bool, body: &str) -> Result<Value> {
    let looks_like_challenge = body.contains("Just a moment") || body.contains("cf-challenge");
    if bot_challenge || (!status.is_success() && looks_like_challenge) {
        return Err(Error::Blocked);
    }
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    match status {
        s if s.is_success() => parsed.ok_or_else(|| Error::Unexpected("the response wasn't JSON".into())),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(Error::SessionExpired),
        StatusCode::TOO_MANY_REQUESTS => {
            let detail = parsed.as_ref().and_then(|v| v["detail"].as_str()).unwrap_or("Wait a moment and try again.");
            Err(Error::RateLimited(detail.to_owned()))
        }
        s => {
            let snippet: String = body.chars().take(200).collect();
            Err(Error::Unexpected(format!("HTTP {}: {}", s.as_u16(), snippet.trim())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_returns_the_json() {
        let v = interpret(StatusCode::OK, false, r#"{"submission_id": 7}"#).unwrap();
        assert_eq!(v["submission_id"], 7);
        assert!(matches!(interpret(StatusCode::OK, false, "<html>"), Err(Error::Unexpected(_))));
    }

    #[test]
    fn auth_failures_mean_the_session_is_gone() {
        for code in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
            let body = r#"{"detail":"Authentication credentials were not provided."}"#;
            assert!(matches!(interpret(code, false, body), Err(Error::SessionExpired)));
        }
    }

    #[test]
    fn a_cloudflare_challenge_is_blocked_not_expired() {
        assert!(matches!(interpret(StatusCode::FORBIDDEN, true, ""), Err(Error::Blocked)));
        let page = "<html><title>Just a moment...</title></html>";
        assert!(matches!(interpret(StatusCode::FORBIDDEN, false, page), Err(Error::Blocked)));
        assert!(matches!(interpret(StatusCode::SERVICE_UNAVAILABLE, false, page), Err(Error::Blocked)));
    }

    #[test]
    fn throttling_passes_leetcodes_explanation_along() {
        let body = r#"{"detail":"Request was throttled. Expected available in 21 seconds."}"#;
        match interpret(StatusCode::TOO_MANY_REQUESTS, false, body) {
            Err(Error::RateLimited(d)) => assert!(d.contains("21 seconds")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(interpret(StatusCode::TOO_MANY_REQUESTS, false, ""), Err(Error::RateLimited(_))));
    }

    #[test]
    fn other_failures_show_the_status_and_start_of_the_body() {
        match interpret(StatusCode::INTERNAL_SERVER_ERROR, false, "  kaboom ") {
            Err(Error::Unexpected(m)) => assert_eq!(m, "HTTP 500: kaboom"),
            other => panic!("{other:?}"),
        }
    }
}
