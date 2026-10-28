-- A turn is one agent run. It answers every user message of its thread up to
-- last_sequence; messages after the newest turn are the queue.
CREATE TABLE turns (
    turn_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(turn_id)) > 0),
    thread_id TEXT NOT NULL REFERENCES threads(id),
    last_sequence INTEGER NOT NULL CHECK (last_sequence > 0),
    response_message_id TEXT UNIQUE REFERENCES messages(message_id),
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'interrupted', 'failed')),
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    finished_at INTEGER CHECK (finished_at IS NULL OR finished_at >= started_at),
    error_message TEXT,
    CHECK (
        (status = 'running' AND finished_at IS NULL AND response_message_id IS NULL
            AND error_message IS NULL) OR
        (status = 'completed' AND finished_at IS NOT NULL AND response_message_id IS NOT NULL
            AND error_message IS NULL) OR
        (status = 'interrupted' AND finished_at IS NOT NULL AND error_message IS NULL) OR
        (status = 'failed' AND finished_at IS NOT NULL AND response_message_id IS NULL
            AND length(trim(error_message)) > 0)
    )
);

-- At most one agent run per thread at a time.
CREATE UNIQUE INDEX turns_one_running ON turns(thread_id) WHERE status = 'running';
CREATE INDEX turns_thread_sequence ON turns(thread_id, last_sequence);
