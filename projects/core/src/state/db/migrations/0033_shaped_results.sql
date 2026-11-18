-- A command's output as the model is sent it, with passes, progress and repeats left
-- out; the full output stays in messages. It is decided once, when the result is
-- stored, and never rewritten, so every request carries the same bytes and the
-- provider's cached prefix holds. A stub may later stand in for the result: the two are
-- separate rows, since a stub is made by a later move and counts what it frees against
-- the text sent before it. It goes with its message on undo.
CREATE TABLE IF NOT EXISTS shaped_results (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES messages(message_id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    text TEXT NOT NULL CHECK (length(trim(text)) > 0),
    saved_tokens INTEGER NOT NULL CHECK (saved_tokens >= 0)
);
CREATE INDEX IF NOT EXISTS shaped_results_thread ON shaped_results(thread_id);
