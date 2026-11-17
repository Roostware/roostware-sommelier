-- user.db, schema v2: what the recommendations are built from and what they produce.
--
--   * reviews:         the spaced-repetition log. One row each time the user solved a problem
--                      (an Accepted submission), with how it went. The memory model's state
--                      for a problem is always recomputed by replaying its rows, so this log
--                      is the source of truth and the model's weights can be refitted to it
--                      later. Follows the sync conventions (UUIDv7 ids, tombstones).
--   * settings:        small user preferences (key/value). `daily_problem_count` for now.
--   * daily_sets, daily_set_items: the day's recommendations, stored so the list doesn't
--                      reshuffle every time the dashboard is opened. Derived data (it can be
--                      deleted and regenerated), so it carries no sync columns.

CREATE TABLE reviews (
  id            TEXT    PRIMARY KEY,
  problem_id    INTEGER NOT NULL,
  submission_id TEXT,                              -- the Accepted submission it came from
  reviewed_at   INTEGER NOT NULL,
  grade         INTEGER NOT NULL CHECK (grade BETWEEN 1 AND 4),  -- 1 Again, 2 Hard, 3 Good, 4 Easy
  grade_source  TEXT    NOT NULL CHECK (grade_source IN ('inferred', 'user')),
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL,
  deleted_at    INTEGER
) WITHOUT ROWID;
CREATE INDEX reviews_by_problem    ON reviews (problem_id, reviewed_at);
CREATE INDEX reviews_by_submission ON reviews (submission_id);

CREATE TABLE settings (
  key        TEXT    PRIMARY KEY,
  value      TEXT    NOT NULL,
  updated_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE daily_sets (
  day        TEXT    PRIMARY KEY,                  -- the user's local date, YYYY-MM-DD
  count      INTEGER NOT NULL,                     -- the daily count the set was made for
  created_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE daily_set_items (
  day        TEXT    NOT NULL REFERENCES daily_sets (day) ON DELETE CASCADE,
  position   INTEGER NOT NULL,
  problem_id INTEGER NOT NULL,
  kind       TEXT    NOT NULL CHECK (kind IN ('new', 'review')),
  reason     TEXT    NOT NULL,
  pattern    TEXT,                                 -- the pattern it mainly serves (its id)
  PRIMARY KEY (day, position)
) WITHOUT ROWID;
