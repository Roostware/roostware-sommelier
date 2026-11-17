//! The overall skill rating: a rough, motivating Elo-style number for "how hard a problem can
//! this user solve", as a pure function of the problem catalog and the user's solves.
//!
//! The user plays against problems whose ratings are fixed (from difficulty and acceptance rate);
//! only the user's rating moves. Each problem is one *game*, played the first time it's solved:
//!
//! * the **score** comes from how the solve went: [`score`] gives 0.2 for Again (3+ failed
//!   attempts), 0.55 for Hard (1 or 2) and 1.0 for Good (none), a full win as in chess. Grades
//!   here come only from failed attempts, never from the user's own override, so the rating
//!   can't be talked up;
//! * the **change** is `K × (score − expected)`, where expected is the usual Elo expectation, so
//!   the same result is worth more against a problem that's hard for the user and less against
//!   one that's easy for them. A clean solve is a full win and expected is always below 1, so it
//!   can never lower the rating; it just pays less and less as problems fall far below the
//!   user. There is no hard ceiling, only that tapering (and a finite catalog);
//! * **K** shrinks as games accumulate (40, then 24, then 16): quick to find the level, steadier
//!   after that.
//!
//! Solving a problem *better* later earns part of the difference back: a re-solve with a better
//! grade than the problem's credited one adds `RECOVERY × K × (new score − credited score)` (K as
//! at the first solve), and the credited score becomes the new one. Re-solves that aren't better,
//! or come within a day of the previous solve of that problem, change nothing, and a worse
//! re-solve never lowers the rating (forgetting is what pattern strength and reviews are for).
//! Improvements add up the same however they are split, and each problem can give back at most
//! `RECOVERY × K × (1 − first score)` in all (nothing at all after a clean first solve), so
//! re-solving can't be farmed.
//!
//! **Giving up** on a problem that has no result yet is the problem's game, scored as a plain loss
//! ([`GIVE_UP_SCORE`], 0: not Again's 0.2, which would *raise* the rating against a problem far
//! above the user). A later solve is then a recovery on that same game, so most of the loss can be
//! won back, but only [`GIVE_UP_RESOLVE_GAP_MS`] after the give-up, since the solution was just
//! seen. Giving up on a problem that already has a result changes nothing (forgetting is free),
//! though it does restart that wait.

use std::collections::HashMap;

use super::fsrs::Grade;

/// Where everyone starts.
pub const START_RATING: f64 = 1000.0;
/// A problem's rating before acceptance: Easy, Medium, Hard.
const BASE: [f64; 3] = [1100.0, 1600.0, 2200.0];
/// Acceptance moves a problem this far either way within its difficulty: the lowest acceptance
/// rate is `BASE + SPREAD`, the highest `BASE - SPREAD`.
const ACCEPTANCE_SPREAD: f64 = 150.0;
/// The share of the score difference a better re-solve earns (at the original K).
pub const RECOVERY: f64 = 0.75;
/// A re-solve counts only this long after the previous solve of the same problem.
const RESOLVE_GAP_MS: i64 = 24 * 60 * 60 * 1000;
/// After a give-up the wait is longer: the solution was just shown, so an immediate retype proves nothing.
pub const GIVE_UP_RESOLVE_GAP_MS: i64 = 3 * 24 * 60 * 60 * 1000;
/// What giving up counts as: a loss.
pub const GIVE_UP_SCORE: f64 = 0.0;
/// Until this many games, the rating is shown as provisional.
pub const PROVISIONAL_GAMES: usize = 10;
const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// A catalog problem as the rating needs it.
#[derive(Debug, Clone, Copy)]
pub struct ProblemInfo {
    pub id: i64,
    /// 0 Easy, 1 Medium, 2 Hard.
    pub difficulty: u8,
    /// 0..1.
    pub acceptance: f64,
}

/// A problem the user solved: the first time, or a later one. `grade` is from failed attempts.
#[derive(Debug, Clone, Copy)]
pub struct Solve {
    pub problem_id: i64,
    pub at: i64,
    pub grade: Grade,
}

/// The user gave up on a problem (and was shown its solution).
#[derive(Debug, Clone, Copy)]
pub struct GiveUp {
    pub problem_id: i64,
    pub at: i64,
}

