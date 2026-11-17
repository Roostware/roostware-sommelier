-- user.db, schema v4: the AI coach's conversations.
--
--   * coach_threads:  a conversation about one problem. "New conversation" tombstones the old
--                     one and starts another; the latest live thread is the one that's shown.
--                     state_json is the frontend's own bookkeeping for the thread (how far the
--                     hint ladders have gone, which submission has an analysis, which context
--                     chips are on); the database only keeps it.
--   * coach_messages: what was said, in order (seq). body_json is the frontend's message body
--                     (text, an analysis, a solution), kept so a reply that cost money isn't
--                     paid for twice. about_code is the code the message was written about, so
--                     line references can tell when the editor has moved on. Token counts are
--                     what the provider reported, for the usage line.
-- Both follow the sync conventions (UUIDv7 ids, tombstones).

CREATE TABLE coach_threads (
  id          TEXT    PRIMARY KEY,
  problem_id  INTEGER NOT NULL,
  state_json  TEXT    NOT NULL DEFAULT '{}',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  deleted_at  INTEGER
) WITHOUT ROWID;
CREATE INDEX coach_threads_by_problem ON coach_threads (problem_id, created_at);

CREATE TABLE coach_messages (
  id            TEXT    PRIMARY KEY,
  thread_id     TEXT    NOT NULL REFERENCES coach_threads (id),
  seq           INTEGER NOT NULL,
  role          TEXT    NOT NULL CHECK (role IN ('user', 'coach')),
  label         TEXT,
  body_json     TEXT    NOT NULL,
  about_code    TEXT,
  input_tokens  INTEGER,
  output_tokens INTEGER,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL,
  deleted_at    INTEGER
) WITHOUT ROWID;
CREATE UNIQUE INDEX coach_messages_by_seq ON coach_messages (thread_id, seq);
