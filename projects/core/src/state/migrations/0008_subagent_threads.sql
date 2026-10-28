-- A subagent works in a thread of its own under the thread that asked it. Main threads
-- have neither column set.
ALTER TABLE threads ADD COLUMN parent_thread_id TEXT REFERENCES threads(id);
ALTER TABLE threads ADD COLUMN agent TEXT CHECK (agent IS NULL OR length(trim(agent)) > 0);

CREATE INDEX threads_parent ON threads(parent_thread_id);
