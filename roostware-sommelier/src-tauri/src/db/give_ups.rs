//! Giving up on a problem: the user asks to be shown the solution instead of solving it.
//!
//! What it counts as:
//!   * **spaced repetition:** always an Again review, however many attempts came before
//!     ([`reviews::record_give_up`]), so the problem comes back soon;
//!   * **the rating:** a loss, scored 0, unless the problem already has a result (it was solved or
//!     given up on before), in which case nothing changes. A later solve wins most of it back, but
//!     only `GIVE_UP_RESOLVE_GAP_MS` after the give-up (see `recommend::elo`).
//!
//! Nothing here decides *whether* the user may give up (the UI only offers it before a problem is
//! solved); the rules above hold whenever it happens.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::{new_id, now_ms, rating, reviews, Error, Result};
use crate::recommend::{
    elo::{self, GiveUp, Solve, GIVE_UP_RESOLVE_GAP_MS},
    fsrs::Grade,
};

/// Giving up again this soon after the last time is the same sitting (a double click, say): it
/// isn't recorded twice.
const SAME_SITTING_MS: i64 = 12 * 60 * 60 * 1000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// What the Solve tab needs to know about giving up on one problem.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiveUpInfo {
    /// When the user gave up, if they did and haven't solved the problem since (epoch ms).
    pub gave_up_at: Option<i64>,
    /// From when a re-solve counts toward the rating again (`gave_up_at` plus the wait).
    pub retry_at: Option<i64>,
    /// How much rating a clean re-solve would win back from `retry_at` on (0 when there is
    /// nothing to win back, or no give-up).
    pub recoverable: i64,
    /// Whether giving up now would move the rating at all (it doesn't for a problem that already
    /// has a result).
    pub affects_rating: bool,
    /// What giving up now would do to the rating: zero or negative.
    pub preview_change: i64,
    /// What a clean re-solve after that would win back.
    pub preview_recoverable: i64,
    /// The wait before a re-solve counts, in days.
    pub wait_days: i64,
}

/// Records that the user gave up on a problem at `at`: the give-up itself and its Again review,
/// together. Does nothing if they already gave up on it within the last half day. Fails, writing
/// nothing, if the problem doesn't exist.
pub fn record(conn: &mut Connection, problem_id: i64, at: i64) -> Result<()> {
    let exists: bool =
        conn.query_row("SELECT EXISTS (SELECT 1 FROM catalog.problems WHERE id = ?)", [problem_id], |r| r.get(0))?;
    if !exists {
        return Err(Error::NotFound(format!("problem {problem_id}")));
    }
    let latest: Option<i64> = conn.query_row(
        "SELECT MAX(gave_up_at) FROM main.give_ups WHERE problem_id = ? AND deleted_at IS NULL",
        [problem_id],
        |r| r.get(0),
    )?;
    if latest.is_some_and(|t| at - t < SAME_SITTING_MS) {
        return Ok(());
    }

    let now = now_ms();
    let tx = conn.transaction()?;
    let review_id = reviews::record_give_up(&tx, problem_id, at)?;
    tx.execute(
        "INSERT INTO main.give_ups (id, problem_id, review_id, gave_up_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![new_id(), problem_id, review_id, at, now],
    )?;
    tx.commit()?;
    Ok(())
}

/// How rating would change, replayed from everything stored.
fn gain_from_clean_solve(
    solves: &[Solve],
    give_ups: &[GiveUp],
    ratings: &std::collections::HashMap<i64, f64>,
    problem_id: i64,
    at: i64,
    base: f64,
) -> i64 {
    let mut with = solves.to_vec();
    with.push(Solve { problem_id, at, grade: Grade::Good });
    elo::replay_with(&with, give_ups, ratings).rating.round() as i64 - base.round() as i64
}

