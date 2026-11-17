//! The user's collections: custom, user-defined groups of problems (not
//! LeetCode's topic tags). Deleted collections and memberships are tombstones
//! and never shown.
//!
//! The rule this module owns: **a collection exists only while it has problems.** A collection
//! is created together with its first problem, and taking out its last problem deletes it. So
//! there is no such thing as an empty collection to create, rename around or clean up.

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use super::{default_collections, new_id, now_ms, Error, Result};

/// Longest collection name, in characters.
const MAX_NAME_CHARS: usize = 60;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Problems currently in the collection.
    pub problem_count: i64,
    /// One of the app's own lists (Blind 75, …): it can't be changed, unlike the user's.
    pub builtin: bool,
}

/// Built-in collections are fixed lists, so nothing can be added to or removed from them.
fn ensure_editable(collection_id: &str) -> Result<()> {
    if default_collections::is_builtin(collection_id) {
        Err(Error::Invalid("Built-in collections can't be changed.".into()))
    } else {
        Ok(())
    }
}

/// All live collections in the user's chosen order (`sort_key`, then oldest first).
pub fn list(conn: &Connection) -> Result<Vec<CollectionSummary>> {
    let collections = conn
        .prepare(
            "SELECT c.id, c.name, c.description,
                    (SELECT COUNT(*) FROM main.collection_problems cp
                      WHERE cp.collection_id = c.id AND cp.deleted_at IS NULL) AS problem_count
               FROM main.collections c
              WHERE c.deleted_at IS NULL
              ORDER BY c.sort_key, c.created_at, c.id",
        )?
        .query_map([], |r| {
            let id: String = r.get(0)?;
            Ok(CollectionSummary {
                builtin: default_collections::is_builtin(&id),
                id,
                name: r.get(1)?,
                description: r.get(2)?,
                problem_count: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(collections)
}

/// A sort key that places a new row after every existing one: the clock, zero-padded so that
/// string order is time order. (Rows made at the same millisecond fall back on `created_at`
/// and then the id, which is time-sortable too.)
fn append_key(now: i64) -> String {
    format!("{now:016x}")
}

fn problem_exists(conn: &Connection, problem_id: i64) -> Result<()> {
    let exists: bool =
        conn.query_row("SELECT EXISTS (SELECT 1 FROM catalog.problems WHERE id = ?)", [problem_id], |r| r.get(0))?;
    if exists {
        Ok(())
    } else {
        Err(Error::NotFound(format!("problem {problem_id}")))
    }
}

/// Puts the problem in the collection (a no-op if it's already there). A membership the user
/// removed earlier is brought back, at the end.
fn put_member(conn: &Connection, collection_id: &str, problem_id: i64, now: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO main.collection_problems (collection_id, problem_id, sort_key, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT (collection_id, problem_id) DO UPDATE
           SET deleted_at = NULL, sort_key = excluded.sort_key,
               created_at = excluded.created_at, updated_at = excluded.updated_at
         WHERE collection_problems.deleted_at IS NOT NULL",
        rusqlite::params![collection_id, problem_id, append_key(now), now],
    )?;
    Ok(())
}

/// Makes a new collection holding `problem_id`, and returns it. The name is trimmed; it must be
/// there, fit, and not already belong to one of the user's collections (ignoring case).
pub fn create(conn: &mut Connection, name: &str, problem_id: i64) -> Result<CollectionSummary> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("Give the collection a name.".into()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::Invalid(format!("Collection names can be at most {MAX_NAME_CHARS} characters.")));
    }

    let tx = conn.transaction()?;
    problem_exists(&tx, problem_id)?;
    let taken: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM main.collections WHERE deleted_at IS NULL AND name = ?1 COLLATE NOCASE)",
        [name],
        |r| r.get(0),
    )?;
    if taken {
        return Err(Error::Invalid(format!("You already have a collection named “{name}”.")));
    }

    let (id, now) = (new_id(), now_ms());
    tx.execute(
        "INSERT INTO main.collections (id, name, sort_key, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
        rusqlite::params![id, name, append_key(now), now],
    )?;
    put_member(&tx, &id, problem_id, now)?;
    tx.commit()?;
    Ok(CollectionSummary {
        id,
        name: name.to_string(),
        description: String::new(),
        problem_count: 1,
        builtin: false,
    })
}

/// Adds a problem to one of the user's collections. Fine if it's already in it.
pub fn add_problem(conn: &mut Connection, collection_id: &str, problem_id: i64) -> Result<()> {
    ensure_editable(collection_id)?;
    let tx = conn.transaction()?;
    problem_exists(&tx, problem_id)?;
    let live: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM main.collections WHERE id = ?1 AND deleted_at IS NULL)",
        [collection_id],
        |r| r.get(0),
    )?;
    if !live {
        return Err(Error::NotFound(format!("collection {collection_id}")));
    }
    put_member(&tx, collection_id, problem_id, now_ms())?;
    tx.commit()?;
    Ok(())
}

