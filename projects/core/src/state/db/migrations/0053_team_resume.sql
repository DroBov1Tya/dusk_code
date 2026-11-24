-- A task lost by its member goes back to that member, which keeps its transcript: the
-- thread that held it last, kept when the owner is cleared, and how often the task was
-- given back to it, which names each resume note once.
ALTER TABLE tasks ADD COLUMN last_owner_thread_id TEXT REFERENCES threads(id);
ALTER TABLE tasks ADD COLUMN resumes INTEGER NOT NULL DEFAULT 0 CHECK (resumes >= 0);
