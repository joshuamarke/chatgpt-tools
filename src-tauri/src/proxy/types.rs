//! Shared proxy DTOs (re-export GUI-facing types from providers::models).

pub use crate::providers::models::{
    CircuitConfig, GlobalProxyConfig, ProxyRuntimeStatus,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverStatus {
    pub codex: bool,
    pub grok: bool,
    pub proxy_running: bool,
    pub proxy: GlobalProxyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailoverQueueItem {
    pub provider_id: String,
    pub provider_name: String,
    pub sort_index: usize,
    pub is_current: bool,
    pub health: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppProxySettings {
    pub app: String,
    pub takeover_enabled: bool,
    pub auto_failover_enabled: bool,
    pub max_retries: u32,
    pub circuit: CircuitConfig,
    pub streaming_first_byte_timeout: u64,
    pub streaming_idle_timeout: u64,
    pub non_streaming_timeout: u64,
}
