//! Starting over: erases everything the user has made or saved in `user.db`, and leaves the app as
//! it is on a first launch (the built-in collections come back, the settings go back to their
//! defaults). The catalog is not the user's and is not touched.
//!
//! The tables are found by asking SQLite, not listed here, so a table a later migration adds can't
//! be forgotten and left holding someone's data. Only `meta`'s `device_id` is kept (it names this
//! installation; it isn't something the user made).
//!
//! Logins and API keys are not in the database: they're in the OS credential store, and the
//! frontend clears them with the commands that already do that.

use std::{fs, io, path::Path};

use rusqlite::Connection;

use rusqlite::params_from_iter;

use super::{default_collections, settings, Result};

/// `meta` keys that survive a reset.
const KEEP_META: &str = "'device_id'";

/// Deletes every user row (in one transaction: it all happens or none of it does), makes the file
/// give the space back, removes the automatic upgrade backups (which are copies of the old data),
/// and puts the built-in collections back.
///
/// With `keep_ai_setup`, the settings that say how the AI coach is set up (provider, model, address,
/// prices, the on/off switch) are kept: the API keys are not in the database, and when the user
/// keeps those, the provider they belong to has to stay as it was or the key can't be found.
pub fn wipe(conn: &mut Connection, keep_ai_setup: bool) -> Result<()> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM main.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' AND name <> 'meta'")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;

    let tx = conn.transaction()?;
    // Rows that point at each other are checked once everything is deleted, not one delete at a time.
    tx.execute_batch("PRAGMA defer_foreign_keys = ON")?;
    for table in &tables {
        if keep_ai_setup && table == "settings" {
            let marks = vec!["?"; settings::COACH_KEYS.len()].join(", ");
            tx.execute(&format!("DELETE FROM main.settings WHERE key NOT IN ({marks})"), params_from_iter(settings::COACH_KEYS))?;
        } else {
            tx.execute(&format!("DELETE FROM main.\"{table}\""), [])?;
        }
    }
    tx.execute(&format!("DELETE FROM main.meta WHERE key NOT IN ({KEEP_META})"), [])?;
    tx.commit()?;

    // Deleted rows would otherwise stay readable in the file's free pages and its write-ahead log.
    // VACUUM rebuilds the file without them, but in WAL mode it writes the rebuilt file to the log
    // first, so the checkpoint that moves it into the file (and empties the log) has to come after.
    conn.execute_batch("VACUUM main")?;
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    remove_upgrade_backups(conn)?;

    // The "built-in collections made" flag went with `meta`, so this makes them again.
    default_collections::seed(conn)
}

