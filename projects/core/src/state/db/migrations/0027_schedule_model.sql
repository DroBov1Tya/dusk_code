-- The model a scheduled task runs with, when it names one. A table of its own rather
-- than a column, so the migration can run again on a database that has it, as the
-- legacy-shape tests do.
CREATE TABLE IF NOT EXISTS schedule_models (
    schedule_id TEXT PRIMARY KEY REFERENCES schedules(id) ON DELETE CASCADE,
    model TEXT NOT NULL CHECK (length(trim(model)) > 0)
);
