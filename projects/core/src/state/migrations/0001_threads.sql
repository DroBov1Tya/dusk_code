CREATE TABLE threads (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    "from" TEXT NOT NULL CHECK (length(trim("from")) > 0),
    "to" TEXT NOT NULL CHECK (length(trim("to")) > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    permission_mode TEXT NOT NULL CHECK (length(trim(permission_mode)) > 0),
    sandbox_enabled INTEGER NOT NULL CHECK (sandbox_enabled IN (0, 1)),
    token_count INTEGER NOT NULL CHECK (token_count >= 0)
);
