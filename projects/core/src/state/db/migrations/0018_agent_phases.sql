-- Where a subagent is in its work: planning, executing, reviewing, done or paused. The
-- agent moves itself on with the agent_state tool; the runtime starts each task in
-- planning and pauses what a stop or a crash cut short.
CREATE TABLE agent_phases (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id),
    phase TEXT NOT NULL
        CHECK (phase IN ('planning', 'executing', 'reviewing', 'done', 'paused')),
    note TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
);
