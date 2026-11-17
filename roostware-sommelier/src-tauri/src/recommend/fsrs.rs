//! The FSRS-6 memory model: how well the user still remembers a solved problem.
//!
//! Each problem has a *stability* S (days until recall probability falls to 90%) and a
//! *difficulty* D (1..10). A review, graded Again/Hard/Good/Easy, updates both; between reviews
//! the chance of recalling decays along a power curve. Parameters are FSRS-6's published
//! defaults (21 weights, no fitting yet), formulas as on the open-spaced-repetition wiki
//! ("The Algorithm"). The history is logged in `reviews` so the weights can be fitted to the
//! user's own data later.
//!
//! One deliberate addition: after a lapse (Again) stability never ends up above what it was
//! before it, as the reference implementations also enforce.

/// FSRS-6 default weights w0..w20 (w20 is the forgetting curve's decay).
pub const W: [f64; 21] = [
    0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835, 0.0614,
    0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542,
];

/// A solved problem is due for review once the chance of solving it again falls below this.
/// (Lower means fewer, later reviews; FSRS suggests 0.8 to 0.9.)
pub const DESIRED_RETENTION: f64 = 0.85;

const DAY_MS: f64 = 86_400_000.0;
const S_MIN: f64 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    /// Needed the solution, or gave up.
    Again = 1,
    /// Solved, but only with real struggle.
    Hard = 2,
    Good = 3,
    /// Effortless.
    Easy = 4,
}

impl Grade {
    pub fn from_i64(n: i64) -> Option<Grade> {
        match n {
            1 => Some(Grade::Again),
            2 => Some(Grade::Hard),
            3 => Some(Grade::Good),
            4 => Some(Grade::Easy),
            _ => None,
        }
    }

    pub fn as_i64(self) -> i64 {
        self as i64
    }

    fn g(self) -> f64 {
        self as i64 as f64
    }
}

/// What is remembered about one problem after its latest review.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Memory {
    /// Days until recall probability falls to 90%.
    pub stability: f64,
    /// 1 (easy for this user) to 10 (hard).
    pub difficulty: f64,
    /// Epoch milliseconds of the latest review.
    pub last_review_ms: i64,
}

fn decay() -> f64 {
    -W[20]
}

fn factor() -> f64 {
    0.9f64.powf(1.0 / decay()) - 1.0
}

/// Probability of recalling after `elapsed_days` with stability `stability` (R(S, S) = 0.9).
pub fn retrievability(stability: f64, elapsed_days: f64) -> f64 {
    (1.0 + factor() * elapsed_days.max(0.0) / stability).powf(decay())
}

/// Days after a review at which recall probability falls to `retention`.
pub fn interval_days(stability: f64, retention: f64) -> f64 {
    stability / factor() * (retention.powf(1.0 / decay()) - 1.0)
}

fn initial_stability(grade: Grade) -> f64 {
    W[grade as usize - 1].max(S_MIN)
}

fn raw_initial_difficulty(grade: Grade) -> f64 {
    W[4] - (W[5] * (grade.g() - 1.0)).exp() + 1.0
}

fn initial_difficulty(grade: Grade) -> f64 {
    raw_initial_difficulty(grade).clamp(1.0, 10.0)
}

fn next_difficulty(d: f64, grade: Grade) -> f64 {
    let delta = -W[6] * (grade.g() - 3.0);
    let damped = d + delta * (10.0 - d) / 9.0;
    // Mean reversion toward the difficulty of a first "Easy".
    let reverted = W[7] * raw_initial_difficulty(Grade::Easy) + (1.0 - W[7]) * damped;
    reverted.clamp(1.0, 10.0)
}

/// Stability after a review less than a day after the previous one.
fn same_day_stability(s: f64, grade: Grade) -> f64 {
    let mut increase = (W[17] * (grade.g() - 3.0 + W[18])).exp() * s.powf(-W[19]);
    if grade != Grade::Again {
        increase = increase.max(1.0);
    }
    (s * increase).max(S_MIN)
}

fn recall_stability(d: f64, s: f64, r: f64, grade: Grade) -> f64 {
    let hard_penalty = if grade == Grade::Hard { W[15] } else { 1.0 };
    let easy_bonus = if grade == Grade::Easy { W[16] } else { 1.0 };
    let growth = W[8].exp() * (11.0 - d) * s.powf(-W[9]) * ((W[10] * (1.0 - r)).exp() - 1.0) * hard_penalty * easy_bonus;
    (s * (growth + 1.0)).max(S_MIN)
}

