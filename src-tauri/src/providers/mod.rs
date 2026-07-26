//! API provider management for Codex and Grok Build.
//!
//! Stores provider profiles under ChatGPT Tools app state and projects the
//! active profile into live config files (`~/.codex`, `~/.grok`).
//!
//! Independent of the skin engine. Adapted from the essential Codex / Grok
//! paths in cc-switch (without SQLite, proxy, MCP, or multi-app complexity).

mod catalog;
mod commands;
mod codex;
mod grok;
mod models;
mod presets;
mod probe;
mod store;

pub use commands::*;
