use std::path::Path;

use viztokens::model::{Message, MessageType, Session, SessionStatus};
use viztokens::store::Store;
use viztokens::claude::session::resolve_config_dir;

fn open_memory_store() -> Store {
    Store::open(Path::new(":memory:")).unwrap()
}

fn make_session(id: &str) -> Session {
    Session {
        id: id.to_string(),
        project_name: "test-project".to_string(),
        file_path: std::path::PathBuf::from("/tmp/test.jsonl"),
        first_seen_at: chrono::TimeZone::timestamp_opt(&chrono::Utc, 0, 0).unwrap(),
        last_seen_at: chrono::TimeZone::timestamp_opt(&chrono::Utc, 0, 0).unwrap(),
        status: SessionStatus::Watching,
        message_count: 0,
    }
}

fn make_message(session_id: &str, seq: u64, mt: MessageType) -> Message {
    Message {
        id: None,
        session_id: session_id.to_string(),
        sequence_num: seq,
        message_type: mt,
        timestamp: chrono::TimeZone::timestamp_opt(&chrono::Utc, seq as i64, 0).unwrap(),
        content: format!("content {}", seq),
        tool_name: None,
        tool_use_id: None,
        anthropic_msg_id: Some(format!("msg_{}", seq)),
        request_id: Some(format!("req_{}", seq)),
        input_tokens: Some(10),
        output_tokens: Some(5),
        tokens_estimated: false,
        model: Some("claude-sonnet-4-6".to_string()),
    }
}

#[test]
fn insert_and_query_50_messages_in_order() {
    let store = open_memory_store();
    let session = make_session("sess-50");
    store.insert_session(&session).unwrap();

    for i in 1..=50 {
        let mt = match i % 5 {
            0 => MessageType::System,
            1 => MessageType::User,
            2 => MessageType::Assistant,
            3 => MessageType::ToolCall,
            _ => MessageType::ToolResult,
        };
        let msg = make_message("sess-50", i, mt);
        store.insert_message(&msg).unwrap();
    }

    let msgs = store.query_messages("sess-50").unwrap();
    assert_eq!(msgs.len(), 50);

    // Verify ascending sequence_num order
    for i in 0..msgs.len() - 1 {
        assert!(msgs[i].sequence_num < msgs[i + 1].sequence_num);
    }
    assert_eq!(msgs[0].sequence_num, 1);
    assert_eq!(msgs[49].sequence_num, 50);
}

#[test]
fn insert_multiple_messages_succeeds() {
    let store = open_memory_store();
    let session = make_session("sess-ids");
    store.insert_session(&session).unwrap();

    store
        .insert_message(&make_message("sess-ids", 1, MessageType::User))
        .unwrap();
    store
        .insert_message(&make_message("sess-ids", 2, MessageType::Assistant))
        .unwrap();
    store
        .insert_message(&make_message("sess-ids", 3, MessageType::ToolCall))
        .unwrap();

    let msgs = store.query_messages("sess-ids").unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].sequence_num, 1);
    assert_eq!(msgs[1].sequence_num, 2);
    assert_eq!(msgs[2].sequence_num, 3);
}

#[test]
fn message_exists_detects_duplicates() {
    let store = open_memory_store();
    let session = make_session("sess-dedup");
    store.insert_session(&session).unwrap();

    let msg = make_message("sess-dedup", 1, MessageType::User);
    store.insert_message(&msg).unwrap();

    // Duplicate with same session + anthropic_msg_id + request_id
    assert!(store
        .message_exists("sess-dedup", Some("msg_1"), Some("req_1"))
        .unwrap());
    // Different id
    assert!(!store
        .message_exists("sess-dedup", Some("msg_999"), Some("req_999"))
        .unwrap());
    // Same ids but different session → not a duplicate
    assert!(!store
        .message_exists("other-sess", Some("msg_1"), Some("req_1"))
        .unwrap());
    // None IDs
    assert!(!store.message_exists("sess-dedup", None, None).unwrap());
}

#[test]
fn resolve_config_dir_uses_env_var() {
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_MUTEX.lock().unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap().to_string();
    std::env::set_var("CLAUDE_CONFIG_DIR", &path);
    let resolved = resolve_config_dir(None).unwrap();
    std::env::remove_var("CLAUDE_CONFIG_DIR");
    assert_eq!(resolved, dir.path());
}
