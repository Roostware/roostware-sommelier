-- user.db, schema v3: giving up on a problem.
--
--   * give_ups: one row each time the user gave up on a problem and was shown its solution.
--               Giving up also logs an Again review (reviews row with no submission), linked
--               here by review_id, so the spaced-repetition replay sees it as a lapse while the
--               rating replay can tell it from a solve. The rating (which isn't stored) is
--               replayed from these rows too. Follows the sync conventions (UUIDv7 ids,
--               tombstones).

CREATE TABLE give_ups (
  id          TEXT    PRIMARY KEY,
  problem_id  INTEGER NOT NULL,
  review_id   TEXT,                                -- the Again review it logged
  gave_up_at  INTEGER NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  deleted_at  INTEGER
) WITHOUT ROWID;
CREATE INDEX give_ups_by_problem ON give_ups (problem_id, gave_up_at);
