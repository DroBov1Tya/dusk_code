-- Files a thread's commands created, changed or deleted in the project, as git sees them.
CREATE TABLE IF NOT EXISTS thread_files (
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    kind TEXT NOT NULL,
    PRIMARY KEY (thread_id, path)
);
