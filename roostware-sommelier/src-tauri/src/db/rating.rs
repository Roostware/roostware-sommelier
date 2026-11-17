//! Feeds the overall skill rating (see `recommend::elo`) from the database.
//!
//! Nothing is stored: the rating is replayed from the solves every time, so it always agrees
//! with the review log. Each solve's grade is worked out from the failed attempts that preceded it
//! (see `reviews::inferred_grade`), *not* read from the `reviews` table, so the "How did it go?"
//! picker, which exists for spaced repetition, can't move the rating.

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Serialize;

use super::{reviews, Result};
use crate::recommend::elo::{self, GiveUp, ProblemInfo, Solve, START_RATING};

/// The rating after one game (or one improvement).
#[derive(Debug, Clone, Serialize)]
pub struct RatingPoint {
    /// Epoch milliseconds, UTC.
    pub at: i64,
    pub rating: i64,
}

/// What the dashboard's Rating card shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RatingSummary {
    pub rating: i64,
    /// Problems the rating is based on.
    pub games: i64,
    /// Too few games for the number to mean much yet.
    pub provisional: bool,
    /// How far it moved over the last 7 days.
    pub weekly_change: i64,
    /// The rating over time, oldest first. Empty before the first solve; otherwise the first entry
    /// is the starting rating, stamped with the time of the first solve.
    pub history: Vec<RatingPoint>,
}

/// Every catalog problem, as the rating needs it.
pub(super) fn problem_infos(conn: &Connection) -> Result<Vec<ProblemInfo>> {
    let mut stmt = conn.prepare("SELECT id, difficulty, acceptance_rate FROM catalog.problems")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?)))?;
    let mut infos = Vec::new();
    for row in rows {
        let (id, difficulty, acceptance) = row?;
        let difficulty = match difficulty.as_str() {
            "Easy" => 0,
            "Medium" => 1,
            _ => 2,
        };
        infos.push(ProblemInfo { id, difficulty, acceptance });
    }
    Ok(infos)
}

/// Everything the rating is replayed from: every time the user solved a problem, graded by the
/// failed attempts since the previous review of it, and every time they gave up on one.
pub(super) fn events(conn: &Connection) -> Result<(Vec<Solve>, Vec<GiveUp>)> {
    // Failed attempts per problem, oldest first. Compile errors say nothing about understanding.
    let mut failures: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT problem_id, submitted_at FROM main.submissions
          WHERE deleted_at IS NULL AND status NOT IN ('Accepted', 'Compile Error')
          ORDER BY submitted_at",
    )?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (problem, at) = row?;
        failures.entry(problem).or_default().push(at);
    }

    let mut give_ups = Vec::new();
    let mut gave_up = conn.prepare("SELECT problem_id, gave_up_at FROM main.give_ups WHERE deleted_at IS NULL")?;
    for row in gave_up.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (problem_id, at) = row?;
        give_ups.push(GiveUp { problem_id, at });
    }

    // When each problem was reviewed: every logged review, plus the first Accepted submission of a
    // problem solved before reviews were logged. The flag marks reviews that were give-ups: they
    // start a fresh run of attempts like any review, but they aren't solves.
    let mut times: HashMap<i64, Vec<(i64, bool)>> = HashMap::new();
    let mut logged = conn.prepare(&format!(
        "SELECT problem_id, reviewed_at, NOT ({}) FROM main.reviews WHERE deleted_at IS NULL",
        reviews::NOT_A_SOLVE
    ))?;
    for row in logged.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?)))? {
        let (problem, at, gave_up) = row?;
        times.entry(problem).or_default().push((at, gave_up));
    }
    let mut legacy = conn.prepare(reviews::LEGACY_SOLVES)?;
    for row in legacy.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (problem, at) = row?;
        times.entry(problem).or_default().push((at, false));
    }

    let mut solves = Vec::new();
    for (problem_id, mut list) in times {
        list.sort_unstable();
        let failed = failures.get(&problem_id).map(Vec::as_slice).unwrap_or(&[]);
        let mut previous = 0;
        for (at, was_give_up) in list {
            if !was_give_up {
                // Failures in (previous, at], as `reviews::record_accepted` counts them.
                let count = failed.partition_point(|&t| t <= at) - failed.partition_point(|&t| t <= previous);
                solves.push(Solve { problem_id, at, grade: reviews::inferred_grade(count as i64) });
            }
            previous = at;
        }
    }
    Ok((solves, give_ups))
}

