-- A thread's standing goal: the model works on it turn after turn until it marks it
-- done, the user pauses or clears it, or the rounds run out. One per thread.
CREATE TABLE goals (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id),
    text TEXT NOT NULL CHECK (length(trim(text)) > 0),
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'done')),
    -- Turns started for the goal without a new message from the user.
    rounds INTEGER NOT NULL CHECK (rounds >= 0),
    -- What the model reported when it marked the goal done, or why it was paused.
    note TEXT CHECK (note IS NULL OR length(trim(note)) > 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
);
