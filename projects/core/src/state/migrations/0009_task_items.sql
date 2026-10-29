-- The steps of a task, in order, each done on its own.
CREATE TABLE task_items (
    item_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(item_id)) > 0),
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    position INTEGER NOT NULL CHECK (position > 0),
    description TEXT NOT NULL CHECK (length(trim(description)) > 0),
    done INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
    UNIQUE (task_id, position)
);
