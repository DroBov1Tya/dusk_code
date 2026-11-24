-- Teams have no budget now: the steps, tokens and deadline columns go. SQLite cannot drop
-- a column a CHECK names, so the table is made again and its rows copied. An old row that
-- ended on a budget keeps 'steps', 'tokens' or 'time' as its reason.
CREATE TABLE teams_new (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id),
    status TEXT NOT NULL CHECK (status IN ('running', 'ended')),
    ended_why TEXT CHECK (ended_why IN
        ('done', 'steps', 'tokens', 'time', 'stalled', 'stopped', 'quiet')),
    at_once INTEGER NOT NULL CHECK (at_once BETWEEN 1 AND 8),
    -- Members each work in a tree of their own; 0 shares the project folder.
    worktrees INTEGER NOT NULL DEFAULT 0 CHECK (worktrees IN (0, 1)),
    -- HEAD when the team's branch was made; NULL when worktrees = 0.
    base_commit TEXT CHECK (base_commit IS NULL OR length(base_commit) = 40),
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    ended_at INTEGER,
    -- Each on its own: together in one AND, a running team could keep a reason.
    CHECK ((status = 'ended') = (ended_why IS NOT NULL)),
    CHECK ((status = 'ended') = (ended_at IS NOT NULL)),
    CHECK (worktrees = 1 OR base_commit IS NULL)
);

INSERT INTO teams_new (thread_id, status, ended_why, at_once, worktrees, base_commit,
                       started_at, ended_at)
SELECT thread_id, status, ended_why, at_once, worktrees, base_commit, started_at, ended_at
FROM teams;

DROP TABLE teams;
ALTER TABLE teams_new RENAME TO teams;
