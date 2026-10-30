-- What the provider reported for each model call. Every call sends the whole context
-- again, so these sums are what the thread cost, not how large it is.
CREATE TABLE model_usage (
    thread_id TEXT NOT NULL REFERENCES threads(id),
    -- NULL for calls outside a turn, such as /compact while the thread is idle.
    turn_id TEXT REFERENCES turns(turn_id),
    -- 'answer' for the agent's steps, 'compact' for summaries, 'title' for naming the thread.
    purpose TEXT NOT NULL CHECK (purpose IN ('answer', 'compact', 'title')),
    -- The compaction the call's context started from; NULL before the first one.
    compaction_id INTEGER REFERENCES compactions(compaction_id),
    model TEXT NOT NULL CHECK (length(trim(model)) > 0),
    -- The newest message the call sent; messages after it are not in prompt_tokens.
    through_sequence INTEGER NOT NULL CHECK (through_sequence > 0),
    -- Includes the cached part.
    prompt_tokens INTEGER NOT NULL CHECK (prompt_tokens >= 0),
    cached_prompt_tokens INTEGER NOT NULL CHECK (cached_prompt_tokens >= 0),
    -- Includes the reasoning part.
    completion_tokens INTEGER NOT NULL CHECK (completion_tokens >= 0),
    reasoning_tokens INTEGER NOT NULL CHECK (reasoning_tokens >= 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
);

CREATE INDEX model_usage_thread ON model_usage(thread_id, purpose, created_at);
