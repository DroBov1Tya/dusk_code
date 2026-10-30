-- The agents found under agents/: each is an AGENT.md whose text is the system prompt
-- of the threads the agent works in. The version grows whenever the file changes, so a
-- thread can tell which identity it was given. The front matter is kept here so the
-- graph is read without opening every file.
CREATE TABLE identities (
    name TEXT PRIMARY KEY NOT NULL CHECK (length(trim(name)) > 0),
    path TEXT NOT NULL CHECK (length(trim(path)) > 0),
    version INTEGER NOT NULL CHECK (version > 0),
    content_hash TEXT NOT NULL,
    description TEXT NOT NULL,
    entry INTEGER NOT NULL CHECK (entry IN (0, 1)),
    -- The agents it may ask, as a JSON array of names.
    links TEXT NOT NULL CHECK (json_valid(links) AND json_type(links) = 'array'),
    -- The tools it gets, as a JSON array; NULL gives it every tool.
    tools TEXT CHECK (tools IS NULL OR (json_valid(tools) AND json_type(tools) = 'array')),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
);
