-- Client messages saved while no model is selected. They are never read by the
-- thread runner until the client explicitly moves one to the pending queue.
CREATE TABLE local_unsent (
    id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    content TEXT NOT NULL CHECK (length(trim(content)) > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
);

CREATE INDEX local_unsent_thread ON local_unsent(thread_id, created_at, id);
