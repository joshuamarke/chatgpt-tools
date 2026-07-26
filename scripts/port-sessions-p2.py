# -*- coding: utf-8 -*-
"""Port Phase 2 session modules from CodexPlusPlus into chatgpt-tools."""
from pathlib import Path

ROOT = Path(r"E:/demo/chatgpt-tools")
SRC = Path(r"E:/demo/CodexPlusPlus/crates/codex-plus-data/src")
SESS = ROOT / "src-tauri/src/sessions"


def extend_discovery() -> None:
    disc = SESS / "discovery.rs"
    text = disc.read_text(encoding="utf-8")
    if "codex_thread_reference_db_paths_from_home" in text:
        print("discovery already extended")
        return
    extra = r'''

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
'''
    disc.write_text(text.rstrip() + "\n" + extra + "\n", encoding="utf-8", newline="\n")
    print("discovery extended")


def port_markdown() -> None:
    md = (SRC / "markdown.rs").read_text(encoding="utf-8")
    md = md.replace(
        "use codex_plus_core::models::{ExportResult, ExportStatus, SessionRef};",
        "use super::models::{ExportResult, ExportStatus, SessionRef};",
    )
    header = (
        "//! Export local Codex sessions to Markdown.\n"
        "//! Adapted from CodexPlusPlus `codex-plus-data::markdown`.\n\n"
    )
    if not md.lstrip().startswith("//!"):
        md = header + md
    (SESS / "markdown.rs").write_text(md, encoding="utf-8", newline="\n")
    print("markdown ok", len(md))


def port_provider_sync() -> None:
    ps = (SRC / "provider_sync.rs").read_text(encoding="utf-8")
    replacements = [
        (
            "codex_plus_core::codex_sqlite::codex_session_db_paths_from_home",
            "crate::sessions::discovery::codex_session_db_paths_from_home",
        ),
        (
            "codex_plus_core::codex_sqlite::codex_thread_reference_db_paths_from_home",
            "crate::sessions::discovery::codex_thread_reference_db_paths_from_home",
        ),
        (
            "codex_plus_core::codex_sqlite::codex_sqlite_sidecar_paths",
            "crate::sessions::discovery::codex_sqlite_sidecar_paths",
        ),
        (
            "codex_plus_core::codex_sqlite::relative_to_codex_home",
            "crate::sessions::discovery::relative_to_codex_home",
        ),
        (
            "codex_plus_core::settings::atomic_write",
            "crate::sessions::util::atomic_write",
        ),
        (
            "codex_plus_core::watcher::find_session_index_cleanup_blocking_processes()",
            "crate::sessions::util::find_session_index_cleanup_blocking_processes()",
        ),
    ]
    for a, b in replacements:
        ps = ps.replace(a, b)
    header = (
        "//! Provider metadata repair + session_index cleanup.\n"
        "//! Adapted from CodexPlusPlus `codex-plus-data::provider_sync`.\n\n"
    )
    if not ps.lstrip().startswith("//!"):
        ps = header + ps
    (SESS / "provider_sync.rs").write_text(ps, encoding="utf-8", newline="\n")
    left = [ln.strip() for ln in ps.splitlines() if "codex_plus_core" in ln]
    print("provider_sync ok", len(ps), "leftover", left)


def main() -> None:
    extend_discovery()
    port_markdown()
    port_provider_sync()


if __name__ == "__main__":
    main()
