//! Local **chat session** management (Codex / ChatGPT desktop + Grok Build).
//!
//! Independent of the skin engine:
//! - Codex: home SQLite + rollouts, delete backups under ChatGPT Tools app state
//! - Grok Build: `~/.grok/sessions` summary.json + chat_history.jsonl
//!
//! Codex: home SQLite + rollouts; Grok: `~/.grok/sessions` summary + history.

mod backup;
pub mod commands;
mod discovery;
mod grok;
mod home;

/// Grok Build home dir (`GROK_HOME` or `~/.grok`) for env / overview probes.
pub use grok::default_grok_home_dir;
/// Codex home (`CODEX_HOME` or `~/.codex`).
pub use home::default_codex_home_dir;
mod markdown;
mod models;
pub mod paths;
mod provider_sync;
mod storage;
mod util;
