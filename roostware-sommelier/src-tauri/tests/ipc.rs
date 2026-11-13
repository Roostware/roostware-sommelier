//! Drives the real command handlers through Tauri's IPC path (mock runtime), so
//! argument names, `State` injection and JSON shapes are tested exactly as the
//! webview's `invoke()` will hit them. Query semantics are covered in
//! `db::problems`; this only proves the wiring and the wire format.

use roostware_sommelier_lib::{
    db::Db,
    leetcode::{
        auth::{CredentialStore, Credentials, MemoryStore, Stored},
        client::Client,
        LeetCode,
    },
    with_commands,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use tauri::{
    ipc::{CallbackFn, InvokeBody},
    test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY},
    webview::InvokeRequest,
    WebviewWindow,
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tempfile::TempDir;

/// Built from the real schema file shared with scripts/build-catalog.mjs.
const CATALOG_SCHEMA: &str = include_str!("../../scripts/catalog-schema.sql");

fn build_catalog(path: &std::path::Path) {
    let c = Connection::open(path).unwrap();
    c.execute_batch(CATALOG_SCHEMA).unwrap();
    c.execute("INSERT INTO meta VALUES ('schema_version', '1')", []).unwrap();
    // (id, frontend_id, title, slug, difficulty, tag, example test cases)
    for (id, fe, title, slug, diff, tag, examples) in [
        (1, 1, "Two Sum", "two-sum", "Easy", "Array", "[2,7,11,15]\n9\n[3,2,4]\n6"),
        (900, 146, "LRU Cache", "lru-cache", "Medium", "Design", ""),
    ] {
        c.execute(
            "INSERT INTO problems VALUES (?1, ?2, ?3, ?4, ?5, '<p>x</p>', ?6, 0, 7, 1, 10, 20, 0.5, NULL)",
            rusqlite::params![id, fe, title, slug, diff, examples],
        )
        .unwrap();
        c.execute("INSERT INTO tags (name) VALUES (?1)", [tag]).unwrap();
        c.execute(
            "INSERT INTO problem_tags SELECT ?1, id FROM tags WHERE name = ?2",
            rusqlite::params![id, tag],
        )
        .unwrap();
        c.execute("INSERT INTO code_snippets VALUES (?1, 'python3', '# start')", [id]).unwrap();
        c.execute(
            "INSERT INTO problems_fts (rowid, title, frontend_id, tags) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, title, fe.to_string(), tag],
        )
        .unwrap();
    }
}

struct Harness {
    _dir: TempDir,
    _app: tauri::App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    /// The LeetCode login the app sees; tests sign in or out by touching it.
    login: Arc<MemoryStore>,
}

/// Lets the test keep a handle on the store the app owns.
struct SharedStore(Arc<MemoryStore>);

impl CredentialStore for SharedStore {
    fn load(&self) -> roostware_sommelier_lib::leetcode::Result<Option<Stored>> {
        self.0.load()
    }
    fn save(&self, stored: &Stored) -> roostware_sommelier_lib::leetcode::Result<()> {
        self.0.save(stored)
    }
    fn clear(&self) -> roostware_sommelier_lib::leetcode::Result<()> {
        self.0.clear()
    }
}

fn harness() -> Harness {
    // Nothing here talks to LeetCode, so point at a port nobody listens on.
    harness_against("http://127.0.0.1:9")
}

fn harness_against(leetcode_base: &str) -> Harness {
    let dir = TempDir::new().unwrap();
    let catalog = dir.path().join("catalog.db");
    build_catalog(&catalog);
    let db = Db::open_at(&dir.path().join("user.db"), &catalog).unwrap();
    let login = Arc::new(MemoryStore::default());
    let client = Client::new(leetcode_base).with_polling(Duration::from_millis(5), 40);
    let leetcode = LeetCode::new(Box::new(SharedStore(login.clone())), client);
    let app = with_commands(mock_builder())
        .manage(db)
        .manage(leetcode)
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    Harness { _dir: dir, _app: app, window, login }
}

fn sign_in(h: &Harness) {
    h.login
        .save(&Stored {
            credentials: Credentials { session: "sess-token".into(), csrf: "csrf-token".into() },
            username: "ada".into(),
        })
        .unwrap();
}

// ---- A stand-in for leetcode.com ------------------------------------------------------------

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}

