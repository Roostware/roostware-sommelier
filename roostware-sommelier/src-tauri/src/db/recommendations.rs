//! Feeds the recommendation engine from the database and stores what it produces.
//!
//! The daily set is generated the first time it's asked for on a given (local) day and then
//! kept, so it doesn't reshuffle when the dashboard is reopened or after solving something;
//! items are shown as done once the user has an Accepted submission for them since the set
//! was made. If the user raises the daily count mid-day the set is topped up, never reshuffled.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};
use serde::Serialize;

use super::{reviews, settings, Error, Result};
use crate::recommend::{
    engine::{self, Kind, Problem, UserData},
    patterns::{definitions, PatternSet},
};

/// One pattern's strength for the dashboard's radar chart and bars.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternStrength {
    pub id: String,
    pub name: String,
    /// A label short enough for a chart axis.
    pub short: String,
    /// Learning order: 0 is the foundations.
    pub tier: u8,
    /// 0 to 100.
    pub value: i64,
    /// Problems solved that count toward it.
    pub solved: i64,
    /// Free curated (Blind 75 / NeetCode 150) problems in it: what a full score is measured against.
    pub curated_total: i64,
    /// Whether its prerequisites have been covered enough (solved, whether or not still remembered). Once true it stays true.
    pub unlocked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    New,
    Review,
}

/// One recommended problem.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyItem {
    /// LeetCode's internal question id (what the rest of the app opens problems by).
    pub problem_id: i64,
    /// The number shown on leetcode.com.
    pub frontend_id: i64,
    pub title: String,
    pub title_slug: String,
    pub difficulty: String,
    pub kind: ItemKind,
    /// Why it's here, in a sentence.
    pub reason: String,
    /// The pattern it mainly serves.
    pub pattern: Option<String>,
    /// "Blind 75" or "NeetCode 150" when it's on one of the curated lists.
    pub curated: Option<String>,
    /// When the user last solved it (epoch ms), for reviews.
    pub last_solved_at: Option<i64>,
    /// Solved since this set was made.
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailySet {
    /// The local date it is for, YYYY-MM-DD.
    pub day: String,
    pub count: i64,
    pub items: Vec<DailyItem>,
}

// ---- Loading what the engine needs ---------------------------------------------------------

/// Every catalog problem with its patterns worked out from its tags.
pub fn load_problems(conn: &Connection, set: &PatternSet) -> Result<Vec<Problem>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.frontend_id, p.title, p.title_slug, p.difficulty, p.is_paid_only, p.acceptance_rate,
                p.likes, p.dislikes,
                (SELECT group_concat(t.name, char(31)) FROM catalog.problem_tags pt
                   JOIN catalog.tags t ON t.id = pt.tag_id WHERE pt.problem_id = p.id)
           FROM catalog.problems p ORDER BY p.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)? != 0,
            r.get::<_, f64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, Option<String>>(9)?,
        ))
    })?;
    let mut problems = Vec::new();
    for row in rows {
        let (id, frontend_id, title, slug, difficulty, paid, acceptance, likes, dislikes, tags) = row?;
        let tags: Vec<String> = tags.map(|t| t.split('\u{1f}').map(str::to_owned).collect()).unwrap_or_default();
        problems.push(Problem {
            id,
            frontend_id,
            patterns: set.classify(&tags, &slug),
            curated: set.curated(&slug),
            difficulty: match difficulty.as_str() {
                "Easy" => 0,
                "Medium" => 1,
                _ => 2,
            },
            title,
            slug,
            paid,
            acceptance,
            likes,
            dislikes,
        });
    }
    Ok(problems)
}

/// What the user has solved (and how well it's remembered), and where they've been working.
pub fn load_user(conn: &Connection) -> Result<UserData> {
    let memories = reviews::memories(conn)?;

    let mut last_activity = HashMap::new();
    let mut stmt = conn.prepare("SELECT problem_id, MAX(submitted_at) FROM main.submissions WHERE deleted_at IS NULL GROUP BY problem_id")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (problem, at) = row?;
        last_activity.insert(problem, at);
    }

    let mut started: HashSet<i64> = last_activity.keys().copied().collect();
    let mut drafted = conn.prepare("SELECT DISTINCT problem_id FROM main.drafts")?;
    for row in drafted.query_map([], |r| r.get::<_, i64>(0))? {
        started.insert(row?);
    }
    let attempted = started.into_iter().filter(|id| !memories.contains_key(id)).collect();

    Ok(UserData { memories, last_activity, attempted })
}

