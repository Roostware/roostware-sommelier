//! Tauri commands: thin wrappers that hand the frontend the query layer in
//! `db::problems`. They're `async` so they run off the main thread and a slow
//! query can't freeze the window. Keep the TypeScript mirror in
//! `src/lib/db.ts` in sync with the types here.

use tauri::{AppHandle, Runtime, State};

use crate::{
    db::{
        collections::{self, CollectionSummary},
        drafts,
        problems::{self, ProblemDetail, ProblemPage, ProblemQuery, RecentProblem, TagCount},
        submissions::{self, Submission},
        Db, Result,
    },
    leetcode::{
        self,
        auth::{self, AuthStatus},
        result::{self as lc_result, RunResult},
        Error as LeetCodeError, LeetCode,
    },
};

#[tauri::command]
pub async fn list_problems(db: State<'_, Db>, query: ProblemQuery) -> Result<ProblemPage> {
    db.with(|conn| problems::list(conn, &query))
}

/// `None` when no problem has this id.
#[tauri::command]
pub async fn get_problem(db: State<'_, Db>, id: i64) -> Result<Option<ProblemDetail>> {
    db.with(|conn| problems::get_by_id(conn, id))
}

/// `None` when no problem has this slug.
#[tauri::command]
pub async fn get_problem_by_slug(db: State<'_, Db>, slug: String) -> Result<Option<ProblemDetail>> {
    db.with(|conn| problems::get_by_slug(conn, &slug))
}

#[tauri::command]
pub async fn list_tags(db: State<'_, Db>) -> Result<Vec<TagCount>> {
    db.with(|conn| problems::list_tags(conn))
}


#[tauri::command]
pub async fn list_collections(db: State<'_, Db>) -> Result<Vec<CollectionSummary>> {
    db.with(|conn| collections::list(conn))
}

/// Records that the user just opened a problem (feeds the "recently opened" list).
#[tauri::command]
pub async fn record_problem_opened(db: State<'_, Db>, id: i64) -> Result<()> {
    db.with(|conn| problems::mark_opened(conn, id))
}

/// Most recently opened problems, newest first. `limit` defaults to 10.
#[tauri::command]
pub async fn list_recent_problems(db: State<'_, Db>, limit: Option<u32>) -> Result<Vec<RecentProblem>> {
    db.with(|conn| problems::list_recent(conn, limit.unwrap_or(10)))
}

/// Saves the user's code for a problem in a language as an unsubmitted draft. Returns whether a
/// draft is now stored: `false` means the code matched the starter code, so there is none.
#[tauri::command]
pub async fn save_draft(db: State<'_, Db>, problem_id: i64, lang: String, code: String) -> Result<bool> {
    db.with(|conn| drafts::save(conn, problem_id, &lang, &code))
}

/// Discards the draft for a problem in a language (not an error if there isn't one).
#[tauri::command]
pub async fn delete_draft(db: State<'_, Db>, problem_id: i64, lang: String) -> Result<()> {
    db.with(|conn| drafts::delete(conn, problem_id, &lang))
}

// ---- LeetCode: sign-in, Run and Submit -----------------------------------------------------
// These return `leetcode::Error`, which reaches the frontend as `{ kind, message }` so it can
// tell "sign in again" apart from the rest. See `src/lib/leetcode.ts`.

/// Whether a LeetCode login is stored (nothing is sent to LeetCode).
#[tauri::command]
pub async fn leetcode_status(lc: State<'_, LeetCode>) -> leetcode::Result<AuthStatus> {
    lc.status()
}

/// Opens LeetCode's login page in its own window and resolves once the user has signed in, or
/// fails with kind `cancelled` if they close the window first.
#[tauri::command]
pub async fn leetcode_sign_in<R: Runtime>(app: AppHandle<R>, lc: State<'_, LeetCode>) -> leetcode::Result<AuthStatus> {
    auth::sign_in(&app, &lc).await
}

/// Forgets the stored login.
#[tauri::command]
pub async fn leetcode_sign_out(lc: State<'_, LeetCode>) -> leetcode::Result<AuthStatus> {
    lc.store.clear()?;
    Ok(AuthStatus::signed_out())
}

/// The editor may produce Windows line endings, which mean nothing to LeetCode: send plain LF.
fn for_leetcode(code: &str) -> String {
    code.replace("\r\n", "\n")
}

/// Runs code on LeetCode against test cases without submitting it. `input` is one line per
/// argument, cases one after another; when it's missing or blank the problem's own example
/// cases are used.
#[tauri::command]
pub async fn run_code(
    db: State<'_, Db>,
    lc: State<'_, LeetCode>,
    problem_id: i64,
    lang: String,
    code: String,
    input: Option<String>,
) -> leetcode::Result<RunResult> {
    let credentials = lc.credentials()?;
    let (slug, examples) = db.with(|conn| problems::judging_info(conn, problem_id))?;
    let input = input.filter(|i| !i.trim().is_empty()).unwrap_or(examples);
    if input.trim().is_empty() {
        return Err(LeetCodeError::Other("This problem has no example test cases to run.".into()));
    }
    let verdict = lc.client.run(&credentials, &slug, problem_id, &lang, &for_leetcode(&code), &input).await?;
    Ok(lc_result::parse_run(&verdict, &input))
}

/// Submits code to LeetCode for judging against all its tests, records the verdict in the
/// user's submission history, and returns it. Recording also settles the problem's draft for
/// that language (the code is now submitted, not unsubmitted).
#[tauri::command]
pub async fn submit_code(
    db: State<'_, Db>,
    lc: State<'_, LeetCode>,
    problem_id: i64,
    lang: String,
    code: String,
) -> leetcode::Result<Submission> {
    let credentials = lc.credentials()?;
    let (slug, _) = db.with(|conn| problems::judging_info(conn, problem_id))?;
    let (leetcode_id, verdict) =
        lc.client.submit(&credentials, &slug, problem_id, &lang, &for_leetcode(&code)).await?;
    // Store the code as the user wrote it, not as it was sent.
    let record = lc_result::parse_submission(&verdict, problem_id, &lang, &code, Some(leetcode_id));
    Ok(db.with(|conn| submissions::record(conn, record))?)
}

/// A problem's submissions, newest first.
#[tauri::command]
pub async fn list_submissions(db: State<'_, Db>, problem_id: i64) -> Result<Vec<Submission>> {
    db.with(|conn| submissions::list(conn, problem_id))
}
