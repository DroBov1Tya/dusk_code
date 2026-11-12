-- Prompts the core sends to a new chat in a project on a timetable. `repeat` is the
-- protocol's `Repeat` as JSON; the project's removal takes its schedules with it.
CREATE TABLE IF NOT EXISTS schedules (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    prompt TEXT NOT NULL,
    repeat TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    next_run_at INTEGER,
    last_run_at INTEGER,
    last_thread_id TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS schedules_next_run ON schedules(next_run_at);
