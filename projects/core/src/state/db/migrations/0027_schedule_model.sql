-- The model a scheduled task runs with; NULL takes the chosen one.
ALTER TABLE schedules ADD COLUMN model TEXT;
