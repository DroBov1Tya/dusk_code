-- The project graph: for each saved project, the files the core walked, with numbers it
-- computes itself (lines, bytes, hash, change time), and the facts agents or the user
-- wrote about them: a summary per file, the nodes a file holds and the edges from them.
-- The core never reads a language. Every text is data. A fact is stale when the file's
-- hash is no longer the one it was written against (`summary_hash`, `source_hash`); it
-- is kept and marked, never deleted for that. A file that is gone takes its facts along.
-- The tables of 0034 had the same names and were dropped by 0039.
CREATE TABLE IF NOT EXISTS graph_files (
    id INTEGER PRIMARY KEY,
    project TEXT NOT NULL REFERENCES projects(dir) ON DELETE CASCADE ON UPDATE CASCADE,
    path TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 1024),
    lines INTEGER NOT NULL CHECK (lines >= 0),
    bytes INTEGER NOT NULL CHECK (bytes >= 0),
    -- SHA-256 in hex; empty for a file the core does not hash (secrets, too large).
    hash TEXT NOT NULL CHECK (length(hash) IN (0, 64)),
    modified_ms INTEGER NOT NULL,
    summary TEXT NOT NULL DEFAULT '' CHECK (length(summary) <= 400),
    summary_hash TEXT CHECK (summary_hash IS NULL OR length(summary_hash) = 64),
    -- The thread that wrote the summary; not a foreign key, the fact outlives a chat.
    written_by TEXT CHECK (written_by IS NULL OR length(written_by) <= 64),
    by_user INTEGER NOT NULL DEFAULT 0 CHECK (by_user IN (0, 1)),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
    UNIQUE (project, path)
);

CREATE TABLE IF NOT EXISTS graph_nodes (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES graph_files(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK (kind IN
        ('module', 'type', 'function', 'service', 'test', 'config', 'doc', 'other')),
    line INTEGER CHECK (line IS NULL OR line > 0),
    end_line INTEGER CHECK (end_line IS NULL OR (line IS NOT NULL AND end_line >= line)),
    summary TEXT NOT NULL DEFAULT '' CHECK (length(summary) <= 200),
    source_hash TEXT NOT NULL CHECK (length(source_hash) = 64),
    written_by TEXT CHECK (written_by IS NULL OR length(written_by) <= 64),
    by_user INTEGER NOT NULL DEFAULT 0 CHECK (by_user IN (0, 1)),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
    UNIQUE (file_id, name, kind)
);
CREATE INDEX IF NOT EXISTS graph_nodes_name ON graph_nodes(name);

-- An edge goes from a file (`from_node` 0) or one of its nodes to a name: a node's
-- name, `path#name` or a file's path, resolved when asked, so recording one file never
-- rewrites another's rows.
CREATE TABLE IF NOT EXISTS graph_edges (
    id INTEGER PRIMARY KEY,
    from_file INTEGER NOT NULL REFERENCES graph_files(id) ON DELETE CASCADE,
    from_node INTEGER NOT NULL DEFAULT 0,
    to_name TEXT NOT NULL CHECK (length(to_name) BETWEEN 1 AND 256),
    kind TEXT NOT NULL CHECK (kind IN
        ('uses', 'imports', 'calls', 'covers', 'configures', 'implements')),
    source_hash TEXT NOT NULL CHECK (length(source_hash) = 64),
    written_by TEXT CHECK (written_by IS NULL OR length(written_by) <= 64),
    by_user INTEGER NOT NULL DEFAULT 0 CHECK (by_user IN (0, 1)),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
    UNIQUE (from_file, from_node, to_name, kind)
);
CREATE INDEX IF NOT EXISTS graph_edges_to ON graph_edges(to_name);

CREATE TRIGGER IF NOT EXISTS graph_node_edges_delete AFTER DELETE ON graph_nodes BEGIN
    DELETE FROM graph_edges WHERE from_node = old.id;
END;
