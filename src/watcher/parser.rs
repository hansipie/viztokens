use chrono::DateTime;
use serde::Deserialize;

use crate::model::{Message, MessageType};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonlEntry {
    timestamp: Option<String>,
    message: Option<JsonlMessage>,
    #[allow(dead_code)]
    cost_usd: Option<f64>,
    request_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonlMessage {
    id: Option<String>,
    role: Option<String>,
    content: Option<MessageContent>,
    model: Option<String>,
    #[allow(dead_code)]
    stop_reason: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
    #[allow(dead_code)]
    cache_creation_input_tokens: Option<u64>,
    #[allow(dead_code)]
    cache_read_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
    tool_use_id: Option<String>,
    content: Option<serde_json::Value>,
}

pub fn parse_line(line: &str, session_id: &str, sequence_num: u64) -> anyhow::Result<Vec<Message>> {
    let entry: JsonlEntry = serde_json::from_str(line)?;

    let msg = match entry.message {
        Some(m) => m,
        None => return Ok(vec![]),
    };

    let sid = session_id.to_string();

    let timestamp = entry
        .timestamp
        .as_deref()
        .and_then(|t| t.parse::<DateTime<chrono::Utc>>().ok())
        .unwrap_or_else(chrono::Utc::now);

    let role = msg.role.as_deref().unwrap_or("").to_string();
    let anthropic_msg_id = msg.id.clone();
    let request_id = entry.request_id.clone();
    let model = msg.model.clone();
    let input_tokens = msg
        .usage
        .as_ref()
        .map(|u| u32::try_from(u.input_tokens).unwrap_or(u32::MAX));
    let output_tokens = msg
        .usage
        .as_ref()
        .map(|u| u32::try_from(u.output_tokens).unwrap_or(u32::MAX));

    // User messages have content as a plain string; assistant messages use blocks.
    let (plain_text, blocks) = match msg.content {
        Some(MessageContent::Text(t)) => (Some(t), vec![]),
        Some(MessageContent::Blocks(b)) => (None, b),
        None => (None, vec![]),
    };

    if let Some(text) = plain_text {
        if text.trim().is_empty() {
            return Ok(vec![]);
        }
        let message_type = match role.as_str() {
            "assistant" => MessageType::Assistant,
            "user" => MessageType::User,
            _ => MessageType::System,
        };
        return Ok(vec![Message {
            id: None,
            session_id: sid,
            sequence_num,
            message_type,
            timestamp,
            content: text,
            tool_name: None,
            tool_use_id: None,
            anthropic_msg_id,
            request_id,
            input_tokens,
            output_tokens,
            model,
        }]);
    }

    if blocks.is_empty() {
        return Ok(vec![]);
    }

    let mut messages = Vec::new();
    let mut local_seq = sequence_num;

    for block in &blocks {
        match block.block_type.as_str() {
            "text" => {
                let message_type = if role == "assistant" {
                    MessageType::Assistant
                } else if role == "user" {
                    MessageType::User
                } else {
                    MessageType::System
                };
                let content = block.text.clone().unwrap_or_default();
                messages.push(Message {
                    id: None,
                    session_id: sid.clone(),
                    sequence_num: local_seq,
                    message_type,
                    timestamp,
                    content,
                    tool_name: None,
                    tool_use_id: None,
                    anthropic_msg_id: anthropic_msg_id.clone(),
                    request_id: request_id.clone(),
                    input_tokens,
                    output_tokens,
                    model: model.clone(),
                });
                local_seq += 1;
            }
            "tool_use" => {
                let tool_name = block.name.clone();
                let tool_use_id = block.id.clone();
                let content = if let Some(ref input) = block.input {
                    serde_json::to_string_pretty(input).unwrap_or_default()
                } else {
                    String::new()
                };
                messages.push(Message {
                    id: None,
                    session_id: sid.clone(),
                    sequence_num: local_seq,
                    message_type: MessageType::ToolCall,
                    timestamp,
                    content,
                    tool_name,
                    tool_use_id,
                    anthropic_msg_id: anthropic_msg_id.clone(),
                    request_id: request_id.clone(),
                    input_tokens,
                    output_tokens,
                    model: model.clone(),
                });
                local_seq += 1;
            }
            "tool_result" => {
                let tool_use_id = block.tool_use_id.clone();
                let content = if let Some(ref c) = block.content {
                    match c {
                        serde_json::Value::String(s) => s.clone(),
                        other => serde_json::to_string_pretty(other).unwrap_or_default(),
                    }
                } else {
                    String::new()
                };
                messages.push(Message {
                    id: None,
                    session_id: sid.clone(),
                    sequence_num: local_seq,
                    message_type: MessageType::ToolResult,
                    timestamp,
                    content,
                    tool_name: None,
                    tool_use_id,
                    anthropic_msg_id: anthropic_msg_id.clone(),
                    request_id: request_id.clone(),
                    input_tokens,
                    output_tokens,
                    model: model.clone(),
                });
                local_seq += 1;
            }
            _ => {}
        }
    }

    // Drop messages with no displayable content (e.g. unknown block types only).
    // Tool calls with empty input are kept — they still convey the invocation.
    messages.retain(|m| !m.content.trim().is_empty() || m.tool_name.is_some());

    Ok(messages)
}

pub fn is_sidechain(line: &str) -> bool {
    // Fast path: check for the field before parsing
    if !line.contains("isSidechain") {
        return false;
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Probe {
        is_sidechain: Option<bool>,
    }
    serde_json::from_str::<Probe>(line)
        .ok()
        .and_then(|p| p.is_sidechain)
        .unwrap_or(false)
}
