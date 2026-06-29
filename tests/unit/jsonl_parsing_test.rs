use viztokens::model::MessageType;
use viztokens::claude::parser::{is_sidechain, parse_line};

fn user_text_fixture() -> &'static str {
    r#"{"sessionId":"sess1","timestamp":"2026-06-23T12:00:00Z","message":{"role":"user","content":[{"type":"text","text":"What files are here?"}]}}"#
}

fn assistant_text_fixture() -> &'static str {
    r#"{"sessionId":"sess1","timestamp":"2026-06-23T12:00:01Z","message":{"id":"msg_01","role":"assistant","content":[{"type":"text","text":"Here are the files."}],"model":"claude-sonnet-4-6","usage":{"input_tokens":10,"output_tokens":5}}}"#
}

fn tool_call_fixture() -> &'static str {
    r#"{"sessionId":"sess1","timestamp":"2026-06-23T12:00:02Z","requestId":"req1","message":{"id":"msg_02","role":"assistant","content":[{"type":"tool_use","id":"tu_01","name":"bash","input":{"command":"ls -la"}}],"model":"claude-sonnet-4-6","usage":{"input_tokens":15,"output_tokens":8}}}"#
}

fn tool_result_fixture() -> &'static str {
    r#"{"sessionId":"sess1","timestamp":"2026-06-23T12:00:03Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_01","content":"total 48\ndrwxr-xr-x 5 user group 160 Jun 23 12:00 ."}]}}"#
}

fn system_fixture() -> &'static str {
    r#"{"sessionId":"sess1","timestamp":"2026-06-23T12:00:00Z","message":{"role":"system","content":[{"type":"text","text":"You are a helpful assistant."}]}}"#
}

fn mixed_assistant_fixture() -> &'static str {
    r#"{"sessionId":"sess1","timestamp":"2026-06-23T12:00:04Z","message":{"id":"msg_03","role":"assistant","content":[{"type":"text","text":"Let me check that."},{"type":"tool_use","id":"tu_02","name":"bash","input":{"command":"pwd"}}],"model":"claude-sonnet-4-6","usage":{"input_tokens":20,"output_tokens":12}}}"#
}

fn sidechain_fixture() -> &'static str {
    r#"{"sessionId":"sess1","timestamp":"2026-06-23T12:00:05Z","isSidechain":true,"message":{"id":"msg_sc","role":"assistant","content":[{"type":"text","text":"sidechain reply"}]}}"#
}

fn no_message_fixture() -> &'static str {
    r#"{"sessionId":"sess1","timestamp":"2026-06-23T12:00:06Z","costUsd":0.001}"#
}

#[test]
fn parse_user_text() {
    let msgs = parse_line(user_text_fixture(), "sess1", 1).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].message_type, MessageType::User);
    assert_eq!(msgs[0].content, "What files are here?");
}

#[test]
fn parse_assistant_text() {
    let msgs = parse_line(assistant_text_fixture(), "sess1", 2).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].message_type, MessageType::Assistant);
    assert_eq!(msgs[0].content, "Here are the files.");
}

#[test]
fn parse_tool_call() {
    let msgs = parse_line(tool_call_fixture(), "sess1", 3).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].message_type, MessageType::ToolCall);
    assert_eq!(msgs[0].tool_name.as_deref(), Some("bash"));
    assert_eq!(msgs[0].tool_use_id.as_deref(), Some("tu_01"));
    assert!(msgs[0].content.contains("ls -la"));
}

#[test]
fn parse_tool_result() {
    let msgs = parse_line(tool_result_fixture(), "sess1", 4).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].message_type, MessageType::ToolResult);
    assert_eq!(msgs[0].tool_use_id.as_deref(), Some("tu_01"));
    assert!(msgs[0].content.contains("total 48"));
}

#[test]
fn parse_system() {
    let msgs = parse_line(system_fixture(), "sess1", 5).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].message_type, MessageType::System);
    assert_eq!(msgs[0].content, "You are a helpful assistant.");
}

#[test]
fn parse_mixed_assistant_splits_into_two() {
    let msgs = parse_line(mixed_assistant_fixture(), "sess1", 6).unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].message_type, MessageType::Assistant);
    assert_eq!(msgs[0].content, "Let me check that.");
    assert_eq!(msgs[1].message_type, MessageType::ToolCall);
    assert_eq!(msgs[1].tool_name.as_deref(), Some("bash"));
}

#[test]
fn is_sidechain_detects_flag() {
    assert!(is_sidechain(sidechain_fixture()));
    assert!(!is_sidechain(user_text_fixture()));
}

#[test]
fn no_message_field_returns_empty() {
    let msgs = parse_line(no_message_fixture(), "sess1", 7).unwrap();
    assert_eq!(msgs.len(), 0);
}
