-- What a goal's work looked like after its last round: the state of the files its chat
-- changed and the tests it ran, hashed. `same_rounds` counts the rounds in a row that
-- ended in it, 0 after a new goal, an approval or a resume, which start over; a goal
-- whose rounds stop changing anything pauses for the user.
ALTER TABLE goals ADD COLUMN round_fingerprint TEXT;
ALTER TABLE goals ADD COLUMN same_rounds INTEGER NOT NULL DEFAULT 0 CHECK (same_rounds >= 0);
