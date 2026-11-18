-- A team: the run of a plan by the core. One per root thread; the lead is that thread.
-- A later run after an end resets the row. What it has used is counted from model_usage,
-- never kept here.
CREATE TABLE IF NOT EXISTS teams (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id),
    status TEXT NOT NULL CHECK (status IN ('running', 'ended')),
    ended_why TEXT CHECK (ended_why IN
        ('done', 'steps', 'tokens', 'time', 'stalled', 'stopped', 'quiet')),
    at_once INTEGER NOT NULL CHECK (at_once BETWEEN 1 AND 8),
    max_steps INTEGER NOT NULL CHECK (max_steps > 0),
    max_tokens INTEGER NOT NULL CHECK (max_tokens > 0),
    -- Unix ms after which nothing is given out; the budget's minutes after started_at.
    deadline INTEGER NOT NULL,
    -- Members each work in a tree of their own; 0 shares the project folder.
    worktrees INTEGER NOT NULL DEFAULT 0 CHECK (worktrees IN (0, 1)),
    -- HEAD when the team's branch was made; NULL when worktrees = 0.
    base_commit TEXT CHECK (base_commit IS NULL OR length(base_commit) = 40),
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    ended_at INTEGER,
    -- Each on its own: together in one AND, a running team could keep a reason.
    CHECK ((status = 'ended') = (ended_why IS NOT NULL)),
    CHECK ((status = 'ended') = (ended_at IS NOT NULL)),
    CHECK (deadline > started_at),
    CHECK (worktrees = 1 OR base_commit IS NULL)
);
