-- Jobs are never restarted: a new runtime marks unfinished records interrupted.
-- The owner is the runtime that started a job, proved by a lease file it keeps locked.
-- A NULL owner has no reliable proof, so recovery leaves the job's effects unknown
-- instead of assuming its process died.
CREATE TABLE background_jobs (
    job_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    command TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'cancelled', 'timed_out', 'failed', 'interrupted')),
    created_at INTEGER NOT NULL,
    finished_at INTEGER,
    output TEXT NOT NULL DEFAULT '',
    owner_runtime_id TEXT,
    owner_lease_path TEXT
);

CREATE INDEX background_jobs_owner ON background_jobs(thread_id, created_at);
CREATE INDEX background_jobs_runtime ON background_jobs(owner_runtime_id, status);
