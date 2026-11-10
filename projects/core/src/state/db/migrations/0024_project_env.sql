-- Variables a project's commands run with. Values are secrets: they are read back only
-- to start a command, never to show a client.
CREATE TABLE IF NOT EXISTS project_env (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (project_id, name)
);
