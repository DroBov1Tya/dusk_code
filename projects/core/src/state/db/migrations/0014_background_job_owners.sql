-- NULL means an older job has no reliable owner proof. Recovery must leave its
-- effects unknown instead of assuming that another application's process died.
ALTER TABLE background_jobs ADD COLUMN owner_runtime_id TEXT;
ALTER TABLE background_jobs ADD COLUMN owner_lease_path TEXT;
CREATE INDEX background_jobs_runtime ON background_jobs(owner_runtime_id, status);