// ---- Pattern strength ----------------------------------------------------------------------

pub fn pattern_strengths(conn: &Connection, now: i64) -> Result<Vec<PatternStrength>> {
    let set = definitions();
    let stats = engine::pattern_stats(set, &load_problems(conn, set)?, &load_user(conn)?, now);
    Ok(set
        .patterns
        .iter()
        .zip(stats)
        .map(|(p, s)| PatternStrength {
            id: p.id.clone(),
            name: p.name.clone(),
            short: p.short.clone(),
            tier: p.tier,
            value: (s.strength * 100.0).round() as i64,
            solved: s.solved as i64,
            curated_total: s.curated_total as i64,
            unlocked: s.unlocked,
        })
        .collect())
}

// ---- The daily set -------------------------------------------------------------------------

fn valid_day(day: &str) -> bool {
    let b = day.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

/// FNV-1a: a stable hash of the date, so each day has its own variety and the same day always
/// gives the same picks.
fn seed_for(day: &str) -> u64 {
    day.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

fn kind_str(kind: Kind) -> &'static str {
    match kind {
        Kind::New => "new",
        Kind::Review => "review",
    }
}

/// The day's recommendations, made now if there aren't any yet. `day` is the user's local date
/// (YYYY-MM-DD); the caller knows their time zone, the database doesn't.
pub fn daily_set(conn: &mut Connection, day: &str, now: i64) -> Result<DailySet> {
    if !valid_day(day) {
        return Err(Error::Invalid(format!("{day:?} is not a date like 2026-11-13")));
    }
    let set = definitions();
    let count = settings::daily_count(conn)?;
    let problems = load_problems(conn, set)?;

    let existing: Option<i64> = conn
        .query_row("SELECT created_at FROM main.daily_sets WHERE day = ?", [day], |r| r.get(0))
        .map(Some)
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })?;
    let mut stored = stored_items(conn, day)?;

    if existing.is_none() || stored.len() < count {
        let user = load_user(conn)?;
        let exclude: HashSet<i64> = stored.iter().map(|(id, ..)| *id).collect();
        let wanted = count.saturating_sub(stored.len());
        let picks = engine::recommend(set, &problems, &user, now, wanted, &exclude, seed_for(day) ^ stored.len() as u64);

        let tx = conn.transaction()?;
        if existing.is_none() {
            tx.execute("INSERT INTO main.daily_sets (day, count, created_at) VALUES (?1, ?2, ?3)", params![day, count as i64, now])?;
        } else {
            tx.execute("UPDATE main.daily_sets SET count = ?2 WHERE day = ?1", params![day, count as i64])?;
        }
        for pick in &picks {
            let position = stored.len() as i64;
            let pattern = pick.pattern.map(|i| set.patterns[i].id.clone());
            tx.execute(
                "INSERT INTO main.daily_set_items (day, position, problem_id, kind, reason, pattern) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![day, position, pick.problem_id, kind_str(pick.kind), pick.reason, pattern],
            )?;
            stored.push((pick.problem_id, kind_str(pick.kind).to_owned(), pick.reason.clone(), pattern));
        }
        tx.commit()?;
    }

    let created_at: i64 = conn.query_row("SELECT created_at FROM main.daily_sets WHERE day = ?", [day], |r| r.get(0))?;
    let (solved_since, last_solved) = solve_times(conn, created_at)?;

    let by_id: HashMap<i64, &Problem> = problems.iter().map(|p| (p.id, p)).collect();
    let items: Vec<DailyItem> = stored
        .into_iter()
        .filter_map(|(problem_id, kind, reason, pattern)| {
            let p = by_id.get(&problem_id)?;
            Some(DailyItem {
                problem_id,
                frontend_id: p.frontend_id,
                title: p.title.clone(),
                title_slug: p.slug.clone(),
                difficulty: ["Easy", "Medium", "Hard"][p.difficulty.min(2) as usize].to_owned(),
                kind: if kind == "review" { ItemKind::Review } else { ItemKind::New },
                reason,
                pattern: pattern.and_then(|id| set.patterns.iter().find(|x| x.id == id)).map(|x| x.name.clone()),
                curated: p.curated.label().map(str::to_owned),
                last_solved_at: last_solved.get(&problem_id).copied(),
                done: solved_since.contains(&problem_id),
            })
        })
        .collect();
    Ok(DailySet { day: day.to_owned(), count: count as i64, items: trim_to(items, count) })
}

