-- Something for the user arrived in a chat since a client last showed it: an answer, a
-- request for a yes, the end of a turn. Only a chat sets it, never an agent's thread.
ALTER TABLE threads ADD COLUMN unread INTEGER NOT NULL DEFAULT 0 CHECK (unread IN (0, 1));
