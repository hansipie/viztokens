use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use tokio::sync::mpsc;

use viztokens::model::MessageType;
use viztokens::store::Store;
use viztokens::watcher::session::DiscoveredSession;
use viztokens::watcher::{Watcher, WatcherEvent};

fn make_store(dir: &TempDir) -> Arc<Store> {
    let db_path = dir.path().join("test.db");
    Arc::new(Store::open(&db_path).unwrap())
}

fn discovered_session(dir: &TempDir, file_name: &str) -> DiscoveredSession {
    DiscoveredSession {
        session_id: "sess-test".to_string(),
        project_name: "test-project".to_string(),
        file_path: dir.path().join(file_name),
        last_modified: std::time::SystemTime::now(),
    }
}

fn user_line() -> &'static str {
    "{\"sessionId\":\"sess-test\",\"timestamp\":\"2026-06-23T12:00:00Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"Hello\"}]}}\n"
}

fn tool_call_line() -> &'static str {
    "{\"sessionId\":\"sess-test\",\"timestamp\":\"2026-06-23T12:00:01Z\",\"requestId\":\"req1\",\"message\":{\"id\":\"msg_01\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"tu_01\",\"name\":\"bash\",\"input\":{\"command\":\"ls\"}}],\"model\":\"claude-sonnet-4-6\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n"
}

fn tool_result_line() -> &'static str {
    "{\"sessionId\":\"sess-test\",\"timestamp\":\"2026-06-23T12:00:02Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"tu_01\",\"content\":\"file.txt\"}]}}\n"
}

fn assistant_line() -> &'static str {
    "{\"sessionId\":\"sess-test\",\"timestamp\":\"2026-06-23T12:00:03Z\",\"message\":{\"id\":\"msg_02\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}],\"model\":\"claude-sonnet-4-6\",\"usage\":{\"input_tokens\":15,\"output_tokens\":3}}}\n"
}

#[tokio::test]
async fn watcher_receives_four_message_types() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("sess-test.jsonl");

    // Create empty file first
    std::fs::File::create(&file_path).unwrap();

    let store = make_store(&dir);
    let session = discovered_session(&dir, "sess-test.jsonl");

    let (tx, mut rx) = mpsc::channel(256);
    let w = Watcher { tx, store };

    tokio::spawn(viztokens::watcher::run(w, session));

    // Give watcher time to start and seek to end
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Append 4 fixture lines
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&file_path)
        .unwrap();
    file.write_all(user_line().as_bytes()).unwrap();
    file.write_all(tool_call_line().as_bytes()).unwrap();
    file.write_all(tool_result_line().as_bytes()).unwrap();
    file.write_all(assistant_line().as_bytes()).unwrap();
    file.flush().unwrap();
    drop(file);

    // Collect messages with 2s timeout
    let mut received = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(WatcherEvent::Message(msg))) => {
                received.push(msg);
                if received.len() == 4 {
                    break;
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }

    assert_eq!(
        received.len(),
        4,
        "expected 4 messages, got {}",
        received.len()
    );
    assert_eq!(received[0].message_type, MessageType::User);
    assert_eq!(received[0].content, "Hello");
    assert_eq!(received[1].message_type, MessageType::ToolCall);
    assert_eq!(received[1].tool_name.as_deref(), Some("bash"));
    assert_eq!(received[2].message_type, MessageType::ToolResult);
    assert_eq!(received[3].message_type, MessageType::Assistant);
    assert_eq!(received[3].content, "Done.");
}

#[tokio::test]
async fn scroll_mode_messages_still_arrive() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("sess-scroll.jsonl");
    std::fs::File::create(&file_path).unwrap();

    let store = make_store(&dir);
    let session = DiscoveredSession {
        session_id: "sess-scroll".to_string(),
        project_name: "test-project".to_string(),
        file_path: file_path.clone(),
        last_modified: std::time::SystemTime::now(),
    };

    let (tx, mut rx) = mpsc::channel(512);
    let w = Watcher { tx, store };

    tokio::spawn(viztokens::watcher::run(w, session));

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Write 25 lines
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&file_path)
        .unwrap();
    for i in 0..25 {
        let line = format!(
            "{{\"sessionId\":\"sess-scroll\",\"timestamp\":\"2026-06-23T12:00:{:02}Z\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"msg {}\"}}]}}}}\n",
            i % 60, i
        );
        file.write_all(line.as_bytes()).unwrap();
    }
    file.flush().unwrap();

    // Collect 25 messages
    let mut count = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(WatcherEvent::Message(_))) => {
                count += 1;
                if count == 25 {
                    break;
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    assert_eq!(count, 25);

    // Write 5 more while "scrolled" (scroll_offset unchanged by watcher itself)
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&file_path)
        .unwrap();
    for i in 25..30 {
        let line = format!(
            "{{\"sessionId\":\"sess-scroll\",\"timestamp\":\"2026-06-23T12:01:{:02}Z\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"msg {}\"}}]}}}}\n",
            i % 60, i
        );
        file.write_all(line.as_bytes()).unwrap();
    }
    file.flush().unwrap();

    // Verify 5 more arrive
    let mut extra = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(WatcherEvent::Message(_))) => {
                extra += 1;
                if extra == 5 {
                    break;
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    assert_eq!(extra, 5);
}