/// The rating after a game (or an improvement).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub at: i64,
    pub rating: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub rating: f64,
    /// Problems played (first solves).
    pub games: usize,
    /// Every change, oldest first; the starting rating isn't one of them.
    pub points: Vec<Point>,
}

impl Outcome {
    pub fn provisional(&self) -> bool {
        self.games < PROVISIONAL_GAMES
    }

    /// The rating as it stood at `t`.
    pub fn rating_at(&self, t: i64) -> f64 {
        self.points.iter().take_while(|p| p.at <= t).last().map_or(START_RATING, |p| p.rating)
    }

    /// How far the rating has moved over the 7 days up to `now`.
    pub fn weekly_change(&self, now: i64) -> f64 {
        self.rating - self.rating_at(now - WEEK_MS)
    }
}

/// Each problem's fixed rating: its difficulty's base, moved up (down) by how low (high) its
/// acceptance rate ranks among problems of the same difficulty.
pub fn problem_ratings(problems: &[ProblemInfo]) -> HashMap<i64, f64> {
    let acceptance = |p: &ProblemInfo| if p.acceptance.is_finite() { p.acceptance } else { 0.0 };
    let mut by_difficulty: [Vec<f64>; 3] = Default::default();
    for p in problems {
        by_difficulty[p.difficulty.min(2) as usize].push(acceptance(p));
    }
    for list in &mut by_difficulty {
        list.sort_by(f64::total_cmp);
    }
    problems
        .iter()
        .map(|p| {
            let d = p.difficulty.min(2) as usize;
            let list = &by_difficulty[d];
            let a = acceptance(p);
            // Rank in 0..1 (ties share the middle of their run); a lone problem is mid-range.
            let rank = if list.len() < 2 {
                0.5
            } else {
                let below = list.partition_point(|&x| x < a);
                let through = list.partition_point(|&x| x <= a);
                (below + through - 1) as f64 / 2.0 / (list.len() - 1) as f64
            };
            (p.id, BASE[d] + ACCEPTANCE_SPREAD * (1.0 - 2.0 * rank))
        })
        .collect()
}

/// What a solve counts as, 0..1.
pub fn score(grade: Grade) -> f64 {
    match grade {
        Grade::Again => 0.2,
        Grade::Hard => 0.55,
        Grade::Good => 1.0,
        // Never produced by failed attempts, and no better than a clean solve.
        Grade::Easy => 1.0,
    }
}

/// The Elo expectation: the chance-like score the user is expected to get against a problem.
pub fn expected(rating: f64, problem: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf((problem - rating) / 400.0))
}

/// How big a step is, given how many games came before.
fn k_for(games: usize) -> f64 {
    match games {
        0..=14 => 40.0,
        15..=59 => 24.0,
        _ => 16.0,
    }
}

/// The rating change for a game that came to `score`.
fn game_delta(rating: f64, problem: f64, k: f64, score: f64) -> f64 {
    k * (score - expected(rating, problem))
}

/// What the problem has been credited so far.
struct Credit {
    score: f64,
    /// The K in force at the first result.
    k: f64,
    /// The latest result that was taken into account.
    last_at: i64,
    /// How long after `last_at` a re-solve starts to count.
    gap: i64,
}

/// One thing that happened to a problem: solved, or given up on.
#[derive(Clone, Copy)]
struct Event {
    problem_id: i64,
    at: i64,
    /// `None` for a give-up.
    grade: Option<Grade>,
}

/// Plays `solves` (any order) forward from the starting rating. Solves of problems not in
/// `ratings` are ignored.
pub fn replay(solves: &[Solve], ratings: &HashMap<i64, f64>) -> Outcome {
    replay_with(solves, &[], ratings)
}

