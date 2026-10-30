-- Jobs are never restarted: a new runtime marks unfinished records interrupted.
CREATE TABLE background_jobs (
    job_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    command TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'cancelled', 'timed_out', 'failed', 'interrupted')),
    created_at INTEGER NOT NULL,
    finished_at INTEGER,
    output TEXT NOT NULL DEFAULT ''
);
CREATE INDEX background_jobs_owner ON background_jobs(thread_id, created_at);

-- A job result is a distinct event, never a user request or an instruction prompt.
-- The storage role remains system for compatibility with the existing role constraint;
-- decoding uses this provenance to expose MessageRole::Job instead.
ALTER TABLE messages ADD COLUMN from_job_id TEXT CHECK (from_job_id IS NULL OR role = 'system');
CREATE UNIQUE INDEX messages_job ON messages(from_job_id) WHERE from_job_id IS NOT NULL;
ALTER TABLE pending_messages ADD COLUMN from_job_id TEXT;
CREATE UNIQUE INDEX pending_messages_job ON pending_messages(from_job_id) WHERE from_job_id IS NOT NULL;