/// Deletes `user.db.bak-v*` beside the database: copies of it made before an upgrade.
fn remove_upgrade_backups(conn: &Connection) -> Result<()> {
    let file: String = conn.query_row("SELECT file FROM pragma_database_list WHERE name = 'main'", [], |r| r.get(0))?;
    let path = Path::new(&file);
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return Ok(()) };
    let prefix = format!("{}.bak-v", name.to_string_lossy());
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support;
    use tempfile::TempDir;

    fn count(c: &Connection, table: &str) -> i64 {
        c.query_row(&format!("SELECT COUNT(*) FROM main.\"{table}\""), [], |r| r.get(0)).unwrap()
    }

    /// One row in every table the app has, as if it had been used for a while.
    fn use_the_app(c: &Connection) {
        c.execute_batch(
            "INSERT INTO collections (id, name, sort_key, created_at, updated_at) VALUES ('mine', 'My list', 'z', 1, 1);
             INSERT INTO collection_problems (collection_id, problem_id, sort_key, created_at, updated_at) VALUES ('mine', 2, 'a', 1, 1);
             INSERT INTO problem_state (problem_id, notes, updated_at) VALUES (1, 'my notes', 1);
             INSERT INTO drafts (problem_id, lang, code, updated_at) VALUES (1, 'python3', 'secret code', 1);
             INSERT INTO submissions (id, problem_id, lang, code, status, submitted_at, created_at, updated_at) VALUES ('s1', 1, 'python3', 'secret code', 'Accepted', 1, 1, 1);
             INSERT INTO reviews (id, problem_id, submission_id, reviewed_at, grade, grade_source, created_at, updated_at) VALUES ('r1', 1, 's1', 1, 3, 'inferred', 1, 1);
             INSERT INTO settings (key, value, updated_at) VALUES ('daily_problem_count', '7', 1);
             INSERT INTO daily_sets (day, count, created_at) VALUES ('2026-11-16', 3, 1);
             INSERT INTO daily_set_items (day, position, problem_id, kind, reason) VALUES ('2026-11-16', 0, 1, 'new', 'why');
             INSERT INTO give_ups (id, problem_id, gave_up_at, created_at, updated_at) VALUES ('g1', 2, 1, 1, 1);
             INSERT INTO coach_threads (id, problem_id, created_at, updated_at) VALUES ('t1', 1, 1, 1);
             INSERT INTO coach_messages (id, thread_id, seq, role, body_json, created_at, updated_at) VALUES ('m1', 't1', 0, 'user', '{}', 1, 1);",
        )
        .unwrap();
    }

    #[test]
    fn a_reset_leaves_a_first_launch_and_nothing_of_the_users() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        let device_id = |c: &Connection| -> String {
            c.query_row("SELECT value FROM meta WHERE key = 'device_id'", [], |r| r.get(0)).unwrap()
        };

        let (before_id, builtin_before) = db
            .with(|c| {
                default_collections::seed(c)?;
                use_the_app(c);
                Ok((device_id(c), count(c, "collections") - 1)) // minus the user's own
            })
            .unwrap();
        assert!(builtin_before >= 1, "the fixture catalog has problems from the built-in lists");

        db.with(|c| wipe(c, false)).unwrap();

        db.with(|c| {
            // Every table: empty, except the built-in collections that a first launch has.
            let tables: Vec<String> = c
                .prepare("SELECT name FROM main.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'")?
                .query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            for table in tables.iter().filter(|t| !["meta", "collections", "collection_problems"].contains(&t.as_str())) {
                assert_eq!(count(c, table), 0, "{table} still has rows");
            }
            // Only the built-ins are left, and the user's own list is gone.
            assert_eq!(count(c, "collections"), builtin_before);
            let mine: i64 = c.query_row("SELECT COUNT(*) FROM collections WHERE id = 'mine'", [], |r| r.get(0))?;
            assert_eq!(mine, 0);
            let strays: i64 = c.query_row(
                "SELECT COUNT(*) FROM collection_problems WHERE collection_id NOT LIKE 'builtin-%'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(strays, 0);
            // The installation's id stays; nothing else is in `meta` but the built-ins flag.
            assert_eq!(device_id(c), before_id);
            assert_eq!(count(c, "meta"), 2);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn keeping_the_ai_setup_keeps_the_provider_the_keys_belong_to_and_nothing_else() {
        use crate::db::settings::CoachSettings;
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        db.with(|c| {
            use_the_app(c); // includes an ordinary setting (the daily count)
            settings::set_coach(
                c,
                &CoachSettings {
                    enabled: true,
                    provider: "openai".into(),
                    model: "gpt-x".into(),
                    base_url: String::new(),
                    price_input: Some(1.5),
                    price_output: Some(6.0),
                },
            )?;

            wipe(c, true)?;
            let kept = settings::coach(c)?;
            assert_eq!((kept.provider.as_str(), kept.model.as_str()), ("openai", "gpt-x"), "the provider the key was saved for");
            assert_eq!((kept.price_input, kept.price_output, kept.enabled), (Some(1.5), Some(6.0), true));
            // Only the AI setup stayed: every other setting, and all the other data, is gone.
            let other: i64 = c.query_row("SELECT COUNT(*) FROM settings WHERE key NOT LIKE 'ai\\_%' ESCAPE '\\'", [], |r| r.get(0))?;
            assert_eq!(other, 0);
            for table in ["drafts", "submissions", "reviews", "coach_threads", "coach_messages", "problem_state"] {
                assert_eq!(count(c, table), 0, "{table}");
            }

            // Not keeping it: back to the defaults, as on a first launch.
            wipe(c, false)?;
            assert_eq!(settings::coach(c)?.provider, "anthropic");
            assert_eq!(count(c, "settings"), 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_reset_app_works_and_can_be_reset_again() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        db.with(|c| {
            wipe(c, false)?; // nothing to erase yet: fine
            use_the_app(c);
            wipe(c, false)?;
            // New data after a reset is stored as normal.
            c.execute(
                "INSERT INTO drafts (problem_id, lang, code, updated_at) VALUES (1, 'python3', 'new', 1)",
                [],
            )?;
            assert_eq!(count(c, "drafts"), 1);
            wipe(c, false)?;
            assert_eq!(count(c, "drafts"), 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn the_upgrade_backups_go_but_other_files_stay() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        for name in ["user.db.bak-v1", "user.db.bak-v3", "catalog.db.bak-v1", "notes.txt"] {
            fs::write(dir.path().join(name), "x").unwrap();
        }
        db.with(|c| wipe(c, false)).unwrap();
        assert!(!dir.path().join("user.db.bak-v1").exists());
        assert!(!dir.path().join("user.db.bak-v3").exists());
        // Not this database's backups, and not a backup at all.
        assert!(dir.path().join("catalog.db.bak-v1").exists());
        assert!(dir.path().join("notes.txt").exists());
        assert!(dir.path().join("user.db").exists() && dir.path().join("catalog.db").exists());
    }

    #[test]
    fn a_wiped_database_no_longer_contains_the_old_text() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        db.with(|c| {
            use_the_app(c);
            c.execute("UPDATE drafts SET code = ?1", ["unmistakable-needle-12345"])?;
            Ok(())
        })
        .unwrap();
        db.with(|c| wipe(c, false)).unwrap();
        // Look at the raw bytes of the file and its log: the text must be gone from both.
        for name in ["user.db", "user.db-wal"] {
            if let Ok(bytes) = fs::read(dir.path().join(name)) {
                let found = bytes.windows(25).any(|w| w == b"unmistakable-needle-12345");
                assert!(!found, "{name} still holds the deleted text");
            }
        }
    }
}
