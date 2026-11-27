-- A failed task the lead or the user retries keeps its id, history and dependents: how
-- often it was retried, shown beside it, and when last, so the review rounds of the
-- new try count from then.
ALTER TABLE tasks ADD COLUMN retries INTEGER NOT NULL DEFAULT 0 CHECK (retries >= 0);
ALTER TABLE tasks ADD COLUMN retried_at INTEGER;
