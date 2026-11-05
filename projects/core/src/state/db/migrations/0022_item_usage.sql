-- Durable usage counters for context items the model can use: skills loaded with
-- load_skill and MCP tools called. One row per item, so counts survive restarts and
-- span every thread; the per-thread rows give the current chat's counts.
-- `IF NOT EXISTS`: the legacy-shape tests re-run the migrations on databases that
-- already carry them.
CREATE TABLE IF NOT EXISTS item_usage (
    kind TEXT NOT NULL CHECK (kind IN ('skill', 'mcp tool')),
    -- The name the model calls it by: the skill name, or `server__tool`.
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    -- Uses across all threads.
    uses_total INTEGER NOT NULL DEFAULT 0 CHECK (uses_total >= 0),
    PRIMARY KEY (kind, name)
);

CREATE TABLE IF NOT EXISTS thread_item_usage (
    thread_id TEXT NOT NULL REFERENCES threads(id),
    kind TEXT NOT NULL CHECK (kind IN ('skill', 'mcp tool')),
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    uses INTEGER NOT NULL DEFAULT 0 CHECK (uses >= 0),
    PRIMARY KEY (thread_id, kind, name)
);