struct FakeLeetCode {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl FakeLeetCode {
    fn requests(&self, method: &str, path: &str) -> Vec<Seen> {
        self.seen.lock().unwrap().iter().filter(|r| r.method == method && r.path == path).cloned().collect()
    }
}

type Handler = dyn Fn(&Seen) -> (u16, String) + Send + Sync;

/// Serves HTTP on a local port, answering every request with whatever `handler` returns
/// (status, JSON body). Requests are recorded for the test to inspect.
fn fake_leetcode(handler: impl Fn(&Seen) -> (u16, String) + Send + Sync + 'static) -> FakeLeetCode {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (log, handler) = (seen.clone(), Arc::new(handler));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (log, handler) = (log.clone(), handler.clone());
            std::thread::spawn(move || serve_one(stream, &log, &*handler));
        }
    });
    FakeLeetCode { base, seen }
}

fn serve_one(mut stream: std::net::TcpStream, log: &Mutex<Vec<Seen>>, handler: &Handler) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap().split(' ');
    let (method, path) = (first.next().unwrap().to_owned(), first.next().unwrap().to_owned());
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    while buf.len() < header_end + length {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = serde_json::from_slice(&buf[header_end..]).unwrap_or(Value::Null);
    let seen = Seen { method, path, headers, body };
    log.lock().unwrap().push(seen.clone());

    let (status, body) = handler(&seen);
    let response = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

/// Judges every submission as `verdict`, after one "still pending" answer like the real thing.
fn judging(verdict: Value) -> impl Fn(&Seen) -> (u16, String) + Send + Sync + 'static {
    let polls = AtomicUsize::new(0);
    move |r: &Seen| match (r.method.as_str(), r.path.as_str()) {
        ("POST", "/problems/two-sum/submit/") => (200, json!({ "submission_id": 2194979879u64 }).to_string()),
        ("POST", "/problems/two-sum/interpret_solution/") => (200, json!({ "interpret_id": "runcode_1" }).to_string()),
        ("GET", p) if p.starts_with("/submissions/detail/") && p.ends_with("/check/") => {
            if polls.fetch_add(1, Ordering::SeqCst) % 2 == 0 {
                (200, json!({ "state": "PENDING" }).to_string())
            } else {
                (200, verdict.to_string())
            }
        }
        _ => (404, "{}".into()),
    }
}

fn invoke(h: &Harness, cmd: &str, body: Value) -> Result<Value, Value> {
    get_ipc_response(
        &h.window,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|b| b.deserialize::<Value>().unwrap())
}

#[test]
fn list_problems_over_ipc() {
    let h = harness();
    let res = invoke(&h, "list_problems", json!({
        "query": {
            "filter": { "difficulties": ["Medium"], "status": "all", "paid": "free" },
            "sort": { "key": "title", "dir": "desc" },
            "limit": 10, "offset": 0
        }
    }))
    .unwrap();
    assert_eq!(res["total"], 1);
    assert_eq!(res["items"][0]["title"], "LRU Cache");
    assert_eq!(res["items"][0]["frontendId"], 146);
    assert_eq!(res["items"][0]["progress"], "untouched");
    assert_eq!(res["items"][0]["tags"], json!(["Design"]));

    // Omitting everything is valid: the defaults list all problems.
    assert_eq!(invoke(&h, "list_problems", json!({ "query": {} })).unwrap()["total"], 2);
}

#[test]
fn get_problem_over_ipc() {
    let h = harness();
    let p = invoke(&h, "get_problem", json!({ "id": 900 })).unwrap();
    assert_eq!(p["titleSlug"], "lru-cache");
    assert_eq!(p["snippets"], json!([{ "lang": "python3", "code": "# start" }]));
    assert_eq!(p["state"]["starred"], false);

    let by_slug = invoke(&h, "get_problem_by_slug", json!({ "slug": "two-sum" })).unwrap();
    assert_eq!(by_slug["id"], 1);

    assert_eq!(invoke(&h, "get_problem", json!({ "id": 424242 })).unwrap(), Value::Null);
}

#[test]
fn list_tags_over_ipc() {
    let h = harness();
    let tags = invoke(&h, "list_tags", json!({})).unwrap();
    assert_eq!(tags, json!([{ "name": "Array", "count": 1 }, { "name": "Design", "count": 1 }]));
}

#[test]
fn malformed_arguments_are_rejected_not_executed() {
    let h = harness();
    let bad_sort = invoke(&h, "list_problems", json!({ "query": { "sort": { "key": "id; DROP TABLE problems" } } }));
    assert!(bad_sort.is_err(), "unknown sort key must be rejected");
    assert!(invoke(&h, "get_problem", json!({})).is_err(), "missing id must be rejected");
}

