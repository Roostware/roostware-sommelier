-- catalog.db schema (read-only LeetCode problem data).
-- Shared by scripts/build-catalog.mjs (which creates the file) and the Rust
-- tests (which build a fixture catalog), so the two cannot drift apart.
-- If this changes incompatibly, bump SCHEMA_VERSION in the script AND
-- CATALOG_SCHEMA_VERSION in src-tauri/src/db/mod.rs.
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) WITHOUT ROWID;

-- id is LeetCode's internal question id (what the submit endpoint expects);
-- frontend_id is the number shown on the site.
CREATE TABLE problems (
  id                 INTEGER PRIMARY KEY,
  frontend_id        INTEGER NOT NULL,
  title              TEXT    NOT NULL,
  title_slug         TEXT    NOT NULL UNIQUE,
  difficulty         TEXT    NOT NULL CHECK (difficulty IN ('Easy', 'Medium', 'Hard')),
  content_html       TEXT    NOT NULL,   -- third-party HTML: sanitize before rendering
  example_test_cases TEXT    NOT NULL,   -- cases separated by newlines
  is_paid_only       INTEGER NOT NULL CHECK (is_paid_only IN (0, 1)),
  -- Snapshot stats as of dataset build time:
  likes              INTEGER NOT NULL,
  dislikes           INTEGER NOT NULL,
  total_accepted     INTEGER NOT NULL,
  total_submissions  INTEGER NOT NULL,
  acceptance_rate    REAL    NOT NULL,   -- 0..1
  created_at_approx  TEXT                -- ISO 8601, guessed by the dataset author
);
CREATE INDEX problems_frontend_id ON problems (frontend_id);
CREATE INDEX problems_difficulty  ON problems (difficulty);

CREATE TABLE tags (
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
);

CREATE TABLE problem_tags (
  problem_id INTEGER NOT NULL REFERENCES problems (id),
  tag_id     INTEGER NOT NULL REFERENCES tags (id),
  PRIMARY KEY (problem_id, tag_id)
) WITHOUT ROWID;
CREATE INDEX problem_tags_by_tag ON problem_tags (tag_id, problem_id);

-- Starter code per language; lang is LeetCode's langSlug (python3, cpp, …).
CREATE TABLE code_snippets (
  problem_id INTEGER NOT NULL REFERENCES problems (id),
  lang       TEXT    NOT NULL,
  code       TEXT    NOT NULL,
  PRIMARY KEY (problem_id, lang)
) WITHOUT ROWID;

-- Full-text search; rowid = problems.id.
-- Tags are indexed, but the app deliberately does not search them by default (see
-- SEARCH_COLUMNS in src-tauri/src/db/problems.rs).
CREATE VIRTUAL TABLE problems_fts USING fts5 (
  title, frontend_id, tags,
  tokenize = 'unicode61'
);
