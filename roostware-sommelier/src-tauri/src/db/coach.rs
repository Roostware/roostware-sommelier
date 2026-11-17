//! The AI coach's saved conversations: one live thread per problem, the messages in it, and how
//! many tokens they used.
//!
//! The database only keeps what the frontend gives it. A message's body and a thread's state are
//! JSON the frontend owns (see `src/lib/coach.svelte.ts`), so the coach's features can change
//! without a migration each time; what the database does check is that they're JSON and not
//! absurdly big. "New conversation" tombstones the live thread; the token totals keep counting
//! everything ever said about a problem, since that money was spent either way.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{new_id, now_ms, Error, Result};

/// A message's body can hold a whole analysis or solution, but not more than this.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// The code a message was written about.
const MAX_CODE_BYTES: usize = 256 * 1024;
const MAX_STATE_BYTES: usize = 64 * 1024;
/// A thread this long is a runaway; it must be started over.
const MAX_MESSAGES: i64 = 500;

/// One saved message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRecord {
    /// Its place in the thread, assigned by the frontend. Saving the same `seq` again replaces it.
    pub seq: i64,
    /// "user" or "coach".
    pub role: String,
    pub label: Option<String>,
    pub body: Value,
    /// The code the message was written about, so its line references can be checked later.
    pub about_code: Option<String>,
    /// What the provider said the reply used.
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTotals {
    pub input: i64,
    pub output: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// The frontend's own bookkeeping, as saved.
    pub state: Value,
    /// In order.
    pub messages: Vec<MessageRecord>,
}

/// What a problem's saved coaching looks like.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loaded {
    /// The live conversation, if there is one.
    pub thread: Option<Thread>,
    /// Tokens used by every conversation about this problem, live or not.
    pub problem_usage: TokenTotals,
}

