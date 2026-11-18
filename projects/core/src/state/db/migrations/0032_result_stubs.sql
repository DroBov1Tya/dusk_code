-- A tool result sent as a short stub in place of its stored text, so old results stop
-- riding in every request. The full text stays in messages. A stub is decided once and
-- frozen: the row holds the exact text sent, so a restart or a changed threshold never
-- rewrites it and the provider's cached prefix holds. It goes with its message on undo.
-- `made_at_sequence` is the newest message of the thread when the stub was made, which
-- tells whether the provider's last count still holds the full text.
CREATE TABLE IF NOT EXISTS result_stubs (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES messages(message_id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    stub TEXT NOT NULL CHECK (length(trim(stub)) > 0),
    saved_tokens INTEGER NOT NULL CHECK (saved_tokens >= 0),
    made_at_sequence INTEGER NOT NULL CHECK (made_at_sequence > 0)
);
CREATE INDEX IF NOT EXISTS result_stubs_thread ON result_stubs(thread_id);
