//! Problem queries: catalog data (`catalog.*`) joined with the user's own
//! tables (`main.*`) for progress, collections and per-problem state.
//!
//! Progress is never stored; it is derived, in this order of priority: a problem is
//! `solved` if it has an Accepted submission, else `attempted` if it has any submission
//! (whether or not it also has unsubmitted code), else `inProgress` if it has a draft
//! (unsubmitted code that differs from the starter), else `untouched`. Tombstoned (deleted)
//! rows are ignored.
//!
//! All SQL here is built from fixed fragments; user-supplied values are only
//! ever bound as parameters (sort keys are mapped from enums, never spliced).

use rusqlite::{params_from_iter, types::Value, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::{
    drafts::{self, Draft},
    submissions::{self, LastSubmitted},
    now_ms, Error, Result,
};

// ------------------------------------------------------------------ types --

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    fn as_str(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
        }
    }

    fn parse(s: &str) -> std::result::Result<Self, String> {
        match s {
            "Easy" => Ok(Difficulty::Easy),
            "Medium" => Ok(Difficulty::Medium),
            "Hard" => Ok(Difficulty::Hard),
            other => Err(format!("unknown difficulty {other:?} in catalog")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Progress {
    /// Has an Accepted submission.
    Solved,
    /// Has submissions, none Accepted. (Unsubmitted code doesn't change this.)
    Attempted,
    /// Has unsubmitted code that differs from the starter, and no submissions.
    #[serde(rename = "inProgress")]
    InProgress,
    Untouched,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemSummary {
    /// LeetCode's internal question id: what submissions and user data reference.
    pub id: i64,
    /// The number shown on leetcode.com.
    pub frontend_id: i64,
    pub title: String,
    pub title_slug: String,
    pub difficulty: Difficulty,
    pub is_paid_only: bool,
    /// 0..1, a snapshot from when the dataset was built.
    pub acceptance_rate: f64,
    pub progress: Progress,
    /// Number of Accepted submissions.
    pub solve_count: i64,
    pub tags: Vec<String>,
    pub collection_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    /// LeetCode langSlug (python3, cpp, …).
    pub lang: String,
    pub code: String,
}

/// The user's own state for a problem (all defaults when they've set none).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemState {
    pub status: Option<String>,
    pub starred: bool,
    pub notes: String,
    pub last_opened_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDetail {
    #[serde(flatten)]
    pub summary: ProblemSummary,
    /// Third-party HTML: sanitize before rendering.
    pub content_html: String,
    /// Raw, newline-separated; one case may span several lines (one per argument).
    pub example_test_cases: String,
    pub likes: i64,
    pub dislikes: i64,
    pub total_accepted: i64,
    pub total_submissions: i64,
    pub created_at_approx: Option<String>,
    pub snippets: Vec<Snippet>,
    /// The user's unsubmitted code, one per language, most recently edited first.
    pub drafts: Vec<Draft>,
    /// The code of the user's latest submission in each language, newest first. What the editor
    /// shows for a language with no draft.
    pub last_submitted: Vec<LastSubmitted>,
    pub state: ProblemState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemPage {
    pub items: Vec<ProblemSummary>,
    /// Matches for the filter, ignoring limit/offset.
    pub total: i64,
}

/// A problem the user has opened before, newest first in [`list_recent`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProblem {
    #[serde(flatten)]
    pub summary: ProblemSummary,
    /// Epoch milliseconds, UTC.
    pub last_opened_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusFilter {
    #[default]
    All,
    Solved,
    /// Not solved: includes attempted-but-never-accepted.
    Unsolved,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaidFilter {
    #[default]
    Any,
    Free,
    Paid,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProblemFilter {
    /// Free text over title and displayed number (not tags; use `tags` to filter by topic).
    /// Each word is a prefix match and all must match.
    pub query: Option<String>,
    /// `None` = any difficulty; `Some(empty)` = nothing matches.
    pub difficulties: Option<Vec<Difficulty>>,
    /// Problem must have *all* of these tags.
    pub tags: Vec<String>,
    pub status: StatusFilter,
    pub collection_id: Option<String>,
    pub paid: PaidFilter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    Number,
    Title,
    Difficulty,
    Progress,
    SolveCount,
    AcceptanceRate,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Sort {
    pub key: SortKey,
    #[serde(default)]
    pub dir: SortDir,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProblemQuery {
    pub filter: ProblemFilter,
    /// `None` = problem number, or best text match first when `filter.query` is set.
    /// Ties always fall back to problem number.
    pub sort: Option<Sort>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

// -------------------------------------------------------------------- sql --

/// Per-problem submission aggregates, from the user's non-deleted submissions.
const CTE: &str = "WITH sub AS (
    SELECT problem_id, COUNT(*) AS attempts, SUM(status = 'Accepted') AS solves
    FROM main.submissions WHERE deleted_at IS NULL GROUP BY problem_id
), drafted AS (
    SELECT DISTINCT problem_id FROM main.drafts
)";
const FROM: &str = "FROM catalog.problems p
    LEFT JOIN sub ON sub.problem_id = p.id
    LEFT JOIN drafted ON drafted.problem_id = p.id";
const FTS_JOIN: &str = " JOIN catalog.problems_fts f ON f.rowid = p.id";

const SOLVES: &str = "COALESCE(sub.solves, 0)";
/// 3 solved, 2 attempted, 1 in progress, 0 untouched (also the sort order).
const PROGRESS: &str = "CASE WHEN COALESCE(sub.solves, 0) > 0 THEN 3
                              WHEN COALESCE(sub.attempts, 0) > 0 THEN 2
                              WHEN drafted.problem_id IS NOT NULL THEN 1 ELSE 0 END";

/// Column order must match [`summary_from_row`]. `\x1f` (unit separator) joins
/// the list columns; neither tag names nor UUIDs can contain it.
const COLUMNS: &str = "p.id, p.frontend_id, p.title, p.title_slug, p.difficulty, p.is_paid_only,
    p.acceptance_rate,
    COALESCE(sub.solves, 0) AS solve_count,
    CASE WHEN COALESCE(sub.solves, 0) > 0 THEN 3
         WHEN COALESCE(sub.attempts, 0) > 0 THEN 2
         WHEN drafted.problem_id IS NOT NULL THEN 1 ELSE 0 END AS progress,
    (SELECT group_concat(t.name, char(31))
       FROM catalog.problem_tags pt JOIN catalog.tags t ON t.id = pt.tag_id
      WHERE pt.problem_id = p.id) AS tags,
    (SELECT group_concat(cp.collection_id, char(31))
       FROM main.collection_problems cp JOIN main.collections c ON c.id = cp.collection_id
      WHERE cp.problem_id = p.id AND cp.deleted_at IS NULL AND c.deleted_at IS NULL) AS collection_ids";

fn split_list(joined: Option<String>) -> Vec<String> {
    let mut items: Vec<String> = joined
        .map(|s| s.split('\u{1f}').map(str::to_owned).collect())
        .unwrap_or_default();
    items.sort();
    items
}

fn summary_from_row(row: &Row) -> rusqlite::Result<ProblemSummary> {
    let difficulty: String = row.get(4)?;
    let difficulty = Difficulty::parse(&difficulty).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, e.into())
    })?;
    let progress = match row.get::<_, i64>(8)? {
        3 => Progress::Solved,
        2 => Progress::Attempted,
        1 => Progress::InProgress,
        _ => Progress::Untouched,
    };
    Ok(ProblemSummary {
        id: row.get(0)?,
        frontend_id: row.get(1)?,
        title: row.get(2)?,
        title_slug: row.get(3)?,
        difficulty,
        is_paid_only: row.get::<_, i64>(5)? != 0,
        acceptance_rate: row.get(6)?,
        solve_count: row.get(7)?,
        progress,
        tags: split_list(row.get(9)?),
        collection_ids: split_list(row.get(10)?),
    })
}

/// The FTS columns text search looks at. The index also holds each problem's topic tags,
/// but they are deliberately left out: tags are a strong hint when solving, so matching on
/// them is something the user opts into, never the default. (To add that opt-in later,
/// include `tags` here when it is enabled.)
const SEARCH_COLUMNS: &str = "title frontend_id";

/// Turns free text into a safe FTS5 query: each alphanumeric word becomes a
/// quoted prefix term (`"two"* "sum"*`), implicitly ANDed, restricted to
/// [`SEARCH_COLUMNS`]. Quoting means user input can never be parsed as FTS syntax.
/// `None` if there are no words.
pub fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!terms.is_empty()).then(|| format!("{{{SEARCH_COLUMNS}}} : ({})", terms.join(" ")))
}

struct Where {
    clauses: Vec<String>,
    params: Vec<Value>,
    uses_fts: bool,
}

fn build_where(filter: &ProblemFilter) -> Where {
    let mut w = Where { clauses: Vec::new(), params: Vec::new(), uses_fts: false };

    if let Some(q) = filter.query.as_deref().and_then(fts_query) {
        w.uses_fts = true;
        // FTS5's MATCH column is named after the *table*, even through an alias.
        w.clauses.push("problems_fts MATCH ?".into());
        w.params.push(Value::Text(q));
    }

    if let Some(difficulties) = &filter.difficulties {
        if difficulties.is_empty() {
            w.clauses.push("0".into());
        } else {
            let marks = vec!["?"; difficulties.len()].join(", ");
            w.clauses.push(format!("p.difficulty IN ({marks})"));
            w.params.extend(difficulties.iter().map(|d| Value::Text(d.as_str().into())));
        }
    }

    let mut tags: Vec<&String> = filter.tags.iter().collect();
    tags.sort();
    tags.dedup();
    if !tags.is_empty() {
        let marks = vec!["?"; tags.len()].join(", ");
        w.clauses.push(format!(
            "p.id IN (SELECT pt.problem_id FROM catalog.problem_tags pt
                        JOIN catalog.tags t ON t.id = pt.tag_id
                       WHERE t.name IN ({marks})
                       GROUP BY pt.problem_id HAVING COUNT(DISTINCT t.id) = ?)"
        ));
        w.params.extend(tags.iter().map(|t| Value::Text((*t).clone())));
        w.params.push(Value::Integer(tags.len() as i64));
    }

    match filter.status {
        StatusFilter::All => {}
        StatusFilter::Solved => w.clauses.push(format!("{SOLVES} > 0")),
        StatusFilter::Unsolved => w.clauses.push(format!("{SOLVES} = 0")),
    }

    if let Some(id) = &filter.collection_id {
        w.clauses.push(
            "EXISTS (SELECT 1 FROM main.collection_problems cp
                       JOIN main.collections c ON c.id = cp.collection_id
                      WHERE cp.problem_id = p.id AND cp.collection_id = ?
                        AND cp.deleted_at IS NULL AND c.deleted_at IS NULL)"
                .into(),
        );
        w.params.push(Value::Text(id.clone()));
    }

    match filter.paid {
        PaidFilter::Any => {}
        PaidFilter::Free => w.clauses.push("p.is_paid_only = 0".into()),
        PaidFilter::Paid => w.clauses.push("p.is_paid_only = 1".into()),
    }

    w
}

/// Always ends with `p.id`: frontend ids are not unique in the dataset, and without a
/// total order, LIMIT/OFFSET paging can repeat or drop rows between pages.
fn order_by(sort: Option<Sort>, uses_fts: bool) -> String {
    let Some(Sort { key, dir }) = sort else {
        return if uses_fts {
            "f.rank, p.frontend_id, p.id".into()
        } else {
            "p.frontend_id, p.id".into()
        };
    };
    let expr = match key {
        SortKey::Number => "p.frontend_id".to_string(),
        SortKey::Title => "p.title COLLATE NOCASE".to_string(),
        SortKey::Difficulty => {
            "CASE p.difficulty WHEN 'Easy' THEN 0 WHEN 'Medium' THEN 1 ELSE 2 END".to_string()
        }
        SortKey::Progress => PROGRESS.to_string(),
        SortKey::SolveCount => SOLVES.to_string(),
        SortKey::AcceptanceRate => "p.acceptance_rate".to_string(),
    };
    let dir = if dir == SortDir::Desc { "DESC" } else { "ASC" };
    format!("{expr} {dir}, p.frontend_id ASC, p.id ASC")
}

// ---------------------------------------------------------------- queries --

pub fn list(conn: &Connection, query: &ProblemQuery) -> Result<ProblemPage> {
    let w = build_where(&query.filter);
    let from = format!("{FROM}{}", if w.uses_fts { FTS_JOIN } else { "" });
    let where_sql =
        if w.clauses.is_empty() { String::new() } else { format!(" WHERE {}", w.clauses.join(" AND ")) };

    let total: i64 = conn.query_row(
        &format!("{CTE} SELECT COUNT(*) {from}{where_sql}"),
        params_from_iter(w.params.iter()),
        |r| r.get(0),
    )?;

    let sql = format!(
        "{CTE} SELECT {COLUMNS} {from}{where_sql} ORDER BY {} LIMIT ? OFFSET ?",
        order_by(query.sort, w.uses_fts)
    );
    let mut params = w.params;
    params.push(Value::Integer(query.limit.map_or(-1, i64::from))); // -1 = no limit
    params.push(Value::Integer(query.offset.map_or(0, i64::from)));

    let mut stmt = conn.prepare(&sql)?;
    let items = stmt
        .query_map(params_from_iter(params.iter()), summary_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ProblemPage { items, total })
}

/// What judging a problem on LeetCode needs from the catalog: its slug (part of the URL) and
/// its example test cases. Errors if there's no such problem.
pub fn judging_info(conn: &Connection, id: i64) -> Result<(String, String)> {
    conn.query_row(
        "SELECT title_slug, example_test_cases FROM catalog.problems WHERE id = ?",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("problem {id}")))
}

pub fn get_by_id(conn: &Connection, id: i64) -> Result<Option<ProblemDetail>> {
    get(conn, "p.id = ?", Value::Integer(id))
}

pub fn get_by_slug(conn: &Connection, slug: &str) -> Result<Option<ProblemDetail>> {
    get(conn, "p.title_slug = ?", Value::Text(slug.to_owned()))
}

fn get(conn: &Connection, condition: &str, key: Value) -> Result<Option<ProblemDetail>> {
    let sql = format!("{CTE} SELECT {COLUMNS} {FROM} WHERE {condition}");
    let Some(summary) =
        conn.query_row(&sql, [&key], summary_from_row).optional()?
    else {
        return Ok(None);
    };
    let id = summary.id;

    let (content_html, example_test_cases, likes, dislikes, total_accepted, total_submissions, created_at_approx) =
        conn.query_row(
            "SELECT content_html, example_test_cases, likes, dislikes, total_accepted,
                    total_submissions, created_at_approx
               FROM catalog.problems WHERE id = ?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )?;

    let snippets = conn
        .prepare("SELECT lang, code FROM catalog.code_snippets WHERE problem_id = ? ORDER BY lang")?
        .query_map([id], |r| Ok(Snippet { lang: r.get(0)?, code: r.get(1)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let drafts = drafts::list(conn, id)?;
    let last_submitted = submissions::last_submitted(conn, id)?;

    let state = conn
        .query_row(
            "SELECT status, starred, notes, last_opened_at FROM main.problem_state WHERE problem_id = ?",
            [id],
            |r| {
                Ok(ProblemState {
                    status: r.get(0)?,
                    starred: r.get::<_, i64>(1)? != 0,
                    notes: r.get(2)?,
                    last_opened_at: r.get(3)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default();

    Ok(Some(ProblemDetail {
        summary,
        content_html,
        example_test_cases,
        likes,
        dislikes,
        total_accepted,
        total_submissions,
        created_at_approx,
        snippets,
        drafts,
        last_submitted,
        state,
    }))
}

/// Notes that the user opened a problem just now. Only touches `last_opened_at` (and
/// `updated_at`), so notes, stars and status already saved for it are left alone.
pub fn mark_opened(conn: &Connection, id: i64) -> Result<()> {
    let exists: bool =
        conn.query_row("SELECT EXISTS (SELECT 1 FROM catalog.problems WHERE id = ?)", [id], |r| r.get(0))?;
    if !exists {
        return Err(Error::NotFound(format!("problem {id}")));
    }
    let now = now_ms();
    conn.execute(
        "INSERT INTO main.problem_state (problem_id, last_opened_at, updated_at) VALUES (?1, ?2, ?2)
         ON CONFLICT (problem_id) DO UPDATE
           SET last_opened_at = excluded.last_opened_at, updated_at = excluded.updated_at",
        rusqlite::params![id, now],
    )?;
    Ok(())
}

/// The problems opened most recently, newest first. `limit` is clamped to 1..=100.
pub fn list_recent(conn: &Connection, limit: u32) -> Result<Vec<RecentProblem>> {
    let sql = format!(
        "{CTE} SELECT {COLUMNS}, ps.last_opened_at {FROM}
           JOIN main.problem_state ps ON ps.problem_id = p.id
          WHERE ps.last_opened_at IS NOT NULL
          ORDER BY ps.last_opened_at DESC, p.id
          LIMIT ?"
    );
    let recent = conn
        .prepare(&sql)?
        .query_map([i64::from(limit.clamp(1, 100))], |row| {
            Ok(RecentProblem { summary: summary_from_row(row)?, last_opened_at: row.get(11)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(recent)
}

/// Every tag with the number of catalog problems that have it, most common first.
pub fn list_tags(conn: &Connection) -> Result<Vec<TagCount>> {
    let tags = conn
        .prepare(
            "SELECT t.name, COUNT(*) AS n
               FROM catalog.tags t JOIN catalog.problem_tags pt ON pt.tag_id = t.id
              GROUP BY t.id ORDER BY n DESC, t.name",
        )?
        .query_map([], |r| Ok(TagCount { name: r.get(0)?, count: r.get(1)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tags)
}

// ------------------------------------------------------------------ tests --

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{new_id, now_ms, test_support};
    use serde_json::json;
    use tempfile::TempDir;

    /// Fixture catalog (built from the real schema):
    ///   id   fe#  title                         diff    paid  tags
    ///   1    1    Two Sum                       Easy    no    Array, Hash Table
    ///   2    2    Add Two Numbers               Medium  no    Linked List, Math
    ///   4    4    Median of Two Sorted Arrays   Hard    no    Array, Binary Search
    ///   900  146  LRU Cache                     Medium  yes   Design, Hash Table
    fn setup() -> (TempDir, crate::db::Db) {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        (dir, db)
    }

    fn submit(c: &Connection, problem_id: i64, status: &str, deleted: bool) {
        let t = now_ms();
        c.execute(
            "INSERT INTO submissions (id, problem_id, lang, code, status, submitted_at, created_at, updated_at, deleted_at)
             VALUES (?1, ?2, 'python3', '', ?3, ?4, ?4, ?4, ?5)",
            rusqlite::params![new_id(), problem_id, status, t, deleted.then_some(t)],
        )
        .unwrap();
    }

    fn ids(page: &ProblemPage) -> Vec<i64> {
        page.items.iter().map(|p| p.id).collect()
    }

    fn q(filter: ProblemFilter) -> ProblemQuery {
        ProblemQuery { filter, ..Default::default() }
    }

    #[test]
    fn default_order_is_by_displayed_number_not_internal_id() {
        let (_d, db) = setup();
        let page = db.with(|c| list(c, &ProblemQuery::default())).unwrap();
        assert_eq!(ids(&page), vec![1, 2, 4, 900]); // fe# 1, 2, 4, 146
        assert_eq!(page.total, 4);
        let lru = &page.items[3];
        assert_eq!((lru.frontend_id, lru.is_paid_only), (146, true));
        assert_eq!(lru.tags, vec!["Design", "Hash Table"]);
        assert_eq!(page.items[0].progress, Progress::Untouched);
    }

    #[test]
    fn filters_by_difficulty() {
        let (_d, db) = setup();
        let f = |d: Option<Vec<Difficulty>>| {
            db.with(|c| list(c, &q(ProblemFilter { difficulties: d, ..Default::default() }))).unwrap()
        };
        assert_eq!(ids(&f(Some(vec![Difficulty::Medium]))), vec![2, 900]);
        assert_eq!(ids(&f(Some(vec![Difficulty::Easy, Difficulty::Hard]))), vec![1, 4]);
        assert_eq!(f(Some(vec![])).total, 0, "all difficulties toggled off shows nothing");
        assert_eq!(f(None).total, 4);
    }

    #[test]
    fn filters_by_paid() {
        let (_d, db) = setup();
        let f = |paid| db.with(|c| list(c, &q(ProblemFilter { paid, ..Default::default() }))).unwrap();
        assert_eq!(ids(&f(PaidFilter::Paid)), vec![900]);
        assert_eq!(ids(&f(PaidFilter::Free)), vec![1, 2, 4]);
        assert_eq!(f(PaidFilter::Any).total, 4);
    }

    #[test]
    fn tag_filter_requires_all_tags() {
        let (_d, db) = setup();
        let f = |tags: &[&str]| {
            let tags = tags.iter().map(|t| t.to_string()).collect();
            ids(&db.with(|c| list(c, &q(ProblemFilter { tags, ..Default::default() }))).unwrap())
        };
        assert_eq!(f(&["Array"]), vec![1, 4]);
        assert_eq!(f(&["Hash Table"]), vec![1, 900]);
        assert_eq!(f(&["Array", "Hash Table"]), vec![1]);
        assert_eq!(f(&["Hash Table", "Array", "Array"]), vec![1], "duplicates don't break the count");
        assert!(f(&["Array", "Math"]).is_empty());
        assert!(f(&["No Such Tag"]).is_empty());
    }

    #[test]
    fn progress_and_status_come_from_non_deleted_submissions() {
        let (_d, db) = setup();
        db.with(|c| {
            submit(c, 1, "Accepted", false);
            submit(c, 1, "Accepted", false);
            submit(c, 1, "Wrong Answer", false);
            submit(c, 2, "Wrong Answer", false);
            submit(c, 4, "Accepted", true); // tombstoned: must not count
            Ok(())
        })
        .unwrap();

        let all = db.with(|c| list(c, &ProblemQuery::default())).unwrap();
        let by_id = |id| all.items.iter().find(|p| p.id == id).unwrap();
        assert_eq!((by_id(1).progress, by_id(1).solve_count), (Progress::Solved, 2));
        assert_eq!((by_id(2).progress, by_id(2).solve_count), (Progress::Attempted, 0));
        assert_eq!((by_id(4).progress, by_id(4).solve_count), (Progress::Untouched, 0));

        let status = |s| {
            ids(&db.with(|c| list(c, &q(ProblemFilter { status: s, ..Default::default() }))).unwrap())
        };
        assert_eq!(status(StatusFilter::Solved), vec![1]);
        assert_eq!(status(StatusFilter::Unsolved), vec![2, 4, 900], "attempted counts as unsolved");
        assert_eq!(status(StatusFilter::All), vec![1, 2, 4, 900]);
    }

    #[test]
    fn collection_filter_ignores_tombstones() {
        let (_d, db) = setup();
        let (live, dead) = (new_id(), new_id());
        db.with(|c| {
            let t = now_ms();
            for (id, name, deleted) in [(&live, "Live", None), (&dead, "Dead", Some(t))] {
                c.execute(
                    "INSERT INTO collections (id, name, sort_key, created_at, updated_at, deleted_at) VALUES (?1, ?2, 'a0', ?3, ?3, ?4)",
                    rusqlite::params![id, name, t, deleted],
                )?;
            }
            for (cid, pid, deleted) in [(&live, 1, None), (&live, 900, None), (&live, 2, Some(t)), (&dead, 4, None)] {
                c.execute(
                    "INSERT INTO collection_problems (collection_id, problem_id, sort_key, created_at, updated_at, deleted_at) VALUES (?1, ?2, 'a0', ?3, ?3, ?4)",
                    rusqlite::params![cid, pid, t, deleted],
                )?;
            }
            Ok(())
        })
        .unwrap();

        let in_collection = |id: &str| {
            db.with(|c| {
                list(c, &q(ProblemFilter { collection_id: Some(id.to_string()), ..Default::default() }))
            })
            .unwrap()
        };
        assert_eq!(ids(&in_collection(&live)), vec![1, 900]);
        assert_eq!(in_collection(&dead).total, 0, "deleted collection matches nothing");

        let all = db.with(|c| list(c, &ProblemQuery::default())).unwrap();
        assert_eq!(all.items[0].collection_ids, vec![live.clone()]);
        assert!(all.items[1].collection_ids.is_empty(), "tombstoned membership hidden");
        assert!(all.items[2].collection_ids.is_empty(), "deleted collection hidden");
    }

    #[test]
    fn text_search_matches_title_prefix_and_number_but_not_tags() {
        let (_d, db) = setup();
        let search = |text: &str| {
            ids(&db
                .with(|c| list(c, &q(ProblemFilter { query: Some(text.into()), ..Default::default() })))
                .unwrap())
        };
        let mut two = search("two");
        two.sort();
        assert_eq!(two, vec![1, 2, 4]);
        assert_eq!(search("med"), vec![4], "prefix match");
        assert_eq!(search("two sum"), vec![1], "all words must match");
        assert_eq!(search("146"), vec![900], "matches the displayed number");
        // Topic tags are filterable but not searchable text.
        for tag_word in ["linked", "hash", "math", "design"] {
            assert!(search(tag_word).is_empty(), "{tag_word:?} is only a tag; search must not match it");
        }
        assert!(search("zzz").is_empty());
    }

    #[test]
    fn hostile_search_text_cannot_break_the_query() {
        let (_d, db) = setup();
        for text in ["\"", "((", "a\" OR \"b", "two*", "NEAR(", "col:two", "-two", "'; DROP TABLE x;--"] {
            db.with(|c| list(c, &q(ProblemFilter { query: Some(text.into()), ..Default::default() })))
                .unwrap_or_else(|e| panic!("{text:?} errored: {e}"));
        }
        // No usable words means no text filter, not "match nothing".
        let page = db
            .with(|c| list(c, &q(ProblemFilter { query: Some("  ((  ".into()), ..Default::default() })))
            .unwrap();
        assert_eq!(page.total, 4);
    }

    #[test]
    fn fts_query_quotes_every_word_and_skips_tags() {
        assert_eq!(fts_query("two sum").as_deref(), Some("{title frontend_id} : (\"two\"* \"sum\"*)"));
        assert_eq!(
            fts_query("a\" OR \"b").as_deref(),
            Some("{title frontend_id} : (\"a\"* \"OR\"* \"b\"*)")
        );
        assert_eq!(fts_query("  --  "), None);
        assert_eq!(fts_query("é-tree").as_deref(), Some("{title frontend_id} : (\"é\"* \"tree\"*)"));
        assert!(!fts_query("anything").unwrap().contains("tags"));
    }

    #[test]
    fn sorting_with_stable_number_tiebreak() {
        let (_d, db) = setup();
        db.with(|c| {
            submit(c, 900, "Accepted", false);
            Ok(())
        })
        .unwrap();
        let sorted = |key, dir| {
            ids(&db
                .with(|c| list(c, &ProblemQuery { sort: Some(Sort { key, dir }), ..Default::default() }))
                .unwrap())
        };
        assert_eq!(sorted(SortKey::Difficulty, SortDir::Desc), vec![4, 2, 900, 1]);
        assert_eq!(sorted(SortKey::Difficulty, SortDir::Asc), vec![1, 2, 900, 4]);
        assert_eq!(sorted(SortKey::SolveCount, SortDir::Desc), vec![900, 1, 2, 4]);
        assert_eq!(sorted(SortKey::Progress, SortDir::Desc), vec![900, 1, 2, 4]);
        assert_eq!(sorted(SortKey::Title, SortDir::Asc), vec![2, 900, 4, 1]);
        assert_eq!(sorted(SortKey::Number, SortDir::Desc), vec![900, 4, 2, 1]);
        assert_eq!(sorted(SortKey::AcceptanceRate, SortDir::Desc), vec![1, 2, 900, 4]);
    }

    #[test]
    fn paging_reports_total_ignoring_limit() {
        let (_d, db) = setup();
        let page = |limit, offset| {
            db.with(|c| list(c, &ProblemQuery { limit, offset, ..Default::default() })).unwrap()
        };
        let p = page(Some(2), Some(1));
        assert_eq!((ids(&p), p.total), (vec![2, 4], 4));
        assert_eq!(page(Some(2), Some(10)).items.len(), 0);
        assert_eq!(page(Some(2), Some(10)).total, 4);
        assert_eq!(page(None, None).items.len(), 4);
    }

    #[test]
    fn filters_combine() {
        let (_d, db) = setup();
        db.with(|c| {
            submit(c, 1, "Accepted", false);
            Ok(())
        })
        .unwrap();
        let page = db
            .with(|c| {
                list(c, &q(ProblemFilter {
                    query: Some("two".into()),
                    difficulties: Some(vec![Difficulty::Easy, Difficulty::Hard]),
                    tags: vec!["Array".into()],
                    status: StatusFilter::Unsolved,
                    paid: PaidFilter::Free,
                    ..Default::default()
                }))
            })
            .unwrap();
        assert_eq!(ids(&page), vec![4]);
    }

    #[test]
    fn get_problem_returns_detail_with_snippets_and_default_state() {
        let (_d, db) = setup();
        let p = db.with(|c| get_by_id(c, 900)).unwrap().expect("found");
        assert_eq!(p.summary.title, "LRU Cache");
        assert_eq!(p.summary.frontend_id, 146);
        assert_eq!(p.content_html, "<p>LRU Cache</p>");
        assert_eq!(p.likes, 7);
        assert_eq!(p.snippets.iter().map(|s| s.lang.as_str()).collect::<Vec<_>>(), vec!["cpp", "python3"]);
        assert_eq!(p.state.notes, "");
        assert!(!p.state.starred && p.state.status.is_none());

        let by_slug = db.with(|c| get_by_slug(c, "lru-cache")).unwrap().expect("found by slug");
        assert_eq!(by_slug.summary.id, 900);
    }

    #[test]
    fn get_problem_includes_user_state_and_progress() {
        let (_d, db) = setup();
        db.with(|c| {
            submit(c, 1, "Accepted", false);
            c.execute(
                "INSERT INTO problem_state (problem_id, status, starred, notes, updated_at) VALUES (1, 'review', 1, 'use a hashmap', 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let p = db.with(|c| get_by_id(c, 1)).unwrap().unwrap();
        assert_eq!(p.summary.progress, Progress::Solved);
        assert_eq!(p.state.status.as_deref(), Some("review"));
        assert!(p.state.starred);
        assert_eq!(p.state.notes, "use a hashmap");
    }

    #[test]
    fn get_problem_missing_is_none() {
        let (_d, db) = setup();
        assert!(db.with(|c| get_by_id(c, 12345)).unwrap().is_none());
        assert!(db.with(|c| get_by_slug(c, "nope")).unwrap().is_none());
        // A slug that looks like SQL is just an unmatched parameter.
        assert!(db.with(|c| get_by_slug(c, "x' OR '1'='1")).unwrap().is_none());
    }

    fn draft(c: &Connection, problem_id: i64, lang: &str, code: &str) {
        assert!(drafts::save(c, problem_id, lang, code).unwrap(), "expected a draft to be stored");
    }

    fn progress_of(db: &crate::db::Db) -> std::collections::HashMap<i64, Progress> {
        db.with(|c| list(c, &ProblemQuery::default())).unwrap().items.into_iter().map(|p| (p.id, p.progress)).collect()
    }

    #[test]
    fn a_draft_alone_makes_a_problem_in_progress() {
        let (_d, db) = setup();
        db.with(|c| {
            draft(c, 1, "python3", "my solution so far");
            Ok(())
        })
        .unwrap();
        let progress = progress_of(&db);
        assert_eq!(progress[&1], Progress::InProgress);
        assert_eq!(progress[&2], Progress::Untouched, "other problems are unaffected");
        let one = db.with(|c| list(c, &ProblemQuery::default())).unwrap().items.into_iter().find(|p| p.id == 1).unwrap();
        assert_eq!(one.solve_count, 0, "a draft is not a solve");
    }

    #[test]
    fn submissions_outrank_drafts() {
        let (_d, db) = setup();
        db.with(|c| {
            draft(c, 1, "python3", "only a draft");
            draft(c, 2, "cpp", "draft plus a failed submission");
            submit(c, 2, "Wrong Answer", false);
            draft(c, 4, "python3", "draft plus an accepted submission");
            submit(c, 4, "Accepted", false);
            Ok(())
        })
        .unwrap();
        let progress = progress_of(&db);
        assert_eq!(progress[&1], Progress::InProgress);
        assert_eq!(progress[&2], Progress::Attempted, "unsuccessful submissions win over unsubmitted code");
        assert_eq!(progress[&4], Progress::Solved, "and an accepted one wins over everything");
        assert_eq!(progress[&900], Progress::Untouched);
    }

    #[test]
    fn a_draft_in_any_language_counts_and_the_last_one_going_ends_it() {
        let (_d, db) = setup();
        db.with(|c| {
            draft(c, 1, "python3", "py");
            draft(c, 1, "cpp", "cpp");
            Ok(())
        })
        .unwrap();
        assert_eq!(progress_of(&db)[&1], Progress::InProgress);
        db.with(|c| drafts::delete(c, 1, "python3")).unwrap();
        assert_eq!(progress_of(&db)[&1], Progress::InProgress, "still has the C++ draft");
        db.with(|c| drafts::delete(c, 1, "cpp")).unwrap();
        assert_eq!(progress_of(&db)[&1], Progress::Untouched, "no drafts left");
    }

    #[test]
    fn undoing_back_to_the_starter_ends_in_progress() {
        let (_d, db) = setup();
        db.with(|c| {
            draft(c, 1, "python3", "changed");
            Ok(())
        })
        .unwrap();
        assert_eq!(progress_of(&db)[&1], Progress::InProgress);
        db.with(|c| drafts::save(c, 1, "python3", "// python3 starter for Two Sum")).unwrap();
        assert_eq!(progress_of(&db)[&1], Progress::Untouched);
    }

    #[test]
    fn progress_sorts_untouched_then_in_progress_then_attempted_then_solved() {
        let (_d, db) = setup();
        db.with(|c| {
            draft(c, 1, "python3", "draft");
            submit(c, 2, "Wrong Answer", false);
            submit(c, 4, "Accepted", false);
            Ok(())
        })
        .unwrap();
        let sorted = |dir| {
            ids(&db
                .with(|c| {
                    list(c, &ProblemQuery { sort: Some(Sort { key: SortKey::Progress, dir }), ..Default::default() })
                })
                .unwrap())
        };
        assert_eq!(sorted(SortDir::Asc), vec![900, 1, 2, 4]);
        assert_eq!(sorted(SortDir::Desc), vec![4, 2, 1, 900]);
    }

    #[test]
    fn unsolved_includes_in_progress_problems() {
        let (_d, db) = setup();
        db.with(|c| {
            draft(c, 1, "python3", "draft");
            submit(c, 4, "Accepted", false);
            Ok(())
        })
        .unwrap();
        let status = |s| {
            ids(&db.with(|c| list(c, &q(ProblemFilter { status: s, ..Default::default() }))).unwrap())
        };
        assert_eq!(status(StatusFilter::Solved), vec![4]);
        assert_eq!(status(StatusFilter::Unsolved), vec![1, 2, 900]);
    }

    #[test]
    fn the_detail_carries_the_users_drafts_newest_first() {
        let (_d, db) = setup();
        db.with(|c| {
            draft(c, 1, "python3", "py work");
            draft(c, 1, "cpp", "cpp work");
            c.execute("UPDATE drafts SET updated_at = 1000 WHERE lang = 'python3'", [])?;
            c.execute("UPDATE drafts SET updated_at = 2000 WHERE lang = 'cpp'", [])?;
            Ok(())
        })
        .unwrap();
        let detail = db.with(|c| get_by_id(c, 1)).unwrap().unwrap();
        assert_eq!(detail.summary.progress, Progress::InProgress);
        let got: Vec<_> = detail.drafts.iter().map(|d| (d.lang.as_str(), d.code.as_str())).collect();
        assert_eq!(got, vec![("cpp", "cpp work"), ("python3", "py work")]);
        assert!(db.with(|c| get_by_id(c, 2)).unwrap().unwrap().drafts.is_empty());
    }

    #[test]
    fn progress_serializes_in_progress_as_camel_case() {
        let (_d, db) = setup();
        db.with(|c| {
            draft(c, 1, "python3", "draft");
            Ok(())
        })
        .unwrap();
        let v = serde_json::to_value(db.with(|c| get_by_id(c, 1)).unwrap().unwrap()).unwrap();
        assert_eq!(v["progress"], "inProgress");
        assert_eq!(v["drafts"][0]["lang"], "python3");
        assert!(v["drafts"][0]["updatedAt"].as_i64().unwrap() > 0);
    }

    fn set_opened_at(c: &Connection, problem_id: i64, at: Option<i64>) {
        c.execute(
            "INSERT INTO problem_state (problem_id, last_opened_at, updated_at) VALUES (?1, ?2, 0)
             ON CONFLICT (problem_id) DO UPDATE SET last_opened_at = excluded.last_opened_at",
            rusqlite::params![problem_id, at],
        )
        .unwrap();
    }

    #[test]
    fn recent_lists_newest_first_and_skips_never_opened() {
        let (_d, db) = setup();
        let recent = db
            .with(|c| {
                set_opened_at(c, 1, Some(1_000));
                set_opened_at(c, 900, Some(3_000));
                set_opened_at(c, 2, Some(2_000));
                set_opened_at(c, 4, None); // has a problem_state row (e.g. notes) but was never opened
                submit(c, 900, "Accepted", false);
                list_recent(c, 10)
            })
            .unwrap();
        let order: Vec<_> = recent.iter().map(|r| r.summary.id).collect();
        assert_eq!(order, vec![900, 2, 1]);
        assert_eq!(recent[0].last_opened_at, 3_000);
        assert_eq!(recent[0].summary.title, "LRU Cache");
        assert_eq!(recent[0].summary.progress, Progress::Solved, "summary fields are fully populated");
    }

    #[test]
    fn recent_respects_limit_and_breaks_ties_by_id() {
        let (_d, db) = setup();
        let ids = |limit| -> Vec<i64> {
            db.with(|c| {
                for id in [1, 2, 4] {
                    set_opened_at(c, id, Some(5_000)); // identical timestamps
                }
                list_recent(c, limit)
            })
            .unwrap()
            .iter()
            .map(|r| r.summary.id)
            .collect()
        };
        assert_eq!(ids(2), vec![1, 2], "stable order when timestamps tie");
        assert_eq!(ids(0), vec![1], "limit is clamped up to 1");
        assert_eq!(ids(10_000).len(), 3);
    }

    #[test]
    fn recent_is_empty_for_a_new_user() {
        let (_d, db) = setup();
        assert!(db.with(|c| list_recent(c, 10)).unwrap().is_empty());
    }

    #[test]
    fn mark_opened_creates_then_updates_one_row_and_keeps_user_state() {
        let (_d, db) = setup();
        let before = now_ms();
        db.with(|c| {
            c.execute(
                "INSERT INTO problem_state (problem_id, status, starred, notes, updated_at) VALUES (1, 'review', 1, 'my notes', 0)",
                [],
            )?;
            mark_opened(c, 1)?;
            mark_opened(c, 1)?; // opening again must not duplicate or reset anything
            mark_opened(c, 2)?; // no prior state row at all
            Ok(())
        })
        .unwrap();
        let (rows, opened, status, starred, notes): (i64, i64, String, i64, String) = db
            .with(|c| {
                Ok((
                    c.query_row("SELECT COUNT(*) FROM problem_state", [], |r| r.get(0))?,
                    c.query_row("SELECT last_opened_at FROM problem_state WHERE problem_id = 1", [], |r| r.get(0))?,
                    c.query_row("SELECT status FROM problem_state WHERE problem_id = 1", [], |r| r.get(0))?,
                    c.query_row("SELECT starred FROM problem_state WHERE problem_id = 1", [], |r| r.get(0))?,
                    c.query_row("SELECT notes FROM problem_state WHERE problem_id = 1", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!(rows, 2, "one row per problem");
        assert!(opened >= before);
        assert_eq!((status.as_str(), starred, notes.as_str()), ("review", 1, "my notes"));
    }

    #[test]
    fn mark_opened_rejects_unknown_problems_without_writing() {
        let (_d, db) = setup();
        let err = db.with(|c| mark_opened(c, 424242)).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "got: {err}");
        assert_eq!(err.to_string(), "problem 424242 not found");
        let rows: i64 =
            db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM problem_state", [], |r| r.get(0))?)).unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn opened_problem_shows_up_in_recent_and_is_reflected_in_detail() {
        let (_d, db) = setup();
        db.with(|c| mark_opened(c, 900)).unwrap();
        let recent = db.with(|c| list_recent(c, 5)).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].summary.id, 900);
        let detail = db.with(|c| get_by_id(c, 900)).unwrap().unwrap();
        assert_eq!(detail.state.last_opened_at, Some(recent[0].last_opened_at));
    }

    #[test]
    fn list_tags_counts_most_common_first() {
        let (_d, db) = setup();
        let tags = db.with(|c| list_tags(c)).unwrap();
        let top: Vec<_> = tags.iter().take(2).map(|t| (t.name.as_str(), t.count)).collect();
        assert_eq!(top, vec![("Array", 2), ("Hash Table", 2)]); // count desc, then name
        assert_eq!(tags.len(), 6);
        assert!(tags[2..].iter().all(|t| t.count == 1));
    }

    #[test]
    fn query_deserializes_from_the_json_the_frontend_sends() {
        let empty: ProblemQuery = serde_json::from_value(json!({})).unwrap();
        assert!(empty.filter.difficulties.is_none() && empty.sort.is_none());

        let full: ProblemQuery = serde_json::from_value(json!({
            "filter": {
                "query": "two", "difficulties": ["Easy", "Hard"], "tags": ["Array"],
                "status": "unsolved", "collectionId": "abc", "paid": "free"
            },
            "sort": { "key": "solveCount", "dir": "desc" },
            "limit": 50, "offset": 100
        }))
        .unwrap();
        assert_eq!(full.filter.status, StatusFilter::Unsolved);
        assert_eq!(full.filter.collection_id.as_deref(), Some("abc"));
        assert_eq!(full.sort.unwrap().key, SortKey::SolveCount);
        assert_eq!((full.limit, full.offset), (Some(50), Some(100)));

        assert!(serde_json::from_value::<ProblemQuery>(json!({"sort": {"key": "id; DROP"}})).is_err());
        assert!(serde_json::from_value::<ProblemQuery>(json!({"filter": {"difficulties": ["Nightmare"]}})).is_err());
    }

    #[test]
    fn serializes_camel_case_with_flattened_summary() {
        let (_d, db) = setup();
        let p = db.with(|c| get_by_id(c, 1)).unwrap().unwrap();
        let v = serde_json::to_value(&p).unwrap();
        for key in ["id", "frontendId", "titleSlug", "isPaidOnly", "acceptanceRate", "solveCount",
                    "collectionIds", "contentHtml", "exampleTestCases", "totalAccepted", "state", "snippets", "drafts", "lastSubmitted"] {
            assert!(v.get(key).is_some(), "missing key {key} in {v}");
        }
        assert_eq!(v["progress"], "untouched");
        assert_eq!(v["difficulty"], "Easy");
    }

    #[test]
    fn every_ordering_is_a_total_order() {
        // Paging is only exact if no two rows can tie, whatever the sort.
        let sorts = [
            None,
            Some(Sort { key: SortKey::Number, dir: SortDir::Asc }),
            Some(Sort { key: SortKey::Title, dir: SortDir::Desc }),
            Some(Sort { key: SortKey::Difficulty, dir: SortDir::Asc }),
            Some(Sort { key: SortKey::Progress, dir: SortDir::Desc }),
            Some(Sort { key: SortKey::SolveCount, dir: SortDir::Asc }),
            Some(Sort { key: SortKey::AcceptanceRate, dir: SortDir::Desc }),
        ];
        for sort in sorts {
            for fts in [false, true] {
                let sql = order_by(sort, fts);
                assert!(sql.ends_with("p.id") || sql.ends_with("p.id ASC"), "{sort:?} fts={fts}: {sql}");
            }
        }
    }

    /// Runs the same queries against the real artifact from `npm run catalog`.
    #[test]
    fn real_catalog_paging_is_exact() {
        let Some((_dir, db)) = test_support::open_real_catalog() else {
            eprintln!("skipping: catalog.db not built");
            return;
        };
        // The real catalog has a duplicated frontend_id (4030); walk it in awkward
        // page sizes under several sorts and require every problem exactly once.
        for (limit, sort) in [
            (7, None),
            (100, None),
            (13, Some(Sort { key: SortKey::Difficulty, dir: SortDir::Desc })),
            (50, Some(Sort { key: SortKey::AcceptanceRate, dir: SortDir::Asc })),
        ] {
            db.with(|c| {
                let total = list(c, &ProblemQuery::default())?.total;
                let mut seen = std::collections::HashSet::new();
                let mut offset = 0u32;
                loop {
                    let page = list(c, &ProblemQuery { sort, limit: Some(limit), offset: Some(offset), ..Default::default() })?;
                    if page.items.is_empty() {
                        break;
                    }
                    for p in &page.items {
                        assert!(seen.insert(p.id), "problem {} repeated (limit {limit}, {sort:?})", p.id);
                    }
                    offset += limit;
                }
                assert_eq!(seen.len() as i64, total, "paging dropped rows (limit {limit}, {sort:?})");
                Ok(())
            })
            .unwrap();
        }
    }

    /// The real index includes tags, so this is the check that matters: every hit for a
    /// word that is also a topic tag must have that word in its *title*.
    #[test]
    fn real_catalog_search_never_matches_on_tags() {
        let Some((_dir, db)) = test_support::open_real_catalog() else {
            eprintln!("skipping: catalog.db not built");
            return;
        };
        db.with(|c| {
            let tags: Vec<String> = list_tags(c)?.into_iter().map(|t| t.name).collect();
            assert!(tags.len() > 50, "expected many real tags, got {}", tags.len());
            let mut hits_checked = 0;
            for tag in &tags {
                for word in tag.split(|ch: char| !ch.is_alphanumeric()).filter(|w| w.len() > 2) {
                    let page = list(c, &q(ProblemFilter { query: Some(word.to_string()), ..Default::default() }))?;
                    for p in &page.items {
                        let title = p.title.to_lowercase();
                        assert!(
                            title.contains(&word.to_lowercase()),
                            "searching {word:?} (from tag {tag:?}) matched {:?}, which only has it as a tag",
                            p.title
                        );
                        hits_checked += 1;
                    }
                }
            }
            assert!(hits_checked > 100, "sanity: only {hits_checked} hits checked");
            Ok(())
        })
        .unwrap();
    }

    /// Runs the same queries against the real artifact from `npm run catalog`.
    #[test]
    fn real_catalog_smoke() {
        let Some((_dir, db)) = test_support::open_real_catalog() else {
            eprintln!("skipping: catalog.db not built");
            return;
        };
        db.with(|c| {
            let all = list(c, &ProblemQuery { limit: Some(5), ..Default::default() })?;
            assert!(all.total > 3000, "only {} problems", all.total);
            assert_eq!(all.items[0].frontend_id, 1);
            assert_eq!(all.items[0].title, "Two Sum");

            let hits = list(c, &q(ProblemFilter { query: Some("two sum".into()), ..Default::default() }))?;
            assert_eq!(hits.items[0].title, "Two Sum", "best text match first");

            let dp_hard = list(c, &q(ProblemFilter {
                tags: vec!["Dynamic Programming".into()],
                difficulties: Some(vec![Difficulty::Hard]),
                ..Default::default()
            }))?;
            assert!(dp_hard.total > 50);
            assert!(dp_hard.items.iter().all(|p| p.difficulty == Difficulty::Hard));

            let one = get_by_slug(c, "two-sum")?.expect("two-sum exists");
            assert_eq!(one.summary.id, 1);
            assert!(one.snippets.iter().any(|s| s.lang == "python3"));
            assert!(one.content_html.contains("<p>"));

            let tags = list_tags(c)?;
            assert!(tags.len() > 50 && tags[0].count >= tags[1].count);
            Ok(())
        })
        .unwrap();
    }
}