/// Takes a problem out of a collection, and deletes the collection if that leaves it empty.
/// Fine if the problem isn't in it, or the collection is already gone.
pub fn remove_problem(conn: &mut Connection, collection_id: &str, problem_id: i64) -> Result<()> {
    ensure_editable(collection_id)?;
    let tx = conn.transaction()?;
    let deleted_at: Option<Option<i64>> = tx
        .query_row("SELECT deleted_at FROM main.collections WHERE id = ?1", [collection_id], |r| r.get(0))
        .optional()?;
    match deleted_at {
        None => return Err(Error::NotFound(format!("collection {collection_id}"))),
        Some(Some(_)) => return Ok(()), // already deleted
        Some(None) => {}
    }

    let now = now_ms();
    tx.execute(
        "UPDATE main.collection_problems SET deleted_at = ?3, updated_at = ?3
          WHERE collection_id = ?1 AND problem_id = ?2 AND deleted_at IS NULL",
        rusqlite::params![collection_id, problem_id, now],
    )?;
    tx.execute(
        "UPDATE main.collections SET deleted_at = ?2, updated_at = ?2
          WHERE id = ?1 AND NOT EXISTS (
            SELECT 1 FROM main.collection_problems WHERE collection_id = ?1 AND deleted_at IS NULL)",
        rusqlite::params![collection_id, now],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{new_id, now_ms, test_support};
    use tempfile::TempDir;

    fn add_collection(c: &Connection, name: &str, sort_key: &str, deleted: bool) -> String {
        let (id, t) = (new_id(), now_ms());
        c.execute(
            "INSERT INTO collections (id, name, sort_key, created_at, updated_at, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            rusqlite::params![id, name, sort_key, t, deleted.then_some(t)],
        )
        .unwrap();
        id
    }

    fn add_member(c: &Connection, collection: &str, problem_id: i64, deleted: bool) {
        let t = now_ms();
        c.execute(
            "INSERT INTO collection_problems (collection_id, problem_id, sort_key, created_at, updated_at, deleted_at)
             VALUES (?1, ?2, 'a0', ?3, ?3, ?4)",
            rusqlite::params![collection, problem_id, t, deleted.then_some(t)],
        )
        .unwrap();
    }

    #[test]
    fn empty_when_the_user_has_none() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        assert!(db.with(|c| list(c)).unwrap().is_empty());
    }

    #[test]
    fn lists_in_sort_order_with_live_counts_only() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        let names = db
            .with(|c| {
                let later = add_collection(c, "Later", "b0", false);
                let first = add_collection(c, "First", "a0", false);
                let gone = add_collection(c, "Gone", "a5", true);
                add_member(c, &first, 1, false);
                add_member(c, &first, 2, false);
                add_member(c, &first, 4, true); // tombstoned membership: not counted
                add_member(c, &later, 1, false);
                add_member(c, &gone, 1, false);
                list(c)
            })
            .unwrap();
        let got: Vec<_> = names.iter().map(|c| (c.name.as_str(), c.problem_count)).collect();
        assert_eq!(got, vec![("First", 2), ("Later", 1)]);
    }

    fn open() -> (TempDir, crate::db::Db) {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        (dir, db)
    }

    /// (name, problem ids) of every live collection, in list order.
    fn contents(c: &Connection) -> Vec<(String, Vec<i64>)> {
        list(c)
            .unwrap()
            .into_iter()
            .map(|col| {
                let ids = c
                    .prepare(
                        "SELECT problem_id FROM collection_problems
                          WHERE collection_id = ? AND deleted_at IS NULL ORDER BY sort_key, problem_id",
                    )
                    .unwrap()
                    .query_map([&col.id], |r| r.get(0))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<i64>>>()
                    .unwrap();
                (col.name, ids)
            })
            .collect()
    }

    #[test]
    fn creating_a_collection_puts_the_problem_in_it() {
        let (_dir, db) = open();
        let made = db.with(|c| create(c, "  Favourites ", 2)).unwrap();
        assert_eq!((made.name.as_str(), made.problem_count), ("Favourites", 1), "name is trimmed");
        let all = db.with(|c| Ok(contents(c))).unwrap();
        assert_eq!(all, vec![("Favourites".to_string(), vec![2])]);
    }

    #[test]
    fn new_collections_go_after_existing_ones() {
        let (_dir, db) = open();
        db.with(|c| {
            create(c, "First", 1)?;
            create(c, "Second", 1)?;
            create(c, "Third", 1)?;
            Ok(())
        })
        .unwrap();
        let names: Vec<_> = db.with(|c| Ok(list(c)?.into_iter().map(|x| x.name).collect())).unwrap();
        assert_eq!(names, vec!["First", "Second", "Third"] as Vec<&str>);
    }

    #[test]
    fn bad_names_and_unknown_problems_are_refused_without_writing() {
        let (_dir, db) = open();
        db.with(|c| {
            create(c, "Mine", 1)?;
            for bad in ["", "   ", &"x".repeat(MAX_NAME_CHARS + 1)] {
                assert!(matches!(create(c, bad, 1), Err(Error::Invalid(_))), "{bad:?}");
            }
            assert!(matches!(create(c, "mine", 1), Err(Error::Invalid(_))), "duplicate, ignoring case");
            assert!(matches!(create(c, "Fresh", 12345), Err(Error::NotFound(_))));
            assert_eq!(contents(c), vec![("Mine".to_string(), vec![1])], "nothing else was written");
            create(c, &"x".repeat(MAX_NAME_CHARS), 1)?; // exactly the limit is fine
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_deleted_collections_name_can_be_used_again() {
        let (_dir, db) = open();
        db.with(|c| {
            let old = create(c, "Hard ones", 4)?;
            remove_problem(c, &old.id, 4)?;
            let again = create(c, "Hard ones", 4)?;
            assert_ne!(old.id, again.id);
            assert_eq!(contents(c), vec![("Hard ones".to_string(), vec![4])]);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_problem_can_be_in_any_number_of_collections() {
        let (_dir, db) = open();
        db.with(|c| {
            let a = create(c, "A", 1)?;
            let b = create(c, "B", 1)?;
            add_problem(c, &a.id, 2)?;
            add_problem(c, &b.id, 2)?;
            add_problem(c, &b.id, 2)?; // adding twice changes nothing
            assert_eq!(
                contents(c),
                vec![("A".to_string(), vec![1, 2]), ("B".to_string(), vec![1, 2])]
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn removing_a_problem_keeps_the_rest_and_a_removed_one_can_come_back() {
        let (_dir, db) = open();
        db.with(|c| {
            let a = create(c, "A", 1)?;
            add_problem(c, &a.id, 2)?;
            add_problem(c, &a.id, 4)?;
            remove_problem(c, &a.id, 1)?;
            remove_problem(c, &a.id, 1)?; // already out: nothing happens
            assert_eq!(contents(c), vec![("A".to_string(), vec![2, 4])]);
            add_problem(c, &a.id, 1)?;
            let mut back = contents(c).remove(0).1;
            back.sort(); // where it sits among the others depends on the clock
            assert_eq!(back, vec![1, 2, 4], "back in again");
            let rows: i64 = c.query_row("SELECT COUNT(*) FROM collection_problems", [], |r| r.get(0))?;
            assert_eq!(rows, 3, "the old row was revived, not duplicated");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn removing_the_last_problem_deletes_the_collection() {
        let (_dir, db) = open();
        db.with(|c| {
            let a = create(c, "A", 1)?;
            let keep = create(c, "Keep", 1)?;
            add_problem(c, &a.id, 2)?;
            remove_problem(c, &a.id, 1)?;
            assert_eq!(list(c)?.len(), 2, "still has a problem");
            remove_problem(c, &a.id, 2)?;
            let left: Vec<_> = list(c)?.into_iter().map(|x| x.id).collect();
            assert_eq!(left, vec![keep.id.clone()]);

            // The collection is a tombstone, not gone from the table, and can't be added to.
            let deleted: Option<i64> =
                c.query_row("SELECT deleted_at FROM collections WHERE id = ?", [&a.id], |r| r.get(0))?;
            assert!(deleted.is_some());
            assert!(matches!(add_problem(c, &a.id, 4), Err(Error::NotFound(_))));
            remove_problem(c, &a.id, 2)?; // removing from a deleted collection is harmless
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_deleted_collection_disappears_from_the_problem_and_its_filter() {
        use crate::db::problems::{self, ProblemFilter, ProblemQuery};
        let (_dir, db) = open();
        db.with(|c| {
            let a = create(c, "A", 1)?;
            let in_a = |c: &Connection| {
                problems::list(
                    c,
                    &ProblemQuery {
                        filter: ProblemFilter { collection_id: Some(a.id.clone()), ..Default::default() },
                        ..Default::default()
                    },
                )
                .unwrap()
                .total
            };
            assert_eq!(in_a(c), 1);
            remove_problem(c, &a.id, 1)?;
            assert_eq!(in_a(c), 0);
            assert!(problems::get_by_id(c, 1)?.unwrap().summary.collection_ids.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn unknown_ids_are_reported() {
        let (_dir, db) = open();
        db.with(|c| {
            let a = create(c, "A", 1)?;
            assert!(matches!(add_problem(c, "nope", 1), Err(Error::NotFound(_))));
            assert!(matches!(add_problem(c, &a.id, 12345), Err(Error::NotFound(_))));
            assert!(matches!(remove_problem(c, "nope", 1), Err(Error::NotFound(_))));
            assert_eq!(contents(c), vec![("A".to_string(), vec![1])]);
            Ok(())
        })
        .unwrap();
    }
}
