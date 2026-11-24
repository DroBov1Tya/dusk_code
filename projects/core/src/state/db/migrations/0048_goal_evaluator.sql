-- A goal is judged after each turn by an evaluator, not by approved criteria and saved
-- evidence. `rounds` now counts the turns it judged, `reason` keeps its newest reason
-- and `started_at` when the goal was set or the app started again with it. The contract
-- and verification columns of migration 16, goal_confirmations and the goal provenance
-- of tool_executions and background_jobs are no longer read or written.
ALTER TABLE goals ADD COLUMN started_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE goals ADD COLUMN reason TEXT;
-- A goal is cleared once met, so a done one is history nobody reads; a draft the user
-- never approved never ran.
DELETE FROM goals WHERE status = 'done' OR approved_at IS NULL;
UPDATE goals SET started_at = updated_at, rounds = 0;
