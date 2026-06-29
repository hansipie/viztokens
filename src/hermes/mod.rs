use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use chrono::DateTime;
use notify::{EventKind, RecursiveMode, Watcher as NotifyWatcher};
use rusqlite::{Connection, OpenFlags, params};

use crate::model::{Message, MessageType, Session, SessionStatus};
use crate::store::Store;
use crate::watcher::DiscoveredSession;
use crate::watcher::{Watcher, WatcherEvent};

pub fn default_db_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".hermes").join("state.db"))
}

pub fn discover_sessions(db_path: &Path) -> anyhow::Result<Vec<DiscoveredSession>> {
    let conn = open_readonly(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, cwd, git_repo_root, started_at
         FROM sessions
         WHERE model IS NOT NULL AND TRIM(model) != '' AND archived = 0
         ORDER BY started_at DESC",
    )?;

    let rows: Vec<(String, Option<String>, Option<String>, f64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    Ok(rows
        .into_iter()
        .map(|(id, cwd, git_repo_root, started_at)| {
            let project_name = git_repo_root
                .as_deref()
                .or(cwd.as_deref())
                .and_then(|p| Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("hermes")
                .to_string();

            let secs = started_at.trunc() as u64;
            let last_modified = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);

            DiscoveredSession {
                session_id: id,
                project_name,
                harness: "hermes".to_string(),
                file_path: db_path.to_path_buf(),
                last_modified,
            }
        })
        .collect())
}

pub async fn run(watcher: Watcher, session: DiscoveredSession, db_path: PathBuf) {
    if let Err(e) = run_inner(watcher, session, db_path).await {
        tracing::error!("hermes watcher error: {e:#}");
    }
}

async fn run_inner(
    watcher: Watcher,
    session: DiscoveredSession,
    db_path: PathBuf,
) -> anyhow::Result<()> {
    let session_id = session.session_id.clone();

    // Resume from the last message we already stored
    let mut last_id: u64 = {
        let store = watcher.store.clone();
        let sid = session_id.clone();
        tokio::task::spawn_blocking(move || store.max_sequence_num(&sid)).await??
    };

    // Fetch model once from the Hermes sessions table
    let model: Option<String> = {
        let conn = open_readonly(&db_path)?;
        conn.query_row(
            "SELECT model FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .ok()
    };

    // Set up the file watcher BEFORE the initial load so no writes are missed
    // during the load phase.
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<()>(16);
    let watched_db = db_path.clone();
    let mut watcher_handle =
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let relevant = event.paths.iter().any(|p| {
                    p == &watched_db
                        || p.file_name()
                            .map(|n| n == "state.db-wal" || n == "state.db")
                            .unwrap_or(false)
                });
                if relevant
                    && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
                {
                    let _ = notify_tx.blocking_send(());
                }
            }
        })?;
    let watch_dir = db_path.parent().context("hermes db has no parent directory")?;
    watcher_handle.watch(watch_dir, RecursiveMode::NonRecursive)?;

    // Load and emit existing messages (all if first run, only new ones if resuming).
    let msgs = load_new_messages(&db_path, &session_id, last_id, model.as_deref())?;
    for msg in msgs {
        last_id = msg.sequence_num;
        let store = watcher.store.clone();
        let msg_clone = msg.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = store.insert_message(&msg_clone) {
                tracing::warn!("hermes: persist error: {e:#}");
            }
        });
        if watcher.tx.send(WatcherEvent::Message(msg)).await.is_err() {
            return Ok(());
        }
    }
    // Emit session-level token totals after the initial load.
    send_tokens_update(&watcher, &db_path, &session_id).await;

    // Poll every second for new messages, and also immediately on any notify
    // event. Polling on every tick (not only on events) is intentional: WAL-mode
    // SQLite produces a mix of state.db-wal writes, SHM updates, and periodic
    // checkpoints — some of these may not fire an inotify MODIFY and would be
    // silently missed if we relied on notifications alone.
    loop {
        match tokio::time::timeout(Duration::from_secs(1), notify_rx.recv()).await {
            Ok(None) => break, // notify sender dropped — watcher handle gone
            _ => {}            // timeout or notification — poll either way
        }
        // Drain any extra queued notifications before querying
        while notify_rx.try_recv().is_ok() {}

        let msgs =
            match load_new_messages(&db_path, &session_id, last_id, model.as_deref()) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("hermes: query failed: {e:#}");
                    continue;
                }
            };

        let had_new = !msgs.is_empty();
        for msg in msgs {
            last_id = msg.sequence_num;
            let store = watcher.store.clone();
            let msg_clone = msg.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = store.insert_message(&msg_clone) {
                    tracing::warn!("hermes: persist error: {e:#}");
                }
            });
            if watcher.tx.send(WatcherEvent::Message(msg)).await.is_err() {
                return Ok(());
            }
        }
        // Refresh session-level token totals whenever new messages arrived.
        if had_new {
            send_tokens_update(&watcher, &db_path, &session_id).await;
        }
    }

    let store = watcher.store.clone();
    let sid = session_id.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = store.set_session_status(&sid, SessionStatus::Stale) {
            tracing::warn!("hermes: failed to mark {sid} stale: {e:#}");
        }
    });
    let _ = watcher
        .tx
        .send(WatcherEvent::SessionEnded(session_id))
        .await;

    Ok(())
}

