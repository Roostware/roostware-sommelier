//! Pattern strength and the daily set of recommendations, as pure functions of the problem
//! catalog and the user's history (no database in here, so it can be simulated and tested).
//!
//! **Strength** of a pattern is `Σ weight × recall ÷ target`, capped at 1: each solved problem
//! contributes its weight (harder problems count more; a problem in k patterns counts 1/k in
//! each) times the FSRS chance the user would still solve it today, divided by what a full
//! curated set of that pattern would be worth. So it rises with coverage and falls as
//! problems are forgotten.
//!
//! **A daily set** is a few due reviews (the problems the user is most likely to have
//! forgotten) plus new problems chosen for what the user knows least. See [`recommend`].

use std::collections::{HashMap, HashSet};

use super::{
    fsrs::{Memory, DESIRED_RETENTION},
    patterns::{Curated, PatternSet},
};

const DAY_MS: f64 = 86_400_000.0;

/// How much a solved problem of each difficulty (Easy, Medium, Hard) counts toward strength.
const DIFFICULTY_WEIGHT: [f64; 3] = [0.7, 1.0, 1.3];
/// A pattern's target is at least this much weight, so a pattern with a tiny curated set
/// doesn't read as mastered after a single problem.
const TARGET_FLOOR: f64 = 6.0;
/// A pattern counts as open once every pattern it requires has at least this *coverage* (0..1):
/// the share of its target the user has solved, whether or not they still remember it.
/// Unlocking is deliberately one-way: forgetting is what reviews are for, and it must not take
/// away a pattern the user has already learned their way up to (or make a pattern near the
/// threshold flip between open and locked from one day to the next).
const UNLOCK_AT: f64 = 0.12;
/// What a new problem's score is multiplied by when any of its patterns is still locked.
const LOCKED_FACTOR: f64 = 0.2;

/// A catalog problem, as the engine needs to know it.
#[derive(Debug, Clone)]
pub struct Problem {
    pub id: i64,
    pub frontend_id: i64,
    pub title: String,
    pub slug: String,
    /// 0 Easy, 1 Medium, 2 Hard.
    pub difficulty: u8,
    /// LeetCode Premium: never recommended.
    pub paid: bool,
    /// 0..1.
    pub acceptance: f64,
    pub likes: i64,
    pub dislikes: i64,
    /// Indices into the pattern set; empty for an untagged problem.
    pub patterns: Vec<usize>,
    pub curated: Curated,
}

impl Problem {
    fn weight(&self) -> f64 {
        DIFFICULTY_WEIGHT[self.difficulty.min(2) as usize] / self.patterns.len().max(1) as f64
    }
}

