CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT    PRIMARY KEY,
    project_name    TEXT    NOT NULL,
    file_path       TEXT    NOT NULL,
    first_seen_at   TEXT    NOT NULL,
    last_seen_at    TEXT    NOT NULL,
    status          TEXT    NOT NULL
                    CHECK(status IN ('watching','ended','stale')),
    message_count   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id      TEXT    NOT NULL REFERENCES sessions(id),
    sequence_num    INTEGER NOT NULL,
    message_type    TEXT    NOT NULL
                    CHECK(message_type IN
                         ('system','user','assistant','tool_call','tool_result')),
    timestamp       TEXT    NOT NULL,
    content         TEXT    NOT NULL DEFAULT '',
    tool_name       TEXT,
    tool_use_id     TEXT,
    anthropic_msg_id TEXT,
    request_id      TEXT,
    input_tokens    INTEGER,
    output_tokens   INTEGER,
    model           TEXT,
    UNIQUE(session_id, sequence_num)
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, sequence_num);
CREATE INDEX IF NOT EXISTS idx_messages_dedup   ON messages(anthropic_msg_id, request_id);
