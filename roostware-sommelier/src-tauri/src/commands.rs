//! Tauri commands: thin wrappers that hand the frontend the query layer in
//! `db::problems`. They're `async` so they run off the main thread and a slow
//! query can't freeze the window. Keep the TypeScript mirror in
//! `src/lib/db.ts` in sync with the types here.

use tauri::{AppHandle, Runtime, State};

use crate::{
    db::{
        collections::{self, CollectionSummary},
        drafts,
        give_ups::{self, GiveUpInfo},
        now_ms,
        problems::{self, ProblemDetail, ProblemPage, ProblemQuery, RecentProblem, TagCount},
        rating::{self, RatingSummary},
        recommendations::{self, DailySet, PatternStrength},
        reset, reviews,
        settings::{self, CoachSettings, Settings},
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

/// Makes a new collection holding this problem (collections only exist while they have
/// problems, so it can't be made empty). Fails if the name is blank, too long or already used.
#[tauri::command]
pub async fn create_collection(db: State<'_, Db>, name: String, problem_id: i64) -> Result<CollectionSummary> {
    db.with(|conn| collections::create(conn, &name, problem_id))
}

/// Adds a problem to one of the user's collections. Fine if it's already in it.
#[tauri::command]
pub async fn add_problem_to_collection(db: State<'_, Db>, collection_id: String, problem_id: i64) -> Result<()> {
    db.with(|conn| collections::add_problem(conn, &collection_id, problem_id))
}

/// Takes a problem out of a collection. A collection left with no problems is deleted.
#[tauri::command]
pub async fn remove_problem_from_collection(
    db: State<'_, Db>,
    collection_id: String,
    problem_id: i64,
) -> Result<()> {
    db.with(|conn| collections::remove_problem(conn, &collection_id, problem_id))
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

/// Erases everything the user has saved (drafts, submissions, ratings, reviews, collections,
/// notes, settings, saved conversations) and leaves the app as on a first launch. The LeetCode
/// login and API keys live in the OS credential store, and are cleared with `leetcode_sign_out`
/// and `clear_coach_key`. With `keep_ai_setup`, the AI provider, model, address and prices stay as
/// they were (for a user who is keeping their API keys, which only work with their provider).
#[tauri::command]
pub async fn reset_user_data(db: State<'_, Db>, keep_ai_setup: bool) -> Result<()> {
    db.with(|conn| reset::wipe(conn, keep_ai_setup))
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
/// cases are used. Like Submit, it's retried from a browser window if Cloudflare challenges it.
#[tauri::command]
pub async fn run_code<R: Runtime>(
    app: AppHandle<R>,
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
    let code = for_leetcode(&code);
    let verdict = match lc.client.run(&credentials, &slug, problem_id, &lang, &code, &input).await {
        Err(LeetCodeError::Blocked(_)) => {
            leetcode::browser::run(&app, &lc.client, &credentials, &slug, problem_id, &lang, &code, &input).await?
        }
        other => other?,
    };
    Ok(lc_result::parse_run(&verdict, &input))
}

/// Submits code to LeetCode for judging against all its tests, records the verdict in the
/// user's submission history, and returns it. Recording also settles the problem's draft for
/// that language (the code is now submitted, not unsubmitted).
///
/// Cloudflare sometimes challenges the app's own request, which only a browser can answer; then
/// the submit is made again from a real browser window (see `leetcode::browser`).
#[tauri::command]
pub async fn submit_code<R: Runtime>(
    app: AppHandle<R>,
    db: State<'_, Db>,
    lc: State<'_, LeetCode>,
    problem_id: i64,
    lang: String,
    code: String,
) -> leetcode::Result<Submission> {
    let credentials = lc.credentials()?;
    let (slug, _) = db.with(|conn| problems::judging_info(conn, problem_id))?;
    let code_for_lc = for_leetcode(&code);
    let (leetcode_id, verdict) = match lc.client.submit(&credentials, &slug, problem_id, &lang, &code_for_lc).await {
        Err(LeetCodeError::Blocked(_)) => {
            leetcode::browser::submit(&app, &lc.client, &credentials, &slug, problem_id, &lang, &code_for_lc).await?
        }
        other => other?,
    };
    // Store the code as the user wrote it, not as it was sent.
    let record = lc_result::parse_submission(&verdict, problem_id, &lang, &code, Some(leetcode_id));
    Ok(db.with(|conn| submissions::record(conn, record))?)
}

/// A problem's submissions, newest first.
#[tauri::command]
pub async fn list_submissions(db: State<'_, Db>, problem_id: i64) -> Result<Vec<Submission>> {
    db.with(|conn| submissions::list(conn, problem_id))
}

// ---- Recommendations -----------------------------------------------------------------------

/// Today's recommended problems: a few due reviews, then new problems for the patterns the user
/// knows least. `day` is the user's local date (YYYY-MM-DD). The set is made the first time it's
/// asked for each day and then kept, so it doesn't reshuffle.
#[tauri::command]
pub async fn get_daily_recommendations(db: State<'_, Db>, day: String) -> Result<DailySet> {
    db.with(|conn| recommendations::daily_set(conn, &day, now_ms()))
}

/// How strong the user is in each of the 17 patterns (0 to 100), in learning order.
#[tauri::command]
pub async fn get_pattern_strengths(db: State<'_, Db>) -> Result<Vec<PatternStrength>> {
    db.with(|conn| recommendations::pattern_strengths(conn, now_ms()))
}

/// The user's overall skill rating, its history, and how it moved this week. Replayed from the
/// solves each time; nothing is stored.
#[tauri::command]
pub async fn get_rating(db: State<'_, Db>) -> Result<RatingSummary> {
    db.with(|conn| rating::summary(conn, now_ms()))
}

// ---- Settings ------------------------------------------------------------------------------

/// The user's preferences (defaults for any they haven't changed).
#[tauri::command]
pub async fn get_settings(db: State<'_, Db>) -> Result<Settings> {
    db.with(|conn| settings::all(conn))
}

/// How many problems the daily set has, 1 to 10. Today's set follows at once: raising it tops
/// it up, lowering it trims it (keeping solved problems).
#[tauri::command]
pub async fn set_daily_problem_count(db: State<'_, Db>, count: usize) -> Result<()> {
    db.with(|conn| settings::set_daily_count(conn, count))
}

/// Whether the app's curated collections (Blind 75, …) are shown in the problem list.
#[tauri::command]
pub async fn set_show_curated_collections(db: State<'_, Db>, show: bool) -> Result<()> {
    db.with(|conn| settings::set_show_curated_collections(conn, show))
}

/// The AI coach's settings: whether it shows in the Solve tab, which provider and model it uses, the
/// address of an OpenAI-compatible server, and optional prices for the usage line. Rejects an
/// unknown provider, a model name over 100 characters, a base URL that isn't http(s), or a price
/// out of range. (The API key is separate: `set_coach_key`.)
#[tauri::command]
pub async fn set_coach_settings(db: State<'_, Db>, settings: CoachSettings) -> Result<()> {
    db.with(|conn| settings::set_coach(conn, &settings))
}

// ---- Giving up -----------------------------------------------------------------------------

/// Gives up on a problem: logs an Again review for it and, if it has no result yet, a loss in the
/// rating that a later solve can win most of back (after a wait). Returns the problem's give-up
/// state as it now stands. The caller shows the solution.
#[tauri::command]
pub async fn give_up(db: State<'_, Db>, problem_id: i64) -> Result<GiveUpInfo> {
    db.with(|conn| {
        let now = now_ms();
        give_ups::record(conn, problem_id, now)?;
        give_ups::info(conn, problem_id, now)
    })
}

/// A problem's give-up state, and a preview of what giving up now would do to the rating.
#[tauri::command]
pub async fn get_give_up_info(db: State<'_, Db>, problem_id: i64) -> Result<GiveUpInfo> {
    db.with(|conn| give_ups::info(conn, problem_id, now_ms()))
}

/// Sets how a solve went (1 Again, 2 Hard, 3 Good, 4 Easy), replacing the grade that was guessed.
/// The review's id is on the Accepted submission.
#[tauri::command]
pub async fn set_review_grade(db: State<'_, Db>, review_id: String, grade: i64) -> Result<()> {
    db.with(|conn| reviews::set_grade(conn, &review_id, grade))
}

/// When every submission was made, oldest first (epoch milliseconds, UTC). Feeds the dashboard's
/// activity graph and streak.
#[tauri::command]
pub async fn list_submission_times(db: State<'_, Db>) -> Result<Vec<i64>> {
    db.with(|conn| submissions::times(conn))
}
