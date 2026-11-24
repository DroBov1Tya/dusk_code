-- The branch a thread works on in a worktree of its own, made by the `branch` tool or by
-- `ask_agent` with `isolated`. At most one per thread; the row goes when the worktree is
-- merged back or abandoned, so a thread with no row works in the folder it started in.
CREATE TABLE thread_worktrees (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id),
    -- The worktree's folder, resolved: the thread's tools work here.
    dir TEXT NOT NULL UNIQUE CHECK (length(trim(dir)) > 0),
    -- The folder the branch was made from, where `base` is checked out and the merge goes.
    worktree_of TEXT NOT NULL CHECK (length(trim(worktree_of)) > 0),
    -- The folder memory, project variables and episodes are kept for: the saved project
    -- the work belongs to, even when `worktree_of` is itself a worktree.
    project TEXT NOT NULL CHECK (length(trim(project)) > 0),
    branch TEXT NOT NULL CHECK (length(trim(branch)) > 0),
    base TEXT NOT NULL CHECK (length(trim(base)) > 0),
    -- The thread that made it: the thread itself, or the one that asked it as an agent.
    made_by TEXT NOT NULL REFERENCES threads(id),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
);

CREATE INDEX thread_worktrees_made_by ON thread_worktrees(made_by);
