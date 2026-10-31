-- What clients and other agents sent, stored before the send is confirmed, so nothing
-- sent is lost.
-- The thread moves each one into messages, or runs it as a command, in order: right away
-- when idle, after the turn when one is running, and on the next start after a crash.
CREATE TABLE pending_messages (
    pending_id INTEGER PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    -- Becomes the message's request_uid, so moving it twice adds it once.
    request_uid TEXT NOT NULL UNIQUE CHECK (length(trim(request_uid)) > 0),
    content TEXT NOT NULL CHECK (length(trim(content)) > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    -- The agent's thread for a message another agent sent; NULL for a client's.
    from_thread_id TEXT REFERENCES threads(id),
    -- The job for a result that finished on its own; one result is queued once.
    from_job_id TEXT
);

CREATE UNIQUE INDEX pending_messages_job ON pending_messages(from_job_id) WHERE from_job_id IS NOT NULL;
CREATE INDEX pending_messages_thread ON pending_messages(thread_id, pending_id);
