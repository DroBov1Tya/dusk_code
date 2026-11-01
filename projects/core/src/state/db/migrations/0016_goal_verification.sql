-- Existing goals have no approved contract. Completed history stays unverified;
-- active legacy goals wait for the user to replace and approve their contract.
ALTER TABLE goals ADD COLUMN revision TEXT NOT NULL DEFAULT '';
ALTER TABLE goals ADD COLUMN criteria_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(criteria_json));
ALTER TABLE goals ADD COLUMN approved_at INTEGER;
ALTER TABLE goals ADD COLUMN work_epoch INTEGER NOT NULL DEFAULT 0;
ALTER TABLE goals ADD COLUMN awaiting_user INTEGER NOT NULL DEFAULT 0;
ALTER TABLE goals ADD COLUMN evidence_after_sequence INTEGER NOT NULL DEFAULT 0;
ALTER TABLE goals ADD COLUMN verification TEXT NOT NULL DEFAULT 'unverified'
    CHECK (verification IN ('unverified', 'pending', 'checking', 'gaps', 'needs_user', 'passed'));
ALTER TABLE goals ADD COLUMN verification_id TEXT;
ALTER TABLE goals ADD COLUMN verification_request TEXT;
ALTER TABLE goals ADD COLUMN verification_turn_id TEXT;
ALTER TABLE goals ADD COLUMN verification_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE goals ADD COLUMN verification_note TEXT;
UPDATE goals SET revision = lower(hex(randomblob(16))),
    status = CASE WHEN status = 'active' THEN 'paused' ELSE status END;

-- The runtime records provenance before execution and a typed outcome with its
-- result. Text-only legacy results cannot serve as successful evidence.
CREATE TABLE tool_executions (
    execution_id TEXT PRIMARY KEY NOT NULL,
    turn_id TEXT NOT NULL REFERENCES turns(turn_id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    call_id TEXT NOT NULL,
    call_message_id TEXT NOT NULL REFERENCES messages(message_id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    arguments_json TEXT NOT NULL CHECK (json_valid(arguments_json)),
    goal_revision TEXT,
    goal_epoch INTEGER,
    result_message_id TEXT UNIQUE REFERENCES messages(message_id) ON DELETE CASCADE,
    outcome TEXT NOT NULL DEFAULT 'pending'
        CHECK (outcome IN ('pending', 'succeeded', 'failed', 'cancelled', 'unknown')),
    exit_code INTEGER,
    UNIQUE(turn_id, call_id)
);
CREATE INDEX tool_execution_goal ON tool_executions(thread_id, goal_revision);
ALTER TABLE background_jobs ADD COLUMN goal_revision TEXT;
ALTER TABLE background_jobs ADD COLUMN goal_epoch INTEGER;
CREATE TABLE goal_confirmations (
    thread_id TEXT NOT NULL REFERENCES threads(id),
    revision TEXT NOT NULL,
    criterion_id TEXT NOT NULL,
    result_id TEXT NOT NULL,
    confirmed_at INTEGER NOT NULL,
    PRIMARY KEY(thread_id, revision, criterion_id, result_id)
);
