use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Context;

#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    pub project_name: String,
    pub file_path: PathBuf,
    pub last_modified: SystemTime,
}

pub fn resolve_config_dir(override_path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(p);
    }
    if let Ok(v) = std::env::var("CLAUDE_CONFIG_DIR") {
        return Ok(PathBuf::from(v));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let p = PathBuf::from(xdg).join("claude");
        if p.exists() {
            return Ok(p);
        }
    }
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let xdg_fallback = home.join(".config").join("claude");
    if xdg_fallback.exists() {
        return Ok(xdg_fallback);
    }
    let dot_claude = home.join(".claude");
    if dot_claude.exists() {
        return Ok(dot_claude);
    }
    anyhow::bail!(
        "Claude config directory not found; set CLAUDE_CONFIG_DIR or create ~/.claude"
    )
}

pub fn scan_sessions(config_dir: &Path) -> Vec<DiscoveredSession> {
    let projects_dir = config_dir.join("projects");
    let mut sessions = Vec::new();
    scan_dir(&projects_dir, &projects_dir, &mut sessions);
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    sessions
}

fn scan_dir(root: &Path, dir: &Path, out: &mut Vec<DiscoveredSession>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("cannot scan {}: {e}", dir.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let last_modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);

            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let project_name = path
                .parent()
                .and_then(|p| {
                    if p == root {
                        None
                    } else {
                        // Use the immediate parent relative to projects root
                        p.strip_prefix(root)
                            .ok()
                            .and_then(|rel| rel.iter().next())
                            .and_then(|c| c.to_str())
                            .map(|s| s.to_string())
                    }
                })
                .unwrap_or_else(|| "default".to_string());

            out.push(DiscoveredSession {
                session_id,
                project_name,
                file_path: path,
                last_modified,
            });
        }
    }
}