#[test]
fn list_collections_over_ipc() {
    let h = harness();
    assert_eq!(invoke(&h, "list_collections", json!({})).unwrap(), json!([]));
}

#[test]
fn recent_problems_round_trip_over_ipc() {
    let h = harness();
    assert_eq!(invoke(&h, "list_recent_problems", json!({})).unwrap(), json!([]), "empty for a new user");

    assert_eq!(invoke(&h, "record_problem_opened", json!({ "id": 900 })).unwrap(), Value::Null);
    assert_eq!(invoke(&h, "record_problem_opened", json!({ "id": 1 })).unwrap(), Value::Null);

    let recent = invoke(&h, "list_recent_problems", json!({ "limit": 5 })).unwrap();
    let ids: Vec<_> = recent.as_array().unwrap().iter().map(|p| p["id"].as_i64().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&900) && ids.contains(&1));
    assert!(recent[0]["lastOpenedAt"].as_i64().unwrap() > 0);
    assert!(recent[0]["frontendId"].is_i64(), "full summary fields are flattened in");
    assert_eq!(invoke(&h, "list_recent_problems", json!({ "limit": 1 })).unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn opening_an_unknown_problem_reports_a_readable_error() {
    let h = harness();
    let err = invoke(&h, "record_problem_opened", json!({ "id": 424242 })).unwrap_err();
    assert_eq!(err, json!("problem 424242 not found"));
    assert_eq!(invoke(&h, "list_recent_problems", json!({})).unwrap(), json!([]), "nothing was recorded");
}

#[test]
fn drafts_round_trip_over_ipc() {
    let h = harness();
    // Nothing written yet.
    assert_eq!(invoke(&h, "get_problem", json!({ "id": 1 })).unwrap()["drafts"], json!([]));

    // Changed code is stored, and the problem is now in progress.
    let kept = invoke(&h, "save_draft", json!({ "problemId": 1, "lang": "python3", "code": "my work" })).unwrap();
    assert_eq!(kept, json!(true));
    let p = invoke(&h, "get_problem", json!({ "id": 1 })).unwrap();
    assert_eq!(p["progress"], "inProgress");
    assert_eq!(p["drafts"][0]["lang"], "python3");
    assert_eq!(p["drafts"][0]["code"], "my work");
    let listed = invoke(&h, "list_problems", json!({ "query": {} })).unwrap();
    let in_list = listed["items"].as_array().unwrap().iter().find(|i| i["id"] == 1).unwrap();
    assert_eq!(in_list["progress"], "inProgress");

    // Code equal to the starter ("# start") isn't a draft, and clears the existing one.
    let kept = invoke(&h, "save_draft", json!({ "problemId": 1, "lang": "python3", "code": "# start" })).unwrap();
    assert_eq!(kept, json!(false));
    assert_eq!(invoke(&h, "get_problem", json!({ "id": 1 })).unwrap()["progress"], "untouched");

    // Explicit delete.
    invoke(&h, "save_draft", json!({ "problemId": 900, "lang": "python3", "code": "x" })).unwrap();
    assert_eq!(invoke(&h, "delete_draft", json!({ "problemId": 900, "lang": "python3" })).unwrap(), Value::Null);
    assert_eq!(invoke(&h, "get_problem", json!({ "id": 900 })).unwrap()["drafts"], json!([]));
}

#[test]
fn bad_draft_requests_report_readable_errors() {
    let h = harness();
    let err = invoke(&h, "save_draft", json!({ "problemId": 1, "lang": "cobol", "code": "x" })).unwrap_err();
    assert_eq!(err, json!("cobol starter code for problem 1 not found"));
    assert!(invoke(&h, "save_draft", json!({ "problemId": 1, "lang": "python3" })).is_err(), "code is required");
    assert_eq!(invoke(&h, "get_problem", json!({ "id": 1 })).unwrap()["drafts"], json!([]), "nothing was written");
}

// ---- LeetCode: sign-in state, Run and Submit -----------------------------------------------

fn accepted() -> Value {
    json!({
        "state": "SUCCESS", "status_msg": "Accepted", "status_code": 10, "run_success": true,
        "status_runtime": "1 ms", "memory": 20844000, "status_memory": "20.4 MB",
        "runtime_percentile": 64.4589, "memory_percentile": 51.6892,
        "total_correct": 66, "total_testcases": 66, "last_testcase": "", "expected_output": "", "code_output": ""
    })
}

fn wrong_answer() -> Value {
    json!({
        "state": "SUCCESS", "status_msg": "Wrong Answer", "status_code": 11, "run_success": true,
        "status_runtime": "N/A", "status_memory": "N/A", "total_correct": 0, "total_testcases": 66,
        "last_testcase": "[2,7,11,15]\n9", "expected_output": "[0,1]", "code_output": "[0,0]"
    })
}

fn submit(h: &Harness, code: &str) -> Result<Value, Value> {
    invoke(h, "submit_code", json!({ "problemId": 1, "lang": "python3", "code": code }))
}

#[test]
fn sign_in_state_over_ipc() {
    let h = harness();
    assert_eq!(invoke(&h, "leetcode_status", json!({})).unwrap(), json!({ "signedIn": false, "username": null }));

    sign_in(&h);
    assert_eq!(invoke(&h, "leetcode_status", json!({})).unwrap(), json!({ "signedIn": true, "username": "ada" }));
    let status = invoke(&h, "leetcode_status", json!({})).unwrap().to_string();
    assert!(!status.contains("sess-token") && !status.contains("csrf-token"), "secrets must never reach the frontend");

    assert_eq!(invoke(&h, "leetcode_sign_out", json!({})).unwrap(), json!({ "signedIn": false, "username": null }));
    assert_eq!(h.login.load().unwrap(), None, "the stored login is gone");
    invoke(&h, "leetcode_sign_out", json!({})).unwrap(); // signing out twice is fine
}

#[test]
fn run_and_submit_need_a_login_and_send_nothing_without_one() {
    let fake = fake_leetcode(judging(accepted()));
    let h = harness_against(&fake.base);

    let run = invoke(&h, "run_code", json!({ "problemId": 1, "lang": "python3", "code": "x" })).unwrap_err();
    assert_eq!(run["kind"], "notSignedIn");
    assert_eq!(run["message"], "Sign in to LeetCode first.");
    assert_eq!(submit(&h, "x").unwrap_err()["kind"], "notSignedIn");

    assert!(fake.seen.lock().unwrap().is_empty(), "nothing reached LeetCode");
}

#[test]
fn run_uses_the_example_cases_and_sends_the_login() {
    let run_verdict = json!({
        "state": "SUCCESS", "status_msg": "Accepted", "run_success": true, "compare_result": "10",
        "code_answer": ["[0,1]", "[0,0]"], "expected_code_answer": ["[0,1]", "[1,2]"],
        "correct_answer": false, "status_runtime": "0 ms", "status_memory": "19 MB"
    });
    let fake = fake_leetcode(judging(run_verdict));
    let h = harness_against(&fake.base);
    sign_in(&h);

    let result = invoke(&h, "run_code", json!({ "problemId": 1, "lang": "python3", "code": "a\r\nb" })).unwrap();
    assert_eq!(result["status"], "Wrong Answer", "LeetCode calls any run that finishes Accepted; we look at the answers");
    assert_eq!(result["correct"], false);
    assert_eq!(result["cases"].as_array().unwrap().len(), 2);
    assert_eq!(result["cases"][1]["input"], "[3,2,4]\n6");
    assert_eq!(result["cases"][1]["expected"], "[1,2]");
    assert_eq!(result["cases"][1]["output"], "[0,0]");
    assert_eq!(result["cases"][0]["passed"], true);
    assert_eq!(result["runtimeMs"], 0);
    assert_eq!(result["lastInput"], Value::Null, "a run that finished has per-case results instead");
    assert!(result.get("input").is_none(), "the whole sent input is no longer echoed back");

    let sent = &fake.requests("POST", "/problems/two-sum/interpret_solution/")[0];
    assert_eq!(sent.body["lang"], "python3");
    assert_eq!(sent.body["question_id"], "1");
    assert_eq!(sent.body["typed_code"], "a\nb", "Windows line endings are normalised");
    assert_eq!(sent.body["data_input"], "[2,7,11,15]\n9\n[3,2,4]\n6", "the problem's own examples are the default input");
    assert_eq!(sent.headers["cookie"], "LEETCODE_SESSION=sess-token; csrftoken=csrf-token");
    assert_eq!(sent.headers["x-csrftoken"], "csrf-token");
    assert!(sent.headers["referer"].ends_with("/problems/two-sum/"));
    assert!(sent.headers["user-agent"].starts_with("Mozilla/5.0"));
    assert_eq!(fake.requests("GET", "/submissions/detail/runcode_1/check/").len(), 2, "polled until the verdict arrived");

    // A Run is not a submission: nothing is recorded and progress doesn't move.
    assert_eq!(invoke(&h, "list_submissions", json!({ "problemId": 1 })).unwrap(), json!([]));
    assert_eq!(invoke(&h, "get_problem", json!({ "id": 1 })).unwrap()["progress"], "untouched");
}

#[test]
fn run_can_take_custom_input_and_refuses_when_there_is_none() {
    let fake = fake_leetcode(judging(json!({ "state": "SUCCESS", "status_msg": "Accepted", "run_success": true })));
    let h = harness_against(&fake.base);
    sign_in(&h);

    invoke(&h, "run_code", json!({ "problemId": 1, "lang": "python3", "code": "x", "input": "[1,2]\n3" })).unwrap();
    assert_eq!(fake.requests("POST", "/problems/two-sum/interpret_solution/")[0].body["data_input"], "[1,2]\n3");

    // LRU Cache has no example cases in the fixture catalog, and the user gave none.
    let err = invoke(&h, "run_code", json!({ "problemId": 900, "lang": "python3", "code": "x" })).unwrap_err();
    assert_eq!(err["message"], "This problem has no example test cases to run.");
}

#[test]
fn an_accepted_submission_is_recorded_and_settles_the_draft() {
    let fake = fake_leetcode(judging(accepted()));
    let h = harness_against(&fake.base);
    sign_in(&h);
    invoke(&h, "save_draft", json!({ "problemId": 1, "lang": "python3", "code": "my code" })).unwrap();
    assert_eq!(invoke(&h, "get_problem", json!({ "id": 1 })).unwrap()["progress"], "inProgress");

    let s = submit(&h, "my code").unwrap();
    assert_eq!(s["status"], "Accepted");
    assert_eq!(s["problemId"], 1);
    assert_eq!(s["lang"], "python3");
    assert_eq!(s["code"], "my code");
    assert_eq!(s["leetcodeSubmissionId"], "2194979879");
    assert_eq!((s["runtimeMs"].as_i64(), s["memoryKb"].as_i64()), (Some(1), Some(20844000 / 1024)));
    assert_eq!(s["runtimePercentile"], 64.4589);
    assert_eq!((s["totalCorrect"].as_i64(), s["totalTestcases"].as_i64()), (Some(66), Some(66)));
    assert!(s["submittedAt"].as_i64().unwrap() > 0);
    assert!(s.get("resultJson").is_none());

    let sent = &fake.requests("POST", "/problems/two-sum/submit/")[0];
    assert_eq!(sent.body, json!({ "lang": "python3", "question_id": "1", "typed_code": "my code" }));

    let p = invoke(&h, "get_problem", json!({ "id": 1 })).unwrap();
    assert_eq!(p["progress"], "solved");
    assert_eq!(p["solveCount"], 1);
    assert_eq!(p["drafts"], json!([]), "submitted code is no longer unsubmitted");
    assert_eq!(p["lastSubmitted"][0]["lang"], "python3");
    assert_eq!(p["lastSubmitted"][0]["code"], "my code", "so the editor can show it again");

    let listed = invoke(&h, "list_submissions", json!({ "problemId": 1 })).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], s["id"]);
}

