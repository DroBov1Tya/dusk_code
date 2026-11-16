-- 0: final or unavailable; 1: pending; 2: the one attempt has been claimed.
ALTER TABLE threads ADD COLUMN auto_title INTEGER NOT NULL DEFAULT 0
    CHECK (auto_title IN (0, 1, 2));
