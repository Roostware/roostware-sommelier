//! Persistent storage.
//!
//! Two SQLite files, one connection:
//!   * `user.db`    — read-write, in the app data dir; everything the user creates.
//!   * `catalog.db` — read-only LeetCode problem data shipped as a bundled
//!     resource, attached under the schema name `catalog`.
//!
//! Queries can join across them (`catalog.problems` / `main.collections`, …).
//! `Db` is managed Tauri state; commands reach the connection through [`Db::with`].

mod error;
pub mod coach;
pub mod collections;
pub mod default_collections;
pub mod drafts;
pub mod give_ups;
pub mod migrations;
pub mod problems;
pub mod rating;
pub mod recommendations;
pub mod reset;
pub mod reviews;
pub mod settings;
pub mod submissions;

#[cfg(test)]
pub(crate) mod test_support;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension};
use tauri::{path::BaseDirectory, AppHandle, Manager};

pub use error::{Error, Result};

/// The catalog `meta.schema_version` this build understands. Must match
/// SCHEMA_VERSION in scripts/build-catalog.mjs.
pub const CATALOG_SCHEMA_VERSION: i64 = 1;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Opens (creating and migrating if needed) user.db in the app data dir and
    /// attaches the bundled catalog.
    pub fn open(app: &AppHandle) -> Result<Self> {
        let data_dir = app.path().app_data_dir().map_err(|e| Error::Path(e.to_string()))?;
        fs::create_dir_all(&data_dir)?;
        let db = Self::open_at(&data_dir.join("user.db"), &catalog_path(app)?)?;
        // Only the real app makes the built-in collections (`open_at` is what tests use, and they
        // start with none). Not worth refusing to start over: it is retried at the next launch.
        if let Err(e) = db.with(|conn| default_collections::seed(conn)) {
            eprintln!("Couldn't set up the built-in collections: {e}");
        }
        Ok(db)
    }

    pub fn open_at(user_db: &Path, catalog: &Path) -> Result<Self> {
        let mut conn = Connection::open(user_db)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;

        // Before upgrading an existing database, keep a consistent copy of it.
        // (Migrations are transactional, but a backup is cheap insurance for
        // the only copy of the user's history.)
        let version = migrations::current_version(&conn)?;
        if version > 0 && version < migrations::ALL.len() as i64 {
            backup(&conn, &user_db.with_extension(format!("db.bak-v{version}")))?;
        }
        migrations::run(&mut conn, migrations::ALL)?;
        ensure_device_id(&conn)?;

        attach_catalog(&conn, catalog)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Runs `f` with exclusive access to the connection. Keep `f` short: it
    /// holds the lock, and sync work here blocks other commands.
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        // A poisoned lock only means another command panicked; rusqlite rolls
        // back its open transaction on unwind, so the connection is still sound.
        let mut conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut conn)
    }
}

/// Epoch milliseconds, UTC — the timestamp format used throughout user.db.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A new UUIDv7 (time-sortable) primary key for a user-created row.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn ensure_device_id(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO meta (key, value) VALUES ('device_id', ?1)",
        [new_id()],
    )?;
    Ok(())
}

fn backup(conn: &Connection, dest: &Path) -> Result<()> {
    if dest.exists() {
        return Ok(()); // already backed up at this version
    }
    // VACUUM INTO produces a consistent copy even with a WAL in play.
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
    Ok(())
}

fn catalog_path(app: &AppHandle) -> Result<PathBuf> {
    // In debug builds read the freshly built file straight from the source
    // tree, so `npm run catalog` takes effect without a Rust rebuild.
    #[cfg(debug_assertions)]
    {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("catalog.db");
        if dev.exists() {
            return Ok(dev);
        }
    }
    app.path()
        .resolve("resources/catalog.db", BaseDirectory::Resource)
        .map_err(|e| Error::Path(e.to_string()))
}

