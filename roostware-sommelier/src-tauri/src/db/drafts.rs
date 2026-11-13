//! Unsubmitted work: the code the user has written in the editor, per problem and language.
//!
//! The rule this module owns: **a draft exists only while the code differs from where the
//! user is starting from.** That baseline is the code of their latest submission in the
//! language, or the problem's starter code if they haven't submitted in it. Saving code that
//! matches the baseline removes the draft instead (so undoing back to it leaves nothing
//! behind), and a problem with no submissions counts as "in progress" exactly when it has a
//! draft. Line endings are ignored when comparing, so a CRLF/LF difference alone is not a
//! change.
//!
//! Submitting settles the draft too, see `submissions::record`.

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use super::{now_ms, submissions, Error, Result};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// LeetCode langSlug (python3, cpp, …).
    pub lang: String,
    pub code: String,
    /// Epoch milliseconds, UTC.
    pub updated_at: i64,
}

pub(super) fn same_code(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n") == b.replace("\r\n", "\n")
}

/// Stores the user's code for a problem in a language. Returns `true` if a draft is now
/// stored, or `false` if the code matches the baseline (the latest submission in that language,
/// else the starter code), so any draft was removed.
/// Fails, writing nothing, if the problem has no starter code in that language.
pub fn save(conn: &Connection, problem_id: i64, lang: &str, code: &str) -> Result<bool> {
    let starter: String = conn
        .query_row(
            "SELECT code FROM catalog.code_snippets WHERE problem_id = ?1 AND lang = ?2",
            rusqlite::params![problem_id, lang],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("{lang} starter code for problem {problem_id}")))?;

    let baseline = submissions::latest_code(conn, problem_id, lang)?.unwrap_or(starter);
    if same_code(code, &baseline) {
        delete(conn, problem_id, lang)?;
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO main.drafts (problem_id, lang, code, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (problem_id, lang) DO UPDATE
           SET code = excluded.code, updated_at = excluded.updated_at",
        rusqlite::params![problem_id, lang, code, now_ms()],
    )?;
    Ok(true)
}

/// Removes the draft for a problem in a language. Not an error if there isn't one.
pub fn delete(conn: &Connection, problem_id: i64, lang: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM main.drafts WHERE problem_id = ?1 AND lang = ?2",
        rusqlite::params![problem_id, lang],
    )?;
    Ok(())
}

