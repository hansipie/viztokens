use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "viztokens",
    version,
    about = "Live Claude Code session monitor"
)]
pub struct Args {
    #[arg(short = 's', long, help = "Display a specific session by UUID")]
    pub session: Option<String>,

    #[arg(
        short = 'p',
        long,
        help = "Limit discovery to a specific project directory name"
    )]
    pub project: Option<String>,

    #[arg(
        long,
        help = "Override Claude Code config directory",
        env = "CLAUDE_CONFIG_DIR"
    )]
    pub config_dir: Option<PathBuf>,

    #[arg(long, help = "SQLite database path", env = "VIZTOKENS_DB")]
    pub db: Option<String>,

    #[arg(
        long,
        default_value = "120",
        help = "Only show sessions modified within the last N minutes (0 = show all)"
    )]
    pub max_age: u64,

    #[arg(long, help = "Hermes state.db path (default: ~/.hermes/state.db if present)", env = "HERMES_DB")]
    pub hermes_db: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(
        name = "list-sessions",
        about = "Print discovered sessions as JSON and exit"
    )]
    ListSessions,
    #[command(
        name = "update",
        about = "Update viztokens to the latest version from GitHub"
    )]
    Update,
    #[command(
        name = "prune-stale",
        about = "Delete stale sessions (and their messages) from the database"
    )]
    PruneStale,
    #[command(
        name = "clear",
        about = "Delete all sessions and messages from the database"
    )]
    Clear,
}
