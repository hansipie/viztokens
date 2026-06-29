use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use crate::model::Message;
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    pub project_name: String,
    pub harness: String,
    pub file_path: PathBuf,
    pub last_modified: SystemTime,
}

pub enum WatcherEvent {
    Message(Message),
    ParseError(String),
    SessionEnded(String),
    NewSession(DiscoveredSession),
    /// Session-level token totals (used by adapters that don't have per-message counts).
    TokensUpdate {
        session_id: String,
        input_tokens: u64,
        output_tokens: u64,
    },
}

pub struct Watcher {
    pub tx: tokio::sync::mpsc::Sender<WatcherEvent>,
    pub store: Arc<Store>,
}
