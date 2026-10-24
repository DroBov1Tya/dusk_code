CREATE TABLE jobs (
    job_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(job_id)) > 0),
    thread_id TEXT NOT NULL REFERENCES threads(id),
    instruction TEXT NOT NULL CHECK (length(trim(instruction)) > 0),
    created_by_agent_id TEXT NOT NULL CHECK (length(trim(created_by_agent_id)) > 0),
    worker_agent_id TEXT CHECK (worker_agent_id IS NULL OR length(trim(worker_agent_id)) > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    started_at INTEGER CHECK (started_at IS NULL OR started_at >= created_at),
    finished_at INTEGER CHECK (finished_at IS NULL OR finished_at >= COALESCE(started_at, created_at)),
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed', 'cancelled')),
    error_message TEXT,
    UNIQUE (job_id, thread_id),
    CHECK (
        (started_at IS NULL AND worker_agent_id IS NULL) OR
        (started_at IS NOT NULL AND worker_agent_id IS NOT NULL)
    ),
    CHECK (
        (status = 'queued' AND started_at IS NULL AND finished_at IS NULL AND error_message IS NULL) OR
        (status = 'running' AND started_at IS NOT NULL AND finished_at IS NULL AND error_message IS NULL) OR
        (status = 'completed' AND started_at IS NOT NULL AND finished_at IS NOT NULL AND error_message IS NULL) OR
        (status = 'failed' AND started_at IS NOT NULL AND finished_at IS NOT NULL AND
            error_message IS NOT NULL AND length(trim(error_message)) > 0) OR
        (status = 'cancelled' AND finished_at IS NOT NULL AND error_message IS NULL)
    )
);

CREATE INDEX jobs_thread_created ON jobs(thread_id, created_at, job_id);
CREATE INDEX jobs_queue ON jobs(thread_id, status, created_at, job_id);