fn live_thread(conn: &Connection, problem_id: i64) -> Result<Option<(String, String)>> {
    Ok(conn
        .query_row(
            "SELECT id, state_json FROM main.coach_threads
              WHERE problem_id = ? AND deleted_at IS NULL
              ORDER BY created_at DESC, id DESC LIMIT 1",
            [problem_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// The live thread's id, making one if the problem has none. Fails if the problem doesn't exist.
fn ensure_thread(conn: &Connection, problem_id: i64) -> Result<String> {
    let exists: bool =
        conn.query_row("SELECT EXISTS (SELECT 1 FROM catalog.problems WHERE id = ?)", [problem_id], |r| r.get(0))?;
    if !exists {
        return Err(Error::NotFound(format!("problem {problem_id}")));
    }
    if let Some((id, _)) = live_thread(conn, problem_id)? {
        return Ok(id);
    }
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO main.coach_threads (id, problem_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![id, problem_id, now],
    )?;
    Ok(id)
}

/// The problem's live conversation and its token totals.
pub fn load(conn: &Connection, problem_id: i64) -> Result<Loaded> {
    let (input, output): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(m.input_tokens), 0), COALESCE(SUM(m.output_tokens), 0)
           FROM main.coach_messages m JOIN main.coach_threads t ON t.id = m.thread_id
          WHERE t.problem_id = ?",
        [problem_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let problem_usage = TokenTotals { input, output };

    let Some((id, state)) = live_thread(conn, problem_id)? else {
        return Ok(Loaded { thread: None, problem_usage });
    };
    let mut stmt = conn.prepare(
        "SELECT seq, role, label, body_json, about_code, input_tokens, output_tokens
           FROM main.coach_messages WHERE thread_id = ? AND deleted_at IS NULL ORDER BY seq",
    )?;
    let rows = stmt.query_map([&id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get::<_, String>(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
    })?;
    let mut messages = Vec::new();
    for row in rows {
        let (seq, role, label, body, about_code, input_tokens, output_tokens) = row?;
        // A body that can't be read back is skipped rather than losing the whole conversation.
        let Ok(body) = serde_json::from_str(&body) else { continue };
        messages.push(MessageRecord { seq, role, label, body, about_code, input_tokens, output_tokens });
    }
    let state = serde_json::from_str(&state).unwrap_or(Value::Null);
    Ok(Loaded { thread: Some(Thread { state, messages }), problem_usage })
}

/// Saves a message to the problem's live thread (making the thread if need be), replacing any
/// message already saved at the same `seq`.
pub fn save_message(conn: &mut Connection, problem_id: i64, message: &MessageRecord) -> Result<()> {
    if message.role != "user" && message.role != "coach" {
        return Err(Error::Invalid(format!("{:?} is not a role a message can have", message.role)));
    }
    if message.seq < 0 {
        return Err(Error::Invalid("a message's place in the thread can't be negative".into()));
    }
    let body = serde_json::to_string(&message.body).map_err(|e| Error::Invalid(e.to_string()))?;
    if body.len() > MAX_BODY_BYTES {
        return Err(Error::Invalid("that message is too large to save".into()));
    }
    if message.about_code.as_ref().is_some_and(|c| c.len() > MAX_CODE_BYTES) {
        return Err(Error::Invalid("the code this message is about is too large to save".into()));
    }

    let tx = conn.transaction()?;
    let thread = ensure_thread(&tx, problem_id)?;
    let count: i64 =
        tx.query_row("SELECT COUNT(*) FROM main.coach_messages WHERE thread_id = ?", [&thread], |r| r.get(0))?;
    let replacing: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM main.coach_messages WHERE thread_id = ?1 AND seq = ?2)",
        params![thread, message.seq],
        |r| r.get(0),
    )?;
    if !replacing && count >= MAX_MESSAGES {
        return Err(Error::Invalid("this conversation is too long to keep going: start a new one".into()));
    }
    let now = now_ms();
    tx.execute(
        "INSERT INTO main.coach_messages
            (id, thread_id, seq, role, label, body_json, about_code, input_tokens, output_tokens, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
         ON CONFLICT (thread_id, seq) DO UPDATE SET
            role = excluded.role, label = excluded.label, body_json = excluded.body_json,
            about_code = excluded.about_code, input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens, deleted_at = NULL, updated_at = excluded.updated_at",
        params![
            new_id(),
            thread,
            message.seq,
            message.role,
            message.label,
            body,
            message.about_code,
            message.input_tokens,
            message.output_tokens,
            now
        ],
    )?;
    tx.execute("UPDATE main.coach_threads SET updated_at = ?2 WHERE id = ?1", params![thread, now])?;
    tx.commit()?;
    Ok(())
}

/// Saves the frontend's bookkeeping for the problem's live thread (making the thread if need be).
pub fn save_state(conn: &mut Connection, problem_id: i64, state: &Value) -> Result<()> {
    let json = serde_json::to_string(state).map_err(|e| Error::Invalid(e.to_string()))?;
    if json.len() > MAX_STATE_BYTES {
        return Err(Error::Invalid("that conversation's state is too large to save".into()));
    }
    let tx = conn.transaction()?;
    let thread = ensure_thread(&tx, problem_id)?;
    tx.execute(
        "UPDATE main.coach_threads SET state_json = ?2, updated_at = ?3 WHERE id = ?1",
        params![thread, json, now_ms()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Ends the problem's live conversation. The next message starts a new one. Its token counts stay
/// in the problem's totals.
pub fn new_thread(conn: &Connection, problem_id: i64) -> Result<()> {
    let now = now_ms();
    conn.execute(
        "UPDATE main.coach_threads SET deleted_at = ?2, updated_at = ?2 WHERE problem_id = ?1 AND deleted_at IS NULL",
        params![problem_id, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{test_support, Db};
    use serde_json::json;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Db) {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        (dir, db)
    }

    fn message(seq: i64, role: &str, text: &str) -> MessageRecord {
        MessageRecord {
            seq,
            role: role.into(),
            label: None,
            body: json!({ "kind": "text", "text": text }),
            about_code: None,
            input_tokens: None,
            output_tokens: None,
        }
    }

    fn save(db: &Db, problem: i64, m: &MessageRecord) {
        db.with(|c| save_message(c, problem, m)).unwrap();
    }

    #[test]
    fn a_problem_with_nothing_saved_has_no_thread_and_no_usage() {
        let (_d, db) = setup();
        let loaded = db.with(|c| load(c, 1)).unwrap();
        assert_eq!(loaded, Loaded { thread: None, problem_usage: TokenTotals::default() });
    }

    #[test]
    fn messages_and_state_round_trip_in_order() {
        let (_d, db) = setup();
        let mut reply = message(1, "coach", "Think about what you've already seen.");
        reply.label = Some("Hint 1 of 3 · Idea".into());
        reply.about_code = Some("def f(): pass".into());
        reply.input_tokens = Some(1200);
        reply.output_tokens = Some(80);
        // Saved out of order: loaded by seq.
        save(&db, 1, &reply);
        save(&db, 1, &message(0, "user", "Give me a hint"));
        db.with(|c| save_state(c, 1, &json!({ "hintLevel": 1, "debugLevels": {} }))).unwrap();

        let loaded = db.with(|c| load(c, 1)).unwrap();
        let thread = loaded.thread.expect("a thread");
        assert_eq!(thread.messages.iter().map(|m| m.seq).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(thread.messages[1], reply);
        assert_eq!(thread.state, json!({ "hintLevel": 1, "debugLevels": {} }));
        assert_eq!(loaded.problem_usage, TokenTotals { input: 1200, output: 80 });
        // Other problems are untouched.
        assert_eq!(db.with(|c| load(c, 2)).unwrap().thread, None);
    }

    #[test]
    fn saving_the_same_place_again_replaces_the_message() {
        let (_d, db) = setup();
        save(&db, 1, &message(0, "coach", "partial"));
        let mut done = message(0, "coach", "the whole reply");
        done.output_tokens = Some(10);
        save(&db, 1, &done);
        let thread = db.with(|c| load(c, 1)).unwrap().thread.unwrap();
        assert_eq!(thread.messages, [done]);
    }

    #[test]
    fn a_new_conversation_starts_empty_but_the_usage_keeps_counting() {
        let (_d, db) = setup();
        let mut first = message(0, "coach", "one");
        first.input_tokens = Some(100);
        first.output_tokens = Some(20);
        save(&db, 1, &first);
        db.with(|c| new_thread(c, 1)).unwrap();

        let after = db.with(|c| load(c, 1)).unwrap();
        assert_eq!(after.thread, None);
        assert_eq!(after.problem_usage, TokenTotals { input: 100, output: 20 });

        let mut second = message(0, "coach", "two");
        second.input_tokens = Some(50);
        second.output_tokens = Some(5);
        save(&db, 1, &second);
        let loaded = db.with(|c| load(c, 1)).unwrap();
        assert_eq!(loaded.thread.unwrap().messages, [second], "the old conversation doesn't come back");
        assert_eq!(loaded.problem_usage, TokenTotals { input: 150, output: 25 });
    }

    #[test]
    fn bad_messages_are_refused_and_nothing_is_written() {
        let (_d, db) = setup();
        let refused = |m: &MessageRecord, problem: i64| {
            matches!(db.with(|c| save_message(c, problem, m)), Err(Error::Invalid(_)) | Err(Error::NotFound(_)))
        };
        assert!(refused(&message(0, "robot", "x"), 1));
        assert!(refused(&message(-1, "user", "x"), 1));
        assert!(refused(&message(0, "user", "x"), 12345), "no such problem");
        let mut big = message(0, "coach", "x");
        big.body = json!({ "kind": "text", "text": "x".repeat(MAX_BODY_BYTES) });
        assert!(refused(&big, 1));
        let mut code = message(0, "coach", "x");
        code.about_code = Some("x".repeat(MAX_CODE_BYTES + 1));
        assert!(refused(&code, 1));
        let n: i64 = db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM coach_threads", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 0, "a refused message doesn't leave an empty thread behind");
    }

    #[test]
    fn a_runaway_conversation_is_stopped_but_old_places_can_still_be_rewritten() {
        let (_d, db) = setup();
        db.with(|c| {
            for seq in 0..MAX_MESSAGES {
                save_message(c, 1, &message(seq, "user", "x"))?;
            }
            Ok(())
        })
        .unwrap();
        assert!(matches!(db.with(|c| save_message(c, 1, &message(MAX_MESSAGES, "user", "x"))), Err(Error::Invalid(_))));
        db.with(|c| save_message(c, 1, &message(3, "user", "edited"))).unwrap();
    }

    #[test]
    fn an_unreadable_body_is_skipped_not_fatal() {
        let (_d, db) = setup();
        save(&db, 1, &message(0, "user", "fine"));
        save(&db, 1, &message(1, "coach", "will be damaged"));
        db.with(|c| Ok(c.execute("UPDATE coach_messages SET body_json = 'not json' WHERE seq = 1", [])?)).unwrap();
        let thread = db.with(|c| load(c, 1)).unwrap().thread.unwrap();
        assert_eq!(thread.messages.len(), 1);
    }

    #[test]
    fn the_loaded_shape_matches_what_the_frontend_reads() {
        let (_d, db) = setup();
        save(&db, 1, &message(0, "user", "hi"));
        let json = serde_json::to_value(db.with(|c| load(c, 1)).unwrap()).unwrap();
        assert!(json["problemUsage"]["input"].is_number() && json["problemUsage"]["output"].is_number());
        let m = &json["thread"]["messages"][0];
        for key in ["seq", "role", "label", "body", "aboutCode", "inputTokens", "outputTokens"] {
            assert!(m.get(key).is_some(), "missing {key}");
        }
    }
}
