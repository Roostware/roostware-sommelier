//! The user's submissions: immutable snapshots of what LeetCode said about a piece of code.
//!
//! Recording a submission also settles the draft for that problem and language: the code is
//! now "submitted", not "unsubmitted", so a draft holding exactly that code is removed. (A draft
//! the user has since edited further is kept.) The latest submission for a language is also the
//! baseline drafts are measured against, see [`latest_code`] and `drafts`.

use rusqlite::{Connection, OptionalExtension, Row};
use serde::Serialize;

use super::{drafts, new_id, now_ms, reviews, Error, Result};

/// A stored submission, as shown in a problem's history. (The raw LeetCode response, kept in the
/// `result_json` column for the future, isn't sent to the frontend.)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub id: String,
    pub problem_id: i64,
    /// LeetCode langSlug (python3, cpp, …).
    pub lang: String,
    pub code: String,
    /// LeetCode's id for the submission.
    pub leetcode_submission_id: Option<String>,
    /// LeetCode's verdict: "Accepted", "Wrong Answer", "Time Limit Exceeded", "Compile Error", …
    pub status: String,
    pub status_code: Option<i64>,
    pub runtime_ms: Option<i64>,
    pub runtime_percentile: Option<f64>,
    pub memory_kb: Option<i64>,
    pub memory_percentile: Option<f64>,
    /// For a failure, tests passed before the first failing one is *not* reported by LeetCode
    /// (it stops at the first failure), so treat this as "all passed" only when it equals the total.
    pub total_correct: Option<i64>,
    pub total_testcases: Option<i64>,
    /// Failure details: the input that failed, what was expected and what the code produced.
    pub last_testcase: Option<String>,
    pub expected_output: Option<String>,
    pub code_output: Option<String>,
    /// Compile or runtime error text.
    pub error_message: Option<String>,
    /// Epoch milliseconds, UTC.
    pub submitted_at: i64,
    /// For an Accepted submission, the spaced-repetition review it logged (none when it was a
    /// resubmit in the same sitting as a recent review) and its grade, 1 Again to 4 Easy.
    pub review_id: Option<String>,
    pub review_grade: Option<i64>,
}

/// What to store for a new submission (id and times are assigned here).
#[derive(Debug, Clone, Default)]
pub struct NewSubmission {
    pub problem_id: i64,
    pub lang: String,
    pub code: String,
    pub leetcode_submission_id: Option<String>,
    pub status: String,
    pub status_code: Option<i64>,
    pub runtime_ms: Option<i64>,
    pub runtime_percentile: Option<f64>,
    pub memory_kb: Option<i64>,
    pub memory_percentile: Option<f64>,
    pub total_correct: Option<i64>,
    pub total_testcases: Option<i64>,
    pub last_testcase: Option<String>,
    pub expected_output: Option<String>,
    pub code_output: Option<String>,
    pub error_message: Option<String>,
    /// LeetCode's raw response, kept so API changes never lose data.
    pub result_json: Option<String>,
}

/// The code of the most recent submission in each language, newest first.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSubmitted {
    pub lang: String,
    pub code: String,
    /// Epoch milliseconds, UTC.
    pub submitted_at: i64,
}

const COLUMNS: &str = "id, problem_id, lang, code, leetcode_submission_id, status, status_code, runtime_ms,
    runtime_percentile, memory_kb, memory_percentile, total_correct, total_testcases, last_testcase,
    expected_output, code_output, error_message, submitted_at,
    (SELECT r.id FROM main.reviews r WHERE r.submission_id = submissions.id AND r.deleted_at IS NULL) AS review_id,
    (SELECT r.grade FROM main.reviews r WHERE r.submission_id = submissions.id AND r.deleted_at IS NULL) AS review_grade";

fn from_row(r: &Row) -> rusqlite::Result<Submission> {
    Ok(Submission {
        id: r.get(0)?,
        problem_id: r.get(1)?,
        lang: r.get(2)?,
        code: r.get(3)?,
        leetcode_submission_id: r.get(4)?,
        status: r.get(5)?,
        status_code: r.get(6)?,
        runtime_ms: r.get(7)?,
        runtime_percentile: r.get(8)?,
        memory_kb: r.get(9)?,
        memory_percentile: r.get(10)?,
        total_correct: r.get(11)?,
        total_testcases: r.get(12)?,
        last_testcase: r.get(13)?,
        expected_output: r.get(14)?,
        code_output: r.get(15)?,
        error_message: r.get(16)?,
        submitted_at: r.get(17)?,
        review_id: r.get(18)?,
        review_grade: r.get(19)?,
    })
}

