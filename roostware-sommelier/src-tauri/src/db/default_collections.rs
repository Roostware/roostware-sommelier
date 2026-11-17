//! The built-in collections: Blind 75, NeetCode 150, NeetCode 250 and NeetCode All, filled with
//! the matching LeetCode problems the first time the app runs.
//!
//! They are ordinary rows in `collections` / `collection_problems`, so the Problems tab's
//! collection filter and chips work on them with no special case. What sets them apart is a fixed
//! id (see [`is_builtin`]): the user can't add to or remove from them (see `collections.rs`), so
//! they stay the official lists and never hit the "empty collections are deleted" rule.
//!
//! Seeding happens once, recorded in `meta`, rather than on every launch, so nothing is ever
//! re-created behind the user's back. Lists and the catalog are compiled in / shipped with the app,
//! so a newer catalog does not change collections that already exist.

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

use super::{now_ms, Result};

/// Blind 75 and NeetCode 150 are shared with the recommender.
const CURATED_JSON: &str = include_str!("../../data/curated.json");
const NEETCODE_JSON: &str = include_str!("../../data/neetcode.json");

/// `meta` key marking that the built-in collections have been made.
const SEEDED_KEY: &str = "default_collections_seeded";

/// (id, name, description), in the order they're listed.
const BUILTIN: [(&str, &str, &str); 4] = [
    ("builtin-blind-75", "Blind 75", "The 75 classic interview problems."),
    ("builtin-neetcode-150", "NeetCode 150", "NeetCode's 150 essential problems."),
    ("builtin-neetcode-250", "NeetCode 250", "The NeetCode 150 plus 100 more."),
    ("builtin-neetcode-all", "NeetCode All", "Every LeetCode problem on NeetCode."),
];

/// Whether this is one of the built-in collections (and so can't be changed).
pub fn is_builtin(collection_id: &str) -> bool {
    BUILTIN.iter().any(|(id, _, _)| *id == collection_id)
}

#[derive(Deserialize)]
struct Curated {
    blind75: Vec<String>,
    neetcode150: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NeetCode {
    neetcode250: Vec<String>,
    neetcode_all: Vec<String>,
}

/// The problem slugs of each built-in collection, parallel to [`BUILTIN`].
fn slug_lists() -> [Vec<String>; 4] {
    let curated: Curated = serde_json::from_str(CURATED_JSON).expect("data/curated.json is valid");
    let neetcode: NeetCode = serde_json::from_str(NEETCODE_JSON).expect("data/neetcode.json is valid");
    [curated.blind75, curated.neetcode150, neetcode.neetcode250, neetcode.neetcode_all]
}

/// A sort key for position `n` (counting from 0) in a list; these sort before the time-based keys
/// the user's own collections get, so the built-ins come first.
fn position_key(n: usize) -> String {
    format!("{:016x}", n + 1)
}

/// Makes the built-in collections if that hasn't been done yet. A list with no problem in the
/// catalog is skipped rather than made empty.
pub fn seed(conn: &mut Connection) -> Result<()> {
    let done: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM main.meta WHERE key = ?)", [SEEDED_KEY], |r| r.get(0))?;
    if done {
        return Ok(());
    }

    let tx = conn.transaction()?;
    let now = now_ms();
    for (position, ((id, name, description), slugs)) in BUILTIN.iter().zip(slug_lists()).enumerate() {
        let mut found = tx.prepare("SELECT id FROM catalog.problems WHERE title_slug = ?")?;
        let mut problem_ids = Vec::with_capacity(slugs.len());
        for slug in &slugs {
            if let Some(problem_id) = found.query_row([slug], |r| r.get::<_, i64>(0)).optional()? {
                problem_ids.push(problem_id);
            }
        }
        drop(found);
        if problem_ids.is_empty() {
            continue;
        }

        tx.execute(
            "INSERT OR IGNORE INTO main.collections (id, name, description, sort_key, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            rusqlite::params![id, name, description, position_key(position), now],
        )?;
        let mut member = tx.prepare(
            "INSERT OR IGNORE INTO main.collection_problems (collection_id, problem_id, sort_key, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
        )?;
        for (n, problem_id) in problem_ids.iter().enumerate() {
            member.execute(rusqlite::params![id, problem_id, position_key(n), now])?;
        }
    }
    tx.execute("INSERT OR REPLACE INTO main.meta (key, value) VALUES (?1, '1')", [SEEDED_KEY])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{collections, test_support, Db};
    use std::{collections::HashSet, path::Path};
    use tempfile::TempDir;

