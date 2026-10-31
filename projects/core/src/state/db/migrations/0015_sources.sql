-- Files attached to a chat, kept in the sources folder of its workspace. One goes to the
-- model with the first turn after it was added, then stays passive unless pinned.
CREATE TABLE sources (
    source_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    -- What the user knows it by.
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    -- Its file in the sources folder.
    file TEXT NOT NULL CHECK (length(trim(file)) > 0),
    bytes INTEGER NOT NULL CHECK (bytes >= 0),
    -- Goes with every request rather than once.
    pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    -- A turn that had it finished.
    delivered INTEGER NOT NULL DEFAULT 0 CHECK (delivered IN (0, 1)),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    UNIQUE (thread_id, file)
);

CREATE INDEX sources_thread ON sources(thread_id, created_at);
