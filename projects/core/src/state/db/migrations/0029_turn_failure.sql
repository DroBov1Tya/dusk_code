-- A turn that failed on a model call keeps why and after how many attempts, so a chat
-- opened again shows the same row. Null for other failures and for older turns.
ALTER TABLE turns ADD COLUMN error_kind TEXT;
ALTER TABLE turns ADD COLUMN attempts INTEGER CHECK (attempts IS NULL OR attempts > 0);