    fn members(c: &Connection, id: &str) -> Vec<String> {
        c.prepare(
            "SELECT p.title_slug FROM main.collection_problems cp JOIN catalog.problems p ON p.id = cp.problem_id
              WHERE cp.collection_id = ? AND cp.deleted_at IS NULL ORDER BY cp.sort_key",
        )
        .unwrap()
        .query_map([id], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    #[test]
    fn the_lists_are_nested_and_the_right_size() {
        let [blind, n150, n250, all] = slug_lists();
        assert_eq!((blind.len(), n150.len(), n250.len()), (75, 150, 250));
        let set = |v: &Vec<String>| v.iter().cloned().collect::<HashSet<_>>();
        assert_eq!(set(&all).len(), all.len(), "no repeats in NeetCode All");
        assert!(set(&blind).is_subset(&set(&n150)));
        assert!(set(&n150).is_subset(&set(&n250)));
        assert!(set(&n250).is_subset(&set(&all)));
    }

    #[test]
    fn seeds_what_the_catalog_has_once_and_never_again() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        db.with(|c| {
            seed(c)?;
            let list = collections::list(c)?;
            let names: Vec<_> = list.iter().map(|x| x.name.as_str()).collect();
            assert_eq!(names, vec!["Blind 75", "NeetCode 150", "NeetCode 250", "NeetCode All"]);
            assert!(list.iter().all(|x| x.builtin));
            // The fixture catalog only has Two Sum among the listed problems.
            assert_eq!(members(c, "builtin-blind-75"), vec!["two-sum"]);

            // Whatever happens to them later, a second run puts nothing back.
            c.execute("UPDATE main.collections SET deleted_at = 1", [])?;
            c.execute("UPDATE main.collection_problems SET deleted_at = 1", [])?;
            seed(c)?;
            assert!(collections::list(c)?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_list_with_nothing_in_the_catalog_is_not_made() {
        // A catalog with none of the listed problems at all.
        let dir = TempDir::new().unwrap();
        let catalog = dir.path().join("catalog.db");
        let conn = Connection::open(&catalog).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
             INSERT INTO meta VALUES ('schema_version', '1');
             CREATE TABLE problems (id INTEGER PRIMARY KEY, title_slug TEXT NOT NULL);",
        )
        .unwrap();
        drop(conn);
        let db = Db::open_at(&dir.path().join("user.db"), &catalog).unwrap();
        db.with(|c| {
            seed(c)?;
            assert!(collections::list(c)?.is_empty(), "no empty collections");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn builtin_collections_cannot_be_changed_or_borrowed_for_a_name() {
        use crate::db::Error;
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        db.with(|c| {
            seed(c)?;
            let id = "builtin-blind-75";
            assert!(matches!(collections::add_problem(c, id, 2), Err(Error::Invalid(_))));
            assert!(matches!(collections::remove_problem(c, id, 1), Err(Error::Invalid(_))));
            assert!(matches!(collections::create(c, "blind 75", 2), Err(Error::Invalid(_))), "name is taken");
            assert_eq!(members(c, id), vec!["two-sum"], "nothing changed");
            assert!(!collections::create(c, "Mine", 2)?.builtin);
            let all = collections::list(c)?;
            assert_eq!(all.last().unwrap().name, "Mine", "the user's own come after the built-ins");
            Ok(())
        })
        .unwrap();
    }

    /// Against the real catalog, when it has been built: the data is all real problems and the
    /// sizes are what each list is meant to be.
    #[test]
    fn real_catalog_gets_the_full_lists() {
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("catalog.db");
        if !real.exists() {
            eprintln!("skipping: {} not built", real.display());
            return;
        }
        let dir = TempDir::new().unwrap();
        let db = Db::open_at(&dir.path().join("user.db"), &real).unwrap();
        db.with(|c| {
            seed(c)?;
            let counts: Vec<_> = collections::list(c)?.iter().map(|x| (x.name.clone(), x.problem_count)).collect();
            assert_eq!(
                counts,
                vec![
                    ("Blind 75".to_string(), 75),
                    ("NeetCode 150".to_string(), 150),
                    ("NeetCode 250".to_string(), 250),
                    ("NeetCode All".to_string(), 1097),
                ],
                "every listed slug is a catalog problem"
            );
            Ok(())
        })
        .unwrap();
    }
}
