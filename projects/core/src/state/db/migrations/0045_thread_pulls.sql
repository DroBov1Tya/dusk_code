-- The pull requests of a chat: those a message in it links, kept after they are merged or
-- closed. A pull request is named by the forge repository and its number; `status` is the
-- last `PullStatus` the core read of it, as JSON, NULL until the first read.
CREATE TABLE thread_pulls (
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    -- As the forge names it, such as `owner/repo`.
    repository TEXT NOT NULL CHECK (length(trim(repository)) > 0),
    number INTEGER NOT NULL CHECK (number > 0),
    status TEXT CHECK (status IS NULL OR json_valid(status)),
    -- When the chat first linked it, Unix milliseconds; the list is in this order.
    added_at INTEGER NOT NULL CHECK (added_at >= 0),
    -- When the core last looked it up, Unix milliseconds; NULL when it never did. The
    -- least lately tried is read first, so one that never answers holds no other back.
    tried_at INTEGER CHECK (tried_at IS NULL OR tried_at >= 0),
    -- Reads in a row that found nothing, back to 0 once one finds it; after a few, one
    -- never found is read no more.
    misses INTEGER NOT NULL DEFAULT 0 CHECK (misses >= 0),
    PRIMARY KEY (thread_id, repository, number)
);

-- The newest message of the chat already looked through for links to pull requests.
ALTER TABLE threads ADD COLUMN pulls_scanned INTEGER NOT NULL DEFAULT 0 CHECK (pulls_scanned >= 0);
