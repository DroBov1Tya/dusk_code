-- The name an agent's thread goes by, such as Fable-3a, for people and the asking agent.
-- Unique among the agents one thread asked, so a name is enough to address one.
ALTER TABLE threads ADD COLUMN name TEXT CHECK (name IS NULL OR length(trim(name)) > 0);
CREATE UNIQUE INDEX threads_parent_name ON threads(parent_thread_id, name) WHERE name IS NOT NULL;