/// Every draft for a problem, most recently edited first.
pub fn list(conn: &Connection, problem_id: i64) -> Result<Vec<Draft>> {
    let drafts = conn
        .prepare(
            "SELECT lang, code, updated_at FROM main.drafts
              WHERE problem_id = ? ORDER BY updated_at DESC, lang",
        )?
        .query_map([problem_id], |r| {
            Ok(Draft { lang: r.get(0)?, code: r.get(1)?, updated_at: r.get(2)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(drafts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{test_support, Db};
    use tempfile::TempDir;

    // The fixture's starter code is "// <lang> starter for <title>".
    const PY_STARTER: &str = "// python3 starter for Two Sum";

    fn setup() -> (TempDir, Db) {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        (dir, db)
    }

    fn count(db: &Db) -> i64 {
        db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM drafts", [], |r| r.get(0))?)).unwrap()
    }

    #[test]
    fn saving_changed_code_stores_a_draft() {
        let (_d, db) = setup();
        let kept = db.with(|c| save(c, 1, "python3", "my solution")).unwrap();
        assert!(kept);
        let drafts = db.with(|c| list(c, 1)).unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!((drafts[0].lang.as_str(), drafts[0].code.as_str()), ("python3", "my solution"));
        assert!(drafts[0].updated_at > 0);
    }

    #[test]
    fn once_submitted_the_baseline_is_that_submission_not_the_starter() {
        let (_d, db) = setup();
        db.with(|c| {
            submissions::record(
                c,
                submissions::NewSubmission {
                    problem_id: 1,
                    lang: "python3".into(),
                    code: "submitted".into(),
                    status: "Wrong Answer".into(),
                    ..Default::default()
                },
            )
            .map(|_| ())
        })
        .unwrap();

        assert!(!db.with(|c| save(c, 1, "python3", "submitted")).unwrap(), "same as what was submitted: no draft");
        assert!(db.with(|c| save(c, 1, "python3", PY_STARTER)).unwrap(), "the starter is now a change");
        assert!(!db.with(|c| save(c, 1, "cpp", "// cpp starter for Two Sum\nsecond line")).unwrap(), "other languages still use their starter");
        assert!(!db.with(|c| save(c, 2, "python3", "// python3 starter for Add Two Numbers")).unwrap(), "and so do other problems");
    }

    #[test]
    fn saving_again_updates_in_place_instead_of_duplicating() {
        let (_d, db) = setup();
        db.with(|c| {
            save(c, 1, "python3", "first")?;
            save(c, 1, "python3", "second")
        })
        .unwrap();
        assert_eq!(count(&db), 1);
        assert_eq!(db.with(|c| list(c, 1)).unwrap()[0].code, "second");
    }

    #[test]
    fn drafts_are_per_problem_and_per_language() {
        let (_d, db) = setup();
        db.with(|c| {
            save(c, 1, "python3", "py")?;
            save(c, 1, "cpp", "cpp")?;
            save(c, 2, "python3", "other problem")
        })
        .unwrap();
        assert_eq!(count(&db), 3);
        let langs: Vec<_> = db.with(|c| list(c, 1)).unwrap().into_iter().map(|d| d.lang).collect();
        assert_eq!(langs.len(), 2);
        assert!(langs.contains(&"python3".to_string()) && langs.contains(&"cpp".to_string()));
        assert_eq!(db.with(|c| list(c, 2)).unwrap().len(), 1);
    }

    #[test]
    fn code_equal_to_the_starter_is_not_a_draft_and_clears_an_existing_one() {
        let (_d, db) = setup();
        // Never stored in the first place:
        assert!(!db.with(|c| save(c, 1, "python3", PY_STARTER)).unwrap());
        assert_eq!(count(&db), 0);
        // Undoing back to the starter removes the draft:
        db.with(|c| save(c, 1, "python3", "changed")).unwrap();
        assert_eq!(count(&db), 1);
        assert!(!db.with(|c| save(c, 1, "python3", PY_STARTER)).unwrap());
        assert_eq!(count(&db), 0);
    }

    #[test]
    fn line_endings_alone_do_not_make_a_draft() {
        let (_d, db) = setup();
        // The fixture's C++ starter is two lines: "// cpp starter for Two Sum" / "second line".
        let starter_with_crlf = "// cpp starter for Two Sum\r\nsecond line";
        assert!(!db.with(|c| save(c, 1, "cpp", starter_with_crlf)).unwrap(), "same text, CRLF endings");
        assert_eq!(count(&db), 0);
        // A real edit is still a draft, whatever the endings.
        assert!(db.with(|c| save(c, 1, "cpp", "// cpp starter for Two Sum\r\nsecond line!")).unwrap());
        assert_eq!(count(&db), 1);
        assert!(same_code("a\r\nb", "a\nb") && !same_code("a\r\nb", "a\nc"));
    }

    #[test]
    fn empty_code_is_a_real_change() {
        let (_d, db) = setup();
        assert!(db.with(|c| save(c, 1, "python3", "")).unwrap(), "clearing the editor is still unsubmitted work");
        assert_eq!(db.with(|c| list(c, 1)).unwrap()[0].code, "");
    }

    #[test]
    fn unknown_problem_or_language_is_rejected_and_writes_nothing() {
        let (_d, db) = setup();
        let no_problem = db.with(|c| save(c, 424242, "python3", "x")).unwrap_err();
        assert!(matches!(no_problem, Error::NotFound(_)), "got: {no_problem}");
        let no_lang = db.with(|c| save(c, 1, "cobol", "x")).unwrap_err();
        assert_eq!(no_lang.to_string(), "cobol starter code for problem 1 not found");
        assert_eq!(count(&db), 0);
    }

    #[test]
    fn delete_removes_only_that_draft_and_is_idempotent() {
        let (_d, db) = setup();
        db.with(|c| {
            save(c, 1, "python3", "py")?;
            save(c, 1, "cpp", "cpp")
        })
        .unwrap();
        db.with(|c| delete(c, 1, "python3")).unwrap();
        db.with(|c| delete(c, 1, "python3")).unwrap(); // already gone
        db.with(|c| delete(c, 999, "nope")).unwrap(); // never existed
        let left = db.with(|c| list(c, 1)).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].lang, "cpp");
    }

    #[test]
    fn list_is_newest_first() {
        let (_d, db) = setup();
        db.with(|c| {
            save(c, 1, "python3", "py")?;
            save(c, 1, "cpp", "cpp")?;
            c.execute("UPDATE drafts SET updated_at = 1000 WHERE lang = 'python3'", [])?;
            c.execute("UPDATE drafts SET updated_at = 2000 WHERE lang = 'cpp'", [])?;
            Ok(())
        })
        .unwrap();
        let langs: Vec<_> = db.with(|c| list(c, 1)).unwrap().into_iter().map(|d| d.lang).collect();
        assert_eq!(langs, vec!["cpp", "python3"]);
    }

    #[test]
    fn drafts_survive_closing_and_reopening_the_app() {
        let dir = TempDir::new().unwrap();
        let catalog = dir.path().join("catalog.db");
        test_support::build_fixture_catalog(&catalog);
        let user = dir.path().join("user.db");
        {
            let db = Db::open_at(&user, &catalog).unwrap();
            db.with(|c| save(c, 1, "python3", "work in progress")).unwrap();
        } // closed: the connection is dropped
        let reopened = Db::open_at(&user, &catalog).unwrap();
        let drafts = reopened.with(|c| list(c, 1)).unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].code, "work in progress");
    }
}
