CREATE TABLE threads (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    source TEXT NOT NULL CHECK (length(trim(source)) > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    tokens_used INTEGER NOT NULL CHECK (tokens_used >= 0),
    compaction_count INTEGER NOT NULL CHECK (compaction_count >= 0),
    sandbox_policy TEXT NOT NULL CHECK (length(trim(sandbox_policy)) > 0),
    approval_mode TEXT NOT NULL CHECK (approval_mode IN ('on_request', 'never')),
    git_branch TEXT,
    git_commit_sha TEXT,
    git_origin_url TEXT,
    title TEXT NOT NULL
);
