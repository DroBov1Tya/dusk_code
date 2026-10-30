CREATE TABLE messages (
    message_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(message_id)) > 0),
    thread_id TEXT NOT NULL REFERENCES threads(id),
    -- Order within the thread.
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    role TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    content TEXT NOT NULL,
    -- Model thinking behind an assistant message; NULL when not recorded.
    reasoning TEXT CHECK (reasoning IS NULL OR (role = 'assistant' AND length(trim(reasoning)) > 0)),
    -- Tools an assistant message calls, as a JSON array of {id, name, arguments}.
    tool_calls_json TEXT CHECK (tool_calls_json IS NULL OR (role = 'assistant'
        AND json_valid(tool_calls_json) AND json_type(tool_calls_json) = 'array')),
    -- The call a tool message answers.
    tool_call_id TEXT CHECK ((role = 'tool') = (tool_call_id IS NOT NULL)),
    -- A client's ID for a user message; sending it again does not add the message twice.
    request_uid TEXT UNIQUE CHECK (request_uid IS NULL OR role = 'user'),
    -- Estimated sizes in model tokens. The context holds content and tool calls;
    -- reasoning is never sent back, so it only counts in the thread history.
    content_tokens INTEGER NOT NULL CHECK (content_tokens >= 0),
    tool_calls_tokens INTEGER NOT NULL CHECK (tool_calls_tokens >= 0),
    reasoning_tokens INTEGER NOT NULL CHECK (reasoning_tokens >= 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    -- For a user message another agent sent, such as a subagent writing to the thread
    -- that asked it: the thread it came from.
    from_thread_id TEXT REFERENCES threads(id) CHECK (from_thread_id IS NULL OR role = 'user'),
    UNIQUE (thread_id, sequence),
    CHECK (length(trim(content)) > 0 OR tool_calls_json IS NOT NULL)
);