fn forget_stability(d: f64, s: f64, r: f64) -> f64 {
    let after = W[11] * d.powf(-W[12]) * ((s + 1.0).powf(W[13]) - 1.0) * (W[14] * (1.0 - r)).exp();
    after.min(s).max(S_MIN)
}

/// The memory after a review at `at_ms` graded `grade`, given the memory before it (if any).
pub fn review(previous: Option<Memory>, at_ms: i64, grade: Grade) -> Memory {
    let Some(m) = previous else {
        return Memory { stability: initial_stability(grade), difficulty: initial_difficulty(grade), last_review_ms: at_ms };
    };
    let elapsed = ((at_ms - m.last_review_ms) as f64 / DAY_MS).max(0.0);
    let stability = if elapsed < 1.0 {
        same_day_stability(m.stability, grade)
    } else {
        let r = retrievability(m.stability, elapsed);
        if grade == Grade::Again {
            forget_stability(m.difficulty, m.stability, r)
        } else {
            recall_stability(m.difficulty, m.stability, r, grade)
        }
    };
    Memory { stability, difficulty: next_difficulty(m.difficulty, grade), last_review_ms: at_ms.max(m.last_review_ms) }
}

/// The memory after a whole history of `(time, grade)` reviews, in any order.
pub fn replay(events: &[(i64, Grade)]) -> Option<Memory> {
    let mut sorted = events.to_vec();
    sorted.sort_by_key(|(at, _)| *at);
    sorted.into_iter().fold(None, |memory, (at, grade)| Some(review(memory, at, grade)))
}

impl Memory {
    /// Chance of recalling this problem at `now_ms`.
    pub fn retrievability_at(&self, now_ms: i64) -> f64 {
        retrievability(self.stability, (now_ms - self.last_review_ms) as f64 / DAY_MS)
    }