pub async fn watch_new_sessions(
    db_path: PathBuf,
    tx: tokio::sync::mpsc::Sender<WatcherEvent>,
    store: Arc<Store>,
    max_age: u64,
    initial_ids: HashSet<String>,
) {
    if let Err(e) = watch_new_sessions_inner(db_path, tx, store, max_age, initial_ids).await {
        tracing::error!("hermes session watcher error: {e:#}");
    }
}

async fn watch_new_sessions_inner(
    db_path: PathBuf,
    tx: tokio::sync::mpsc::Sender<WatcherEvent>,
    store: Arc<Store>,
    max_age: u64,
    mut known_ids: HashSet<String>,
) -> anyhow::Result<()> {
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<()>(16);
    let watched_db = db_path.clone();
    let mut watcher_handle =
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let relevant = event.paths.iter().any(|p| {
                    p == &watched_db
                        || p.file_name()
                            .map(|n| n == "state.db-wal" || n == "state.db")
                            .unwrap_or(false)
                });
                if relevant && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    let _ = notify_tx.blocking_send(());
                }
            }
        })?;
    let watch_dir = db_path.parent().context("hermes db has no parent directory")?;
    watcher_handle.watch(watch_dir, RecursiveMode::NonRecursive)?;

    loop {
        match tokio::time::timeout(Duration::from_secs(5), notify_rx.recv()).await {
            Ok(None) => break,
            _ => {}
        }
        while notify_rx.try_recv().is_ok() {}

        let sessions = match discover_sessions(&db_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("hermes: session scan failed: {e:#}");
                continue;
            }
        };

        for ds in sessions {
            if known_ids.contains(&ds.session_id) {
                continue;
            }
            if max_age > 0 {
                let too_old = ds
                    .last_modified
                    .elapsed()
                    .map(|age| age > Duration::from_secs(max_age * 60))
                    .unwrap_or(false);
                if too_old {
                    known_ids.insert(ds.session_id.clone());
                    continue;
                }
            }

            known_ids.insert(ds.session_id.clone());
            tracing::info!("hermes: new session detected: {}", ds.session_id);

            let session = Session {
                id: ds.session_id.clone(),
                project_name: ds.project_name.clone(),
                file_path: ds.file_path.clone(),
                first_seen_at: chrono::Utc::now(),
                last_seen_at: chrono::Utc::now(),
                status: SessionStatus::Watching,
                message_count: 0,
            };
            let store2 = store.clone();
            let sid = ds.session_id.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = store2.insert_session(&session) {
                    tracing::warn!("hermes: store insert_session: {e:#}");
                }
                if let Err(e) = store2.set_session_status(&sid, SessionStatus::Watching) {
                    tracing::warn!("hermes: store set_session_status: {e:#}");
                }
            });

            if tx.send(WatcherEvent::NewSession(ds.clone())).await.is_err() {
                return Ok(());
            }

            tokio::spawn(run(
                Watcher {
                    tx: tx.clone(),
                    store: store.clone(),
                },
                ds,
                db_path.clone(),
            ));
        }
    }

    Ok(())
}

