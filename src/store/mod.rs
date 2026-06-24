use std::path::Path;
use std::sync::Mutex;

use anyhow::Context;
use rusqlite::{params, Connection};

use crate::model::{Message, MessageType, Session, SessionStatus};

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating db directory {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening SQLite at {}", path.display()))?;
        conn.execute_batch(include_str!("schema.sql"))?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert_session(&self, s: &Session) -> anyhow::Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        conn.execute(
            "INSERT OR IGNORE INTO sessions
             (id, project_name, file_path, first_seen_at, last_seen_at, status, message_count)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                s.id,
                s.project_name,
                s.file_path.to_string_lossy().as_ref(),
                s.first_seen_at.to_rfc3339(),
                s.last_seen_at.to_rfc3339(),
                session_status_str(&s.status),
                s.message_count as i64,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_session_last_seen(
        &self,
        id: &str,
        timestamp: &chrono::DateTime<chrono::Utc>,
        message_count: u64,
    ) -> anyhow::Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        conn.execute(
            "UPDATE sessions SET last_seen_at=?1, message_count=?2 WHERE id=?3",
            params![timestamp.to_rfc3339(), message_count as i64, id],
        )?;
        Ok(())
    }

    pub fn insert_message(&self, m: &Message) -> anyhow::Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        conn.execute(
            "INSERT OR IGNORE INTO messages
             (session_id, sequence_num, message_type, timestamp, content,
              tool_name, tool_use_id, anthropic_msg_id, request_id,
              input_tokens, output_tokens, model)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                m.session_id,
                m.sequence_num as i64,
                message_type_str(&m.message_type),
                m.timestamp.to_rfc3339(),
                m.content,
                m.tool_name,
                m.tool_use_id,
                m.anthropic_msg_id,
                m.request_id,
                m.input_tokens.map(|v| v as i64),
                m.output_tokens.map(|v| v as i64),
                m.model,
            ],
        )?;
        Ok(())
    }

    pub fn query_messages(&self, session_id: &str) -> anyhow::Result<Vec<Message>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, sequence_num, message_type, timestamp, content,
                    tool_name, tool_use_id, anthropic_msg_id, request_id,
                    input_tokens, output_tokens, model
             FROM messages WHERE session_id=?1
             ORDER BY sequence_num ASC",
        )?;
        let rows: Vec<_> = stmt
            .query_map(params![session_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        let mut messages = Vec::with_capacity(rows.len());
        for (
            id,
            sid,
            seq,
            mt_str,
            ts_str,
            content,
            tool_name,
            tool_use_id,
            anthropic_msg_id,
            request_id,
            input_tokens,
            output_tokens,
            model,
        ) in rows
        {
            let message_type = parse_message_type(&mt_str);
            let timestamp = parse_ts(&ts_str);
            messages.push(Message {
                id: Some(id),
                session_id: sid,
                sequence_num: seq as u64,
                message_type,
                timestamp,
                content,
                tool_name,
                tool_use_id,
                anthropic_msg_id,
                request_id,
                input_tokens: input_tokens.map(|v| v as u32),
                output_tokens: output_tokens.map(|v| v as u32),
                model,
            });
        }
        Ok(messages)
    }

    pub fn list_sessions(&self) -> anyhow::Result<Vec<Session>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.project_name, s.file_path, s.first_seen_at, s.last_seen_at,
                    s.status, COUNT(m.id)
             FROM sessions s LEFT JOIN messages m ON m.session_id = s.id
             GROUP BY s.id
             ORDER BY s.last_seen_at ASC",
        )?;
        let rows: Vec<_> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        let mut sessions = Vec::with_capacity(rows.len());
        for (id, project_name, file_path, first_seen_at, last_seen_at, status_str, message_count) in
            rows
        {
            sessions.push(Session {
                id,
                project_name,
                file_path: std::path::PathBuf::from(file_path),
                first_seen_at: parse_ts(&first_seen_at),
                last_seen_at: parse_ts(&last_seen_at),
                status: parse_session_status(&status_str),
                message_count: message_count as u64,
            });
        }
        Ok(sessions)
    }

    pub fn count_messages(&self, session_id: &str) -> anyhow::Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id=?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn mark_stale_if_missing(&self) -> anyhow::Result<usize> {
        let watching = {
            let conn = self
                .conn
                .lock()
                .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
            let mut stmt =
                conn.prepare("SELECT id, file_path FROM sessions WHERE status='watching'")?;
            let rows: Vec<(String, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<_, _>>()?;
            rows
        };

        let mut count = 0;
        for (id, file_path) in watching {
            if !std::path::Path::new(&file_path).exists() {
                self.set_session_status(&id, SessionStatus::Stale)?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn list_stale_sessions(&self) -> anyhow::Result<Vec<Session>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.project_name, s.file_path, s.first_seen_at, s.last_seen_at,
                    s.status, COUNT(m.id)
             FROM sessions s LEFT JOIN messages m ON m.session_id = s.id
             WHERE s.status='stale'
             GROUP BY s.id
             ORDER BY s.last_seen_at ASC",
        )?;
        let rows: Vec<_> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        let mut sessions = Vec::with_capacity(rows.len());
        for (id, project_name, file_path, first_seen_at, last_seen_at, status_str, message_count) in
            rows
        {
            sessions.push(Session {
                id,
                project_name,
                file_path: std::path::PathBuf::from(file_path),
                first_seen_at: parse_ts(&first_seen_at),
                last_seen_at: parse_ts(&last_seen_at),
                status: parse_session_status(&status_str),
                message_count: message_count as u64,
            });
        }
        Ok(sessions)
    }

    pub fn clear_all(&self) -> anyhow::Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        conn.execute("DELETE FROM messages", [])?;
        let deleted = conn.execute("DELETE FROM sessions", [])?;
        Ok(deleted)
    }

    pub fn delete_stale_sessions(&self) -> anyhow::Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        conn.execute(
            "DELETE FROM messages WHERE session_id IN (SELECT id FROM sessions WHERE status='stale')",
            [],
        )?;
        let deleted = conn.execute("DELETE FROM sessions WHERE status='stale'", [])?;
        Ok(deleted)
    }

    pub fn set_session_status(&self, id: &str, status: SessionStatus) -> anyhow::Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        conn.execute(
            "UPDATE sessions SET status=?1 WHERE id=?2",
            params![session_status_str(&status), id],
        )?;
        Ok(())
    }

    pub fn message_exists(
        &self,
        session_id: &str,
        anthropic_msg_id: Option<&str>,
        request_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        if anthropic_msg_id.is_none() && request_id.is_none() {
            return Ok(false);
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite mutex poisoned"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id=?1 AND anthropic_msg_id=?2 AND request_id=?3",
            params![session_id, anthropic_msg_id, request_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}

fn parse_ts(s: &str) -> chrono::DateTime<chrono::Utc> {
    s.parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap_or_else(|_| {
            tracing::warn!("unparseable timestamp in DB: {s:?}; using Unix epoch as sentinel");
            chrono::TimeZone::timestamp_opt(&chrono::Utc, 0, 0).unwrap()
        })
}

fn message_type_str(mt: &MessageType) -> &'static str {
    match mt {
        MessageType::System => "system",
        MessageType::User => "user",
        MessageType::Assistant => "assistant",
        MessageType::ToolCall => "tool_call",
        MessageType::ToolResult => "tool_result",
    }
}

fn parse_message_type(s: &str) -> MessageType {
    match s {
        "user" => MessageType::User,
        "assistant" => MessageType::Assistant,
        "tool_call" => MessageType::ToolCall,
        "tool_result" => MessageType::ToolResult,
        _ => MessageType::System,
    }
}

fn session_status_str(s: &SessionStatus) -> &'static str {
    match s {
        SessionStatus::Watching => "watching",
        SessionStatus::Ended => "ended",
        SessionStatus::Stale => "stale",
    }
}

fn parse_session_status(s: &str) -> SessionStatus {
    match s {
        "watching" => SessionStatus::Watching,
        "ended" => SessionStatus::Ended,
        _ => SessionStatus::Stale,
    }
}
