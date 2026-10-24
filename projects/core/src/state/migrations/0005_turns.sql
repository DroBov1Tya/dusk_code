CREATE TABLE turns (
    turn_id TEXT PRIMARY KEY NOT NULL REFERENCES event_logs(request_uid),
    thread_id TEXT NOT NULL REFERENCES threads(id),
    user_message_id TEXT NOT NULL UNIQUE REFERENCES messages(message_id),
    response_message_id TEXT UNIQUE REFERENCES messages(message_id),
    agent_id TEXT CHECK (agent_id IS NULL OR length(trim(agent_id)) > 0),
    attempt INTEGER NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed')),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    started_at INTEGER CHECK (started_at IS NULL OR started_at >= created_at),
    finished_at INTEGER CHECK (finished_at IS NULL OR finished_at >= started_at),
    error_message TEXT,
    CHECK (
        (status = 'queued' AND agent_id IS NULL AND started_at IS NULL AND finished_at IS NULL
            AND response_message_id IS NULL AND error_message IS NULL) OR
        (status = 'running' AND agent_id IS NOT NULL AND started_at IS NOT NULL
            AND finished_at IS NULL AND response_message_id IS NULL AND error_message IS NULL) OR
        (status = 'completed' AND agent_id IS NOT NULL AND started_at IS NOT NULL
            AND finished_at IS NOT NULL AND response_message_id IS NOT NULL AND error_message IS NULL) OR
        (status = 'failed' AND agent_id IS NOT NULL AND started_at IS NOT NULL
            AND finished_at IS NOT NULL AND response_message_id IS NULL
            AND error_message IS NOT NULL AND length(trim(error_message)) > 0)
    )
);

CREATE INDEX turns_queue ON turns(thread_id, status, created_at, turn_id);