fn attach_catalog(conn: &Connection, catalog: &Path) -> Result<()> {
    if !catalog.is_file() {
        return Err(Error::CatalogMissing(catalog.to_path_buf()));
    }
    // mode=ro makes SQLite refuse writes to the catalog no matter what SQL runs.
    conn.execute("ATTACH DATABASE ?1 AS catalog", [file_uri_read_only(catalog)])?;

    let found: Option<String> = conn
        .query_row("SELECT value FROM catalog.meta WHERE key = 'schema_version'", [], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    if found.as_deref().and_then(|v| v.parse::<i64>().ok()) != Some(CATALOG_SCHEMA_VERSION) {
        return Err(Error::CatalogVersion { found, expected: CATALOG_SCHEMA_VERSION });
    }
    Ok(())
}

/// `file:` URI for a local path with `?mode=ro`, percent-encoded so Windows
/// drive letters, spaces and `?`/`#` in the path survive.
fn file_uri_read_only(path: &Path) -> String {
    let raw = path.to_string_lossy();
    // Tauri/canonicalize can yield verbatim `\\?\C:\…` paths on Windows.
    let raw = raw.strip_prefix(r"\\?\").unwrap_or(&raw).replace('\\', "/");
    let mut uri = String::from("file://");
    if !raw.starts_with('/') {
        uri.push('/'); // file:///C:/… for drive-letter paths
    }
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                uri.push(b as char)
            }
            _ => uri.push_str(&format!("%{b:02X}")),
        }
    }
    uri.push_str("?mode=ro");
    uri
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A tiny stand-in catalog with the same `meta` contract as the real one.
    fn fixture_catalog(dir: &Path, schema_version: &str) -> PathBuf {
        let path = dir.join("catalog.db");
        if path.exists() {
            return path; // reopening in the same test dir
        }
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
             CREATE TABLE problems (id INTEGER PRIMARY KEY, title TEXT NOT NULL);
             INSERT INTO problems VALUES (1, 'Two Sum'), (2, 'Add Two Numbers');",
        )
        .unwrap();
        conn.execute("INSERT INTO meta VALUES ('schema_version', ?1)", [schema_version]).unwrap();
        path
    }

    fn open(dir: &TempDir) -> Db {
        let catalog = fixture_catalog(dir.path(), "1");
        Db::open_at(&dir.path().join("user.db"), &catalog).unwrap()
    }

    #[test]
    fn fresh_open_migrates_and_sets_device_id() {
        let dir = TempDir::new().unwrap();
        let db = open(&dir);
        db.with(|c| {
            assert_eq!(migrations::current_version(c)?, migrations::ALL.len() as i64);
            for table in [
                "collections", "collection_problems", "problem_state", "drafts", "submissions", "reviews",
                "settings", "daily_sets", "daily_set_items", "give_ups", "coach_threads", "coach_messages",
            ] {
                let n: i64 = c.query_row(
                    "SELECT COUNT(*) FROM main.sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |r| r.get(0),
                )?;
                assert_eq!(n, 1, "missing table {table}");
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn device_id_is_stable_across_reopen() {
        let dir = TempDir::new().unwrap();
        let get = |db: &Db| -> String {
            db.with(|c| Ok(c.query_row("SELECT value FROM meta WHERE key = 'device_id'", [], |r| r.get(0))?))
                .unwrap()
        };
        let first = get(&open(&dir));
        let second = get(&open(&dir));
        assert_eq!(first, second);
        assert_eq!(first.len(), 36);
    }

    #[test]
    fn can_join_user_data_with_catalog() {
        let dir = TempDir::new().unwrap();
        let db = open(&dir);
        let title: String = db
            .with(|c| {
                let t = now_ms();
                let cid = new_id();
                c.execute(
                    "INSERT INTO collections (id, name, sort_key, created_at, updated_at) VALUES (?1, 'Blind 75', 'a0', ?2, ?2)",
                    (&cid, t),
                )?;
                c.execute(
                    "INSERT INTO collection_problems (collection_id, problem_id, sort_key, created_at, updated_at) VALUES (?1, 2, 'a0', ?2, ?2)",
                    (&cid, t),
                )?;
                Ok(c.query_row(
                    "SELECT p.title FROM collection_problems cp JOIN catalog.problems p ON p.id = cp.problem_id WHERE cp.collection_id = ?1",
                    [&cid],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(title, "Add Two Numbers");
    }

    #[test]
    fn catalog_is_read_only() {
        let dir = TempDir::new().unwrap();
        let db = open(&dir);
        let err = db
            .with(|c| Ok(c.execute("INSERT INTO catalog.problems VALUES (3, 'Nope')", [])?))
            .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("readonly"), "got: {err}");
    }

    #[test]
    fn foreign_keys_are_enforced_within_user_db() {
        let dir = TempDir::new().unwrap();
        let db = open(&dir);
        let res = db.with(|c| {
            Ok(c.execute(
                "INSERT INTO collection_problems (collection_id, problem_id, sort_key, created_at, updated_at) VALUES ('missing', 1, 'a0', 0, 0)",
                [],
            )?)
        });
        assert!(res.is_err());
    }

    #[test]
    fn rejects_wrong_catalog_version() {
        let dir = TempDir::new().unwrap();
        let catalog = fixture_catalog(dir.path(), "99");
        let err = Db::open_at(&dir.path().join("user.db"), &catalog).err().unwrap();
        assert!(matches!(err, Error::CatalogVersion { .. }), "got: {err}");
    }

    #[test]
    fn rejects_missing_catalog() {
        let dir = TempDir::new().unwrap();
        let err = Db::open_at(&dir.path().join("user.db"), &dir.path().join("nope.db")).err().unwrap();
        assert!(matches!(err, Error::CatalogMissing(_)), "got: {err}");
    }

    #[test]
    fn rejects_user_db_from_newer_build_without_touching_it() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("user.db");
        Connection::open(&path).unwrap().pragma_update(None, "user_version", 999).unwrap();
        let catalog = fixture_catalog(dir.path(), "1");
        let err = Db::open_at(&path, &catalog).err().unwrap();
        assert!(matches!(err, Error::UserDbTooNew { found: 999, .. }), "got: {err}");
        assert_eq!(migrations::current_version(&Connection::open(&path).unwrap()).unwrap(), 999);
    }

    #[test]
    fn migrations_upgrade_in_place_and_keep_data() {
        let dir = TempDir::new().unwrap();
        let mut conn = Connection::open(dir.path().join("t.db")).unwrap();
        migrations::run(&mut conn, &["CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (1);"]).unwrap();
        migrations::run(&mut conn, &[
            "CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (1);",
            "ALTER TABLE t ADD COLUMN b INTEGER DEFAULT 7;",
        ])
        .unwrap();
        assert_eq!(migrations::current_version(&conn).unwrap(), 2);
        let (a, b): (i64, i64) = conn.query_row("SELECT a, b FROM t", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((a, b), (1, 7));
    }

    #[test]
    fn failed_migration_rolls_back_and_keeps_last_good_version() {
        let dir = TempDir::new().unwrap();
        let mut conn = Connection::open(dir.path().join("t.db")).unwrap();
        let res = migrations::run(&mut conn, &[
            "CREATE TABLE t (a INTEGER);",
            "CREATE TABLE u (a INTEGER); THIS IS NOT SQL;",
        ]);
        assert!(res.is_err());
        assert_eq!(migrations::current_version(&conn).unwrap(), 1);
        let u: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'u'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(u, 0, "partial migration was not rolled back");
    }

    #[test]
    fn backup_is_a_readable_copy() {
        let dir = TempDir::new().unwrap();
        let conn = Connection::open(dir.path().join("a.db")).unwrap();
        conn.execute_batch("CREATE TABLE t (a); INSERT INTO t VALUES (42);").unwrap();
        let dest = dir.path().join("a.bak");
        backup(&conn, &dest).unwrap();
        let copy = Connection::open(&dest).unwrap();
        assert_eq!(copy.query_row("SELECT a FROM t", [], |r| r.get::<_, i64>(0)).unwrap(), 42);
        backup(&conn, &dest).unwrap(); // second call is a no-op, not an error
    }

    #[test]
    fn file_uri_encodes_windows_and_special_paths() {
        assert_eq!(
            file_uri_read_only(Path::new(r"\\?\C:\Users\A B\data#1\catalog.db")),
            "file:///C:/Users/A%20B/data%231/catalog.db?mode=ro"
        );
        assert_eq!(file_uri_read_only(Path::new("/opt/app/c.db")), "file:///opt/app/c.db?mode=ro");
    }

    /// Smoke test against the real artifact from `npm run catalog`, when present.
    #[test]
    fn real_catalog_attaches_and_searches() {
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("catalog.db");
        if !real.exists() {
            eprintln!("skipping: {} not built", real.display());
            return;
        }
        let dir = TempDir::new().unwrap();
        let db = Db::open_at(&dir.path().join("user.db"), &real).unwrap();
        db.with(|c| {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM catalog.problems", [], |r| r.get(0))?;
            assert!(n > 1000, "only {n} problems");
            // FTS5 must be available in the bundled SQLite.
            let first: String = c.query_row(
                "SELECT p.title FROM catalog.problems_fts f JOIN catalog.problems p ON p.id = f.rowid \
                 WHERE problems_fts MATCH 'two sum' ORDER BY f.rank LIMIT 1",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(first, "Two Sum");
            Ok(())
        })
        .unwrap();
    }
}
