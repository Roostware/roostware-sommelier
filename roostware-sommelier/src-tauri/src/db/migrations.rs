//! Ordered, append-only migrations for user.db, tracked with `PRAGMA user_version`.
//!
//! To change the schema, add `migrations/000N_name.sql` and append it to
//! [`ALL`]. Never edit a migration that has shipped.

use rusqlite::Connection;

use super::{Error, Result};

pub const ALL: &[&str] = &[include_str!("migrations/0001_init.sql")];

pub fn current_version(conn: &Connection) -> Result<i64> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Applies every migration in `migrations` newer than the database's
/// `user_version`, each in its own transaction (so a failed one leaves the
/// database at the last good version). Refuses a database from a newer build.
pub fn run(conn: &mut Connection, migrations: &[&str]) -> Result<()> {
    let current = current_version(conn)?;
    let latest = migrations.len() as i64;
    if current > latest {
        return Err(Error::UserDbTooNew { found: current, supported: latest });
    }
    for (i, sql) in migrations.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}
