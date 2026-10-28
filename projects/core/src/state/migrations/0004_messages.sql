CREATE TABLE messages (
    message_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(message_id)) > 0),
    thread_id TEXT NOT NULL REFERENCES threads(id),
    job_id TEXT,
    request_uid TEXT UNIQUE REFERENCES event_logs(request_uid),
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system', 'tool', 'agent')),
    agent_id TEXT CHECK (agent_id IS NULL OR length(trim(agent_id)) > 0),
    content TEXT NOT NULL,
    -- Model thinking behind an assistant message; NULL when not recorded.
    reasoning TEXT CHECK (reasoning IS NULL OR (role = 'assistant' AND length(trim(reasoning)) > 0)),
    cwd TEXT NOT NULL CHECK (length(trim(cwd)) > 0),
    attachments_json TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(attachments_json) AND json_type(attachments_json) = 'array'
    ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    UNIQUE (thread_id, sequence),
    FOREIGN KEY (job_id, thread_id) REFERENCES jobs(job_id, thread_id),
    CHECK (length(trim(content)) > 0 OR json_array_length(attachments_json) > 0),
    CHECK (request_uid IS NULL OR (role = 'user' AND job_id IS NULL AND agent_id IS NULL))
);

CREATE INDEX messages_thread_sequence ON messages(thread_id, sequence);
CREATE INDEX messages_job_id ON messages(job_id) WHERE job_id IS NOT NULL;
