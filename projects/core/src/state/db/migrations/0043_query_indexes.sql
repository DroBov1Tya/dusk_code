-- When a thread was last active: one index seek per thread instead of reading every
-- message of it, content included, on each thread list.
CREATE INDEX IF NOT EXISTS messages_thread_time ON messages(thread_id, created_at);
-- Messages agents sent: deleting a chat finds those its agents sent, by index.
CREATE INDEX IF NOT EXISTS messages_from_thread ON messages(from_thread_id)
    WHERE from_thread_id IS NOT NULL;
-- What a turn spent, read with every turn loaded and per task on the plan.
CREATE INDEX IF NOT EXISTS model_usage_turn ON model_usage(turn_id, purpose);
-- Keys a deleted row is looked up by, in its foreign key check or its cascade, once for
-- every row deleted; without them each lookup reads the whole table.
CREATE INDEX IF NOT EXISTS model_usage_compaction ON model_usage(compaction_id);
CREATE INDEX IF NOT EXISTS tool_executions_call ON tool_executions(call_message_id);
-- A deleted message leaves the search index with it, however it is deleted. Messages are
-- never updated; if they ever are, an update trigger must delete the old text and add
-- the new.
CREATE TRIGGER IF NOT EXISTS messages_search_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_search (messages_search, rowid, content, tool_calls_json)
    VALUES ('delete', old.rowid, old.content, old.tool_calls_json);
END;
