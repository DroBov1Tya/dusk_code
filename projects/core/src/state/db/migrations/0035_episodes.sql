-- What ended a problem in a project, written by the core from what it saw. The text
-- comes from tool output and model output: it is cleaned of secrets before it is stored
-- and is data for whoever reads it. `id` is an integer so the search index keeps its
-- rows through VACUUM (see backup/archive/export.rs).
CREATE TABLE IF NOT EXISTS episodes (
    id INTEGER PRIMARY KEY,
    project TEXT NOT NULL REFERENCES projects(dir) ON DELETE CASCADE ON UPDATE CASCADE,
    thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL,
    turn_id TEXT REFERENCES turns(turn_id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (kind IN ('fixed', 'verified')),
    -- SHA-256 of what makes two episodes the same one.
    signature TEXT NOT NULL CHECK (length(signature) = 64),
    tool TEXT NOT NULL CHECK (length(tool) BETWEEN 1 AND 64),
    model TEXT NOT NULL CHECK (length(model) BETWEEN 1 AND 128),
    problem TEXT NOT NULL CHECK (length(problem) BETWEEN 1 AND 400),
    failed TEXT NOT NULL DEFAULT '' CHECK (length(failed) <= 600),
    worked TEXT NOT NULL CHECK (length(worked) BETWEEN 1 AND 300),
    lesson TEXT NOT NULL DEFAULT '' CHECK (length(lesson) <= 200),
    seen INTEGER NOT NULL DEFAULT 1 CHECK (seen > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    seen_at INTEGER NOT NULL CHECK (seen_at >= created_at),
    UNIQUE (project, signature)
);

CREATE VIRTUAL TABLE IF NOT EXISTS episodes_search USING fts5(
    problem, failed, worked, lesson,
    content = 'episodes', content_rowid = 'id', tokenize = 'trigram'
);
CREATE TRIGGER IF NOT EXISTS episodes_search_insert AFTER INSERT ON episodes BEGIN
    INSERT INTO episodes_search (rowid, problem, failed, worked, lesson)
    VALUES (new.id, new.problem, new.failed, new.worked, new.lesson);
END;
CREATE TRIGGER IF NOT EXISTS episodes_search_delete AFTER DELETE ON episodes BEGIN
    INSERT INTO episodes_search (episodes_search, rowid, problem, failed, worked, lesson)
    VALUES ('delete', old.id, old.problem, old.failed, old.worked, old.lesson);
END;
CREATE TRIGGER IF NOT EXISTS episodes_search_update
AFTER UPDATE OF problem, failed, worked, lesson ON episodes BEGIN
    INSERT INTO episodes_search (episodes_search, rowid, problem, failed, worked, lesson)
    VALUES ('delete', old.id, old.problem, old.failed, old.worked, old.lesson);
    INSERT INTO episodes_search (rowid, problem, failed, worked, lesson)
    VALUES (new.id, new.problem, new.failed, new.worked, new.lesson);
END;
