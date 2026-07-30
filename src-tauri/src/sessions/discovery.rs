//! Discover Codex session SQLite databases under the Codex home.
//! Discover local Codex session SQLite databases under Codex home.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

pub use super::home::default_codex_home_dir;

pub fn codex_session_db_paths_from_home(home: &Path) -> Vec<PathBuf> {
    let sqlite_home = resolve_sqlite_home_home_or_default(home);
    codex_session_db_paths_in_home(&sqlite_home)
}

fn codex_session_db_paths_in_home(home: &Path) -> Vec<PathBuf> {
    let mut paths = codex_sqlite_dir_session_dbs(home);
    let legacy = legacy_state_db_path(home);
    if !paths.iter().any(|path| path == &legacy) {
        paths.push(legacy);
    }
    paths
}

fn resolve_sqlite_home_from_env() -> Option<PathBuf> {
    resolve_sqlite_home(std::env::var_os("CODEX_SQLITE_HOME"))
}

fn resolve_sqlite_home_home_or_default(home: &Path) -> PathBuf {
    resolve_sqlite_home_from_env().unwrap_or_else(|| home.to_path_buf())
}

fn resolve_sqlite_home(value: Option<OsString>) -> Option<PathBuf> {
    let path = PathBuf::from(value?);
    (!path.as_os_str().is_empty() && path.is_dir()).then_some(path)
}

fn legacy_state_db_path(home: &Path) -> PathBuf {
    home.join("state_5.sqlite")
}

fn codex_sqlite_dir_session_dbs(home: &Path) -> Vec<PathBuf> {
    codex_sqlite_dir_dbs_with_tables(home, &["threads", "automation_runs", "inbox_items"])
}

fn codex_sqlite_dir_dbs_with_tables(home: &Path, tables: &[&str]) -> Vec<PathBuf> {
    let sqlite_dir = home.join("sqlite");
    let Ok(entries) = fs::read_dir(sqlite_dir) else {
        return Vec::new();
    };
    let mut candidates = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| is_sqlite_candidate(path))
        .filter(|path| has_any_table(path, tables))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| {
        (
            path.file_name()
                .map(|name| name != OsStr::new("codex-dev.db"))
                .unwrap_or(true),
            path.file_name().map(|name| name.to_os_string()),
        )
    });
    candidates
}

fn is_sqlite_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some("db") | Some("sqlite") | Some("sqlite3")
    )
}

fn has_any_table(path: &Path, tables: &[&str]) -> bool {
    tables.iter().any(|table| sqlite_has_table(path, table))
}

fn sqlite_has_table(path: &Path, table: &str) -> bool {
    let Ok(db) = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return false;
    };
    db.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        [table],
        |_| Ok(()),
    )
    .is_ok()
}


pub fn codex_thread_reference_db_paths_from_home(home: &Path) -> Vec<PathBuf> {
    let sqlite_home = resolve_sqlite_home_home_or_default(home);
    let mut paths = codex_sqlite_dir_thread_reference_dbs(&sqlite_home);
    let legacy = legacy_state_db_path(&sqlite_home);
    if !paths.iter().any(|path| path == &legacy) {
        paths.push(legacy);
    }
    paths
}

fn codex_sqlite_dir_thread_reference_dbs(home: &Path) -> Vec<PathBuf> {
    codex_sqlite_dir_dbs_with_tables(
        home,
        &[
            "threads",
            "local_thread_catalog",
            "automation_runs",
            "inbox_items",
            "sessions",
            "messages",
            "thread_dynamic_tools",
            "thread_goals",
            "thread_spawn_edges",
            "stage1_outputs",
            "agent_job_items",
        ],
    )
}

pub fn codex_sqlite_sidecar_paths(db_path: &Path) -> [PathBuf; 3] {
    [
        db_path.to_path_buf(),
        PathBuf::from(format!("{}-wal", db_path.to_string_lossy())),
        PathBuf::from(format!("{}-shm", db_path.to_string_lossy())),
    ]
}

pub fn relative_to_codex_home(home: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(home).unwrap_or(path).to_path_buf()
}