#[test]
fn a_wrong_answer_keeps_the_failing_case_and_further_typing() {
    let fake = fake_leetcode(judging(wrong_answer()));
    let h = harness_against(&fake.base);
    sign_in(&h);
    invoke(&h, "save_draft", json!({ "problemId": 1, "lang": "python3", "code": "my code, then more" })).unwrap();

    let s = submit(&h, "my code").unwrap();
    assert_eq!(s["status"], "Wrong Answer");
    assert_eq!(s["lastTestcase"], "[2,7,11,15]\n9");
    assert_eq!(s["expectedOutput"], "[0,1]");
    assert_eq!(s["codeOutput"], "[0,0]");
    assert_eq!((s["runtimeMs"].clone(), s["memoryKb"].clone()), (Value::Null, Value::Null));

    let p = invoke(&h, "get_problem", json!({ "id": 1 })).unwrap();
    assert_eq!(p["progress"], "attempted");
    assert_eq!(p["drafts"][0]["code"], "my code, then more", "the user kept typing after what was submitted");
}

#[test]
fn submitted_code_becomes_the_baseline_for_drafts() {
    let fake = fake_leetcode(judging(accepted()));
    let h = harness_against(&fake.base);
    sign_in(&h);
    submit(&h, "submitted").unwrap();

    // Back to exactly what was submitted: nothing unsubmitted, so no draft.
    let kept = invoke(&h, "save_draft", json!({ "problemId": 1, "lang": "python3", "code": "submitted" })).unwrap();
    assert_eq!(kept, json!(false));
    // The starter now counts as a change (the user chose to go back to it), so it is kept.
    let kept = invoke(&h, "save_draft", json!({ "problemId": 1, "lang": "python3", "code": "# start" })).unwrap();
    assert_eq!(kept, json!(true));
    assert_eq!(invoke(&h, "get_problem", json!({ "id": 1 })).unwrap()["drafts"][0]["code"], "# start");
}