/// What the user has done.
#[derive(Debug, Clone, Default)]
pub struct UserData {
    /// Memory of each solved problem (a problem is solved if it's here).
    pub memories: HashMap<i64, Memory>,
    /// When the user last submitted each problem (any verdict), epoch ms.
    pub last_activity: HashMap<i64, i64>,
    /// Problems started (a submission or a draft) but not solved.
    pub attempted: HashSet<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PatternStat {
    /// 0..1: what the user knows now (solved problems, discounted by how much they've forgotten).
    pub strength: f64,
    /// 0..1: how much of the pattern's target the user has ever solved, forgotten or not. It only
    /// goes up.
    pub coverage: f64,
    /// Solved problems that count toward the pattern.
    pub solved: usize,
    /// Free curated problems in the pattern.
    pub curated_total: usize,
    /// Whether the patterns it requires have been covered enough. Never reverts once true.
    pub unlocked: bool,
    /// The last time the user submitted anything in this pattern.
    pub last_practiced: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    New,
    Review,
}

#[derive(Debug, Clone)]
pub struct Pick {
    pub problem_id: i64,
    pub kind: Kind,
    /// Why it was picked, in a sentence.
    pub reason: String,
    /// The pattern it mainly serves (the user's weakest one it counts toward).
    pub pattern: Option<usize>,
}

/// Strength and bookkeeping for every pattern, in the set's order.
pub fn pattern_stats(set: &PatternSet, problems: &[Problem], user: &UserData, now_ms: i64) -> Vec<PatternStat> {
    let n = set.patterns.len();
    let mut earned = vec![0.0; n];
    let mut covered = vec![0.0; n];
    let mut target = vec![0.0; n];
    let mut solved = vec![0usize; n];
    let mut curated_total = vec![0usize; n];
    let mut last = vec![None::<i64>; n];

    for p in problems {
        let w = p.weight();
        let recall = user.memories.get(&p.id).map(|m| m.retrievability_at(now_ms));
        let activity = user.last_activity.get(&p.id).copied();
        for &i in &p.patterns {
            if p.curated != Curated::None && !p.paid {
                target[i] += w;
                curated_total[i] += 1;
            }
            if let Some(r) = recall {
                earned[i] += w * r;
                covered[i] += w;
                solved[i] += 1;
            }
            if let Some(a) = activity {
                last[i] = Some(last[i].map_or(a, |l| l.max(a)));
            }
        }
    }

    let share = |amount: &[f64], i: usize| (amount[i] / target[i].max(TARGET_FLOOR)).min(1.0);
    let strength: Vec<f64> = (0..n).map(|i| share(&earned, i)).collect();
    let coverage: Vec<f64> = (0..n).map(|i| share(&covered, i)).collect();
    (0..n)
        .map(|i| PatternStat {
            strength: strength[i],
            coverage: coverage[i],
            solved: solved[i],
            curated_total: curated_total[i],
            unlocked: set.patterns[i].requires.iter().all(|&r| coverage[r] >= UNLOCK_AT),
            last_practiced: last[i],
        })
        .collect()
}

// ---- Choosing a daily set -----------------------------------------------------------------

/// Reviews take at most half the day's slots (rounded down) and new problems the rest, so a
/// growing pile of reviews can never crowd out learning. (Rounding up gave a three-a-day learner
/// two reviews every day once a backlog built up, and new problems slowed to about one a day.)
/// With a single slot there is no half: it goes to a review on about every other day, by seed.
fn max_reviews(count: usize, seed: u64) -> usize {
    match count {
        0 => 0,
        1 => (seed & 1) as usize,
        n => n / 2,
    }
}

/// How hard a problem is on a continuous scale: its difficulty, nudged by acceptance rate
/// (a low acceptance rate means harder than its label, a high one easier).
fn difficulty_index(p: &Problem) -> f64 {
    f64::from(p.difficulty) + (0.5 - p.acceptance).clamp(-0.3, 0.3)
}

/// The difficulty to aim for in a pattern: easy where the user has no strength (or the pattern
/// is still locked), rising to hard as strength approaches 0.7.
fn target_difficulty(stat: &PatternStat) -> f64 {
    if !stat.unlocked {
        0.0
    } else {
        2.0 * (stat.strength / 0.7).min(1.0) + 0.3
    }
}

/// Splitmix64: a tiny deterministic generator, so a given day always yields the same set.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A float in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Picks `count` problems for one day: up to half (rounded down) due reviews, then new problems.
///
/// *Reviews* are solved problems whose recall chance has fallen below the desired retention,
/// lowest recall first, avoiding two from the same pattern when there's a choice.
///
/// *New problems* are scored on how much the user needs the pattern (low strength, long since
/// practiced), how well the difficulty fits what they're ready for, a soft preference for the
/// curated lists (Blind 75 over NeetCode 150 over the rest), and a little for how liked the
/// problem is. Locked patterns (prerequisites not yet covered) are strongly deprioritized, so a
/// beginner starts in the foundations, but never excluded, and aim at their easiest problems when
/// they do come up. Picks are sampled from the best few (seeded, for variety), interleaved
/// across patterns, with a second pick allowed in a pattern that is brand new to the user.
/// Paid, untagged and already-solved problems are never picked, nor anything in `exclude`.
pub fn recommend(
    set: &PatternSet,
    problems: &[Problem],
    user: &UserData,
    now_ms: i64,
    count: usize,
    exclude: &HashSet<i64>,
    seed: u64,
) -> Vec<Pick> {
    let stats = pattern_stats(set, problems, user, now_ms);
    let mut picks: Vec<Pick> = Vec::new();
    let mut taken: Vec<usize> = Vec::new(); // patterns already served today

    // ---- Reviews ----
    let mut due: Vec<(&Problem, f64)> = problems
        .iter()
        .filter(|p| !exclude.contains(&p.id))
        .filter_map(|p| {
            let r = user.memories.get(&p.id)?.retrievability_at(now_ms);
            (r < DESIRED_RETENTION).then_some((p, r))
        })
        .collect();
    due.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.id.cmp(&b.0.id)));
    while picks.len() < max_reviews(count, seed) && !due.is_empty() {
        let best = due
            .iter()
            .enumerate()
            .map(|(i, (p, r))| {
                let repeat = p.patterns.iter().any(|x| taken.contains(x));
                (i, (1.0 - r) * if repeat { 0.7 } else { 1.0 })
            })
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|(i, _)| i)
            .unwrap();
        let (p, r) = due.remove(best);
        taken.extend(&p.patterns);
        picks.push(Pick {
            problem_id: p.id,
            kind: Kind::Review,
            reason: format!("Due for review: about {:.0}% chance you'd still solve it", r * 100.0),
            pattern: p.patterns.iter().copied().min_by(|&a, &b| stats[a].strength.total_cmp(&stats[b].strength)),
        });
    }

    // ---- New problems ----
    let mut rng = Rng(seed);
    let mut intro_open: HashSet<usize> = HashSet::new();
    let slots = count.saturating_sub(picks.len());
    let mut chosen: HashSet<i64> = HashSet::new();

    for _ in 0..slots {
        let mut scored: Vec<(&Problem, f64)> = problems
            .iter()
            .filter(|p| !p.paid && !p.patterns.is_empty())
            .filter(|p| !user.memories.contains_key(&p.id) && !exclude.contains(&p.id) && !chosen.contains(&p.id))
            .map(|p| (p, score_new(p, &stats, user, now_ms, &taken, &intro_open)))
            .collect();
        if scored.is_empty() {
            break;
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
        scored.truncate(8);

        // Sample from the best few, favouring the best.
        let top = scored[0].1;
        let weights: Vec<f64> = scored.iter().map(|(_, s)| ((s - top) / 0.05).exp()).collect();
        let mut roll = rng.unit() * weights.iter().sum::<f64>();
        let mut index = scored.len() - 1;
        for (i, w) in weights.iter().enumerate() {
            if roll < *w {
                index = i;
                break;
            }
            roll -= w;
        }
        let p = scored[index].0;

        // The pattern it mainly serves: the weakest one it counts toward.
        let main = p.patterns.iter().copied().min_by(|&a, &b| stats[a].strength.total_cmp(&stats[b].strength)).unwrap();
        let brand_new = stats[main].solved == 0 && stats[main].last_practiced.is_none();
        let name = &set.patterns[main].name;
        let reason = match (brand_new, p.curated.label()) {
            (true, Some(list)) => format!("{list}: a first look at {name}"),
            (true, None) => format!("A first look at {name}"),
            (false, Some(list)) => format!("{list}: builds {name}"),
            (false, None) => format!("Builds {name}"),
        };

        for &x in &p.patterns {
            if intro_open.contains(&x) {
                intro_open.remove(&x); // the second look at a new pattern is used
            } else if stats[x].solved == 0 && stats[x].last_practiced.is_none() && !taken.contains(&x) {
                intro_open.insert(x); // brand new: allow one more here today
            }
        }
        taken.extend(&p.patterns);
        chosen.insert(p.id);
        picks.push(Pick { problem_id: p.id, kind: Kind::New, reason, pattern: Some(main) });
    }
    picks
}

