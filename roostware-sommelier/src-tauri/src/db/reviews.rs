//! The spaced-repetition log.
//!
//! Every Accepted submission is a review of its problem, graded 1 to 4 (Again, Hard, Good,
//! Easy). The grade starts as a guess from how the solve went (see [`inferred_grade`]); the
//! user can override it right after submitting ([`set_grade`]) or just leave it. Rows are only
//! ever added (or have their grade changed): what the memory model says about a problem is
//! recomputed by replaying them ([`memories`]), so the log can be refitted or reinterpreted
//! later without losing anything.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use super::{new_id, now_ms, Error, Result};
use crate::recommend::fsrs::{self, Grade, Memory};

/// A second Accepted submission this soon after a review of the same problem is the same sitting
/// (a resubmit after a tweak), not a new review.
const SAME_SITTING_MS: i64 = 12 * 60 * 60 * 1000;

/// A review as shown next to the submission it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewInfo {
    pub id: String,
    pub grade: i64,
}

/// A first guess at the grade from how many failed attempts preceded this solve (since the
/// previous review of the problem, or ever): none is Good, a couple is Hard, more is Again.
/// (Easy is only ever the user's own call.)
pub fn inferred_grade(failed_attempts: i64) -> Grade {
    match failed_attempts {
        0 => Grade::Good,
        1..=2 => Grade::Hard,
        _ => Grade::Again,
    }
}

/// Logs the review for an Accepted submission, with an inferred grade. Returns `None` when this
/// is the same sitting as a recent review of the problem (nothing is logged then).
///
/// Failed attempts count toward the inferred grade unless they were compile errors, which say
/// nothing about how well the problem is understood.
pub fn record_accepted(conn: &Connection, problem_id: i64, submission_id: &str, at: i64) -> Result<Option<ReviewInfo>> {
    let latest: Option<i64> = conn.query_row(
        "SELECT MAX(reviewed_at) FROM main.reviews WHERE problem_id = ? AND deleted_at IS NULL",
        [problem_id],
        |r| r.get(0),
    )?;
    if latest.is_some_and(|t| at - t < SAME_SITTING_MS) {
        return Ok(None);
    }
    let failures: i64 = conn.query_row(
        "SELECT COUNT(*) FROM main.submissions
          WHERE problem_id = ?1 AND deleted_at IS NULL AND submitted_at > ?2 AND submitted_at <= ?3
            AND status NOT IN ('Accepted', 'Compile Error')",
        params![problem_id, latest.unwrap_or(0), at],
        |r| r.get(0),
    )?;
    let grade = inferred_grade(failures);

    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO main.reviews (id, problem_id, submission_id, reviewed_at, grade, grade_source, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'inferred', ?6, ?6)",
        params![id, problem_id, submission_id, at, grade.as_i64(), now],
    )?;
    Ok(Some(ReviewInfo { id, grade: grade.as_i64() }))
}

/// Logs the review for giving up on a problem: always Again, whatever was tried first. It has no
/// submission, and `give_ups` records which review it is so the rating can tell it from a solve.
pub fn record_give_up(conn: &Connection, problem_id: i64, at: i64) -> Result<String> {
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO main.reviews (id, problem_id, submission_id, reviewed_at, grade, grade_source, created_at, updated_at)
         VALUES (?1, ?2, NULL, ?3, ?4, 'inferred', ?5, ?5)",
        params![id, problem_id, at, Grade::Again.as_i64(), now],
    )?;
    Ok(id)
}

/// Replaces a review's grade with the user's own.
pub fn set_grade(conn: &Connection, review_id: &str, grade: i64) -> Result<()> {
    let grade = Grade::from_i64(grade).ok_or_else(|| Error::Invalid(format!("{grade} is not a grade from 1 to 4")))?;
    let changed = conn.execute(
        "UPDATE main.reviews SET grade = ?2, grade_source = 'user', updated_at = ?3 WHERE id = ?1 AND deleted_at IS NULL",
        params![review_id, grade.as_i64(), now_ms()],
    )?;
    if changed == 0 {
        return Err(Error::NotFound(format!("review {review_id}")));
    }
    Ok(())
}

/// The reviews that came from giving up rather than solving: they're lapses to the memory model,
/// but they aren't a time the problem was solved.
pub const NOT_A_SOLVE: &str = "id NOT IN (SELECT review_id FROM main.give_ups WHERE review_id IS NOT NULL AND deleted_at IS NULL)";

