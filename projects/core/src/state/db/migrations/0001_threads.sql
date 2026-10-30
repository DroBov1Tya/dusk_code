CREATE TABLE threads (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    -- Who started the thread, such as "cli".
    source TEXT NOT NULL CHECK (length(trim(source)) > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    -- Set from the first message, then replaced by a name the model gives the thread.
    title TEXT CHECK (title IS NULL OR length(trim(title)) > 0),
    -- A subagent works in a thread of its own under the thread that asked it; chats have
    -- none of these three set.
    parent_thread_id TEXT REFERENCES threads(id),
    -- The agent it works as, from agents/.
    agent TEXT CHECK (agent IS NULL OR length(trim(agent)) > 0),
    -- The name it goes by, such as Fable-3a, for people and the asking agent.
    name TEXT CHECK (name IS NULL OR length(trim(name)) > 0)
);

CREATE INDEX threads_parent ON threads(parent_thread_id);
-- Unique among the agents one thread asked, so a name is enough to address one.
CREATE UNIQUE INDEX threads_parent_name ON threads(parent_thread_id, name) WHERE name IS NOT NULL;
