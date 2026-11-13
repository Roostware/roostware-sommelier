-- user.db, schema v1: everything the user creates. The LeetCode problem data
-- lives in the separate, read-only catalog.db (attached as `catalog`), so
-- `problem_id` below is a plain reference to catalog.problems.id; SQLite cannot
-- enforce foreign keys across database files, the app checks it.
--
-- Sync-readiness conventions (no sync is implemented yet):
--   * Rows the user creates have UUIDv7 text primary keys, never autoincrement.
--   * Timestamps are epoch milliseconds, UTC.
--   * Mutable rows carry updated_at; deletes are tombstones (deleted_at).
--   * Ordering uses fractional-index strings (sort_key), not integer positions.
--   * Membership is rows, not arrays inside a row.
--   * meta.device_id identifies this install (written at first run).

CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE collections (
  id          TEXT    PRIMARY KEY,
  name        TEXT    NOT NULL,
  description TEXT    NOT NULL DEFAULT '',
  sort_key    TEXT    NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  deleted_at  INTEGER
) WITHOUT ROWID;

CREATE TABLE collection_problems (
  collection_id TEXT    NOT NULL REFERENCES collections (id),
  problem_id    INTEGER NOT NULL,
  sort_key      TEXT    NOT NULL,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL,
  deleted_at    INTEGER,
  PRIMARY KEY (collection_id, problem_id)
) WITHOUT ROWID;
CREATE INDEX collection_problems_by_problem ON collection_problems (problem_id);

-- Per-problem state the user sets themselves. "Solved" is deliberately not
-- stored here: it is derived from submissions (any Accepted submission).
CREATE TABLE problem_state (
  problem_id     INTEGER PRIMARY KEY,
  status         TEXT,                         -- NULL, 'review', 'skipped', …; validated in Rust
  starred        INTEGER NOT NULL DEFAULT 0 CHECK (starred IN (0, 1)),
  notes          TEXT    NOT NULL DEFAULT '',
  last_opened_at INTEGER,
  updated_at     INTEGER NOT NULL
);

-- The working copy in the editor: one row per problem + language, autosaved
-- and overwritten in place. Not history; see submissions for that.
CREATE TABLE drafts (
  problem_id INTEGER NOT NULL,
  lang       TEXT    NOT NULL,                 -- LeetCode langSlug (python3, cpp, …)
  code       TEXT    NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (problem_id, lang)
) WITHOUT ROWID;

-- Immutable snapshots of real LeetCode submissions (proxied via the browser
-- extension), mirroring LeetCode's response. result_json keeps the raw
-- response so unofficial-API changes never lose data.
CREATE TABLE submissions (
  id                     TEXT    PRIMARY KEY,
  problem_id             INTEGER NOT NULL,
  lang                   TEXT    NOT NULL,
  code                   TEXT    NOT NULL,
  leetcode_submission_id TEXT    UNIQUE,       -- dedupe key for imports/backfill
  status                 TEXT    NOT NULL,     -- 'Accepted', 'Wrong Answer', …
  status_code            INTEGER,
  runtime_ms             INTEGER,
  runtime_percentile     REAL,
  memory_kb              INTEGER,
  memory_percentile      REAL,
  total_correct          INTEGER,
  total_testcases        INTEGER,
  last_testcase          TEXT,
  expected_output        TEXT,
  code_output            TEXT,
  error_message          TEXT,
  result_json            TEXT,
  submitted_at           INTEGER NOT NULL,
  created_at             INTEGER NOT NULL,
  updated_at             INTEGER NOT NULL,
  deleted_at             INTEGER
) WITHOUT ROWID;
CREATE INDEX submissions_by_problem ON submissions (problem_id, submitted_at DESC);
CREATE INDEX submissions_by_status  ON submissions (problem_id, status);
