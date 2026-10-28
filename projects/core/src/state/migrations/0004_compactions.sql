-- A summary that replaces the older messages of a thread in the model context. The
-- messages stay stored, and the agent can still search them with read_history.
CREATE TABLE compactions (
    compaction_id INTEGER PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    -- The newest message the summary covers. The context holds the system messages,
    -- the newest summary and the messages after it.
    through_sequence INTEGER NOT NULL CHECK (through_sequence > 0),
    summary TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    summary_tokens INTEGER NOT NULL CHECK (summary_tokens >= 0),
    -- 'manual' after /compact, 'auto' when the context neared the model window.
    reason TEXT NOT NULL CHECK (reason IN ('manual', 'auto')),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
);

CREATE INDEX compactions_thread ON compactions(thread_id, compaction_id);
