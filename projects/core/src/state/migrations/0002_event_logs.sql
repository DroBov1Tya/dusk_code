CREATE TABLE event_logs (
    request_uid TEXT PRIMARY KEY NOT NULL CHECK (length(trim(request_uid)) > 0),
    thread_id TEXT NOT NULL,
    received_at INTEGER NOT NULL CHECK (received_at >= 0),
    status TEXT NOT NULL CHECK (status IN ('accepted', 'rejected', 'failed')),
    error_code TEXT,
    CHECK (
        (status = 'accepted' AND error_code IS NULL) OR
        (status = 'rejected' AND error_code IS NOT NULL AND
            error_code IN ('empty_message', 'empty_thread', 'unknown_thread')) OR
        (status = 'failed' AND error_code IS NOT NULL AND error_code = 'execution_failed')
    )
);

CREATE INDEX event_logs_thread_id ON event_logs(thread_id);
