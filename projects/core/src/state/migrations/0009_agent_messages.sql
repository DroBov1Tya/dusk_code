-- A message one agent sent another, such as a subagent writing to the thread that asked
-- it: the thread it came from. NULL for what a client sent.
ALTER TABLE pending_messages ADD COLUMN from_thread_id TEXT REFERENCES threads(id);
ALTER TABLE messages ADD COLUMN from_thread_id TEXT REFERENCES threads(id)
    CHECK (from_thread_id IS NULL OR role = 'user');
