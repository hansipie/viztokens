use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;

use viztokens::model::{Message, MessageType};
use viztokens::store::Store;
use viztokens::tui::App;

fn make_app_with_all_types() -> App {
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let (_, rx) = mpsc::channel(1);
    let messages = vec![
        make_msg(1, MessageType::System, "sys"),
        make_msg(2, MessageType::User, "user msg"),
        make_msg(3, MessageType::Assistant, "asst msg"),
        make_msg(4, MessageType::ToolCall, "tool call"),
        make_msg(5, MessageType::ToolResult, "tool result"),
    ];
    App::new(rx, store, vec![], messages)
}

fn make_msg(seq: u64, mt: MessageType, content: &str) -> Message {
    Message {
        id: None,
        session_id: "test".to_string(),
        sequence_num: seq,
        message_type: mt,
        timestamp: chrono::Utc::now(),
        content: content.to_string(),
        tool_name: None,
        tool_use_id: None,
        anthropic_msg_id: None,
        request_id: None,
        input_tokens: None,
        output_tokens: None,
        model: None,
    }
}

#[test]
fn all_five_visible_with_no_filter() {
    let app = make_app_with_all_types();
    assert_eq!(app.visible_messages().len(), 5);
}

#[test]
fn cycle_filter_steps_through_all_presets() {
    let mut app = make_app_with_all_types();

    // All → User
    app.cycle_filter();
    let visible = app.visible_messages();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].message_type, MessageType::User);

    // User → Assistant
    app.cycle_filter();
    let visible = app.visible_messages();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].message_type, MessageType::Assistant);

    // Assistant → Tools (ToolCall + ToolResult)
    app.cycle_filter();
    let visible = app.visible_messages();
    assert_eq!(visible.len(), 2);
    assert!(visible
        .iter()
        .any(|m| m.message_type == MessageType::ToolCall));
    assert!(visible
        .iter()
        .any(|m| m.message_type == MessageType::ToolResult));

    // Tools → System
    app.cycle_filter();
    let visible = app.visible_messages();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].message_type, MessageType::System);

    // System → All (clear)
    app.cycle_filter();
    assert_eq!(app.visible_messages().len(), 5);
}

#[test]
fn clear_filter_restores_all() {
    let mut app = make_app_with_all_types();
    app.cycle_filter(); // → User only
    assert_eq!(app.visible_messages().len(), 1);
    app.filter.clear();
    assert_eq!(app.visible_messages().len(), 5);
}

#[test]
fn toggle_user_hides_it() {
    let mut app = make_app_with_all_types();
    app.filter.toggle(MessageType::User);
    // filter is now active with only User allowed
    let visible = app.visible_messages();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].message_type, MessageType::User);

    // Toggle User again to remove it from allowed
    app.filter.toggle(MessageType::User);
    // filter is now empty (no allowed types) → shows all
    assert_eq!(app.visible_messages().len(), 5);
}

#[test]
fn filter_allows_correct_types() {
    let mut filter = viztokens::model::MessageFilter::default();
    assert!(!filter.is_active());
    assert!(filter.allows(&MessageType::User));

    filter.toggle(MessageType::User);
    assert!(filter.is_active());
    assert!(filter.allows(&MessageType::User));
    assert!(!filter.allows(&MessageType::Assistant));
    assert!(!filter.allows(&MessageType::ToolCall));

    filter.toggle(MessageType::Assistant);
    assert!(filter.allows(&MessageType::User));
    assert!(filter.allows(&MessageType::Assistant));
    assert!(!filter.allows(&MessageType::System));

    filter.clear();
    assert!(!filter.is_active());
    for mt in [
        MessageType::User,
        MessageType::Assistant,
        MessageType::ToolCall,
        MessageType::ToolResult,
        MessageType::System,
    ] {
        assert!(filter.allows(&mt));
    }
}

#[test]
fn messages_not_lost_when_filter_active() {
    let mut app = make_app_with_all_types();
    app.cycle_filter(); // → User only
    assert_eq!(app.visible_messages().len(), 1);
    // All messages still in app.messages
    assert_eq!(app.messages.len(), 5);
    // After clear, all reappear
    app.filter.clear();
    assert_eq!(app.visible_messages().len(), 5);
}

#[test]
fn filter_label_returns_correct_strings() {
    let mut filter = viztokens::model::MessageFilter::default();
    assert_eq!(filter.label(), "ALL");

    filter.toggle(MessageType::User);
    assert_eq!(filter.label(), "USER");

    filter.toggle(MessageType::User);
    filter.toggle(MessageType::Assistant);
    assert_eq!(filter.label(), "ASSISTANT");

    filter.toggle(MessageType::Assistant);
    filter.toggle(MessageType::ToolCall);
    filter.toggle(MessageType::ToolResult);
    assert_eq!(filter.label(), "TOOLS");

    filter.toggle(MessageType::User); // now User + ToolCall + ToolResult
    assert_eq!(filter.label(), "CUSTOM");
}
