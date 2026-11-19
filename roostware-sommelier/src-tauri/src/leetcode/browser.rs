//! Running and submitting from inside a real browser window, for when Cloudflare won't take the
//! app's own requests.
//!
//! Run and Submit sometimes get a Cloudflare "managed challenge": a page of JavaScript that only a
//! browser can answer, which the plain HTTP client in `client` can't. So when that happens the
//! app falls back to this: a hidden window on leetcode.com (a webview, i.e. a real browser engine)
//! loads the problem's page, passes Cloudflare's check the way any visitor does, and makes the
//! submit and polling requests with `fetch` from there. If Cloudflare needs a person to click
//! something, the window is shown until they have.
//!
//! The window is private like the sign-in window: nothing it stores is kept. It gets the login
//! by having the stored cookies put into it each time, so the only copy of the login on disk is
//! still the OS credential store.
//!
//! leetcode.com's page can't call into the app (it's a remote page with no permissions), so the
//! result comes back the other way: the page's script leaves it in `window.__rw` and Rust asks for
//! it until it's there.

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{webview::Cookie, AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use super::{
    auth::Credentials,
    client::{interpret, judged, submission_id, too_slow, Client, BROWSER_USER_AGENT},
    error::{Error, Result},
};

const WINDOW: &str = "leetcode-browser";
/// How long to wait for the problem page (and any Cloudflare check on it) to finish loading.
const PAGE_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a check may take before a person is asked to help with it.
const SHOW_AFTER: Duration = Duration::from_secs(5);
/// How long one request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_millis(300);

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

/// Submits `code` to LeetCode from the browser window and waits for the verdict, like
/// `Client::submit`.
pub async fn submit<R: Runtime>(
    app: &AppHandle<R>,
    client: &Client,
    creds: &Credentials,
    slug: &str,
    question_id: i64,
    lang: &str,
    code: &str,
) -> Result<(String, Value)> {
    let body = json!({ "lang": lang, "question_id": question_id.to_string(), "typed_code": code });
    let (window, started) = start(app, client, creds, slug, &format!("/problems/{slug}/submit/"), &body).await?;
    let id = submission_id(&started)?;
    let verdict = poll(&window, client, creds, &id).await?;
    Ok((id, verdict))
}

/// Runs `code` against `data_input` on LeetCode from the browser window and waits for the
/// result, like `Client::run`.
pub async fn run<R: Runtime>(
    app: &AppHandle<R>,
    client: &Client,
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
    let (window, started) = start(app, client, creds, slug, &format!("/problems/{slug}/interpret_solution/"), &body).await?;
    let id = started["interpret_id"]
        .as_str()
        .ok_or_else(|| Error::Unexpected("the run didn't return an id".into()))?
        .to_owned();
    poll(&window, client, creds, &id).await
}

/// Opens the problem's page in the browser window and POSTs `body` to `path` from there.
async fn start<R: Runtime>(
    app: &AppHandle<R>,
    client: &Client,
    creds: &Credentials,
    slug: &str,
    path: &str,
    body: &Value,
) -> Result<(WebviewWindow<R>, Value)> {
    let site: Url = client.base().parse().map_err(|e| Error::Other(format!("bad LeetCode address: {e}")))?;
    let window = window(app, &site, creds)?;
    open_problem(&window, &site, slug).await?;
    let started = fetch(&window, "POST", path, Some(body), creds).await?;
    Ok((window, started))
}

/// Asks for the verdict on `id` until LeetCode has one.
async fn poll<R: Runtime>(window: &WebviewWindow<R>, client: &Client, creds: &Credentials, id: &str) -> Result<Value> {
    let (interval, max_polls) = client.polling();
    for _ in 0..max_polls {
        tokio::time::sleep(interval).await;
        let v = fetch(window, "GET", &format!("/submissions/detail/{id}/check/"), None, creds).await?;
        if judged(&v)? {
            return Ok(v);
        }
    }
    Err(too_slow())
}

// ---- The window -----------------------------------------------------------------------------

/// The cookies the page needs to count as the signed-in user.
fn cookies(site: &Url, creds: &Credentials) -> Vec<Cookie<'static>> {
    let host = site.host_str().unwrap_or("leetcode.com").to_owned();
    let make = |name: &str, value: &str, http_only: bool| {
        Cookie::build((name.to_owned(), value.to_owned()))
            .domain(host.clone())
            .path("/")
            .secure(site.scheme() == "https")
            .http_only(http_only)
            .build()
    };
    vec![make("LEETCODE_SESSION", &creds.session, true), make("csrftoken", &creds.csrf, false)]
}

