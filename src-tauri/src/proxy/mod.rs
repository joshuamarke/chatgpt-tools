//! Local HTTP routing proxy for Codex / Grok Build.
//!
//! Responsibilities:
//! - Listen on a loopback port (default 18964)
//! - Per-app takeover rewrites live base_url → local proxy
//! - Forward with real upstream credentials from provider archives
//! - Ordered failover + circuit breaker
//! - Preserve Codex OAuth in auth.json (never write PROXY_MANAGED there)

mod circuit;
pub mod commands;
mod forwarder;
pub mod log_store;
pub mod runtime;
mod server;
pub mod takeover;
mod types;
mod usage;

pub use commands::*;
pub use runtime::{proxy_status_snapshot, runtime};
pub use takeover::{is_proxy_base_url, proxy_connect_host};

/// Auth / bearer placeholder written only into live *config* during takeover.
pub const PROXY_MANAGED: &str = "PROXY_MANAGED";
/// Codex third-party takeover provider table id.
pub const CODEX_PROXY_PROVIDER_ID: &str = "chatgpt-tools-proxy";
/// Codex official takeover provider table id (OAuth passthrough).
pub const CODEX_OFFICIAL_PROXY_PROVIDER_ID: &str = "chatgpt-tools-official";

/// On app start: if takeover flags are set, re-bind proxy + re-assert live rewrite.
pub fn restore_on_startup() {
    if let Err(e) = runtime().restore_on_startup() {
        eprintln!("[proxy] startup restore: {e}");
    }
}

/// On app exit: restore live configs if still taken over.
pub fn shutdown_on_exit() {
    if let Err(e) = runtime().shutdown_all() {
        eprintln!("[proxy] exit restore: {e}");
    }
}