/// Stores a submission and settles the problem's draft for that language (see the module docs),
/// all in one transaction. Fails, writing nothing, if the problem doesn't exist.
pub fn record(conn: &mut Connection, new: NewSubmission) -> Result<Submission> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM catalog.problems WHERE id = ?)",
        [new.problem_id],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(Error::NotFound(format!("problem {}", new.problem_id)));
    }

    let now = now_ms();
    let id = new_id();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO main.submissions (id, problem_id, lang, code, leetcode_submission_id, status,
            status_code, runtime_ms, runtime_percentile, memory_kb, memory_percentile, total_correct,
            total_testcases, last_testcase, expected_output, code_output, error_message, result_json,
            submitted_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?19, ?19)",
        rusqlite::params![
            id,
            new.problem_id,
            new.lang,
            new.code,
            new.leetcode_submission_id,
            new.status,
            new.status_code,
            new.runtime_ms,
            new.runtime_percentile,
            new.memory_kb,
            new.memory_percentile,
            new.total_correct,
            new.total_testcases,
            new.last_testcase,
            new.expected_output,
            new.code_output,
            new.error_message,
            new.result_json,
            now,
        ],
    )?;

    // Solving a problem is a spaced-repetition review of it (graded by a guess the user can change).
    if new.status == "Accepted" {
        reviews::record_accepted(&tx, new.problem_id, &id, now)?;
    }

    // The draft is only "unsubmitted" work: if it is exactly what was just submitted, it's done.
    let draft: Option<String> = tx
        .query_row(
            "SELECT code FROM main.drafts WHERE problem_id = ?1 AND lang = ?2",
            rusqlite::params![new.problem_id, new.lang],
            |r| r.get(0),
        )
        .optional()?;
    if draft.is_some_and(|d| drafts::same_code(&d, &new.code)) {
        drafts::delete(&tx, new.problem_id, &new.lang)?;
    }

    let stored = tx.query_row(
        &format!("SELECT {COLUMNS} FROM main.submissions WHERE id = ?"),
        [&id],
        from_row,
    )?;
    tx.commit()?;
    Ok(stored)
}

/// A problem's submissions, newest first.
pub fn list(conn: &Connection, problem_id: i64) -> Result<Vec<Submission>> {
    let rows = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM main.submissions
              WHERE problem_id = ? AND deleted_at IS NULL
              ORDER BY submitted_at DESC, created_at DESC, id DESC"
        ))?
        .query_map([problem_id], from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// When each of the user's submissions was made (any problem, any verdict), oldest first, as epoch
/// milliseconds UTC. The dashboard's activity graph and streak are built from these; which
/// calendar day each belongs to is the user's local time, so that's left to the frontend.
pub fn times(conn: &Connection) -> Result<Vec<i64>> {
    let rows = conn
        .prepare("SELECT submitted_at FROM main.submissions WHERE deleted_at IS NULL ORDER BY submitted_at")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<i64>>>()?;
    Ok(rows)
}

/// The code of the most recent submission for a problem in a language, if there is one.
pub fn latest_code(conn: &Connection, problem_id: i64, lang: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT code FROM main.submissions
              WHERE problem_id = ?1 AND lang = ?2 AND deleted_at IS NULL
              ORDER BY submitted_at DESC, created_at DESC, id DESC LIMIT 1",
            rusqlite::params![problem_id, lang],
            |r| r.get(0),
        )
        .optional()?)
}

