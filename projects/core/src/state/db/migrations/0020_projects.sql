-- Folders the user named as projects. A chat belongs to one by the folder it started in.
CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    description TEXT NOT NULL,
    dir TEXT NOT NULL UNIQUE CHECK (length(trim(dir)) > 0),
    created_at INTEGER NOT NULL
);
