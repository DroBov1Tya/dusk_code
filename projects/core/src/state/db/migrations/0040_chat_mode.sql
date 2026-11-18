-- The mode a chat's turns take up: the one the user last wrote to it in. The setting
-- stays global; a turn the core starts in a chat, such as for a line from its team,
-- keeps the chat's own. NULL follows the setting, as before.
ALTER TABLE threads ADD COLUMN mode TEXT CHECK (mode IS NULL OR length(mode) BETWEEN 1 AND 64);
