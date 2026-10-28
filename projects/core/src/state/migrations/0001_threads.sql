CREATE TABLE threads (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    -- Who started the thread, such as "cli".
    source TEXT NOT NULL CHECK (length(trim(source)) > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    -- The model context holds the messages after this sequence; compacting moves it.
    context_from INTEGER NOT NULL DEFAULT 0 CHECK (context_from >= 0)
);
