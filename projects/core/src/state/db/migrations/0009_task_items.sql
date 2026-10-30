-- The steps of a task, each done on its own. When work on a step began and ended is its
-- timer; a step is done exactly when it has ended.
CREATE TABLE task_items (
    item_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(item_id)) > 0),
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    description TEXT NOT NULL CHECK (length(trim(description)) > 0),
    done INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
    started_at INTEGER CHECK (started_at IS NULL OR started_at >= 0),
    finished_at INTEGER CHECK (finished_at IS NULL OR started_at IS NOT NULL),
    CHECK ((done = 1) = (finished_at IS NOT NULL))
);

CREATE INDEX task_items_task ON task_items(task_id);
