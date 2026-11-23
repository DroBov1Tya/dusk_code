-- The model an asker picked for its subagent by tier, as `provider/model`. No row: the
-- subagent answers on its chat's model.
CREATE TABLE agent_models (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    model TEXT NOT NULL CHECK (length(model) BETWEEN 3 AND 200)
);

-- The tier and effort the lead picked for a task of the plan; NULL leaves the member on
-- its role's model, else the chat's, at the agents' usual effort.
ALTER TABLE tasks ADD COLUMN tier TEXT CHECK (tier IS NULL OR tier IN ('strong', 'default', 'weak'));
ALTER TABLE tasks ADD COLUMN effort TEXT
    CHECK (effort IS NULL OR effort IN ('off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'));
