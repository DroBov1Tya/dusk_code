-- A thread's own approval mode. No row: the default held in the settings.
-- `IF NOT EXISTS`: the legacy fixtures replay migrations they forgot from the ledger,
-- and a database already at this one must not fail the replay.
CREATE TABLE IF NOT EXISTS thread_approvals (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id),
    policy TEXT NOT NULL
        CHECK (policy IN ('ask', 'sandbox', 'arbiter', 'full_access'))
);
