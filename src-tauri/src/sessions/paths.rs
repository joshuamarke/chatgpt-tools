//! Paths owned by ChatGPT Tools for session feature state (not Codex home).

use std::path::PathBuf;

/// App state root — same tree as skin engine (`%LOCALAPPDATA%\ChatGPTTools` on Windows).
pub fn app_state_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CODEX_SKIN_MANAGER_STATE") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let name = std::env::var("CODEX_SKIN_STATE_NAME").unwrap_or_else(|_| "ChatGPTTools".into());
    if cfg!(windows) {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
            std::env::var("USERPROFILE")
                .map(|u| format!(r"{u}\AppData\Local"))
                .unwrap_or_else(|_| ".".into())
        });
        PathBuf::from(local).join(name)
    } else if cfg!(target_os = "macos") {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("Library")
            .join("Application Support")
            .join(name)
    } else {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".local")
            .join("share")
            .join(name)
    }
}

/// Delete-undo backups for local sessions.
pub fn session_backups_dir() -> PathBuf {
    app_state_dir().join("session-backups")
}
