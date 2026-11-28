-- The model a chat talks to, as `provider/model`, and the reasoning effort it thinks at:
-- choosing them in one chat leaves every other chat, its agents and its team as they
-- were. NULL follows the settings, as before; the core fills the model in when it opens
-- the chat. An effort of 'default' is the provider's own.
ALTER TABLE threads ADD COLUMN model TEXT CHECK (model IS NULL OR length(model) BETWEEN 3 AND 200);
ALTER TABLE threads ADD COLUMN effort TEXT
    CHECK (effort IS NULL OR effort IN ('default', 'off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'));
