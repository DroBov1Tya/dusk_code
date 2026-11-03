-- The reasoning effort an asker chose for its subagent. No row: the default for agents.
CREATE TABLE agent_efforts (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id),
    effort TEXT NOT NULL
        CHECK (effort IN ('off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'))
);
