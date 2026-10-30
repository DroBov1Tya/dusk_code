-- Full-text search over message text and tool calls for read_history. Trigrams match any
-- substring of three characters or more, so paths, identifiers and inflected words are
-- found too. The index reads its text from messages, which are only ever added.
CREATE VIRTUAL TABLE messages_search USING fts5(
    content,
    tool_calls_json,
    content = 'messages',
    tokenize = 'trigram'
);

CREATE TRIGGER messages_search_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_search (rowid, content, tool_calls_json)
    VALUES (new.rowid, new.content, new.tool_calls_json);
END;
