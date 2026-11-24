-- A team that winds down instead of ending at once: since when, and why. Nothing new is
-- given out, the members at work are asked to hand over, and the team ends when they
-- have or the grace ran out. Both NULL while it runs as usual; kept on an ended row,
-- so its end line can say why, and cleared by the next run.
ALTER TABLE teams ADD COLUMN winding_since INTEGER;
ALTER TABLE teams ADD COLUMN winding_why TEXT
    CHECK (winding_why IN ('stopped', 'quiet', 'provider'))
    CHECK ((winding_since IS NULL) = (winding_why IS NULL));