/// The user's rating now and how it got there.
pub fn summary(conn: &Connection, now: i64) -> Result<RatingSummary> {
    let ratings = elo::problem_ratings(&problem_infos(conn)?);
    let (solves, give_ups) = events(conn)?;
    let outcome = elo::replay_with(&solves, &give_ups, &ratings);

    let mut history = Vec::new();
    if let Some(first) = outcome.points.first() {
        history.push(RatingPoint { at: first.at, rating: START_RATING as i64 });
        history.extend(outcome.points.iter().map(|p| RatingPoint { at: p.at, rating: p.rating.round() as i64 }));
    }
    let rating = outcome.rating.round() as i64;
    Ok(RatingSummary {
        rating,
        games: outcome.games as i64,
        provisional: outcome.provisional(),
        // Differences of the rounded numbers, so the card's figures always add up.
        weekly_change: rating - outcome.rating_at(now - 7 * 24 * 60 * 60 * 1000).round() as i64,
        history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        reviews as review_log,
        submissions::{self, NewSubmission},
        test_support, Db,
    };
    use tempfile::TempDir;

    const DAY: i64 = 86_400_000;

    fn setup() -> (TempDir, Db) {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        (dir, db)
    }

    fn submit(c: &mut Connection, problem: i64, status: &str) -> submissions::Submission {
        submissions::record(c, NewSubmission { problem_id: problem, lang: "python3".into(), code: "x".into(), status: status.into(), ..Default::default() }).unwrap()
    }

    /// Moves everything stored back in time so the next submission is "later".
    fn age(c: &Connection, ms: i64) {
        c.execute("UPDATE submissions SET submitted_at = submitted_at - ?1", [ms]).unwrap();
        c.execute("UPDATE reviews SET reviewed_at = reviewed_at - ?1", [ms]).unwrap();
    }

    fn rating_now(db: &Db) -> RatingSummary {
        db.with(|c| summary(c, crate::db::now_ms())).unwrap()
    }

    #[test]
    fn before_any_solve_it_is_the_start_with_no_history() {
        let (_d, db) = setup();
        let s = rating_now(&db);
        assert_eq!((s.rating, s.games, s.provisional, s.weekly_change), (1000, 0, true, 0));
        assert!(s.history.is_empty());
    }

    #[test]
    fn a_clean_first_solve_raises_it_and_starts_the_history_at_the_start_rating() {
        let (_d, db) = setup();
        db.with(|c| Ok(submit(c, 1, "Accepted"))).unwrap();
        let s = rating_now(&db);
        // Two Sum is the fixture's only Easy, so it sits at 1100: 1000 + 40 × (1 − 0.36).
        assert_eq!(s.rating, 1026);
        assert_eq!(s.games, 1);
        assert_eq!(s.weekly_change, 26);
        assert_eq!(s.history.iter().map(|p| p.rating).collect::<Vec<_>>(), [1000, 1026]);
    }

    #[test]
    fn failed_attempts_before_the_solve_lower_the_gain_but_compile_errors_do_not() {
        let (_d, db) = setup();
        db.with(|c| {
            for status in ["Wrong Answer", "Runtime Error", "Time Limit Exceeded"] {
                submit(c, 1, status);
            }
            submit(c, 1, "Accepted");
            Ok(())
        })
        .unwrap();
        // Graded Again: 1000 + 40 × (0.2 − 0.36).
        assert_eq!(rating_now(&db).rating, 994);

        let (_d2, typos) = setup();
        typos
            .with(|c| {
                for _ in 0..5 {
                    submit(c, 1, "Compile Error");
                }
                submit(c, 1, "Accepted");
                Ok(())
            })
            .unwrap();
        assert_eq!(rating_now(&typos).rating, 1026, "typos aren't struggle");
    }

    #[test]
    fn the_how_did_it_go_picker_does_not_move_the_rating() {
        let (_d, db) = setup();
        let s = db.with(|c| Ok(submit(c, 1, "Accepted"))).unwrap();
        let before = rating_now(&db).rating;
        for grade in [1, 4] {
            db.with(|c| review_log::set_grade(c, s.review_id.as_deref().unwrap(), grade)).unwrap();
            assert_eq!(rating_now(&db).rating, before, "grade {grade}");
        }
    }

    #[test]
    fn a_better_solve_later_wins_back_most_of_what_a_bad_first_one_cost() {
        let (_d, db) = setup();
        db.with(|c| {
            submit(c, 1, "Wrong Answer");
            submit(c, 1, "Wrong Answer");
            submit(c, 1, "Wrong Answer");
            submit(c, 1, "Accepted");
            Ok(())
        })
        .unwrap();
        let bad = rating_now(&db).rating;
        db.with(|c| {
            age(c, 30 * DAY);
            submit(c, 1, "Accepted"); // clean this time
            Ok(())
        })
        .unwrap();
        let s = rating_now(&db);
        // 0.75 × 40 × (1 − 0.2) = 24 more.
        assert_eq!(s.rating, 1018, "was {bad}");
        assert_eq!(s.games, 1);
        assert_eq!(s.history.len(), 3, "start, first solve, improvement");
        assert!(s.rating < 1026, "still below what a clean first solve earns");
    }

    #[test]
    fn re_solving_cleanly_again_and_again_earns_nothing_more() {
        let (_d, db) = setup();
        db.with(|c| {
            submit(c, 1, "Accepted");
            Ok(())
        })
        .unwrap();
        let once = rating_now(&db).rating;
        for _ in 0..5 {
            db.with(|c| {
                age(c, 30 * DAY);
                submit(c, 1, "Accepted");
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(rating_now(&db).rating, once);
    }

    #[test]
    fn a_problem_solved_before_reviews_were_logged_still_counts() {
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
        let s = rating_now(&db);
        assert_eq!((s.rating, s.games), (1026, 1));
    }

    #[test]
    fn harder_problems_are_worth_more() {
        let (_d, easy) = setup();
        easy.with(|c| Ok(submit(c, 1, "Accepted"))).unwrap();
        let (_d2, hard) = setup();
        hard.with(|c| Ok(submit(c, 4, "Accepted"))).unwrap();
        assert!(rating_now(&hard).rating > rating_now(&easy).rating);
    }

    /// On the real catalog: each difficulty has its own band, the bands don't overlap, and the
    /// harder-looking problems within a band are rated higher.
    #[test]
    fn real_catalog_problem_ratings_form_three_separate_bands() {
        let Some((_dir, db)) = test_support::open_real_catalog() else {
            eprintln!("skipping: catalog.db not built");
            return;
        };
        let infos = db.with(|c| problem_infos(c)).unwrap();
        let ratings = elo::problem_ratings(&infos);
        assert_eq!(ratings.len(), infos.len());
        let mut bands = [(f64::MAX, f64::MIN, 0usize); 3];
        for p in &infos {
            let r = ratings[&p.id];
            let band = &mut bands[p.difficulty as usize];
            *band = (band.0.min(r), band.1.max(r), band.2 + 1);
        }
        eprintln!("Easy {:?}\nMedium {:?}\nHard {:?}", bands[0], bands[1], bands[2]);
        assert!(bands.iter().all(|b| b.2 > 100), "{bands:?}");
        assert!(bands[0].1 < bands[1].0 && bands[1].1 < bands[2].0, "bands overlap: {bands:?}");
        assert!(bands[0].0 >= 950.0 - 1e-9 && bands[2].1 <= 2350.0 + 1e-9, "{bands:?}");
    }

    /// A perfect solver: every free problem in the catalog first try, easiest first, a day apart.
    /// There's no hard ceiling (a clean solve always pays a little), but the rating climbs ever
    /// more slowly, and the finite catalog puts the practical top near 3000.
    #[test]
    fn real_catalog_a_perfect_solver_keeps_climbing_ever_more_slowly() {
        let Some((_dir, db)) = test_support::open_real_catalog() else {
            eprintln!("skipping: catalog.db not built");
            return;
        };
        let infos = db.with(|c| problem_infos(c)).unwrap();
        let free: std::collections::HashSet<i64> = db
            .with(|c| {
                let mut stmt = c.prepare("SELECT id FROM catalog.problems WHERE is_paid_only = 0")?;
                let ids = stmt.query_map([], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<_>>()?;
                Ok(ids)
            })
            .unwrap();
        let ratings = elo::problem_ratings(&infos);
        let mut order: Vec<(i64, f64)> = ratings.iter().filter(|(id, _)| free.contains(id)).map(|(&id, &r)| (id, r)).collect();
        order.sort_by(|a, b| a.1.total_cmp(&b.1));
        let solves: Vec<Solve> = order
            .iter()
            .enumerate()
            .map(|(i, &(problem_id, _))| Solve { problem_id, at: i as i64 * DAY, grade: crate::recommend::fsrs::Grade::Good })
            .collect();
        let out = elo::replay(&solves, &ratings);
        let at = |n: usize| out.points[(n - 1).min(out.points.len() - 1)].rating.round();
        eprintln!(
            "{} free problems solved first try: after 10 {}, 50 {}, 100 {}, 300 {}, 1000 {}, 2000 {}, all {}",
            out.games, at(10), at(50), at(100), at(300), at(1000), at(2000), out.rating.round()
        );
        assert!(out.rating > 2800.0 && out.rating < 3100.0, "{}", out.rating);
        assert!(out.points.windows(2).all(|w| w[1].rating >= w[0].rating), "clean solves never go backwards");
        assert!(out.points.iter().all(|p| p.rating < 3100.0), "no runaway growth");
    }

    #[test]
    fn the_summary_serializes_for_the_frontend() {
        let (_d, db) = setup();
        db.with(|c| Ok(submit(c, 1, "Accepted"))).unwrap();
        let json = serde_json::to_value(rating_now(&db)).unwrap();
        for key in ["rating", "games", "provisional", "weeklyChange", "history"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        assert!(json["history"][0]["at"].is_number() && json["history"][0]["rating"].is_number());
    }
}