/// Problems solved before reviews were logged, with when: ones with an Accepted submission but no
/// solving review. (A solve made after giving up isn't one of these: that's the user retyping a
/// solution they were just shown, and it's the rating's and the review log's business, not a
/// forgotten legacy solve.)
pub const LEGACY_SOLVES: &str = "SELECT problem_id, MIN(submitted_at) FROM main.submissions s
      WHERE status = 'Accepted' AND deleted_at IS NULL
        AND problem_id NOT IN (SELECT problem_id FROM main.reviews WHERE deleted_at IS NULL AND id NOT IN
              (SELECT review_id FROM main.give_ups WHERE review_id IS NOT NULL AND deleted_at IS NULL))
        AND submitted_at < COALESCE((SELECT MIN(gave_up_at) FROM main.give_ups g
                                      WHERE g.problem_id = s.problem_id AND g.deleted_at IS NULL), 9223372036854775807)
      GROUP BY problem_id";

/// The memory of every problem the user has reviewed, replayed from the log (given-up problems
/// start with an Again). A problem that has an Accepted submission but no solving review (solved
/// before reviews were logged, see [`LEGACY_SOLVES`]) counts as one Good review at its first solve.
pub fn memories(conn: &Connection) -> Result<HashMap<i64, Memory>> {
    let mut events: HashMap<i64, Vec<(i64, Grade)>> = HashMap::new();

    let mut logged = conn.prepare("SELECT problem_id, reviewed_at, grade FROM main.reviews WHERE deleted_at IS NULL")?;
    for row in logged.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
        let (problem, at, grade) = row?;
        if let Some(grade) = Grade::from_i64(grade) {
            events.entry(problem).or_default().push((at, grade));
        }
    }

    let mut legacy = conn.prepare(LEGACY_SOLVES)?;
    for row in legacy.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (problem, at) = row?;
        events.entry(problem).or_default().push((at, Grade::Good));
    }

    Ok(events.into_iter().filter_map(|(problem, list)| Some((problem, fsrs::replay(&list)?))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        submissions::{self, NewSubmission},
        test_support, Db,
    };
    use tempfile::TempDir;

    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;

    fn setup() -> (TempDir, Db) {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        (dir, db)
    }

    fn submit(c: &mut Connection, problem: i64, status: &str, code: &str) -> submissions::Submission {
        submissions::record(c, NewSubmission { problem_id: problem, lang: "python3".into(), code: code.into(), status: status.into(), ..Default::default() }).unwrap()
    }

    /// Moves every stored submission and review back in time, so the next one is "later".
    fn age(c: &Connection, ms: i64) {
        c.execute("UPDATE submissions SET submitted_at = submitted_at - ?1", [ms]).unwrap();
        c.execute("UPDATE reviews SET reviewed_at = reviewed_at - ?1", [ms]).unwrap();
    }

    #[test]
    fn grades_are_inferred_from_failed_attempts() {
        assert_eq!(inferred_grade(0), Grade::Good);
        assert_eq!(inferred_grade(1), Grade::Hard);
        assert_eq!(inferred_grade(2), Grade::Hard);
        assert_eq!(inferred_grade(3), Grade::Again);
        assert_eq!(inferred_grade(40), Grade::Again);
    }

    #[test]
    fn an_accepted_submission_is_reviewed_with_an_inferred_grade() {
        let (_d, db) = setup();
        let s = db.with(|c| Ok(submit(c, 1, "Accepted", "a"))).unwrap();
        assert_eq!(s.review_grade, Some(3), "a clean first solve is Good");
        let id = s.review_id.clone().expect("a review was logged");
        let (problem, source, at): (i64, String, i64) = db
            .with(|c| Ok(c.query_row("SELECT problem_id, grade_source, reviewed_at FROM reviews WHERE id = ?", [&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
            .unwrap();
        assert_eq!((problem, source.as_str(), at), (1, "inferred", s.submitted_at));
    }

    #[test]
    fn failures_before_the_solve_lower_the_inferred_grade_but_compile_errors_dont() {
        let grade_after = |failures: &[&str]| -> Option<i64> {
            let (_d2, db2) = setup();
            db2.with(|c| {
                for (i, status) in failures.iter().enumerate() {
                    submit(c, 1, status, &format!("try {i}"));
                }
                Ok(submit(c, 1, "Accepted", "done").review_grade)
            })
            .unwrap()
        };
        assert_eq!(grade_after(&[]), Some(3));
        assert_eq!(grade_after(&["Wrong Answer"]), Some(2));
        assert_eq!(grade_after(&["Wrong Answer", "Time Limit Exceeded"]), Some(2));
        assert_eq!(grade_after(&["Wrong Answer", "Runtime Error", "Time Limit Exceeded"]), Some(1));
        assert_eq!(grade_after(&["Compile Error", "Compile Error", "Compile Error", "Compile Error"]), Some(3), "typos aren't struggle");
    }

    #[test]
    fn only_failures_since_the_last_review_count() {
        let (_d, db) = setup();
        db.with(|c| {
            submit(c, 1, "Wrong Answer", "x");
            submit(c, 1, "Wrong Answer", "y");
            submit(c, 1, "Wrong Answer", "z");
            let first = submit(c, 1, "Accepted", "ok");
            assert_eq!(first.review_grade, Some(1));
            age(c, 30 * DAY);
            // A month later it's solved first time: that attempt history belongs to the old review.
            let second = submit(c, 1, "Accepted", "ok again");
            assert_eq!(second.review_grade, Some(3));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_resubmit_in_the_same_sitting_is_not_another_review() {
        let (_d, db) = setup();
        db.with(|c| {
            let first = submit(c, 1, "Accepted", "a");
            let again = submit(c, 1, "Accepted", "a, tweaked");
            assert!(first.review_id.is_some());
            assert_eq!((again.review_id, again.review_grade), (None, None));
            let n: i64 = c.query_row("SELECT COUNT(*) FROM reviews", [], |r| r.get(0))?;
            assert_eq!(n, 1);
            age(c, 13 * HOUR);
            assert!(submit(c, 1, "Accepted", "later").review_id.is_some(), "but half a day later it counts");
            // Other problems are independent.
            assert!(submit(c, 2, "Accepted", "other").review_id.is_some());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn only_accepted_submissions_are_reviews() {
        let (_d, db) = setup();
        let s = db.with(|c| Ok(submit(c, 1, "Wrong Answer", "x"))).unwrap();
        assert_eq!((s.review_id, s.review_grade), (None, None));
        assert_eq!(db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM reviews", [], |r| r.get::<_, i64>(0))?)).unwrap(), 0);
    }

    #[test]
    fn the_user_can_override_the_grade() {
        let (_d, db) = setup();
        let s = db.with(|c| Ok(submit(c, 1, "Accepted", "a"))).unwrap();
        let id = s.review_id.unwrap();
        db.with(|c| set_grade(c, &id, 4)).unwrap();
        let (grade, source): (i64, String) = db
            .with(|c| Ok(c.query_row("SELECT grade, grade_source FROM reviews WHERE id = ?", [&id], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap();
        assert_eq!((grade, source.as_str()), (4, "user"));
        // ...and the history shows it.
        let listed = db.with(|c| submissions::list(c, 1)).unwrap();
        assert_eq!(listed[0].review_grade, Some(4));

        for bad in [0, 5, -1] {
            assert!(matches!(db.with(|c| set_grade(c, &id, bad)), Err(Error::Invalid(_))), "grade {bad}");
        }
        assert!(matches!(db.with(|c| set_grade(c, "nope", 3)), Err(Error::NotFound(_))));
    }

    #[test]
    fn memories_are_replayed_from_the_log() {
        let (_d, db) = setup();
        db.with(|c| {
            submit(c, 1, "Accepted", "a");
            age(c, 10 * DAY);
            submit(c, 1, "Accepted", "b");
            submit(c, 2, "Accepted", "c");
            submit(c, 4, "Wrong Answer", "unsolved");
            Ok(())
        })
        .unwrap();
        let m = db.with(|c| memories(c)).unwrap();
        assert_eq!(m.len(), 2, "problem 4 was never solved");
        assert!(m[&1].stability > m[&2].stability, "two reviews beat one: {} vs {}", m[&1].stability, m[&2].stability);
        assert!(m[&1].last_review_ms > m[&2].last_review_ms - DAY, "the replay ends at the latest review");
    }

    #[test]
    fn a_problem_solved_before_reviews_existed_counts_as_one_good_review() {
        let (_d, db) = setup();
        db.with(|c| {
            c.execute(
                "INSERT INTO submissions (id, problem_id, lang, code, status, submitted_at, created_at, updated_at)
                 VALUES ('legacy', 1, 'python3', 'x', 'Accepted', 5000, 5000, 5000)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let m = db.with(|c| memories(c)).unwrap();
        assert_eq!(m[&1].last_review_ms, 5000);
        assert!((m[&1].stability - 2.3065).abs() < 1e-9, "a first Good review");
    }
}