#[test]
fn a_compile_error_is_a_recorded_attempt_with_its_message() {
    let verdict = json!({
        "state": "SUCCESS", "status_msg": "Compile Error", "status_code": 20, "run_success": false,
        "full_compile_error": "line 3: invalid syntax"
    });
    let fake = fake_leetcode(judging(verdict));
    let h = harness_against(&fake.base);
    sign_in(&h);

    let s = submit(&h, "def (").unwrap();
    assert_eq!(s["status"], "Compile Error");
    assert_eq!(s["errorMessage"], "line 3: invalid syntax");
    assert_eq!(invoke(&h, "get_problem", json!({ "id": 1 })).unwrap()["progress"], "attempted");
}

#[test]
fn an_expired_session_is_reported_and_nothing_is_recorded() {
    let fake = fake_leetcode(|_| (403, json!({ "detail": "Authentication credentials were not provided." }).to_string()));
    let h = harness_against(&fake.base);
    sign_in(&h);

    let err = submit(&h, "x").unwrap_err();
    assert_eq!(err["kind"], "sessionExpired");
    assert_eq!(err["message"], "Your LeetCode session has expired. Sign in again.");
    let err = invoke(&h, "run_code", json!({ "problemId": 1, "lang": "python3", "code": "x" })).unwrap_err();
    assert_eq!(err["kind"], "sessionExpired");
    assert_eq!(invoke(&h, "list_submissions", json!({ "problemId": 1 })).unwrap(), json!([]));
}