/// Like [`replay`], with the times the user gave up on a problem.
pub fn replay_with(solves: &[Solve], give_ups: &[GiveUp], ratings: &HashMap<i64, f64>) -> Outcome {
    let mut events: Vec<Event> = solves
        .iter()
        .map(|s| Event { problem_id: s.problem_id, at: s.at, grade: Some(s.grade) })
        .chain(give_ups.iter().map(|g| Event { problem_id: g.problem_id, at: g.at, grade: None }))
        .collect();
    // At the same instant a give-up comes first: a solve right after seeing the solution is the weaker one.
    events.sort_by_key(|e| (e.at, e.problem_id, e.grade.is_some()));

    let mut out = Outcome { rating: START_RATING, games: 0, points: Vec::new() };
    let mut credits: HashMap<i64, Credit> = HashMap::new();
    for e in events {
        let Some(&problem) = ratings.get(&e.problem_id) else { continue };
        let new_score = e.grade.map_or(GIVE_UP_SCORE, score);
        let gap = if e.grade.is_some() { RESOLVE_GAP_MS } else { GIVE_UP_RESOLVE_GAP_MS };
        match credits.get_mut(&e.problem_id) {
            None => {
                let k = k_for(out.games);
                out.rating += game_delta(out.rating, problem, k, new_score);
                out.games += 1;
                credits.insert(e.problem_id, Credit { score: new_score, k, last_at: e.at, gap });
            }
            Some(credit) => {
                if e.grade.is_none() {
                    // Never changes the rating, but the solution was just seen: the wait starts over.
                    credit.last_at = e.at;
                    credit.gap = gap;
                    continue;
                }
                if e.at - credit.last_at < credit.gap {
                    continue;
                }
                credit.last_at = e.at;
                credit.gap = gap;
                if new_score <= credit.score {
                    continue;
                }
                out.rating += RECOVERY * credit.k * (new_score - credit.score);
                credit.score = new_score;
            }
        }
        out.points.push(Point { at: e.at, rating: out.rating });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    fn info(id: i64, difficulty: u8, acceptance: f64) -> ProblemInfo {
        ProblemInfo { id, difficulty, acceptance }
    }

    fn solve(problem_id: i64, day: i64, grade: Grade) -> Solve {
        Solve { problem_id, at: day * DAY, grade }
    }

    /// Problem 1 is an Easy rated 1100, 2 a Medium rated 1600, 3 a Hard rated 2200.
    fn flat() -> HashMap<i64, f64> {
        HashMap::from([(1, 1100.0), (2, 1600.0), (3, 2200.0), (4, 1100.0), (5, 1100.0)])
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn problem_ratings_follow_difficulty_and_acceptance() {
        let ratings = problem_ratings(&[
            info(1, 0, 0.30), // hardest-looking Easy
            info(2, 0, 0.50),
            info(3, 0, 0.80), // friendliest Easy
            info(4, 1, 0.45),
            info(5, 2, 0.30),
        ]);
        assert!(close(ratings[&1], 1250.0), "{}", ratings[&1]);
        assert!(close(ratings[&2], 1100.0));
        assert!(close(ratings[&3], 950.0));
        assert!(close(ratings[&4], 1600.0), "a lone Medium sits at its base");
        assert!(close(ratings[&5], 2200.0));
        // Difficulty always beats acceptance: the easiest Medium is above the hardest Easy.
        assert!(1600.0 - ACCEPTANCE_SPREAD > 1100.0 + ACCEPTANCE_SPREAD);
    }

    #[test]
    fn equal_acceptance_rates_share_a_rating() {
        let ratings = problem_ratings(&[info(1, 1, 0.4), info(2, 1, 0.4), info(3, 1, 0.4)]);
        assert!(close(ratings[&1], 1600.0) && close(ratings[&2], 1600.0) && close(ratings[&3], 1600.0));
        let junk = problem_ratings(&[info(1, 1, f64::NAN), info(2, 1, 0.5)]);
        assert!(junk[&1].is_finite() && junk[&2].is_finite());
    }

    #[test]
    fn the_expectation_is_the_elo_curve() {
        assert!(close(expected(1500.0, 1500.0), 0.5));
        assert!((expected(1000.0, 1100.0) - 0.36).abs() < 0.001);
        assert!((expected(1300.0, 1100.0) - 0.76).abs() < 0.001);
        assert!(expected(1300.0, 1600.0) < 0.16);
    }

    #[test]
    fn easy_problems_pay_little_and_cost_a_lot_while_hard_ones_do_the_opposite() {
        // A user at 1300, K = 24: an Easy-for-them problem (1100) against a Hard-for-them one (1600).
        let at = |problem, grade| game_delta(1300.0, problem, 24.0, score(grade));
        assert!((at(1100.0, Grade::Again) + 13.4).abs() < 0.1, "{}", at(1100.0, Grade::Again));
        assert!((at(1100.0, Grade::Hard) + 5.0).abs() < 0.1);
        assert!((at(1100.0, Grade::Good) - 5.8).abs() < 0.1, "a clean solve of an easy problem pays a little");
        assert!((at(1600.0, Grade::Good) - 20.4).abs() < 0.1);
        assert!((at(1600.0, Grade::Hard) - 9.6).abs() < 0.1);
        assert!(at(1600.0, Grade::Again) >= 0.0, "struggling through a much harder problem never costs anything");
        // The same result is worth more the harder the problem is for the user.
        assert!(at(1600.0, Grade::Good) > at(1300.0, Grade::Good) && at(1300.0, Grade::Good) > at(1100.0, Grade::Good));
    }

    #[test]
    fn a_clean_solve_always_pays_something_and_never_lowers_the_rating() {
        for rating in [800.0, 1300.0, 2000.0, 3000.0] {
            for problem in [900.0, 1100.0, 1600.0, 2200.0, 2500.0] {
                for k in [16.0, 24.0, 40.0] {
                    for grade in [Grade::Good, Grade::Easy] {
                        assert!(game_delta(rating, problem, k, score(grade)) > 0.0, "{rating} vs {problem}, K {k}, {grade:?}");
                    }
                }
            }
        }
        // ...but a problem far below you pays next to nothing.
        assert!(game_delta(2000.0, 1100.0, 16.0, score(Grade::Good)) < 0.5);
    }

    #[test]
    fn a_first_game_moves_a_new_user_by_the_big_k() {
        let out = replay(&[solve(1, 0, Grade::Good)], &flat());
        // expected 0.36 against the Easy; 40 × (1 − 0.36)
        assert!((out.rating - 1025.6).abs() < 0.1, "{}", out.rating);
        assert_eq!(out.games, 1);
        assert_eq!(out.points.len(), 1);
        assert_eq!(out.points[0].at, 0);
        let nothing = replay(&[], &flat());
        assert_eq!((nothing.rating, nothing.games, nothing.points.len()), (START_RATING, 0, 0));
    }

    #[test]
    fn k_shrinks_as_games_accumulate() {
        assert_eq!((k_for(0), k_for(14)), (40.0, 40.0));
        assert_eq!((k_for(15), k_for(59)), (24.0, 24.0));
        assert_eq!((k_for(60), k_for(500)), (16.0, 16.0));
    }

    #[test]
    fn the_order_the_solves_are_given_in_does_not_matter() {
        let a = [solve(1, 0, Grade::Good), solve(2, 1, Grade::Hard), solve(3, 2, Grade::Again)];
        let b = [a[2], a[0], a[1]];
        assert_eq!(replay(&a, &flat()), replay(&b, &flat()));
    }

    #[test]
    fn solves_of_unknown_problems_are_ignored() {
        let out = replay(&[solve(99, 0, Grade::Good)], &flat());
        assert_eq!((out.games, out.points.len(), out.rating), (0, 0, START_RATING));
    }

    #[test]
    fn a_better_re_solve_earns_part_of_the_gap_back() {
        // Struggle through the Easy, then solve it clean a month later.
        let first = replay(&[solve(1, 0, Grade::Again)], &flat()).rating;
        assert!((first - 993.6).abs() < 0.1, "{first}");
        let later = replay(&[solve(1, 0, Grade::Again), solve(1, 30, Grade::Good)], &flat());
        // 0.75 × 40 × (1 − 0.2)
        assert!(close(later.rating, first + 24.0), "{}", later.rating);
        assert_eq!(later.games, 1, "a re-solve isn't a new game");
        assert_eq!(later.points.len(), 2);
        // Less than a clean first solve would have given (the discount keeps first tries worth more)...
        let clean = replay(&[solve(1, 0, Grade::Good)], &flat()).rating;
        assert!(later.rating < clean);
        // ...but most of the way there.
        assert!(later.rating - first > 0.7 * (clean - first));
    }

    #[test]
    fn improving_in_steps_adds_up_to_the_same_as_in_one_go() {
        let steps = replay(&[solve(1, 0, Grade::Again), solve(1, 10, Grade::Hard), solve(1, 20, Grade::Good)], &flat());
        let jump = replay(&[solve(1, 0, Grade::Again), solve(1, 20, Grade::Good)], &flat());
        assert!(close(steps.rating, jump.rating), "{} vs {}", steps.rating, jump.rating);
    }

    #[test]
    fn re_solving_cannot_be_farmed() {
        let base = replay(&[solve(1, 0, Grade::Good)], &flat()).rating;
        // Two hundred daily re-solves, one of them Easy (a grade failed attempts never produce, but
        // the replay must hold up even so).
        let solves: Vec<Solve> =
            (0..200).map(|d| solve(1, d, if d == 100 { Grade::Easy } else { Grade::Good })).collect();
        let out = replay(&solves, &flat());
        // A clean first solve is already a full win: there is nothing left to earn from the problem.
        assert!(close(out.rating, base), "{}", out.rating - base);
        assert_eq!(out.points.len(), 1);
        // After a struggle the most one problem can ever give back is bounded by its first score,
        // however many times it is solved.
        let mut after_struggle = vec![solve(1, 0, Grade::Again)];
        after_struggle.extend((1..200).map(|d| solve(1, d, Grade::Good)));
        let bounded = replay(&after_struggle, &flat()).rating - replay(&[solve(1, 0, Grade::Again)], &flat()).rating;
        assert!(close(bounded, RECOVERY * 40.0 * (1.0 - 0.2)), "{bounded}");
    }

    #[test]
    fn a_worse_re_solve_costs_nothing() {
        let good = replay(&[solve(1, 0, Grade::Good)], &flat());
        let forgot = replay(&[solve(1, 0, Grade::Good), solve(1, 60, Grade::Again)], &flat());
        assert_eq!(good.rating, forgot.rating);
        assert_eq!(forgot.points.len(), 1);
        // And the credit stays at Good, so a later clean solve doesn't earn anything either.
        let again = replay(&[solve(1, 0, Grade::Good), solve(1, 60, Grade::Again), solve(1, 90, Grade::Good)], &flat());
        assert_eq!(good.rating, again.rating);
    }

    #[test]
    fn a_re_solve_within_a_day_of_the_last_one_does_not_count() {
        let hours = |h: i64| h * 3_600_000;
        let early = [Solve { problem_id: 1, at: 0, grade: Grade::Again }, Solve { problem_id: 1, at: hours(20), grade: Grade::Good }];
        assert_eq!(replay(&early, &flat()).points.len(), 1, "bombing it and redoing it that evening earns nothing");
        let later = [early[0], Solve { problem_id: 1, at: hours(25), grade: Grade::Good }];
        assert_eq!(replay(&later, &flat()).points.len(), 2);
    }

    #[test]
    fn recovery_uses_the_k_from_the_first_solve() {
        // Twenty other Easy problems first push K down from 40 to 24 for later games.
        let mut ratings = flat();
        let mut solves = vec![solve(1, 0, Grade::Again)];
        for i in 0..20 {
            ratings.insert(100 + i, 1100.0);
            solves.push(solve(100 + i, 1 + i, Grade::Good));
        }
        let before = replay(&solves, &ratings).rating;
        solves.push(solve(1, 60, Grade::Good));
        let after = replay(&solves, &ratings).rating;
        assert!(close(after - before, RECOVERY * 40.0 * 0.8), "{}", after - before);
    }

    #[test]
    fn the_rating_follows_effort_over_a_realistic_climb() {
        // Clean solves up the ladder keep raising it; farming easy problems does not.
        let mut climb = vec![];
        for (i, p) in [1, 4, 5, 2, 3].into_iter().enumerate() {
            climb.push(solve(p, i as i64, Grade::Good));
        }
        let ratings = flat();
        let out = replay(&climb, &ratings);
        assert!(out.rating > 1050.0);
        assert!(out.points.windows(2).all(|w| w[1].rating >= w[0].rating), "clean solves never go backwards");

        let mut easy_only = ratings.clone();
        let mut solves = vec![];
        for i in 0..100 {
            easy_only.insert(1000 + i, 1100.0);
            solves.push(solve(1000 + i, i, Grade::Good));
        }
        let out = replay(&solves, &easy_only);
        let first_ten = out.points[9].rating - START_RATING;
        let last_ten = out.rating - out.points[89].rating;
        assert!(last_ten < first_ten / 4.0, "easy problems pay less and less: {first_ten} for the first ten, {last_ten} for the last");
    }

    #[test]
    fn provisional_until_ten_games_and_the_weekly_change_reads_the_history() {
        let mut ratings = flat();
        let mut solves = vec![];
        for i in 0..9 {
            ratings.insert(100 + i, 1600.0);
            solves.push(solve(100 + i, i, Grade::Good));
        }
        let nine = replay(&solves, &ratings);
        assert!(nine.provisional());
        ratings.insert(200, 1600.0);
        solves.push(solve(200, 9, Grade::Good));
        let ten = replay(&solves, &ratings);
        assert!(!ten.provisional());

        // Day 9 is "now": the week before, the rating was whatever it was at the end of day 2.
        let now = 9 * DAY;
        assert!(close(ten.rating_at(2 * DAY), ten.points[2].rating));
        assert!(close(ten.weekly_change(now), ten.rating - ten.points[2].rating));
        assert_eq!(ten.rating_at(-1), START_RATING, "before any game it's the start");
        // A user whose first game was this week gains everything from the start.
        let fresh = replay(&[solve(1, 9, Grade::Good)], &ratings);
        assert!(close(fresh.weekly_change(now), fresh.rating - START_RATING));
    }

    // ---- Giving up ----

    fn give_up(problem_id: i64, day: f64) -> GiveUp {
        GiveUp { problem_id, at: (day * DAY as f64) as i64 }
    }

    #[test]
    fn giving_up_is_a_loss_and_uses_a_game() {
        let out = replay_with(&[], &[give_up(1, 0.0)], &flat());
        // expected 0.36 against the Easy; 40 × (0 − 0.36)
        assert!((out.rating - 985.6).abs() < 0.1, "{}", out.rating);
        assert_eq!((out.games, out.points.len()), (1, 1));
    }

    #[test]
    fn giving_up_never_raises_the_rating_even_against_a_far_harder_problem() {
        // Scored as Again (0.2) it would pay about 40 × 0.2 = +8 against a 2200 problem.
        let again = replay(&[solve(3, 0, Grade::Again)], &flat()).rating;
        assert!(again > START_RATING, "the premise: Again against a much harder problem pays");
        let gave_up = replay_with(&[], &[give_up(3, 0.0)], &flat()).rating;
        assert!(gave_up < START_RATING && gave_up > START_RATING - 1.0, "a hard problem barely costs anything: {gave_up}");
        // And it costs more the easier the problem is for the user.
        let easy = replay_with(&[], &[give_up(1, 0.0)], &flat()).rating;
        let medium = replay_with(&[], &[give_up(2, 0.0)], &flat()).rating;
        assert!(easy < medium && medium < gave_up);
    }

    #[test]
    fn a_later_clean_solve_wins_back_most_of_a_give_up() {
        let loss = replay_with(&[], &[give_up(1, 0.0)], &flat()).rating;
        let later = replay_with(&[solve(1, 5, Grade::Good)], &[give_up(1, 0.0)], &flat());
        // 0.75 × 40 × (1 − 0): more than the loss itself, so you end up ahead of where you began...
        assert!(close(later.rating, loss + 30.0), "{}", later.rating);
        assert!(later.rating > START_RATING);
        // ...but below a clean first solve, and it's the same game, not a new one.
        let clean = replay(&[solve(1, 0, Grade::Good)], &flat()).rating;
        assert!(later.rating < clean);
        assert_eq!((later.games, later.points.len()), (1, 2));
    }

    #[test]
    fn re_solving_after_a_give_up_in_steps_adds_up_to_the_same_as_in_one_go() {
        let steps = replay_with(&[solve(1, 5, Grade::Again), solve(1, 10, Grade::Hard), solve(1, 15, Grade::Good)], &[give_up(1, 0.0)], &flat());
        let jump = replay_with(&[solve(1, 15, Grade::Good)], &[give_up(1, 0.0)], &flat());
        assert!(close(steps.rating, jump.rating), "{} vs {}", steps.rating, jump.rating);
    }

    #[test]
    fn a_re_solve_has_to_wait_three_days_after_a_give_up() {
        let at = |day: f64| replay_with(&[solve_at(1, day, Grade::Good)], &[give_up(1, 0.0)], &flat()).points.len();
        assert_eq!(at(0.5), 1, "retyping it the same day earns nothing");
        assert_eq!(at(1.5), 1, "a day is the normal wait, but not after a give-up");
        assert_eq!(at(2.9), 1);
        assert_eq!(at(3.0), 2);
        assert_eq!(at(10.0), 2);
    }

    fn solve_at(problem_id: i64, day: f64, grade: Grade) -> Solve {
        Solve { problem_id, at: (day * DAY as f64) as i64, grade }
    }

    #[test]
    fn giving_up_again_costs_nothing_more_but_restarts_the_wait() {
        let once = replay_with(&[], &[give_up(1, 0.0)], &flat());
        let twice = replay_with(&[], &[give_up(1, 0.0), give_up(1, 2.0)], &flat());
        assert!(close(once.rating, twice.rating));
        assert_eq!((twice.games, twice.points.len()), (1, 1));
        // Two days after the second give-up isn't enough; three is.
        let early = replay_with(&[solve(1, 4, Grade::Good)], &[give_up(1, 0.0), give_up(1, 2.0)], &flat());
        assert_eq!(early.points.len(), 1);
        let later = replay_with(&[solve(1, 5, Grade::Good)], &[give_up(1, 0.0), give_up(1, 2.0)], &flat());
        assert_eq!(later.points.len(), 2);
    }

    #[test]
    fn giving_up_on_a_problem_already_solved_changes_nothing() {
        let solved = replay(&[solve(1, 0, Grade::Good)], &flat());
        let forgot = replay_with(&[solve(1, 0, Grade::Good)], &[give_up(1, 60.0)], &flat());
        assert_eq!(solved, forgot, "no penalty for forgetting, and no extra game");

        // A struggle (credited Again) followed by a give-up leaves the credit alone too: a later
        // clean solve still recovers what it always could, once the wait is over.
        let base = replay(&[solve(1, 0, Grade::Again)], &flat()).rating;
        let after = replay_with(&[solve(1, 0, Grade::Again), solve(1, 30, Grade::Good)], &[give_up(1, 10.0)], &flat()).rating;
        assert!(close(after - base, RECOVERY * 40.0 * 0.8), "{}", after - base);
    }

    #[test]
    fn a_give_up_after_a_solve_still_restarts_the_wait_for_a_better_one() {
        let attempts = [solve(1, 0, Grade::Again), solve(1, 11, Grade::Good)];
        let blocked = replay_with(&attempts, &[give_up(1, 10.0)], &flat());
        assert_eq!(blocked.points.len(), 1, "a day after the give-up isn't a real re-solve");
        let allowed = replay_with(&[attempts[0], solve(1, 14, Grade::Good)], &[give_up(1, 10.0)], &flat());
        assert_eq!(allowed.points.len(), 2);
    }

    #[test]
    fn give_ups_are_ordered_by_time_not_by_how_they_are_given() {
        let solves = [solve(1, 5, Grade::Good), solve(2, 6, Grade::Hard)];
        let gave_up = [give_up(2, 1.0), give_up(1, 0.0)];
        let reversed = [gave_up[1], gave_up[0]];
        assert_eq!(replay_with(&solves, &gave_up, &flat()), replay_with(&solves, &reversed, &flat()));
    }

    #[test]
    fn give_ups_of_unknown_problems_are_ignored() {
        let out = replay_with(&[], &[give_up(99, 0.0)], &flat());
        assert_eq!((out.games, out.points.len(), out.rating), (0, 0, START_RATING));
    }
}