/// The most recent submitted code in each language the problem has been submitted in.
pub fn last_submitted(conn: &Connection, problem_id: i64) -> Result<Vec<LastSubmitted>> {
    let rows = conn
        .prepare(
            "SELECT lang, code, submitted_at FROM (
               SELECT lang, code, submitted_at,
                      ROW_NUMBER() OVER (PARTITION BY lang
                                         ORDER BY submitted_at DESC, created_at DESC, id DESC) AS n
                 FROM main.submissions WHERE problem_id = ? AND deleted_at IS NULL
             ) WHERE n = 1 ORDER BY submitted_at DESC, lang",
        )?
        .query_map([problem_id], |r| {
            Ok(LastSubmitted { lang: r.get(0)?, code: r.get(1)?, submitted_at: r.get(2)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{test_support, Db};
    use tempfile::TempDir;

    fn setup() -> (TempDir, Db) {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        (dir, db)
    }

    fn new(problem_id: i64, lang: &str, code: &str, status: &str) -> NewSubmission {
        NewSubmission {
            problem_id,
            lang: lang.into(),
            code: code.into(),
            status: status.into(),
            ..Default::default()
        }
    }

    #[test]
    fn records_every_field_and_lists_newest_first() {
        let (_d, db) = setup();
        let stored = db
            .with(|c| {
                record(
                    c,
                    NewSubmission {
                        leetcode_submission_id: Some("123".into()),
                        status_code: Some(11),
                        runtime_ms: Some(4),
                        runtime_percentile: Some(64.5),
                        memory_kb: Some(20900),
                        memory_percentile: Some(51.7),
                        total_correct: Some(0),
                        total_testcases: Some(66),
                        last_testcase: Some("[2,7]\n9".into()),
                        expected_output: Some("[0,1]".into()),
                        code_output: Some("[0,0]".into()),
                        error_message: Some("boom".into()),
                        result_json: Some("{\"a\":1}".into()),
                        ..new(1, "python3", "wrong", "Wrong Answer")
                    },
                )
            })
            .unwrap();
        assert_eq!(stored.status, "Wrong Answer");
        assert_eq!(stored.leetcode_submission_id.as_deref(), Some("123"));
        assert_eq!((stored.runtime_ms, stored.memory_kb), (Some(4), Some(20900)));
        assert_eq!(stored.last_testcase.as_deref(), Some("[2,7]\n9"));
        assert!(stored.submitted_at > 0);

        db.with(|c| {
            c.execute("UPDATE submissions SET submitted_at = 1000", [])?;
            record(c, new(1, "python3", "right", "Accepted")).map(|_| ())
        })
        .unwrap();
        let listed = db.with(|c| list(c, 1)).unwrap();
        assert_eq!(listed.iter().map(|s| s.status.as_str()).collect::<Vec<_>>(), ["Accepted", "Wrong Answer"]);
        assert!(db.with(|c| list(c, 2)).unwrap().is_empty());
    }

    #[test]
    fn raw_response_is_stored_but_not_in_the_listing() {
        let (_d, db) = setup();
        db.with(|c| {
            record(c, NewSubmission { result_json: Some("{\"raw\":true}".into()), ..new(1, "python3", "x", "Accepted") })
                .map(|_| ())
        })
        .unwrap();
        let raw: String = db
            .with(|c| Ok(c.query_row("SELECT result_json FROM submissions", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(raw, "{\"raw\":true}");
        let json = serde_json::to_value(&db.with(|c| list(c, 1)).unwrap()[0]).unwrap();
        assert!(json.get("resultJson").is_none());
    }

    #[test]
    fn unknown_problem_is_rejected_and_writes_nothing() {
        let (_d, db) = setup();
        let err = db.with(|c| record(c, new(12345, "python3", "x", "Accepted"))).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        let n: i64 = db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM submissions", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn submitting_the_drafted_code_removes_that_draft_only() {
        let (_d, db) = setup();
        db.with(|c| {
            drafts::save(c, 1, "python3", "my code")?;
            drafts::save(c, 1, "cpp", "cpp code")?;
            record(c, new(1, "python3", "my code", "Accepted")).map(|_| ())
        })
        .unwrap();
        let left = db.with(|c| drafts::list(c, 1)).unwrap();
        assert_eq!(left.iter().map(|d| d.lang.as_str()).collect::<Vec<_>>(), ["cpp"], "python3's draft was submitted");
    }

    #[test]
    fn a_draft_edited_past_what_was_submitted_survives() {
        let (_d, db) = setup();
        db.with(|c| {
            drafts::save(c, 1, "python3", "my code, plus more typing")?;
            record(c, new(1, "python3", "my code", "Wrong Answer")).map(|_| ())
        })
        .unwrap();
        let left = db.with(|c| drafts::list(c, 1)).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].code, "my code, plus more typing");
    }

    #[test]
    fn line_endings_dont_stop_a_submitted_draft_being_cleared() {
        let (_d, db) = setup();
        db.with(|c| {
            drafts::save(c, 1, "python3", "a\r\nb")?;
            record(c, new(1, "python3", "a\nb", "Accepted")).map(|_| ())
        })
        .unwrap();
        assert!(db.with(|c| drafts::list(c, 1)).unwrap().is_empty());
    }

    #[test]
    fn latest_code_is_per_language_and_ignores_deleted_rows() {
        let (_d, db) = setup();
        db.with(|c| {
            record(c, new(1, "python3", "first", "Wrong Answer"))?;
            c.execute("UPDATE submissions SET submitted_at = 1000", [])?;
            record(c, new(1, "python3", "second", "Accepted"))?;
            record(c, new(1, "cpp", "cpp one", "Accepted"))?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.with(|c| latest_code(c, 1, "python3")).unwrap().as_deref(), Some("second"));
        assert_eq!(db.with(|c| latest_code(c, 1, "cpp")).unwrap().as_deref(), Some("cpp one"));
        assert_eq!(db.with(|c| latest_code(c, 1, "java")).unwrap(), None);

        db.with(|c| {
            c.execute("UPDATE submissions SET deleted_at = 5 WHERE code = 'second'", [])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.with(|c| latest_code(c, 1, "python3")).unwrap().as_deref(), Some("first"));
    }

    #[test]
    fn last_submitted_has_one_entry_per_language_newest_first() {
        let (_d, db) = setup();
        db.with(|c| {
            record(c, new(1, "python3", "old", "Wrong Answer"))?;
            record(c, new(1, "cpp", "cpp", "Accepted"))?;
            c.execute("UPDATE submissions SET submitted_at = 1000", [])?;
            record(c, new(1, "python3", "new", "Accepted"))?;
            Ok(())
        })
        .unwrap();
        let got = db.with(|c| last_submitted(c, 1)).unwrap();
        assert_eq!(got.iter().map(|s| (s.lang.as_str(), s.code.as_str())).collect::<Vec<_>>(), [("python3", "new"), ("cpp", "cpp")]);
        assert!(db.with(|c| last_submitted(c, 2)).unwrap().is_empty());
    }

    #[test]
    fn times_cover_every_problem_and_verdict_oldest_first_without_deleted_ones() {
        let (_d, db) = setup();
        assert!(db.with(|c| times(c)).unwrap().is_empty());
        db.with(|c| {
            record(c, new(1, "python3", "a", "Wrong Answer"))?;
            record(c, new(1, "python3", "b", "Accepted"))?;
            record(c, new(900, "python3", "c", "Accepted"))?;
            record(c, new(900, "python3", "gone", "Accepted"))?;
            // Out of order on purpose, so the sort is what's being tested.
            c.execute("UPDATE submissions SET submitted_at = 3000 WHERE code = 'a'", [])?;
            c.execute("UPDATE submissions SET submitted_at = 1000 WHERE code = 'b'", [])?;
            c.execute("UPDATE submissions SET submitted_at = 2000 WHERE code = 'c'", [])?;
            c.execute("UPDATE submissions SET submitted_at = 500, deleted_at = 5 WHERE code = 'gone'", [])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.with(|c| times(c)).unwrap(), vec![1000, 2000, 3000]);
    }

    #[test]
    fn recording_changes_progress() {
        let (_d, db) = setup();
        let progress = |db: &Db| -> String {
            let d = db.with(|c| crate::db::problems::get_by_id(c, 1)).unwrap().unwrap();
            serde_json::to_value(d.summary.progress).unwrap().as_str().unwrap().to_owned()
        };
        assert_eq!(progress(&db), "untouched");
        db.with(|c| record(c, new(1, "python3", "x", "Wrong Answer")).map(|_| ())).unwrap();
        assert_eq!(progress(&db), "attempted");
        db.with(|c| record(c, new(1, "python3", "y", "Accepted")).map(|_| ())).unwrap();
        assert_eq!(progress(&db), "solved");
    }
}
