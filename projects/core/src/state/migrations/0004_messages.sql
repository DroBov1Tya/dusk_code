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
    -- Tools an assistant message calls, as a JSON array of {id, name, arguments}.
    tool_calls_json TEXT CHECK (tool_calls_json IS NULL OR (role = 'assistant'
        AND json_valid(tool_calls_json) AND json_type(tool_calls_json) = 'array')),
    -- The call a tool message answers.
    tool_call_id TEXT CHECK ((role = 'tool') = (tool_call_id IS NOT NULL)),
    cwd TEXT NOT NULL CHECK (length(trim(cwd)) > 0),
    -- Estimated size in model tokens, used to check the context window.
    token_count INTEGER NOT NULL CHECK (token_count >= 0),
    attachments_json TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(attachments_json) AND json_type(attachments_json) = 'array'
    ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    UNIQUE (thread_id, sequence),
    FOREIGN KEY (job_id, thread_id) REFERENCES jobs(job_id, thread_id),
    CHECK (length(trim(content)) > 0 OR json_array_length(attachments_json) > 0
        OR tool_calls_json IS NOT NULL),
    CHECK (request_uid IS NULL OR (role = 'user' AND job_id IS NULL AND agent_id IS NULL))
);

CREATE INDEX messages_thread_sequence ON messages(thread_id, sequence);
CREATE INDEX messages_job_id ON messages(job_id) WHERE job_id IS NOT NULL;
