-- The project graph: the structure of a saved project's source, never its text. Every
-- row can be built again from the folder, so losing one costs an indexing, not data.
CREATE TABLE IF NOT EXISTS graph_files (
    file_id INTEGER PRIMARY KEY,
    project TEXT NOT NULL REFERENCES projects(dir) ON DELETE CASCADE ON UPDATE CASCADE,
    path TEXT NOT NULL CHECK (length(path) > 0 AND substr(path, 1, 1) != '/'),
    language TEXT NOT NULL
        CHECK (language IN ('rust', 'typescript', 'tsx', 'javascript', 'python', 'manifest')),
    hash TEXT NOT NULL CHECK (length(hash) = 64),
    size INTEGER NOT NULL CHECK (size >= 0),
    modified_ms INTEGER NOT NULL,
    module TEXT NOT NULL,
    is_test INTEGER NOT NULL DEFAULT 0 CHECK (is_test IN (0, 1)),
    -- Why the file has no symbols: it went past a cap. NULL when it was parsed.
    skipped TEXT CHECK (skipped IN ('too_large', 'too_slow', 'too_many', 'long_lines')),
    UNIQUE (project, path)
);

CREATE TABLE IF NOT EXISTS graph_symbols (
    symbol_id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES graph_files(file_id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 128),
    -- The type, impl, class or module the symbol is written in.
    parent TEXT CHECK (parent IS NULL OR length(parent) BETWEEN 1 AND 128),
    kind TEXT NOT NULL
        CHECK (kind IN ('function', 'method', 'type', 'trait', 'constant', 'module', 'test')),
    line INTEGER NOT NULL CHECK (line > 0),
    end_line INTEGER NOT NULL CHECK (end_line >= line)
);
CREATE INDEX IF NOT EXISTS graph_symbols_name ON graph_symbols(name);
CREATE INDEX IF NOT EXISTS graph_symbols_file ON graph_symbols(file_id);

-- One row per name a file refers to: the first line and how many times. For an import
-- `path` is the path as written; for a dependency it is the version asked for.
CREATE TABLE IF NOT EXISTS graph_refs (
    file_id INTEGER NOT NULL REFERENCES graph_files(file_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('use', 'import', 'dependency')),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 128),
    line INTEGER NOT NULL CHECK (line > 0),
    uses INTEGER NOT NULL DEFAULT 1 CHECK (uses > 0),
    path TEXT CHECK (path IS NULL OR length(path) <= 200),
    PRIMARY KEY (file_id, kind, name)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS graph_refs_name ON graph_refs(name, kind);

-- "Changed together" joins the ledger to itself by turn.
CREATE INDEX IF NOT EXISTS changes_turn ON changes(turn_id);
