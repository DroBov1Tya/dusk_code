-- What skills changed during turns, with the text before, kept so a change can be undone.
-- The rows live here and not next to SKILL.md because the agent can write those folders.
CREATE TABLE IF NOT EXISTS skill_changes (
    id INTEGER PRIMARY KEY,
    skill TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('created', 'updated', 'deleted')),
    reason TEXT NOT NULL,
    before TEXT,
    after TEXT NOT NULL,
    changed_at_ms INTEGER NOT NULL,
    undone_at_ms INTEGER
);
CREATE INDEX IF NOT EXISTS skill_changes_thread ON skill_changes (thread_id, id);
CREATE INDEX IF NOT EXISTS skill_changes_skill ON skill_changes (skill, id);
