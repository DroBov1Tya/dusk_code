-- The token that started a chat, by its id. NULL for a chat the local clients started,
-- and for one older than this record.
ALTER TABLE threads ADD COLUMN owner TEXT CHECK (owner IS NULL OR length(owner) BETWEEN 1 AND 128);
