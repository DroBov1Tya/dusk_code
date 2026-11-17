-- A running turn may wait for the user's decision or for the provider, and a restart
-- must find the wait where it was. Waiting stays a sub-state of 'running', so the one
-- running turn per thread still holds and the table is not rebuilt (a rebuild would
-- delete tool_executions by cascade). Null on a turn that does not wait.
ALTER TABLE turns ADD COLUMN waiting_for TEXT CHECK (waiting_for IN
  ('decision', 'rate_limited', 'quota', 'unreachable', 'timed_out', 'overloaded'));
ALTER TABLE turns ADD COLUMN waiting_since INTEGER;
-- Null for 'decision': only the user ends that wait.
ALTER TABLE turns ADD COLUMN wake_at INTEGER;
CREATE TRIGGER turns_wait_only_running BEFORE UPDATE ON turns
  WHEN NEW.status != 'running' AND NEW.waiting_for IS NOT NULL
  BEGIN SELECT RAISE(ABORT, 'only a running turn waits'); END;