// Kept out of async code so the (non-Send) window builder is gone before any `.await`.
fn window<R: Runtime>(app: &AppHandle<R>, site: &Url, creds: &Credentials) -> Result<WebviewWindow<R>> {
    let window = match app.get_webview_window(WINDOW) {
        Some(w) => w,
        None => {
            let blank: Url = "about:blank".parse().expect("a valid address");
            let mut builder = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::External(blank))
                .title("LeetCode needs a check")
                .inner_size(520.0, 700.0)
                .center()
                .visible(false)
                .user_agent(BROWSER_USER_AGENT)
                .incognito(true);
            if let Some(main) = app.get_webview_window("main") {
                builder = builder.parent(&main).map_err(|e| Error::Other(e.to_string()))?;
            }
            builder.build().map_err(|e| Error::Other(format!("couldn't open the browser window: {e}")))?
        }
    };
    // Every time: the login may have changed since the window was made.
    for cookie in cookies(site, creds) {
        window.set_cookie(cookie).map_err(|e| Error::Other(format!("couldn't give the browser window the login: {e}")))?;
    }
    Ok(window)
}

#[derive(Deserialize)]
struct PageState {
    href: String,
    ready: bool,
    title: String,
}

/// Loads the problem's page (unless it's already there) and waits until it's usable, showing
/// the window if Cloudflare needs a person.
async fn open_problem<R: Runtime>(window: &WebviewWindow<R>, site: &Url, slug: &str) -> Result<()> {
    let prefix = format!("{}/problems/{slug}", site.as_str().trim_end_matches('/'));
    let current = page_state(window).await.ok();
    if !current.is_some_and(|s| s.href.starts_with(&prefix)) {
        let url: Url = format!("{prefix}/").parse().map_err(|e| Error::Other(format!("bad problem address: {e}")))?;
        window.navigate(url).map_err(|e| Error::Other(e.to_string()))?;
    }

    let started = Instant::now();
    let mut shown = false;
    let outcome = loop {
        tokio::time::sleep(TICK).await;
        let Ok(state) = page_state(window).await else { continue };
        if state.href.contains("/accounts/login") {
            break Err(Error::SessionExpired);
        }
        let challenged = state.title.contains("Just a moment");
        if state.href.starts_with(&prefix) && state.ready && !challenged {
            break Ok(());
        }
        if challenged && !shown && started.elapsed() > SHOW_AFTER {
            let _ = window.show();
            let _ = window.set_focus();
            shown = true;
        }
        if started.elapsed() > PAGE_TIMEOUT {
            break Err(Error::Blocked("the check in the browser window wasn't completed".into()));
        }
    };
    if shown {
        let _ = window.hide();
    }
    outcome
}

async fn page_state<R: Runtime>(window: &WebviewWindow<R>) -> Result<PageState> {
    let js = "JSON.stringify({ href: location.href, ready: document.readyState === 'complete', title: document.title })";
    let text = eval_string(window, js).await?;
    serde_json::from_str(&text).map_err(|e| Error::Unexpected(format!("the browser window answered oddly: {e}")))
}

// ---- Talking to the page --------------------------------------------------------------------

/// Evaluates JavaScript that gives a string and returns that string.
async fn eval_string<R: Runtime>(window: &WebviewWindow<R>, js: &str) -> Result<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Mutex::new(Some(tx));
    window
        .eval_with_callback(js, move |answer| {
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(answer);
            }
        })
        .map_err(|e| Error::Other(e.to_string()))?;
    let answer = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .map_err(|_| Error::Other("the browser window didn't answer".into()))?
        .map_err(|_| Error::Other("the browser window closed".into()))?;
    // The answer comes back JSON-encoded, so a string is wrapped in quotes.
    serde_json::from_str::<String>(&answer).map_err(|_| Error::Unexpected("the browser window gave no text".into()))
}

