-- The plan as a graph. A plan is the tasks of a root thread, shared by the threads under
-- it. `done` stays for older clients and always equals (status = 'done').
ALTER TABLE tasks ADD COLUMN status TEXT NOT NULL DEFAULT 'open' CHECK (status IN
    ('open', 'claimed', 'running', 'blocked', 'review', 'done', 'failed'));
UPDATE tasks SET status = 'done' WHERE done = 1;
-- Ids of the tasks that must be done first, as a JSON array.
ALTER TABLE tasks ADD COLUMN depends_on TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(depends_on) AND json_type(depends_on) = 'array');
-- The agent definition that does it; NULL is the general agent.
ALTER TABLE tasks ADD COLUMN role TEXT CHECK (role IS NULL OR length(role) BETWEEN 1 AND 128);
-- A label that groups tasks on the strip. The core orders by it and decides nothing by it.
ALTER TABLE tasks ADD COLUMN phase TEXT NOT NULL DEFAULT '' CHECK (length(phase) <= 32);
ALTER TABLE tasks ADD COLUMN acceptance TEXT NOT NULL DEFAULT '' CHECK (length(acceptance) <= 600);
ALTER TABLE tasks ADD COLUMN brief TEXT NOT NULL DEFAULT '' CHECK (length(brief) <= 1200);
-- A GoalCheck as JSON; NULL waits for the lead's review.
ALTER TABLE tasks ADD COLUMN gate TEXT CHECK (gate IS NULL OR json_valid(gate));
-- The paths only this task may change, relative to the project; a folder ends in '/'.
ALTER TABLE tasks ADD COLUMN files TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(files) AND json_type(files) = 'array');
ALTER TABLE tasks ADD COLUMN owner_thread_id TEXT REFERENCES threads(id);
-- Unix ms; the owner's runner moves it on while it lives.
ALTER TABLE tasks ADD COLUMN lease_until INTEGER;
-- Times the task was given out, and times it came back from review.
ALTER TABLE tasks ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0);
ALTER TABLE tasks ADD COLUMN reworks INTEGER NOT NULL DEFAULT 0 CHECK (reworks >= 0);
-- What a task waits for. Blocked: 'task:<id>' or 'message:<id>'. In review: 'lead', when
-- the core held it for the lead whatever its gate says.
ALTER TABLE tasks ADD COLUMN waits_on TEXT;
-- Why it is blocked or failed, or what the review said. Model or program text: data.
ALTER TABLE tasks ADD COLUMN note TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 600);
-- The background job that ran the gate last.
ALTER TABLE tasks ADD COLUMN gate_job TEXT;
-- The commit of the team's branch right after this task was merged; NULL without git.
ALTER TABLE tasks ADD COLUMN checkpoint TEXT CHECK (checkpoint IS NULL OR length(checkpoint) = 40);
-- The model the task was given to, as resolved when it was claimed.
ALTER TABLE tasks ADD COLUMN model TEXT;

CREATE INDEX tasks_status ON tasks(thread_id, status);
-- A member works on one task at a time.
CREATE UNIQUE INDEX tasks_one_at_work ON tasks(owner_thread_id)
    WHERE status IN ('claimed', 'running');

-- Each rule is checked on a new row as well as a changed one: a trigger on UPDATE alone
-- lets an INSERT write what no UPDATE may.
CREATE TRIGGER tasks_done_is_status BEFORE UPDATE ON tasks
    WHEN (NEW.status = 'done') != (NEW.done = 1)
    BEGIN SELECT RAISE(ABORT, 'done and status disagree'); END;
CREATE TRIGGER tasks_done_is_status_new BEFORE INSERT ON tasks
    WHEN (NEW.status = 'done') != (NEW.done = 1)
    BEGIN SELECT RAISE(ABORT, 'done and status disagree'); END;
CREATE TRIGGER tasks_lease_by_status BEFORE UPDATE ON tasks
    WHEN (NEW.status IN ('claimed', 'running')
              AND (NEW.owner_thread_id IS NULL OR NEW.lease_until IS NULL))
        OR (NEW.status NOT IN ('claimed', 'running') AND NEW.lease_until IS NOT NULL)
    BEGIN SELECT RAISE(ABORT, 'a task at work has an owner and a lease; no other has a lease'); END;
CREATE TRIGGER tasks_lease_by_status_new BEFORE INSERT ON tasks
    WHEN (NEW.status IN ('claimed', 'running')
              AND (NEW.owner_thread_id IS NULL OR NEW.lease_until IS NULL))
        OR (NEW.status NOT IN ('claimed', 'running') AND NEW.lease_until IS NOT NULL)
    BEGIN SELECT RAISE(ABORT, 'a task at work has an owner and a lease; no other has a lease'); END;
CREATE TRIGGER tasks_open_has_no_owner BEFORE UPDATE ON tasks
    WHEN NEW.status = 'open' AND NEW.owner_thread_id IS NOT NULL
    BEGIN SELECT RAISE(ABORT, 'an open task has no owner'); END;
CREATE TRIGGER tasks_open_has_no_owner_new BEFORE INSERT ON tasks
    WHEN NEW.status = 'open' AND NEW.owner_thread_id IS NOT NULL
    BEGIN SELECT RAISE(ABORT, 'an open task has no owner'); END;

-- The task a turn worked on, so its changes and tokens are found by a join.
ALTER TABLE turns ADD COLUMN task_id TEXT REFERENCES tasks(task_id);
CREATE INDEX turns_task ON turns(task_id) WHERE task_id IS NOT NULL;