#[test]
fn throttling_is_reported_with_leetcodes_explanation() {
    let fake = fake_leetcode(|_| (429, json!({ "detail": "Request was throttled. Expected available in 21 seconds." }).to_string()));
    let h = harness_against(&fake.base);
    sign_in(&h);

    let err = submit(&h, "x").unwrap_err();
    assert_eq!(err["kind"], "rateLimited");
    assert!(err["message"].as_str().unwrap().contains("21 seconds"));
}

#[test]
fn a_verdict_that_never_arrives_gives_up_without_recording() {
    let fake = fake_leetcode(|r| match r.method.as_str() {
        "POST" => (200, json!({ "submission_id": 5 }).to_string()),
        _ => (200, json!({ "state": "PENDING" }).to_string()),
    });
    let h = harness_against(&fake.base);
    sign_in(&h);

    let err = submit(&h, "x").unwrap_err();
    assert_eq!(err["kind"], "other");
    assert!(err["message"].as_str().unwrap().contains("taking too long"));
    assert_eq!(invoke(&h, "list_submissions", json!({ "problemId": 1 })).unwrap(), json!([]));
}

#[test]
fn an_unknown_problem_is_rejected_before_anything_is_sent() {
    let fake = fake_leetcode(judging(accepted()));
    let h = harness_against(&fake.base);
    sign_in(&h);

    let err = invoke(&h, "submit_code", json!({ "problemId": 424242, "lang": "python3", "code": "x" })).unwrap_err();
    assert_eq!(err["kind"], "storage");
    assert_eq!(err["message"], "problem 424242 not found");
    assert!(fake.seen.lock().unwrap().is_empty());
}

#[test]
fn a_network_failure_is_reported_as_such() {
    let h = harness(); // nothing is listening at the harness's default address
    sign_in(&h);
    let err = submit(&h, "x").unwrap_err();
    assert_eq!(err["kind"], "network");
    assert!(err["message"].as_str().unwrap().starts_with("Couldn't reach LeetCode"));
}