/// The script that makes one request from the page and leaves what happened in `window.__rw`.
/// Everything that varies goes in as a JSON literal, so nothing in a path or the code can break
/// out of it.
fn request_script(id: u64, method: &str, path: &str, body: Option<&Value>, csrf: &str) -> String {
    let literal = |v: &Value| serde_json::to_string(v).expect("a JSON value always serializes");
    let body = match body {
        Some(b) => literal(&Value::String(b.to_string())),
        None => "undefined".to_owned(),
    };
    r#"(async () => {
  const id = __ID__;
  try {
    const r = await fetch(__PATH__, {
      method: __METHOD__,
      credentials: 'include',
      headers: { 'content-type': 'application/json', 'x-csrftoken': __CSRF__ },
      body: __BODY__,
    });
    const text = await r.text();
    window.__rw = { id, ok: true, status: r.status, challenged: r.headers.has('cf-mitigated'), body: text };
  } catch (e) {
    window.__rw = { id, ok: false, error: String(e) };
  }
})();"#
        .replace("__ID__", &id.to_string())
        .replace("__PATH__", &literal(&json!(path)))
        .replace("__METHOD__", &literal(&json!(method)))
        .replace("__CSRF__", &literal(&json!(csrf)))
        .replace("__BODY__", &body)
}

#[derive(Deserialize)]
struct Outcome {
    id: u64,
    ok: bool,
    status: Option<u16>,
    challenged: Option<bool>,
    body: Option<String>,
    error: Option<String>,
}

/// Makes a request from the page and returns the parsed response, with the same error handling
/// as the app's own client.
async fn fetch<R: Runtime>(
    window: &WebviewWindow<R>,
    method: &str,
    path: &str,
    body: Option<&Value>,
    creds: &Credentials,
) -> Result<Value> {
    let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    window.eval(request_script(id, method, path, body, &creds.csrf)).map_err(|e| Error::Other(e.to_string()))?;

    let started = Instant::now();
    loop {
        tokio::time::sleep(TICK).await;
        let text = eval_string(window, "JSON.stringify(window.__rw || null)").await?;
        let Some(outcome) = serde_json::from_str::<Option<Outcome>>(&text).ok().flatten().filter(|o| o.id == id)
        else {
            if started.elapsed() > REQUEST_TIMEOUT {
                return Err(Error::Network("the browser window got no answer from LeetCode".into()));
            }
            continue;
        };
        if !outcome.ok {
            return Err(Error::Network(outcome.error.unwrap_or_default()));
        }
        let status = StatusCode::from_u16(outcome.status.unwrap_or(0))
            .map_err(|_| Error::Unexpected("the browser window reported a bad status".into()))?;
        return interpret(status, outcome.challenged.unwrap_or(false), &outcome.body.unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_carries_values_as_literals_that_cannot_break_out() {
        let nasty = "x\"); alert(1); (\"</script>\n'\\";
        let body = json!({ "typed_code": nasty });
        let js = request_script(7, "POST", "/problems/two-sum/submit/", Some(&body), "tok\"en");
        assert!(js.contains("const id = 7;"));
        assert!(js.contains(r#"fetch("/problems/two-sum/submit/""#));
        assert!(js.contains(r#"method: "POST""#));
        assert!(js.contains(r#""x-csrftoken": "tok\"en""#) || js.contains(r#"'x-csrftoken': "tok\"en""#));
        // The code only ever appears inside a string literal: the raw text isn't there.
        assert!(!js.contains("alert(1); (\"</script>"), "{js}");
        for placeholder in ["__ID__", "__PATH__", "__METHOD__", "__CSRF__", "__BODY__"] {
            assert!(!js.contains(placeholder), "{placeholder} was left unfilled");
        }
    }

    #[test]
    fn a_request_without_a_body_sends_none() {
        let js = request_script(1, "GET", "/submissions/detail/5/check/", None, "c");
        assert!(js.contains("body: undefined"));
    }

    #[test]
    fn the_window_gets_both_login_cookies() {
        let site: Url = "https://leetcode.com".parse().unwrap();
        let creds = Credentials { session: "s".into(), csrf: "c".into() };
        let all = cookies(&site, &creds);
        let names: Vec<_> = all.iter().map(|c| c.name().to_owned()).collect();
        assert_eq!(names, ["LEETCODE_SESSION", "csrftoken"]);
        assert!(all.iter().all(|c| c.domain() == Some("leetcode.com") && c.path() == Some("/") && c.secure() == Some(true)));
    }
}
