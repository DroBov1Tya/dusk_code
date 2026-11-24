-- A team's flow: what each member reported, the rows the core adds itself (a review of a
-- task that changed files, the completeness check at the end), the tasks the lead gave up
-- so the next wave can start, and the goal the lead ran the team toward.

-- The member's footer as it handed in, and a reviewer's verdict; cleaned of secrets.
-- Model text: data.
ALTER TABLE tasks ADD COLUMN result TEXT NOT NULL DEFAULT '' CHECK (length(result) <= 4000);
-- What the row is for: the lead's work, a review of another task, or the critic.
ALTER TABLE tasks ADD COLUMN duty TEXT NOT NULL DEFAULT 'work'
    CHECK (duty IN ('work', 'review', 'critic'));
-- For a review: the task it reviews.
ALTER TABLE tasks ADD COLUMN review_of TEXT REFERENCES tasks(task_id);
-- The lead gave it up: a failed or blocked task that no longer holds the next wave.
ALTER TABLE tasks ADD COLUMN skipped INTEGER NOT NULL DEFAULT 0 CHECK (skipped IN (0, 1));
CREATE INDEX tasks_review_of ON tasks(review_of) WHERE review_of IS NOT NULL;

ALTER TABLE teams ADD COLUMN goal TEXT NOT NULL DEFAULT '' CHECK (length(goal) <= 600);
