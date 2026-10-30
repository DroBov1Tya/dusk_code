-- What an agent set itself to do in a thread: a task in a few words with its steps, done
-- once every step is or the agent says so. The agent keeps these outside its context, so
-- they outlive a compaction.
CREATE TABLE tasks (
    task_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(task_id)) > 0),
    thread_id TEXT NOT NULL REFERENCES threads(id),
    description TEXT NOT NULL CHECK (length(trim(description)) > 0),
    -- The IDs of its steps in order, as a JSON array.
    item_ids TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(item_ids) AND json_type(item_ids) = 'array'),
    done INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
);

CREATE INDEX tasks_thread ON tasks(thread_id, created_at);
