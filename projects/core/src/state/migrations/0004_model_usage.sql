-- What the provider reported for each model call. Every call sends the whole context
-- again, so these sums are what the thread cost, not how large it is.
CREATE TABLE model_usage (
    turn_id TEXT NOT NULL REFERENCES turns(turn_id),
    model TEXT NOT NULL CHECK (length(trim(model)) > 0),
    -- Includes the cached part.
    prompt_tokens INTEGER NOT NULL CHECK (prompt_tokens >= 0),
    cached_prompt_tokens INTEGER NOT NULL CHECK (cached_prompt_tokens >= 0),
    -- Includes the reasoning part.
    completion_tokens INTEGER NOT NULL CHECK (completion_tokens >= 0),
    reasoning_tokens INTEGER NOT NULL CHECK (reasoning_tokens >= 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
);

CREATE INDEX model_usage_turn ON model_usage(turn_id);