/// Shows only `count` of a day's items when the setting was lowered after the set was made.
/// Solved ones are kept first, then the earliest of the rest, all in their original order. The
/// others stay stored, so raising the count again brings the same problems back.
fn trim_to(items: Vec<DailyItem>, count: usize) -> Vec<DailyItem> {
    if items.len() <= count {
        return items;
    }
    let mut keep = vec![false; items.len()];
    let order = (0..items.len()).filter(|&i| items[i].done).chain((0..items.len()).filter(|&i| !items[i].done));
    for i in order.take(count) {
        keep[i] = true;
    }
    items.into_iter().zip(keep).filter_map(|(item, keep)| keep.then_some(item)).collect()
}

type StoredItem = (i64, String, String, Option<String>);

fn stored_items(conn: &Connection, day: &str) -> Result<Vec<StoredItem>> {
    let mut stmt = conn.prepare("SELECT problem_id, kind, reason, pattern FROM main.daily_set_items WHERE day = ? ORDER BY position")?;
    let rows = stmt
        .query_map([day], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Problems with an Accepted submission since `since`, and when each was last solved.
fn solve_times(conn: &Connection, since: i64) -> Result<(HashSet<i64>, HashMap<i64, i64>)> {
    let mut last = HashMap::new();
    let mut stmt = conn.prepare("SELECT problem_id, MAX(submitted_at) FROM main.submissions WHERE status = 'Accepted' AND deleted_at IS NULL GROUP BY problem_id")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (problem, at) = row?;
        last.insert(problem, at);
    }
    let done = last.iter().filter(|(_, &at)| at >= since).map(|(&id, _)| id).collect();
    Ok((done, last))
}

/// A set with just these picks, for tests that want to inspect the engine's choices by id.
#[cfg(test)]
pub fn picks_of(set: &DailySet) -> Vec<(i64, bool)> {
    set.items.iter().map(|i| (i.problem_id, matches!(i.kind, ItemKind::Review))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        submissions::{self, NewSubmission},
        test_support, Db,
    };
    use crate::recommend::fsrs::{self, Grade};
    use tempfile::TempDir;

    const DAY: i64 = 86_400_000;

    fn setup() -> (TempDir, Db) {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        (dir, db)
    }

    fn solve(db: &Db, problem: i64) {
        db.with(|c| {
            submissions::record(c, NewSubmission { problem_id: problem, lang: "python3".into(), code: "x".into(), status: "Accepted".into(), ..Default::default() })
                .map(|_| ())
        })
        .unwrap();
    }

    fn day_set(db: &Db, day: &str, now: i64) -> DailySet {
        db.with(|c| daily_set(c, day, now)).unwrap()
    }

    #[test]
    fn day_strings_are_validated() {
        for ok in ["2026-11-13", "1999-01-01"] {
            assert!(valid_day(ok), "{ok}");
        }
        for bad in ["", "2026-1-13", "2026/11/13", "2026-11-13T00", "abcd-ef-gh", "20261113", "2026-11-1x"] {
            assert!(!valid_day(bad), "{bad}");
        }
        let (_d, db) = setup();
        assert!(matches!(db.with(|c| daily_set(c, "yesterday", 0)), Err(Error::Invalid(_))));
    }

    #[test]
    fn the_first_visit_of_a_day_makes_a_set_and_later_ones_reuse_it() {
        let (_d, db) = setup();
        let now = 1_800_000_000_000;
        let first = day_set(&db, "2026-11-13", now);
        // The fixture catalog has three free problems (the fourth is Premium), all untouched.
        assert_eq!(first.items.len(), 3);
        assert!(first.items.iter().all(|i| matches!(i.kind, ItemKind::New) && !i.done));
        assert!(first.items.iter().all(|i| i.problem_id != 900), "the Premium problem is never recommended");
        assert_eq!(first.count, 3);
        let again = day_set(&db, "2026-11-13", now + 5 * 3_600_000);
        assert_eq!(picks_of(&first), picks_of(&again), "it doesn't reshuffle");
        let reasons: Vec<_> = first.items.iter().map(|i| i.reason.as_str()).collect();
        assert!(reasons.iter().all(|r| !r.is_empty()), "{reasons:?}");
        assert!(first.items.iter().all(|i| i.pattern.is_some()));
    }

    #[test]
    fn solving_a_recommendation_marks_it_done_without_changing_the_set() {
        let (_d, db) = setup();
        let now = crate::db::now_ms();
        let first = day_set(&db, "2026-11-13", now);
        let target = first.items[0].problem_id;
        solve(&db, target);
        let after = day_set(&db, "2026-11-13", now);
        assert_eq!(picks_of(&first), picks_of(&after), "same problems, same order");
        let done: Vec<_> = after.items.iter().filter(|i| i.done).map(|i| i.problem_id).collect();
        assert_eq!(done, [target]);
        // Tomorrow's set no longer offers it as new.
        let tomorrow = day_set(&db, "2026-11-14", now + DAY);
        assert!(tomorrow.items.iter().all(|i| i.problem_id != target || matches!(i.kind, ItemKind::Review)));
    }

    #[test]
    fn a_solve_from_before_the_set_existed_is_not_marked_done() {
        let (_d, db) = setup();
        solve(&db, 1);
        let now = crate::db::now_ms() + 60_000;
        let set = day_set(&db, "2026-11-13", now);
        assert!(set.items.iter().all(|i| !i.done));
    }

    #[test]
    fn raising_the_daily_count_tops_up_the_set_and_lowering_it_trims_it() {
        let (_d, db) = setup();
        let now = 1_800_000_000_000;
        db.with(|c| settings::set_daily_count(c, 1)).unwrap();
        let one = day_set(&db, "2026-11-13", now);
        assert_eq!(one.items.len(), 1);
        db.with(|c| settings::set_daily_count(c, 3)).unwrap();
        let three = day_set(&db, "2026-11-13", now);
        assert_eq!(three.items.len(), 3);
        assert_eq!(three.items[0].problem_id, one.items[0].problem_id, "what was there stays, in place");
        let ids: HashSet<_> = three.items.iter().map(|i| i.problem_id).collect();
        assert_eq!(ids.len(), 3, "no repeats");
        db.with(|c| settings::set_daily_count(c, 2)).unwrap();
        let two = day_set(&db, "2026-11-13", now);
        assert_eq!(picks_of(&two), picks_of(&three)[..2], "the set is cut down, keeping the first ones");
        assert_eq!(two.count, 2);
        assert_eq!(day_set(&db, "2026-11-14", now + DAY).items.len(), 2, "the next day uses the new count");
        db.with(|c| settings::set_daily_count(c, 3)).unwrap();
        assert_eq!(picks_of(&day_set(&db, "2026-11-13", now)), picks_of(&three), "raising it again brings the same problems back");
    }

    #[test]
    fn lowering_the_count_keeps_the_problems_already_solved() {
        let (_d, db) = setup();
        let now = crate::db::now_ms();
        let three = day_set(&db, "2026-11-13", now);
        let last = three.items[2].problem_id;
        solve(&db, last);
        db.with(|c| settings::set_daily_count(c, 1)).unwrap();
        let one = day_set(&db, "2026-11-13", now);
        assert_eq!(one.items.len(), 1);
        assert_eq!((one.items[0].problem_id, one.items[0].done), (last, true), "the solved one stays over earlier unsolved ones");
        db.with(|c| settings::set_daily_count(c, 2)).unwrap();
        let two = day_set(&db, "2026-11-13", now);
        assert_eq!(two.items.iter().map(|i| i.problem_id).collect::<Vec<_>>(), [three.items[0].problem_id, last], "original order");
    }

    #[test]
    fn a_problem_due_for_review_is_recommended_as_a_review_with_its_last_solved_date() {
        let (_d, db) = setup();
        solve(&db, 1);
        let solved_at = db.with(|c| Ok(c.query_row("SELECT MAX(submitted_at) FROM submissions", [], |r| r.get::<_, i64>(0))?)).unwrap();
        // Not due the next day...
        let soon = day_set(&db, "2026-11-13", solved_at + DAY / 2);
        assert!(soon.items.iter().all(|i| matches!(i.kind, ItemKind::New)));
        // ...but due a couple of months later.
        let later = day_set(&db, "2027-01-20", solved_at + 70 * DAY);
        let review = later.items.iter().find(|i| matches!(i.kind, ItemKind::Review)).expect("a review");
        assert_eq!(review.problem_id, 1);
        assert_eq!(review.last_solved_at, Some(solved_at));
        assert!(review.reason.contains("Due for review"));
        assert_eq!(later.items[0].problem_id, 1, "reviews come first");
    }

    #[test]
    fn the_set_for_a_day_is_the_same_whenever_it_is_made() {
        let (_d1, a) = setup();
        let (_d2, b) = setup();
        let now = 1_800_000_000_000;
        assert_eq!(picks_of(&day_set(&a, "2026-11-13", now)), picks_of(&day_set(&b, "2026-11-13", now + 9 * 3_600_000)));
    }

    #[test]
    fn pattern_strengths_cover_all_seventeen_and_rise_with_solves() {
        let (_d, db) = setup();
        let now = crate::db::now_ms();
        let before = db.with(|c| pattern_strengths(c, now)).unwrap();
        assert_eq!(before.len(), 17);
        assert!(before.iter().all(|p| p.value == 0 && p.solved == 0));
        assert!(before.iter().any(|p| p.unlocked) && before.iter().any(|p| !p.unlocked));
        assert_eq!(before[0].id, "arrays-hashing");
        solve(&db, 1); // Two Sum: Array + Hash Table
        let after = db.with(|c| pattern_strengths(c, now)).unwrap();
        let arrays = &after[0];
        assert_eq!(arrays.solved, 1);
        assert!(arrays.value > 0 && arrays.value <= 100, "{}", arrays.value);
        let json = serde_json::to_value(arrays).unwrap();
        assert!(json["curatedTotal"].is_number() && json["unlocked"].is_boolean());
    }

    /// Months of practice on the real catalog: someone who does every recommendation, every day.
    /// Checks the whole system behaves: nothing illegal is ever picked, curated problems come
    /// first, reviews don't crowd out new learning, and every pattern is eventually reached.
    #[test]
    fn a_simulated_learner_reaches_every_pattern_on_the_real_catalog() {
        let Some((_dir, db)) = test_support::open_real_catalog() else {
            eprintln!("skipping: catalog.db not built");
            return;
        };
        const HOUR: i64 = 3_600_000;
        let set = definitions();
        let problems = db.with(|c| load_problems(c, set)).unwrap();
        let by_id: HashMap<i64, &Problem> = problems.iter().map(|p| (p.id, p)).collect();
        let start = 1_800_000_000_000_i64;
        let days = 150;

        let mut user = UserData::default();
        let mut seen_new: HashSet<i64> = HashSet::new();
        let mut first_solved: HashMap<usize, i64> = HashMap::new();
        let (mut new_early, mut curated_early, mut reviews_total, mut picks_total) = (0, 0, 0, 0);
        let mut new_by_day: Vec<usize> = vec![];

        for day in 0..days {
            let now = start + day * 24 * HOUR + 9 * HOUR;
            let picks = engine::recommend(set, &problems, &user, now, 3, &HashSet::new(), seed_for(&format!("sim-{day}")));
            assert!(!picks.is_empty() && picks.len() <= 3, "day {day}: {} picks", picks.len());
            let mut new_today = 0;
            for (i, pick) in picks.iter().enumerate() {
                let p = by_id[&pick.problem_id];
                picks_total += 1;
                match pick.kind {
                    Kind::New => {
                        assert!(!p.paid, "day {day}: premium problem {} recommended", p.title);
                        assert!(!p.patterns.is_empty(), "day {day}: untagged problem {} recommended", p.title);
                        assert!(!user.memories.contains_key(&p.id), "day {day}: already-solved {} recommended as new", p.title);
                        assert!(seen_new.insert(p.id), "day {day}: {} recommended as new twice", p.title);
                        new_today += 1;
                        if day < 30 {
                            new_early += 1;
                            curated_early += (p.curated != crate::recommend::patterns::Curated::None) as usize;
                        }
                    }
                    Kind::Review => {
                        reviews_total += 1;
                        assert!(user.memories.contains_key(&p.id), "day {day}: reviewing something never solved");
                    }
                }
                // Solve it an hour or so later; one in five goes less smoothly.
                let at = now + HOUR * (i as i64 + 1);
                let grade = if (day + i as i64) % 5 == 0 { Grade::Hard } else { Grade::Good };
                let memory = fsrs::review(user.memories.get(&p.id).copied(), at, grade);
                user.memories.insert(p.id, memory);
                user.last_activity.insert(p.id, at);
                for &pat in &p.patterns {
                    first_solved.entry(pat).or_insert(day);
                }
            }
            new_by_day.push(new_today);
        }

        let end = start + days * 24 * HOUR;
        let stats = engine::pattern_stats(set, &problems, &user, end);
        eprintln!("\n{:<30} tier  first solved on day   solved  strength", "pattern");
        for (i, p) in set.patterns.iter().enumerate() {
            eprintln!(
                "{:<30} {:>3}   {:>18}   {:>6}  {:>7.0}%",
                p.name,
                p.tier,
                first_solved.get(&i).map_or("never".to_string(), |d| d.to_string()),
                stats[i].solved,
                stats[i].strength * 100.0
            );
        }
        let new_total: usize = new_by_day.iter().sum();
        eprintln!(
            "new problems: {new_total} over {days} days; reviews: {reviews_total} of {picks_total} picks; curated share of new picks in the first 30 days: {curated_early}/{new_early}"
        );

        for (i, p) in set.patterns.iter().enumerate() {
            assert!(first_solved.contains_key(&i), "{} was never reached in {days} days", p.name);
        }
        assert!(curated_early * 10 >= new_early * 8, "early on, curated problems dominate: {curated_early}/{new_early}");
        assert!(reviews_total * 100 <= picks_total * 40, "reviews stay at a third: {reviews_total}/{picks_total}");
        assert!(new_total >= 280, "learning doesn't stall: only {new_total} new problems in {days} days");
    }

    #[test]
    /// The very first sets, on the real catalog: only the foundations are open to a beginner, so
    /// that is all they should be asked to do until they have some strength there.
    fn a_new_user_starts_in_the_foundations_on_the_real_catalog() {
        let Some((_dir, db)) = test_support::open_real_catalog() else {
            eprintln!("skipping: catalog.db not built");
            return;
        };
        let set = definitions();
        let problems = db.with(|c| load_problems(c, set)).unwrap();
        let by_id: HashMap<i64, &Problem> = problems.iter().map(|p| (p.id, p)).collect();
        let mut seen = HashSet::new();
        for day in 0..300 {
            let seed = seed_for(&format!("first-{day}"));
            let picks = engine::recommend(set, &problems, &UserData::default(), 1_800_000_000_000, 3, &HashSet::new(), seed);
            assert_eq!(picks.len(), 3);
            for pick in &picks {
                let p = by_id[&pick.problem_id];
                let names: Vec<_> = p.patterns.iter().map(|&i| set.patterns[i].name.as_str()).collect();
                assert!(p.patterns.iter().all(|&i| set.patterns[i].tier == 0), "day {day}: {} {names:?} isn't a foundation problem", p.title);
                seen.insert(set.patterns[pick.pattern.unwrap()].name.clone());
            }
        }
        assert!(seen.len() >= 2, "both foundation patterns come up: {seen:?}");
    }

    #[test]
    fn solving_in_the_log_but_with_an_edited_grade_changes_the_memory() {
        let (_d, db) = setup();
        let s = db
            .with(|c| submissions::record(c, NewSubmission { problem_id: 1, lang: "python3".into(), code: "x".into(), status: "Accepted".into(), ..Default::default() }))
            .unwrap();
        let easy = fsrs::review(None, s.submitted_at, Grade::Easy).stability;
        db.with(|c| reviews::set_grade(c, s.review_id.as_deref().unwrap(), 4)).unwrap();
        let m = db.with(|c| reviews::memories(c)).unwrap();
        assert!((m[&1].stability - easy).abs() < 1e-9);
    }
}
