-- The change ledger: each file a tool call changed in its project, found by walking the
-- project before and after the call, so it works without git. A row goes with its
-- thread. An undo removes turns but leaves the files as they are, so the row outlives
-- its turn and only loses the link. `diff` is NULL when the text before was not known
-- or the file may hold secrets; `diff_cut` says the stored diff hit its cap.
CREATE TABLE IF NOT EXISTS changes (
    change_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    turn_id TEXT REFERENCES turns(turn_id) ON DELETE SET NULL,
    call_id TEXT,
    project TEXT NOT NULL CHECK (length(trim(project)) > 0),
    path TEXT NOT NULL CHECK (length(path) > 0 AND substr(path, 1, 1) != '/'),
    kind TEXT NOT NULL CHECK (kind IN ('created', 'modified', 'deleted')),
    size_before INTEGER CHECK (size_before >= 0),
    size_after INTEGER CHECK (size_after >= 0),
    hash_after TEXT,
    diff TEXT,
    diff_cut INTEGER NOT NULL DEFAULT 0 CHECK (diff_cut IN (0, 1)),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    CHECK (
        (kind = 'created' AND size_before IS NULL AND size_after IS NOT NULL) OR
        (kind = 'modified' AND size_before IS NOT NULL AND size_after IS NOT NULL) OR
        (kind = 'deleted' AND size_before IS NOT NULL AND size_after IS NULL
            AND hash_after IS NULL)
    ),
    CHECK (diff IS NOT NULL OR diff_cut = 0)
);
CREATE INDEX IF NOT EXISTS changes_project_path ON changes(project, path);
CREATE INDEX IF NOT EXISTS changes_thread_time ON changes(thread_id, created_at);
