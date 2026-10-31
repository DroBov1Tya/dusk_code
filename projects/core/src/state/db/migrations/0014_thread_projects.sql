-- The folder a chat was started in, so a client lists only the chats of that folder.
-- A chat made before this table has no row: where it was started is not known.
CREATE TABLE thread_projects (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id),
    dir TEXT NOT NULL CHECK (length(trim(dir)) > 0)
);

CREATE INDEX thread_projects_dir ON thread_projects(dir);
