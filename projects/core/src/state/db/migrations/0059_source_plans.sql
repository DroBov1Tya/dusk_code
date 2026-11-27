-- When a plan was last written; null for a source that is no plan. A chat keeps several
-- plans as sources, and this tells them from attached files, so old ones can be deleted.
ALTER TABLE sources ADD COLUMN plan_written_at INTEGER
    CHECK (plan_written_at IS NULL OR plan_written_at >= 0);

-- Before this, the plan the user and the model shared was the source named plan.md.
UPDATE sources SET plan_written_at = created_at WHERE name = 'plan.md';
