use chrono::{DateTime, Utc};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    System,
    User,
    Assistant,
    ToolCall,
    ToolResult,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: Option<i64>,
    pub session_id: String,
    pub sequence_num: u64,
    pub message_type: MessageType,
    pub timestamp: DateTime<Utc>,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub anthropic_msg_id: Option<String>,
    pub request_id: Option<String>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub tokens_estimated: bool,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub project_name: String,
    pub file_path: PathBuf,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub status: SessionStatus,
    pub message_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Watching,
    Ended,
    Stale,
}