    /// When recall probability falls to `retention`, in epoch milliseconds.
    pub fn due_at_ms(&self, retention: f64) -> i64 {
        self.last_review_ms + (interval_days(self.stability, retention) * DAY_MS) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn the_curve_is_ninety_percent_at_one_stability() {
        for s in [0.5, 2.3, 30.0, 400.0] {
            assert!(close(retrievability(s, s), 0.9, 1e-9), "S = {s}");
        }
        assert!(close(retrievability(5.0, 0.0), 1.0, 1e-12));
        assert!(retrievability(5.0, 10.0) < retrievability(5.0, 5.0));
        assert!(retrievability(5.0, 100.0) > 0.0);
    }

    #[test]
    fn intervals_invert_the_curve() {
        for s in [1.0, 7.5, 90.0] {
            assert!(close(interval_days(s, 0.9), s, 1e-9), "90% retention is one stability");
            for r in [0.7, 0.85, 0.95] {
                assert!(close(retrievability(s, interval_days(s, r)), r, 1e-9));
            }
        }
        assert!(interval_days(10.0, 0.8) > interval_days(10.0, 0.9), "accepting more forgetting waits longer");
    }

    #[test]
    fn a_first_review_starts_from_the_default_weights() {
        let m = review(None, 1000, Grade::Good);
        assert!(close(m.stability, 2.3065, 1e-9), "S0(Good) = w2");
        // D0(3) = w4 - e^(w5 * 2) + 1
        assert!(close(m.difficulty, 6.4133 - (0.8334f64 * 2.0).exp() + 1.0, 1e-9));
        assert!(close(m.difficulty, 2.12, 0.02), "about 2.12, got {}", m.difficulty);
        assert!(close(review(None, 0, Grade::Again).stability, 0.212, 1e-9));
        assert!(close(review(None, 0, Grade::Easy).stability, 8.2956, 1e-9));
        assert_eq!(m.last_review_ms, 1000);
        // Harder grades make harder problems, and difficulty stays in range.
        let d = |g| review(None, 0, g).difficulty;
        assert!(d(Grade::Again) > d(Grade::Hard) && d(Grade::Hard) > d(Grade::Good) && d(Grade::Good) > d(Grade::Easy));
        assert!((1.0..=10.0).contains(&d(Grade::Again)) && (1.0..=10.0).contains(&d(Grade::Easy)));
    }

    #[test]
    fn a_successful_review_grows_stability_and_grades_order_the_growth() {
        let first = review(None, 0, Grade::Good);
        let at = (first.stability * DAY as f64) as i64; // reviewed right when it was due
        let [hard, good, easy] = [Grade::Hard, Grade::Good, Grade::Easy].map(|g| review(Some(first), at, g).stability);
        assert!(good > first.stability * 1.5, "Good roughly multiplies stability: {} -> {good}", first.stability);
        assert!(hard > first.stability && hard < good && good < easy, "{hard} < {good} < {easy}");
    }

    #[test]
    fn reviewing_later_than_due_earns_more_than_reviewing_early() {
        let first = review(None, 0, Grade::Good);
        let at = |days: f64| (days * DAY as f64) as i64;
        let early = review(Some(first), at(1.5), Grade::Good).stability;
        let late = review(Some(first), at(8.0), Grade::Good).stability;
        assert!(late > early, "remembering after a long gap says more: early {early}, late {late}");
    }

    #[test]
    fn forgetting_costs_stability_but_never_grows_it() {
        let mut m = review(None, 0, Grade::Easy);
        m = review(Some(m), (m.stability * DAY as f64) as i64, Grade::Good);
        let lapse = review(Some(m), m.last_review_ms + 30 * DAY, Grade::Again);
        assert!(lapse.stability < m.stability, "{} -> {}", m.stability, lapse.stability);
        assert!(lapse.stability >= S_MIN);
        assert!(lapse.difficulty > m.difficulty, "a lapse makes the problem harder for you");
    }

    #[test]
    fn a_review_within_a_day_barely_moves_stability() {
        let first = review(None, 0, Grade::Good);
        let again_soon = review(Some(first), DAY / 4, Grade::Good);
        assert!(again_soon.stability >= first.stability);
        assert!(again_soon.stability < first.stability * 1.3, "{} -> {}", first.stability, again_soon.stability);
        let hard_soon = review(Some(first), DAY / 4, Grade::Hard);
        assert!(hard_soon.stability >= first.stability, "same-day Hard never lowers stability");
    }

    #[test]
    fn difficulty_stays_within_one_to_ten() {
        let mut m = review(None, 0, Grade::Again);
        for i in 1..60 {
            m = review(Some(m), i * 3 * DAY, Grade::Again);
            assert!((1.0..=10.0).contains(&m.difficulty), "{}", m.difficulty);
        }
        let mut m = review(None, 0, Grade::Easy);
        for i in 1..60 {
            m = review(Some(m), i * 3 * DAY, Grade::Easy);
            assert!((1.0..=10.0).contains(&m.difficulty), "{}", m.difficulty);
        }
    }

    #[test]
    fn repeated_good_reviews_space_out_geometrically() {
        let mut m = review(None, 0, Grade::Good);
        let mut gaps = vec![];
        for _ in 0..6 {
            let due = m.due_at_ms(0.9);
            gaps.push(((due - m.last_review_ms) as f64 / DAY as f64).round());
            m = review(Some(m), due, Grade::Good);
        }
        assert!(gaps.windows(2).all(|w| w[1] > w[0]), "intervals only grow: {gaps:?}");
        assert!(*gaps.last().unwrap() > 60.0, "after six clean reviews, months apart: {gaps:?}");
    }

    #[test]
    fn replay_orders_events_and_handles_nothing() {
        assert_eq!(replay(&[]), None);
        let a = replay(&[(0, Grade::Good), (5 * DAY, Grade::Good), (20 * DAY, Grade::Hard)]).unwrap();
        let b = replay(&[(20 * DAY, Grade::Hard), (0, Grade::Good), (5 * DAY, Grade::Good)]).unwrap();
        assert_eq!(a, b, "the order they were logged in doesn't matter");
        assert_eq!(a.last_review_ms, 20 * DAY);
    }

    #[test]
    fn memory_reports_recall_and_due_time() {
        let m = review(None, 0, Grade::Good);
        assert!(close(m.retrievability_at(0), 1.0, 1e-12));
        let due = m.due_at_ms(0.9);
        assert!(close(m.retrievability_at(due), 0.9, 1e-3));
        assert!(m.due_at_ms(DESIRED_RETENTION) > due, "0.85 waits longer than 0.9");
        assert!(m.retrievability_at(-5 * DAY) <= 1.0, "a clock that went backwards doesn't exceed certainty");
    }
}
