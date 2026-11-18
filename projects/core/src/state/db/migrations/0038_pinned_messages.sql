-- A message the user keeps one click away. It is for the person only: nothing here
-- reaches the model and a pin keeps nothing from a compaction. It goes with its message,
-- on /undo, and with its thread. At most eight a thread, which the store holds to.
CREATE TABLE IF NOT EXISTS pinned_messages (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES messages(message_id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    pinned_at INTEGER NOT NULL CHECK (pinned_at >= 0)
);
CREATE INDEX IF NOT EXISTS pinned_messages_thread ON pinned_messages(thread_id);
