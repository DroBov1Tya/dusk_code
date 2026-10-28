CREATE TABLE threads (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    -- Who started the thread, such as "cli".
    source TEXT NOT NULL CHECK (length(trim(source)) > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
);
