pub mod parser;
pub mod session;

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use notify::{EventKind, RecursiveMode, Watcher as NotifyWatcher};
use tokio::sync::mpsc;

use crate::model::Message;
use crate::store::Store;
use session::DiscoveredSession;

pub enum WatcherEvent {
    Message(Message),
    ParseError(String),
    SessionEnded(String),
}

pub struct Watcher {
    pub tx: mpsc::Sender<WatcherEvent>,
    pub store: Arc<Store>,
}

pub async fn run(watcher: Watcher, session: DiscoveredSession) {
    if let Err(e) = run_inner(watcher, session).await {
        tracing::error!("watcher error: {e:#}");
    }
}

async fn run_inner(watcher: Watcher, session: DiscoveredSession) -> anyhow::Result<()> {
    let path = session.file_path.clone();
    let session_id = session.session_id.clone();

    let mut file =
        std::fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;

    // If SQLite already has messages for this session, tail from the end.
    // On first run (empty DB), read from the beginning to replay the full JSONL file.
    let has_history = {
        let store = watcher.store.clone();
        let sid = session_id.clone();
        tokio::task::spawn_blocking(move || store.count_messages(&sid))
            .await??
    };
    let mut offset = if has_history > 0 {
        file.seek(SeekFrom::End(0))?
    } else {
        0u64
    };

    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<()>(16);

    let watch_dir = path
        .parent()
        .context("session file has no parent dir")?
        .to_path_buf();

    let mut watcher_handle =
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_)) {
                    let _ = notify_tx.blocking_send(());
                }
            }
        })?;
    watcher_handle.watch(&watch_dir, RecursiveMode::NonRecursive)?;

    let mut sequence_num: u64 = 1;
    let mut buf = String::new();
    let mut partial = String::new();

    loop {
        match tokio::time::timeout(Duration::from_millis(100), notify_rx.recv()).await {
            Err(_) => continue,    // no event yet
            Ok(None) => break,     // notify sender dropped
            Ok(Some(())) => {}     // file modified, read below
        }

        // Read newly appended bytes
        file.seek(SeekFrom::Start(offset))?;
        buf.clear();
        file.read_to_string(&mut buf).unwrap_or_else(|_| {
            // Non-UTF-8: hex-escape and continue
            let mut raw = Vec::new();
            let _ = file.read_to_end(&mut raw);
            buf = raw.iter().map(|b| format!("\\x{b:02x}")).collect();
            buf.len()
        });

        if !buf.is_empty() {
            offset += buf.len() as u64;
            partial.push_str(&buf);

            // Split on newlines, keeping incomplete trailing line in partial
            let ends_with_newline = partial.ends_with('\n');
            let mut lines: Vec<String> = partial.split('\n').map(str::to_owned).collect();
            partial = if ends_with_newline {
                String::new()
            } else {
                lines.pop().unwrap_or_default()
            };

            for line in &lines {
                let line = line.as_str();
                if line.is_empty() {
                    continue;
                }

                let sidechain = parser::is_sidechain(line);

                let msgs = match parser::parse_line(line, &session_id, sequence_num) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("parse error: {e} — line: {}", &line[..line.len().min(200)]);
                        let _ = watcher
                            .tx
                            .send(WatcherEvent::ParseError(format!("{e}")))
                            .await;
                        continue;
                    }
                };

                for msg in msgs {
                    // Dedup sidechain entries
                    if sidechain {
                        let store = watcher.store.clone();
                        let mid = msg.anthropic_msg_id.clone();
                        let rid = msg.request_id.clone();
                        let exists = tokio::task::spawn_blocking(move || {
                            store.message_exists(mid.as_deref(), rid.as_deref())
                        })
                        .await
                        .unwrap_or(Ok(false))
                        .unwrap_or(false);
                        if exists {
                            continue;
                        }
                    }

                    sequence_num += 1;

                    let store = watcher.store.clone();
                    let msg_clone = msg.clone();
                    tokio::task::spawn_blocking(move || {
                        let _ = store.insert_message(&msg_clone);
                    });

                    if watcher.tx.send(WatcherEvent::Message(msg)).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
    Ok(())
}