fn score_new(
    p: &Problem,
    stats: &[PatternStat],
    user: &UserData,
    now_ms: i64,
    taken: &[usize],
    intro_open: &HashSet<usize>,
) -> f64 {
    let k = p.patterns.len() as f64;
    let d = difficulty_index(p);
    let (mut gap, mut stale, mut fit) = (0.0, 0.0, 0.0);
    for &i in &p.patterns {
        let s = &stats[i];
        gap += 1.0 - s.strength;
        stale += match s.last_practiced {
            None => 1.0,
            Some(t) => ((now_ms - t) as f64 / DAY_MS / 14.0).clamp(0.0, 1.0),
        };
        let off = d - target_difficulty(s);
        fit += (-(off * off) / (2.0 * 0.6 * 0.6)).exp();
    }
    let (gap, stale, fit) = (gap / k, stale / k, fit / k);

    let curated = match p.curated {
        Curated::Blind75 => 0.30,
        Curated::NeetCode150 => 0.18,
        Curated::None => 0.0,
    };
    let attempted = if user.attempted.contains(&p.id) { 0.08 } else { 0.0 };
    let total_votes = (p.likes + p.dislikes).max(1) as f64;
    let liked = if p.likes + p.dislikes == 0 { 0.5 } else { p.likes as f64 / total_votes };
    let quality = 0.08 * ((liked - 0.6) / 0.4).clamp(0.0, 1.0);

    let base = 0.55 * gap + 0.25 * stale + curated + attempted + quality;
    let mut score = base * (0.35 + 0.65 * fit);

    // Interleave: a pattern already served today is less attractive, except the second look at a
    // pattern that is brand new to the user.
    let mut factor = 1.0_f64;
    for &i in &p.patterns {
        if intro_open.contains(&i) {
            factor *= 1.6;
        } else if taken.contains(&i) {
            factor *= 0.55;
        }
    }
    score *= factor.clamp(0.3, 1.6);

    // The groundwork gate: a problem that belongs to a pattern whose prerequisites aren't covered yet
    // is far less attractive than anything open, so a beginner works through the foundations
    // first. It stays possible (never zero), so a small or exhausted pool, or a user who has
    // already outgrown a prerequisite, isn't stuck.
    if p.patterns.iter().any(|&i| !stats[i].unlocked) {
        score *= LOCKED_FACTOR;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recommend::{
        fsrs::{self, Grade},
        patterns::definitions,
    };

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_800_000_000_000;

    fn pat(id: &str) -> usize {
        definitions().patterns.iter().position(|p| p.id == id).unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn problem(id: i64, diff: u8, patterns: &[&str], curated: Curated, paid: bool) -> Problem {
        Problem {
            id,
            frontend_id: id,
            title: format!("Problem {id}"),
            slug: format!("problem-{id}"),
            difficulty: diff,
            paid,
            acceptance: 0.5,
            likes: 900,
            dislikes: 100,
            patterns: patterns.iter().map(|p| pat(p)).collect(),
            curated,
        }
    }

    /// A small world: a few problems in each of several patterns, easiest first.
    fn world() -> Vec<Problem> {
        let mut v = vec![];
        let mut id = 1;
        for (pattern, curated) in [
            ("arrays-hashing", Curated::Blind75),
            ("two-pointers", Curated::NeetCode150),
            ("graphs", Curated::None),
            ("dynamic-programming", Curated::Blind75),
        ] {
            for diff in [0, 1, 1, 2] {
                v.push(problem(id, diff, &[pattern], curated, false));
                id += 1;
            }
        }
        v
    }

    fn solved(problems: &[(i64, i64, Grade)]) -> UserData {
        let mut u = UserData::default();
        for &(id, at, grade) in problems {
            u.memories.insert(id, fsrs::review(None, at, grade));
            u.last_activity.insert(id, at);
        }
        u
    }

    fn rec(problems: &[Problem], user: &UserData, count: usize, seed: u64) -> Vec<Pick> {
        recommend(definitions(), problems, user, NOW, count, &HashSet::new(), seed)
    }

    #[test]
    fn a_new_user_gets_new_problems_from_the_open_foundations() {
        let w = world();
        let picks = rec(&w, &UserData::default(), 3, 1);
        assert_eq!(picks.len(), 3);
        assert!(picks.iter().all(|p| p.kind == Kind::New));
        let ids: HashSet<_> = picks.iter().map(|p| p.problem_id).collect();
        assert_eq!(ids.len(), 3, "no repeats");
        // Only Arrays & Hashing is unlocked here; everything else is locked, so it asks for easy ones.
        let first = w.iter().find(|p| p.id == picks[0].problem_id).unwrap();
        assert!(first.difficulty <= 1, "starts gently");
    }

    #[test]
    fn nothing_paid_untagged_or_solved_is_ever_picked_as_new() {
        let mut w = world();
        w.push(problem(100, 0, &["arrays-hashing"], Curated::Blind75, true)); // premium
        w.push(Problem { patterns: vec![], ..problem(101, 0, &["arrays-hashing"], Curated::Blind75, false) }); // untagged
        let user = solved(&[(1, NOW - 40 * DAY, Grade::Good)]);
        for seed in 0..40 {
            for p in rec(&w, &user, 6, seed) {
                if p.kind == Kind::New {
                    assert!(![100, 101, 1].contains(&p.problem_id), "picked {}", p.problem_id);
                }
            }
        }
    }

    #[test]
    fn curated_problems_are_preferred_but_not_required() {
        // Two otherwise identical Arrays problems; one is on Blind 75.
        let w = vec![
            problem(1, 0, &["arrays-hashing"], Curated::None, false),
            problem(2, 0, &["arrays-hashing"], Curated::Blind75, false),
        ];
        let mut picked_curated = 0;
        for seed in 0..60 {
            if rec(&w, &UserData::default(), 1, seed)[0].problem_id == 2 {
                picked_curated += 1;
            }
        }
        assert!(picked_curated > 45, "the curated one wins most of the time ({picked_curated}/60)");

        // Once the curated one is solved (recently, so it isn't due), the rest is still recommended.
        let user = solved(&[(2, NOW - DAY / 4, Grade::Good)]);
        assert_eq!(rec(&w, &user, 1, 1)[0].problem_id, 1);
    }

    #[test]
    fn due_reviews_come_first_most_forgotten_first_and_are_capped_at_half_rounded_down() {
        let w = world();
        // Three solved long ago, differing in how forgotten they are.
        let user = solved(&[(1, NOW - 200 * DAY, Grade::Good), (5, NOW - 60 * DAY, Grade::Good), (9, NOW - 30 * DAY, Grade::Good)]);
        let picks = rec(&w, &user, 3, 3);
        let kinds: Vec<_> = picks.iter().map(|p| p.kind).collect();
        assert_eq!(kinds, [Kind::Review, Kind::New, Kind::New], "a third of three slots, rounded down to one");
        assert_eq!(picks[0].problem_id, 1, "the one most likely forgotten");
        assert!(picks[0].reason.contains("Due for review"));
        let count = |n| rec(&w, &user, n, 3).iter().filter(|p| p.kind == Kind::Review).count();
        assert_eq!((count(2), count(4), count(5), count(6)), (1, 2, 2, 3));
        assert_eq!(count(10), 3, "and never more than are due");
    }

    #[test]
    fn with_one_slot_a_review_gets_about_every_other_day() {
        let w = world();
        let user = solved(&[(1, NOW - 200 * DAY, Grade::Good)]);
        let reviews = (0..200).filter(|&seed| rec(&w, &user, 1, seed)[0].kind == Kind::Review).count();
        assert!((60..=140).contains(&reviews), "{reviews} of 200 days");
        let none_due = UserData::default();
        assert!((0..50).all(|seed| rec(&w, &none_due, 1, seed)[0].kind == Kind::New), "and only when one is due");
    }

    #[test]
    fn nothing_is_reviewed_until_it_is_due() {
        let w = world();
        let user = solved(&[(1, NOW - DAY / 2, Grade::Good), (2, NOW - 2 * DAY, Grade::Good)]);
        assert!(rec(&w, &user, 3, 1).iter().all(|p| p.kind == Kind::New), "recently solved problems aren't due");
    }

    #[test]
    fn a_pattern_the_user_has_never_touched_gets_its_turn() {
        let w = world();
        // Strong at arrays, nothing else.
        let history: Vec<_> = (1..=4).map(|id| (id, NOW - 3 * DAY, Grade::Good)).collect();
        let user = solved(&history);
        let mut seen = HashSet::new();
        for seed in 0..30 {
            for p in rec(&w, &user, 3, seed) {
                seen.insert(w.iter().find(|x| x.id == p.problem_id).unwrap().patterns[0]);
            }
        }
        assert!(!seen.contains(&pat("arrays-hashing")) || seen.len() > 1);
        assert!(seen.contains(&pat("two-pointers")), "the newly unlocked pattern is recommended: {seen:?}");
    }

    #[test]
    fn locked_patterns_only_get_their_easiest_problems() {
        // Graphs needs trees and recursion, which this user has no strength in.
        let w = vec![
            problem(1, 0, &["graphs"], Curated::None, false),
            problem(2, 2, &["graphs"], Curated::None, false),
            problem(3, 1, &["arrays-hashing"], Curated::None, false),
        ];
        let stats = pattern_stats(definitions(), &w, &UserData::default(), NOW);
        assert!(!stats[pat("graphs")].unlocked);
        assert!(stats[pat("arrays-hashing")].unlocked);
        for seed in 0..30 {
            assert_ne!(rec(&w, &UserData::default(), 1, seed)[0].problem_id, 2, "no hard graph problem for a beginner");
        }
    }

    #[test]
    fn a_beginner_is_kept_in_the_foundations_but_a_locked_pattern_is_still_possible() {
        // A tempting easy Blind 75 problem in a locked pattern, among plain foundation problems.
        let mut w = vec![problem(500, 0, &["dynamic-programming"], Curated::Blind75, false)];
        for id in 1..=12 {
            w.push(problem(id, 1, &["arrays-hashing"], Curated::None, false));
        }
        for seed in 0..100 {
            assert!(
                rec(&w, &UserData::default(), 3, seed).iter().all(|p| p.problem_id != 500),
                "seed {seed}: a beginner was sent to dynamic programming"
            );
        }
        // Nothing else to do: the locked problem is better than an empty day.
        let only = vec![problem(500, 0, &["dynamic-programming"], Curated::Blind75, false)];
        assert_eq!(rec(&only, &UserData::default(), 3, 1).len(), 1);
        // Once the groundwork is down it is an ordinary candidate.
        let history: Vec<_> = (1..=10).map(|id| (id, NOW - DAY, Grade::Good)).collect();
        let stats = pattern_stats(definitions(), &w, &solved(&history), NOW);
        assert!(stats[pat("arrays-hashing")].unlocked);
    }

    #[test]
    fn picks_are_interleaved_across_patterns() {
        // Two open patterns with plenty in each.
        let mut w = vec![];
        for id in 1..=8 {
            w.push(problem(id, 1, &["arrays-hashing"], Curated::None, false));
            w.push(problem(100 + id, 1, &["math-bits"], Curated::None, false));
        }
        let user = UserData::default();
        let mut same_pattern = 0;
        for seed in 0..40 {
            let picks = rec(&w, &user, 3, seed);
            let pats: Vec<_> = picks.iter().map(|p| w.iter().find(|x| x.id == p.problem_id).unwrap().patterns[0]).collect();
            if pats.iter().collect::<HashSet<_>>().len() == 1 {
                same_pattern += 1;
            }
        }
        assert!(same_pattern < 8, "three from the same pattern is rare ({same_pattern}/40)");
    }

    #[test]
    fn a_brand_new_pattern_may_get_a_pair_on_its_first_day() {
        let w = world();
        let mut pairs = 0;
        for seed in 0..40 {
            let picks = rec(&w, &UserData::default(), 2, seed);
            let same = w.iter().find(|x| x.id == picks[0].problem_id).unwrap().patterns
                == w.iter().find(|x| x.id == picks[1].problem_id).unwrap().patterns;
            pairs += same as usize;
        }
        assert!(pairs > 10, "an intro pair happens regularly for a new pattern ({pairs}/40)");
    }

    #[test]
    fn the_same_seed_gives_the_same_set_and_other_seeds_vary() {
        let w = world();
        let user = UserData::default();
        let a: Vec<_> = rec(&w, &user, 3, 42).iter().map(|p| p.problem_id).collect();
        assert_eq!(a, rec(&w, &user, 3, 42).iter().map(|p| p.problem_id).collect::<Vec<_>>());
        let variety: HashSet<Vec<i64>> = (0..30).map(|s| rec(&w, &user, 3, s).iter().map(|p| p.problem_id).collect()).collect();
        assert!(variety.len() > 3, "different days differ: {}", variety.len());
    }

    #[test]
    fn excluded_problems_are_skipped_and_a_small_pool_just_runs_out() {
        let w = vec![problem(1, 0, &["arrays-hashing"], Curated::None, false), problem(2, 0, &["arrays-hashing"], Curated::None, false)];
        let exclude: HashSet<i64> = [1].into();
        let picks = recommend(definitions(), &w, &UserData::default(), NOW, 5, &exclude, 1);
        assert_eq!(picks.iter().map(|p| p.problem_id).collect::<Vec<_>>(), [2], "only what's left, however many were asked for");
        assert!(recommend(definitions(), &[], &UserData::default(), NOW, 3, &HashSet::new(), 1).is_empty());
        assert!(rec(&w, &UserData::default(), 0, 1).is_empty());
    }

    #[test]
    fn strength_grows_with_solves_and_fades_with_forgetting() {
        let w = world();
        let set = definitions();
        let at = |u: &UserData, now| pattern_stats(set, &w, u, now)[pat("arrays-hashing")].strength;
        assert_eq!(at(&UserData::default(), NOW), 0.0);
        let one = solved(&[(1, NOW, Grade::Good)]);
        let three = solved(&[(1, NOW, Grade::Good), (2, NOW, Grade::Good), (3, NOW, Grade::Good)]);
        assert!(at(&one, NOW) > 0.0 && at(&three, NOW) > at(&one, NOW));
        assert!(at(&three, NOW + 300 * DAY) < at(&three, NOW) * 0.6, "months later most of it has faded");
        let stat = &pattern_stats(set, &w, &three, NOW)[pat("arrays-hashing")];
        assert_eq!((stat.solved, stat.curated_total), (3, 4));
        assert_eq!(stat.last_practiced, Some(NOW));
        assert!(stat.strength <= 1.0);
    }

    #[test]
    fn a_pattern_with_few_curated_problems_isnt_mastered_by_one_solve() {
        let w = vec![problem(1, 1, &["trees"], Curated::Blind75, false)];
        let user = solved(&[(1, NOW, Grade::Easy)]);
        let s = pattern_stats(definitions(), &w, &user, NOW)[pat("trees")].strength;
        assert!(s < 0.25, "the target has a floor: {s}");
    }

    #[test]
    fn unlocking_is_one_way_forgetting_never_locks_a_pattern_again() {
        // One Medium solved: just over the unlock threshold (1 of a target floor of 6 is 0.17), so
        // it's the case where memory fading would, if it counted, take it back under.
        let w = world();
        let set = definitions();
        let user = solved(&[(2, NOW, Grade::Good)]);
        let at = |days: i64| pattern_stats(set, &w, &user, NOW + days * DAY);

        let fresh = at(0);
        assert!(fresh[pat("arrays-hashing")].coverage >= UNLOCK_AT);
        assert!(fresh[pat("two-pointers")].unlocked, "solving a bit of Arrays opens Two Pointers");

        let mut coverage_before = 0.0;
        let mut strength_before = f64::MAX;
        for days in [1, 10, 30, 100, 365, 3 * 365, 20 * 365] {
            let s = at(days);
            let arrays = &s[pat("arrays-hashing")];
            assert!(arrays.coverage >= coverage_before, "coverage only ever rises");
            assert!(arrays.strength <= strength_before, "memory only fades");
            (coverage_before, strength_before) = (arrays.coverage, arrays.strength);
            assert!(s[pat("two-pointers")].unlocked, "{days} days on, Two Pointers locked again");
            assert!(s[pat("sliding-window")].unlocked == (s[pat("two-pointers")].coverage >= UNLOCK_AT));
        }
        let faded = at(100);
        assert!(
            faded[pat("arrays-hashing")].strength < UNLOCK_AT,
            "by 100 days the memory has dropped below the threshold ({}), which used to re-lock",
            faded[pat("arrays-hashing")].strength
        );
        assert!(faded[pat("arrays-hashing")].strength < faded[pat("arrays-hashing")].coverage, "strength fades; coverage doesn't");
    }

    #[test]
    fn a_faded_prerequisite_still_leaves_dependents_unlocked_in_recommendations() {
        // Two Pointers problems are plentiful; one Arrays problem solved long ago and mostly forgotten.
        let mut w: Vec<Problem> = (1..=6).map(|id| problem(id, 1, &["arrays-hashing"], Curated::None, false)).collect();
        w.extend((10..=30).map(|id| problem(id, 0, &["two-pointers"], Curated::Blind75, false)));
        let user = solved(&[(1, NOW - 20_000 * DAY, Grade::Good)]);
        let stats = pattern_stats(definitions(), &w, &user, NOW);
        assert!(stats[pat("arrays-hashing")].strength < UNLOCK_AT, "forgotten: {}", stats[pat("arrays-hashing")].strength);
        assert!(stats[pat("two-pointers")].unlocked);
        let picks: Vec<_> = (0..30).flat_map(|seed| rec(&w, &user, 3, seed)).filter(|p| p.kind == Kind::New).collect();
        assert!(picks.iter().all(|p| p.problem_id >= 10), "only the unsolved ones are new");
        assert!(!picks.is_empty());
    }

    #[test]
    fn unlocking_follows_the_prerequisites() {
        let w = world();
        let none = pattern_stats(definitions(), &w, &UserData::default(), NOW);
        assert!(none[pat("arrays-hashing")].unlocked && !none[pat("two-pointers")].unlocked);
        let history: Vec<_> = (1..=4).map(|id| (id, NOW, Grade::Good)).collect();
        let some = pattern_stats(definitions(), &w, &solved(&history), NOW);
        assert!(some[pat("two-pointers")].unlocked, "arrays strength {}", some[pat("arrays-hashing")].strength);
        assert!(!some[pat("sliding-window")].unlocked, "needs two pointers first");
    }
}
