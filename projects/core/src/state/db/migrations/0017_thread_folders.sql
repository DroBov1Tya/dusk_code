-- Where a thread's files live under the workspace root: a chat's folder is named by the
-- day it began, an agent's sits inside its chat. A thread made before this table has no
-- row: its folder is named by its ID.
CREATE TABLE thread_folders (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id),
    folder TEXT NOT NULL UNIQUE CHECK (length(trim(folder)) > 0)
);
