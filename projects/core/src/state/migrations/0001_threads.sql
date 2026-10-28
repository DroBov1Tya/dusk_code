CREATE TABLE threads (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    -- Who started the thread, such as "cli".
    source TEXT NOT NULL CHECK (length(trim(source)) > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    -- Set from the first message, then replaced by a name the model gives the thread.
    title TEXT CHECK (title IS NULL OR length(trim(title)) > 0)
);
