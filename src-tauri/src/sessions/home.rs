//! Resolve Codex user home (`CODEX_HOME` or `~/.codex`).
//! Resolve Codex home (`CODEX_HOME` or `~/.codex`).

use std::path::PathBuf;

pub fn default_codex_home_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(|path| codex_home_env_dir_is_valid(path))
        .unwrap_or_else(default_user_codex_home_dir)
}

fn codex_home_env_dir_is_valid(path: &PathBuf) -> bool {
    !path.as_os_str().is_empty()
        && !path.to_string_lossy().trim().is_empty()
        && path.is_dir()
}

fn default_user_codex_home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        return PathBuf::from(home).join(".codex");
    }
    PathBuf::from(".codex")
}
