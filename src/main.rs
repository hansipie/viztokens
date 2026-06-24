use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use viztokens::cli::{Args, Command};
use viztokens::model::{Session, SessionStatus};
use viztokens::store::Store;
use viztokens::tui::{init_terminal, run, App};
use viztokens::watcher::session::{resolve_config_dir, scan_sessions};
use viztokens::watcher::{run as watcher_run, Watcher};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    // Resolve DB path
    let db_path = match args.db {
        Some(ref p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            let data_dir =
                dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("~/.local/share"));
            data_dir.join("viztokens").join("sessions.db")
        }
    };

    let store = Arc::new(Store::open(&db_path)?);

    // Handle subcommands that don't need a store
    if let Some(Command::Update) = args.command {
        let status = std::process::Command::new("cargo")
            .args([
                "install",
                "--git",
                "https://github.com/hansipie/viztokens",
                "--force",
            ])
            .status()?;
        std::process::exit(status.code().unwrap_or(1));
    }

    // Handle list-sessions subcommand
    if let Some(Command::ListSessions) = args.command {
        let sessions = store.list_sessions()?;
        for s in sessions {
            let obj = serde_json::json!({
                "id": s.id,
                "project": s.project_name,
                "file": s.file_path.to_string_lossy(),
                "first_seen": s.first_seen_at.to_rfc3339(),
                "last_seen": s.last_seen_at.to_rfc3339(),
                "status": format!("{:?}", s.status).to_lowercase(),
                "message_count": s.message_count,
            });
            println!("{}", serde_json::to_string(&obj)?);
        }
        return Ok(());
    }

    // Discover sessions
    let config_dir = resolve_config_dir(args.config_dir)?;
    let mut discovered = scan_sessions(&config_dir);

    // Filter by age unless --max-age 0
    if args.max_age > 0 {
        let cutoff = std::time::Duration::from_secs(args.max_age * 60);
        discovered.retain(|s| {
            s.last_modified
                .elapsed()
                .map(|age| age <= cutoff)
                .unwrap_or(false)
        });
    }

    // Filter by project if requested
    if let Some(ref project) = args.project {
        discovered.retain(|s| &s.project_name == project);
    }

    if discovered.is_empty() {
        anyhow::bail!("no Claude Code session files found in {}", config_dir.display());
    }

    // Register all sessions and spawn one watcher per session
    let (tx, rx) = tokio::sync::mpsc::channel(256);

    for ds in &discovered {
        let session = Session {
            id: ds.session_id.clone(),
            project_name: ds.project_name.clone(),
            file_path: ds.file_path.clone(),
            first_seen_at: chrono::Utc::now(),
            last_seen_at: chrono::Utc::now(),
            status: SessionStatus::Watching,
            message_count: 0,
        };
        store.insert_session(&session)?;
        let watcher = Watcher {
            tx: tx.clone(),
            store: store.clone(),
        };
        tokio::spawn(watcher_run(watcher, ds.clone()));
    }

    // Load history from all discovered sessions, sorted by timestamp
    let mut history = Vec::new();
    for ds in &discovered {
        history.extend(store.query_messages(&ds.session_id)?);
    }
    history.sort_by_key(|m: &viztokens::model::Message| m.timestamp);

    // Start TUI
    let app = App::new(rx, store, discovered, history);
    let terminal = init_terminal()?;
    run(app, terminal)?;

    Ok(())
}
