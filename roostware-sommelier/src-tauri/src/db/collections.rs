//! The user's collections: custom, user-defined groups of problems (not
//! LeetCode's topic tags). Deleted collections and memberships are tombstones
//! and never shown.

use rusqlite::Connection;
use serde::Serialize;

use super::Result;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Problems currently in the collection.
    pub problem_count: i64,
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
            Ok(CollectionSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                problem_count: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(collections)
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
}