/// The give-up state of a problem as of `now`, plus a preview of what giving up now would do.
pub fn info(conn: &Connection, problem_id: i64, now: i64) -> Result<GiveUpInfo> {
    // The latest give-up counts only until the problem is solved again.
    let gave_up: Option<i64> = conn.query_row(
        "SELECT MAX(gave_up_at) FROM main.give_ups WHERE problem_id = ? AND deleted_at IS NULL",
        [problem_id],
        |r| r.get(0),
    )?;
    let solved: Option<i64> = conn
        .query_row(
            "SELECT MAX(submitted_at) FROM main.submissions
              WHERE problem_id = ? AND status = 'Accepted' AND deleted_at IS NULL",
            [problem_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let gave_up_at = gave_up.filter(|&g| solved.is_none_or(|s| s < g));

    let ratings = elo::problem_ratings(&rating::problem_infos(conn)?);
    let (solves, give_ups) = rating::events(conn)?;
    let current = elo::replay_with(&solves, &give_ups, &ratings);

    // If they gave up right now.
    let mut hypothetical = give_ups.clone();
    hypothetical.push(GiveUp { problem_id, at: now });
    let after = elo::replay_with(&solves, &hypothetical, &ratings);
    let preview_change = after.rating.round() as i64 - current.rating.round() as i64;
    let preview_recoverable =
        gain_from_clean_solve(&solves, &hypothetical, &ratings, problem_id, now + GIVE_UP_RESOLVE_GAP_MS + 1, after.rating);

    // As things stand: what a clean solve once the wait is over would add.
    let retry_at = gave_up_at.map(|g| g + GIVE_UP_RESOLVE_GAP_MS);
    let recoverable = gave_up_at.map_or(0, |g| {
        let at = g.max(now) + GIVE_UP_RESOLVE_GAP_MS + 1;
        gain_from_clean_solve(&solves, &give_ups, &ratings, problem_id, at, current.rating)
    });

    Ok(GiveUpInfo {
        gave_up_at,
        retry_at,
        recoverable,
        affects_rating: after.games > current.games,
        preview_change,
        preview_recoverable,
        wait_days: GIVE_UP_RESOLVE_GAP_MS / DAY_MS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        rating::summary,
        reviews as review_log,
        submissions::{self, NewSubmission},
        test_support, Db,
    };
    use tempfile::TempDir;

    const DAY: i64 = DAY_MS;

    fn setup() -> (TempDir, Db) {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        (dir, db)
    }

    fn submit(c: &mut Connection, problem: i64, status: &str) -> submissions::Submission {
        submissions::record(c, NewSubmission { problem_id: problem, lang: "python3".into(), code: "x".into(), status: status.into(), ..Default::default() }).unwrap()
    }

    fn give_up(db: &Db, problem: i64) {
        db.with(|c| record(c, problem, now_ms())).unwrap();
    }

    /// Moves everything stored back in time, so the next thing that happens is "later".
    fn age(c: &Connection, ms: i64) {
        c.execute("UPDATE submissions SET submitted_at = submitted_at - ?1", [ms]).unwrap();
        c.execute("UPDATE reviews SET reviewed_at = reviewed_at - ?1", [ms]).unwrap();
        c.execute("UPDATE give_ups SET gave_up_at = gave_up_at - ?1", [ms]).unwrap();
    }

    fn rating_now(db: &Db) -> i64 {
        db.with(|c| summary(c, now_ms())).unwrap().rating
    }

    fn count(db: &Db, table: &str) -> i64 {
        db.with(|c| Ok(c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)).unwrap()
    }

    #[test]
    fn giving_up_logs_the_give_up_and_an_again_review_with_no_submission() {
        let (_d, db) = setup();
        // However many attempts came first, it's Again.
        db.with(|c| {
            submit(c, 1, "Wrong Answer");
            Ok(())
        })
        .unwrap();
        give_up(&db, 1);
        let (grade, source, submission): (i64, String, Option<String>) = db
            .with(|c| Ok(c.query_row("SELECT grade, grade_source, submission_id FROM reviews", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
            .unwrap();
        assert_eq!((grade, source.as_str(), submission), (1, "inferred", None));
        assert_eq!(count(&db, "give_ups"), 1);
        let linked: i64 = db
            .with(|c| Ok(c.query_row("SELECT COUNT(*) FROM give_ups g JOIN reviews r ON r.id = g.review_id", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(linked, 1, "the give-up points at its review");
    }

    #[test]
    fn a_give_up_with_no_attempts_is_also_again() {
        let (_d, db) = setup();
        give_up(&db, 2);
        let grade: i64 = db.with(|c| Ok(c.query_row("SELECT grade FROM reviews", [], |r| r.get(0))?)).unwrap();
        assert_eq!(grade, 1);
    }

    #[test]
    fn the_memory_model_sees_a_lapse_so_the_problem_comes_back_soon() {
        let (_d, db) = setup();
        give_up(&db, 1);
        let memories = db.with(|c| review_log::memories(c)).unwrap();
        let m = memories.get(&1).expect("a given-up problem has a memory (a lapse)");
        // An Again first review has the shortest stability a grade can give.
        let (_d2, good) = setup();
        good.with(|c| Ok(submit(c, 1, "Accepted"))).unwrap();
        let solved = good.with(|c| review_log::memories(c)).unwrap();
        assert!(m.stability < solved[&1].stability, "{} vs {}", m.stability, solved[&1].stability);
    }

    #[test]
    fn giving_up_twice_in_one_sitting_is_recorded_once() {
        let (_d, db) = setup();
        give_up(&db, 1);
        give_up(&db, 1);
        assert_eq!((count(&db, "give_ups"), count(&db, "reviews")), (1, 1));
        db.with(|c| {
            age(c, 13 * 60 * 60 * 1000);
            Ok(())
        })
        .unwrap();
        give_up(&db, 1);
        assert_eq!((count(&db, "give_ups"), count(&db, "reviews")), (2, 2), "half a day later it's a new one");
        give_up(&db, 2);
        assert_eq!(count(&db, "give_ups"), 3, "other problems are independent");
    }

    #[test]
    fn an_unknown_problem_is_refused_and_nothing_is_written() {
        let (_d, db) = setup();
        let err = db.with(|c| record(c, 12345, now_ms())).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "{err}");
        assert_eq!((count(&db, "give_ups"), count(&db, "reviews")), (0, 0));
    }

    #[test]
    fn it_lowers_the_rating_and_a_later_clean_solve_wins_most_of_it_back() {
        let (_d, db) = setup();
        assert_eq!(rating_now(&db), 1000);
        give_up(&db, 1);
        // Two Sum is the fixture's Easy, rated 1100: 1000 + 40 × (0 − 0.36).
        assert_eq!(rating_now(&db), 986);
        let s = db.with(|c| summary(c, now_ms())).unwrap();
        assert_eq!((s.games, s.history.len()), (1, 2), "one game, and the start plus the loss on the graph");

        db.with(|c| {
            age(c, 4 * DAY);
            let solved = submit(c, 1, "Accepted");
            assert_eq!(solved.review_grade, Some(3), "the failures before the give-up aren't held against the retry");
            Ok(())
        })
        .unwrap();
        // 0.75 × 40 × (1 − 0) more.
        assert_eq!(rating_now(&db), 1016);
        let s = db.with(|c| summary(c, now_ms())).unwrap();
        assert_eq!(s.games, 1, "it's the same game, not a new one");
    }

    #[test]
    fn retyping_the_solution_the_same_day_earns_no_rating_and_no_extra_review() {
        let (_d, db) = setup();
        give_up(&db, 1);
        let before = rating_now(&db);
        let solved = db.with(|c| Ok(submit(c, 1, "Accepted"))).unwrap();
        assert_eq!(solved.review_id, None, "the same sitting as the give-up's review");
        assert_eq!(rating_now(&db), before);
        assert_eq!(count(&db, "reviews"), 1);
        // Still a lapse as far as the memory model is concerned: no stray Good review appeared.
        let m = db.with(|c| review_log::memories(c)).unwrap();
        let (_d2, again_only) = setup();
        give_up(&again_only, 1);
        let reference = again_only.with(|c| review_log::memories(c)).unwrap();
        assert!((m[&1].stability - reference[&1].stability).abs() < 1e-9);
    }

    #[test]
    fn the_wait_is_three_days_not_one() {
        let (_d, db) = setup();
        give_up(&db, 1);
        db.with(|c| {
            age(c, 2 * DAY);
            submit(c, 1, "Accepted");
            Ok(())
        })
        .unwrap();
        assert_eq!(rating_now(&db), 986, "two days later it's too soon");

        let (_d2, later) = setup();
        give_up(&later, 1);
        later
            .with(|c| {
                age(c, 3 * DAY + 1000);
                submit(c, 1, "Accepted");
                Ok(())
            })
            .unwrap();
        assert_eq!(rating_now(&later), 1016);
    }

    #[test]
    fn giving_up_on_a_solved_problem_is_a_review_lapse_only() {
        let (_d, db) = setup();
        db.with(|c| Ok(submit(c, 1, "Accepted"))).unwrap();
        let solved = rating_now(&db);
        db.with(|c| {
            age(c, 30 * DAY);
            Ok(())
        })
        .unwrap();
        give_up(&db, 1);
        assert_eq!(rating_now(&db), solved, "no penalty for forgetting");
        assert_eq!(db.with(|c| summary(c, now_ms())).unwrap().games, 1);
        let m = db.with(|c| review_log::memories(c)).unwrap();
        assert!(m.contains_key(&1));
    }

    #[test]
    fn a_problem_solved_before_reviews_were_logged_still_counts_when_given_up_on_later() {
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
        give_up(&db, 1);
        // The old solve is still the problem's first game (a clean one), so giving up changes nothing.
        let s = db.with(|c| summary(c, now_ms())).unwrap();
        assert_eq!((s.rating, s.games), (1026, 1));
    }

    #[test]
    fn info_before_any_give_up_previews_the_cost_and_the_way_back() {
        let (_d, db) = setup();
        let info = db.with(|c| info(c, 1, now_ms())).unwrap();
        assert_eq!((info.gave_up_at, info.retry_at, info.recoverable), (None, None, 0));
        assert!(info.affects_rating);
        assert_eq!(info.preview_change, -14, "40 × 0.36");
        assert_eq!(info.preview_recoverable, 30, "0.75 × 40");
        assert_eq!(info.wait_days, 3);
        // The serialized shape the frontend reads.
        let json = serde_json::to_value(&info).unwrap();
        for key in ["gaveUpAt", "retryAt", "recoverable", "affectsRating", "previewChange", "previewRecoverable", "waitDays"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn info_for_a_problem_that_already_has_a_result_says_the_rating_is_safe() {
        let (_d, db) = setup();
        db.with(|c| {
            submit(c, 1, "Accepted");
            age(c, DAY); // a day passes: giving up can't happen in the same instant as the solve
            Ok(())
        })
        .unwrap();
        let info = db.with(|c| info(c, 1, now_ms())).unwrap();
        assert_eq!((info.affects_rating, info.preview_change, info.preview_recoverable), (false, 0, 0));
    }

    #[test]
    fn info_after_a_give_up_counts_down_to_when_a_re_solve_counts() {
        let (_d, db) = setup();
        let at = now_ms();
        db.with(|c| record(c, 1, at)).unwrap();
        let info = db.with(|c| info(c, 1, at + 1000)).unwrap();
        assert_eq!(info.gave_up_at, Some(at));
        assert_eq!(info.retry_at, Some(at + 3 * DAY));
        assert_eq!(info.recoverable, 30);
        assert!(!info.affects_rating, "it already has its result: giving up again changes nothing");
        assert_eq!(info.preview_change, 0);
    }

    #[test]
    fn info_stops_reporting_a_give_up_once_the_problem_is_solved_again() {
        let (_d, db) = setup();
        give_up(&db, 1);
        db.with(|c| {
            age(c, 4 * DAY);
            submit(c, 1, "Accepted");
            Ok(())
        })
        .unwrap();
        let info = db.with(|c| info(c, 1, now_ms())).unwrap();
        assert_eq!((info.gave_up_at, info.retry_at, info.recoverable), (None, None, 0));
        // Another problem is unaffected by all this.
        assert!(db.with(|c| super::info(c, 2, now_ms())).unwrap().affects_rating);
    }
}
