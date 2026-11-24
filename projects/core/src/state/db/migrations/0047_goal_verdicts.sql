-- The evaluator's verdicts on a goal are kept in the chat, so its transcript shows them
-- again once reopened, in any client. A verdict is stored with the role system, as a
-- job result is, and read as a goal verdict, so the role constraint stays as it is. It
-- is never sent to the model.
ALTER TABLE messages ADD COLUMN goal_verdict INTEGER NOT NULL DEFAULT 0
    CHECK (goal_verdict IN (0, 1) AND (goal_verdict = 0 OR role = 'system'));
