-- What an agent set itself to do in a thread: a task in a few words, done once the agent
-- says it is. The agent keeps these outside its context, so they outlive a compaction.
CREATE TABLE tasks (
    task_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(task_id)) > 0),
    thread_id TEXT NOT NULL REFERENCES threads(id),
    description TEXT NOT NULL CHECK (length(trim(description)) > 0),
    done INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
);

CREATE INDEX tasks_thread ON tasks(thread_id, created_at);
