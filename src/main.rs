use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use std::io::Write;

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

    let store = Arc::new(Store::open(&db_path)?);

    // Reconcile: sessions marked watching in a previous run whose file is gone → stale
    if let Ok(n) = store.mark_stale_if_missing() {
        if n > 0 {
            tracing::info!("{n} session(s) marked stale (file no longer present)");
        }
    }

    // Handle prune-stale subcommand
    if let Some(Command::PruneStale) = args.command {
        let stale = store.list_stale_sessions()?;
        if stale.is_empty() {
            println!("No stale sessions found.");
            return Ok(());
        }

        // Compute column widths
        let w_id = stale.iter().map(|s| s.id.len()).max().unwrap_or(0).max(2);
        let w_proj = stale
            .iter()
            .map(|s| s.project_name.len())
            .max()
            .unwrap_or(0)
            .max(7);
        let w_last = 19; // "YYYY-MM-DD HH:MM:SS"
        let w_msgs = stale
            .iter()
            .map(|s| s.message_count.to_string().len())
            .max()
            .unwrap_or(0)
            .max(4);

        let sep = format!(
            " {0}  {1}  {2}  {3}",
            "─".repeat(w_id),
            "─".repeat(w_proj),
            "─".repeat(w_last),
            "─".repeat(w_msgs),
        );

        println!(
            " {:<w_id$}  {:<w_proj$}  {:<w_last$}  {:>w_msgs$}",
            "ID",
            "Project",
            "Last seen",
            "Msgs",
            w_id = w_id,
            w_proj = w_proj,
            w_last = w_last,
            w_msgs = w_msgs,
        );
        println!("{sep}");
        for s in &stale {
            println!(
                " {:<w_id$}  {:<w_proj$}  {:<w_last$}  {:>w_msgs$}",
                s.id,
                s.project_name,
                s.last_seen_at.format("%Y-%m-%d %H:%M:%S"),
                s.message_count,
                w_id = w_id,
                w_proj = w_proj,
                w_last = w_last,
                w_msgs = w_msgs,
            );
        }
        println!("{sep}");
        println!(" {} session(s) will be deleted.", stale.len());

        print!("\nDelete? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("y") {
            let n = store.delete_stale_sessions()?;
            println!("{n} session(s) deleted.");
        } else {
            println!("Aborted.");
        }
        return Ok(());
    }

    // Handle clear subcommand
    if let Some(Command::Clear) = args.command {
        let sessions = store.list_sessions()?;
        if sessions.is_empty() {
            println!("Database is already empty.");
            return Ok(());
        }

        let w_id = sessions
            .iter()
            .map(|s| s.id.len())
            .max()
            .unwrap_or(0)
            .max(2);
        let w_proj = sessions
            .iter()
            .map(|s| s.project_name.len())
            .max()
            .unwrap_or(0)
            .max(7);
        let w_last = 19;
        let w_status = 8;
        let w_msgs = sessions
            .iter()
            .map(|s| s.message_count.to_string().len())
            .max()
            .unwrap_or(0)
            .max(4);

        let sep = format!(
            " {0}  {1}  {2}  {3}  {4}",
            "─".repeat(w_id),
            "─".repeat(w_proj),
            "─".repeat(w_last),
            "─".repeat(w_status),
            "─".repeat(w_msgs),
        );
        println!(
            " {:<w_id$}  {:<w_proj$}  {:<w_last$}  {:<w_status$}  {:>w_msgs$}",
            "ID",
            "Project",
            "Last seen",
            "Status",
            "Msgs",
            w_id = w_id,
            w_proj = w_proj,
            w_last = w_last,
            w_status = w_status,
            w_msgs = w_msgs,
        );
        println!("{sep}");
        for s in &sessions {
            println!(
                " {:<w_id$}  {:<w_proj$}  {:<w_last$}  {:<w_status$}  {:>w_msgs$}",
                s.id,
                s.project_name,
                s.last_seen_at.format("%Y-%m-%d %H:%M:%S"),
                format!("{:?}", s.status).to_lowercase(),
                s.message_count,
                w_id = w_id,
                w_proj = w_proj,
                w_last = w_last,
                w_status = w_status,
                w_msgs = w_msgs,
            );
        }
        println!("{sep}");

        let msg_count: u64 = sessions.iter().map(|s| s.message_count).sum();
        println!(
            " {} session(s), {} message(s) will be permanently deleted.",
            sessions.len(),
            msg_count
        );
        print!("\nDelete all? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("y") {
            let n = store.clear_all()?;
            println!("{n} session(s) deleted.");
        } else {
            println!("Aborted.");
        }
        return Ok(());
    }

    // Handle list-sessions subcommand
    if let Some(Command::ListSessions) = args.command {
        let sessions = store.list_sessions()?;
        if sessions.is_empty() {
            println!("No sessions found.");
            return Ok(());
        }

        let w_id = sessions
            .iter()
            .map(|s| s.id.len())
            .max()
            .unwrap_or(0)
            .max(2);
        let w_proj = sessions
            .iter()
            .map(|s| s.project_name.len())
            .max()
            .unwrap_or(0)
            .max(7);
        let w_last = 19;
        let w_status = sessions
            .iter()
            .map(|s| format!("{:?}", s.status).len())
            .max()
            .unwrap_or(0)
            .max(6);
        let w_msgs = sessions
            .iter()
            .map(|s| s.message_count.to_string().len())
            .max()
            .unwrap_or(0)
            .max(4);

        let sep = format!(
            " {0}  {1}  {2}  {3}  {4}",
            "─".repeat(w_id),
            "─".repeat(w_proj),
            "─".repeat(w_last),
            "─".repeat(w_status),
            "─".repeat(w_msgs),
        );

        println!(
            " {:<w_id$}  {:<w_proj$}  {:<w_last$}  {:<w_status$}  {:>w_msgs$}",
            "ID",
            "Project",
            "Last seen",
            "Status",
            "Msgs",
            w_id = w_id,
            w_proj = w_proj,
            w_last = w_last,
            w_status = w_status,
            w_msgs = w_msgs,
        );
        println!("{sep}");
        for s in &sessions {
            println!(
                " {:<w_id$}  {:<w_proj$}  {:<w_last$}  {:<w_status$}  {:>w_msgs$}",
                s.id,
                s.project_name,
                s.last_seen_at.format("%Y-%m-%d %H:%M:%S"),
                format!("{:?}", s.status).to_lowercase(),
                s.message_count,
                w_id = w_id,
                w_proj = w_proj,
                w_last = w_last,
                w_status = w_status,
                w_msgs = w_msgs,
            );
        }
        println!("{sep}");
        println!(" {} session(s)", sessions.len());
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

    // Filter by session if requested
    if let Some(ref session_id) = args.session {
        discovered.retain(|s| s.session_id == *session_id);
    }

    if discovered.is_empty() {
        anyhow::bail!(
            "no Claude Code session files found in {}",
            config_dir.display()
        );
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
        store.set_session_status(&ds.session_id, SessionStatus::Watching)?;
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
    history.sort_by_key(|m| m.timestamp);

    // Keep a handle to mark sessions ended on exit
    let store_exit = store.clone();
    let session_ids: Vec<String> = discovered.iter().map(|ds| ds.session_id.clone()).collect();

    // Start TUI
    let app = App::new(rx, store, discovered, history);
    let terminal = init_terminal()?;
    run(app, terminal)?;

    // Mark all watched sessions as ended on clean exit
    for id in &session_ids {
        if let Err(e) = store_exit.set_session_status(id, SessionStatus::Ended) {
            tracing::warn!("failed to mark session {id} as ended: {e:#}");
        }
    }

    Ok(())
}
