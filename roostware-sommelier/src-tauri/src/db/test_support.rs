//! Shared fixtures for db tests.

use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;

use super::Db;

/// The real catalog schema, shared with scripts/build-catalog.mjs so the
/// fixture cannot drift from what the build script produces.
const CATALOG_SCHEMA: &str = include_str!("../../../scripts/catalog-schema.sql");

/// (id, frontend_id, title, slug, difficulty, paid, acceptance_rate, tags)
const PROBLEMS: &[(i64, i64, &str, &str, &str, bool, f64, &[&str])] = &[
    (1, 1, "Two Sum", "two-sum", "Easy", false, 0.55, &["Array", "Hash Table"]),
    (2, 2, "Add Two Numbers", "add-two-numbers", "Medium", false, 0.45, &["Linked List", "Math"]),
    (4, 4, "Median of Two Sorted Arrays", "median-of-two-sorted-arrays", "Hard", false, 0.38, &["Array", "Binary Search"]),
    // Internal id deliberately differs from the displayed number.
    (900, 146, "LRU Cache", "lru-cache", "Medium", true, 0.40, &["Design", "Hash Table"]),
];

pub fn build_fixture_catalog(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(CATALOG_SCHEMA).unwrap();
    conn.execute("INSERT INTO meta VALUES ('schema_version', '1')", []).unwrap();

    for &(id, frontend_id, title, slug, difficulty, paid, acceptance, tags) in PROBLEMS {
        conn.execute(
            "INSERT INTO problems (id, frontend_id, title, title_slug, difficulty, content_html,
               example_test_cases, is_paid_only, likes, dislikes, total_accepted, total_submissions,
               acceptance_rate, created_at_approx)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7, 7, 1, 10, 20, ?8, NULL)",
            rusqlite::params![id, frontend_id, title, slug, difficulty, format!("<p>{title}</p>"), paid, acceptance],
        )
        .unwrap();
        for tag in tags {
            conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [tag]).unwrap();
            conn.execute(
                "INSERT INTO problem_tags (problem_id, tag_id) SELECT ?1, id FROM tags WHERE name = ?2",
                rusqlite::params![id, tag],
            )
            .unwrap();
        }
        for lang in ["python3", "cpp"] {
            conn.execute(
                "INSERT INTO code_snippets (problem_id, lang, code) VALUES (?1, ?2, ?3)",
                // C++ gets two lines so tests can exercise line-ending handling.
                rusqlite::params![
                    id,
                    lang,
                    if lang == "cpp" {
                        format!("// {lang} starter for {title}\nsecond line")
                    } else {
                        format!("// {lang} starter for {title}")
                    }
                ],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO problems_fts (rowid, title, frontend_id, tags) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, title, frontend_id.to_string(), tags.join(" ")],
        )
        .unwrap();
    }
}

/// A fresh user.db plus the fixture catalog, both inside `dir`.
pub fn open_with_fixture(dir: &TempDir) -> Db {
    let catalog = dir.path().join("catalog.db");
    build_fixture_catalog(&catalog);
    Db::open_at(&dir.path().join("user.db"), &catalog).unwrap()
}

/// A fresh user.db attached to the real catalog built by `npm run catalog`,
/// or `None` if it hasn't been built. Keep the `TempDir` alive while using the `Db`.
pub fn open_real_catalog() -> Option<(TempDir, Db)> {
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("catalog.db");
    real.exists().then(|| {
        let dir = TempDir::new().unwrap();
        let db = Db::open_at(&dir.path().join("user.db"), &real).unwrap();
        (dir, db)
    })
}
