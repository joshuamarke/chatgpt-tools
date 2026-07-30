//! API provider management for Codex and Grok Build.
//!
//! Stores provider profiles under ChatGPT Tools app state and projects the
//! active profile into live config files (`~/.codex`, `~/.grok`).
//!
//! Independent of the skin engine. Focused on Codex / Grok profile storage
//! and live projection (plus optional local routing).

pub(crate) mod catalog;
mod commands;
pub(crate) mod codex;
pub(crate) mod grok;
pub(crate) mod model_unlock;
pub(crate) mod models;
mod presets;
mod probe;
pub(crate) mod store;

pub use commands::*;
