ALTER TABLE threads RENAME TO threads_legacy;

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

INSERT INTO threads (
    id, source, created_at, updated_at, tokens_used, compaction_count,
    sandbox_policy, approval_mode, git_branch, git_commit_sha, git_origin_url, title
)
SELECT
    id, "from", created_at, updated_at, token_count, 0,
    CASE sandbox_enabled WHEN 1 THEN 'enabled' ELSE 'disabled' END,
    CASE permission_mode WHEN 'never' THEN 'never' ELSE 'on_request' END,
    NULL, NULL, NULL, ''
FROM threads_legacy;

DROP TABLE threads_legacy;