async fn send_tokens_update(watcher: &Watcher, db_path: &Path, session_id: &str) {
    if let Ok(conn) = open_readonly(db_path) {
        let result: rusqlite::Result<(i64, i64)> = conn.query_row(
            "SELECT input_tokens, output_tokens FROM sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );
        if let Ok((input, output)) = result {
            let _ = watcher
                .tx
                .send(WatcherEvent::TokensUpdate {
                    session_id: session_id.to_string(),
                    input_tokens: input as u64,
                    output_tokens: output as u64,
                })
                .await;
        }
    }
}

/// Returns messages with Hermes message `id` as `sequence_num` for cursor tracking.
/// Each row produces exactly one Message (tool_calls array collapsed to one ToolCall entry).
fn load_new_messages(
    db_path: &Path,
    session_id: &str,
    after_id: u64,
    model: Option<&str>,
) -> anyhow::Result<Vec<Message>> {
    let conn = open_readonly(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, role, content, tool_call_id, tool_calls, tool_name, timestamp, token_count
         FROM messages
         WHERE session_id = ?1 AND id > ?2 AND active = 1
         ORDER BY id ASC",
    )?;

    let rows: Vec<(
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        f64,
        Option<i64>,
    )> = stmt
        .query_map(params![session_id, after_id as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, Option<i64>>(7)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    let mut messages = Vec::new();
    for (id, role, content, tool_call_id, tool_calls_json, tool_name, timestamp, token_count) in
        rows
    {
        let ts = DateTime::from_timestamp(timestamp as i64, (timestamp.fract() * 1e9) as u32)
            .unwrap_or_else(chrono::Utc::now);
        let seq = id as u64;

        let msg = match role.as_str() {
            "user" => {
                let text = content.unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }
                Message {
                    id: None,
                    session_id: session_id.to_string(),
                    sequence_num: seq,
                    message_type: MessageType::User,
                    timestamp: ts,
                    content: text,
                    tool_name: None,
                    tool_use_id: None,
                    anthropic_msg_id: None,
                    request_id: None,
                    input_tokens: token_count.map(|t| t as u32),
                    output_tokens: None,
                    tokens_estimated: false,
                    model: model.map(str::to_string),
                }
            }
            "assistant" if tool_calls_json.is_some() => {
                let calls_json = tool_calls_json.unwrap();
                let calls: Vec<serde_json::Value> =
                    serde_json::from_str(&calls_json).unwrap_or_default();
                let first_name = calls
                    .first()
                    .and_then(|c| c["function"]["name"].as_str())
                    .map(str::to_string);
                let first_id = calls
                    .first()
                    .and_then(|c| c["call_id"].as_str().or(c["id"].as_str()))
                    .map(str::to_string);
                let content_str = if calls.len() == 1 {
                    calls[0]["function"]["arguments"]
                        .as_str()
                        .map(|s| {
                            serde_json::from_str::<serde_json::Value>(s)
                                .ok()
                                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                                .unwrap_or_else(|| s.to_string())
                        })
                        .unwrap_or_default()
                } else {
                    serde_json::to_string_pretty(&calls).unwrap_or(calls_json)
                };
                Message {
                    id: None,
                    session_id: session_id.to_string(),
                    sequence_num: seq,
                    message_type: MessageType::ToolCall,
                    timestamp: ts,
                    content: content_str,
                    tool_name: first_name,
                    tool_use_id: first_id,
                    anthropic_msg_id: None,
                    request_id: None,
                    input_tokens: None,
                    output_tokens: None,
                    tokens_estimated: false,
                    model: model.map(str::to_string),
                }
            }
            "assistant" => {
                let text = content.unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }
                Message {
                    id: None,
                    session_id: session_id.to_string(),
                    sequence_num: seq,
                    message_type: MessageType::Assistant,
                    timestamp: ts,
                    content: text,
                    tool_name: None,
                    tool_use_id: None,
                    anthropic_msg_id: None,
                    request_id: None,
                    input_tokens: None,
                    output_tokens: token_count.map(|t| t as u32),
                    tokens_estimated: false,
                    model: model.map(str::to_string),
                }
            }
            "tool" => {
                let text = content.unwrap_or_default();
                Message {
                    id: None,
                    session_id: session_id.to_string(),
                    sequence_num: seq,
                    message_type: MessageType::ToolResult,
                    timestamp: ts,
                    content: text,
                    tool_name,
                    tool_use_id: tool_call_id,
                    anthropic_msg_id: None,
                    request_id: None,
                    input_tokens: None,
                    output_tokens: None,
                    tokens_estimated: false,
                    model: model.map(str::to_string),
                }
            }
            _ => continue,
        };

        messages.push(msg);
    }

    Ok(messages)
}

fn open_readonly(path: &Path) -> anyhow::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening Hermes DB at {}", path.display()))
}
