//! Provider domain types (Codex + Grok Build).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Supported CLI / desktop tools for provider switching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    Codex,
    Grok,
}

impl AppKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AppKind::Codex => "codex",
            AppKind::Grok => "grok",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "codex" | "chatgpt" | "openai" => Some(AppKind::Codex),
            "grok" | "grokbuild" | "grok-build" | "grok_build" => Some(AppKind::Grok),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AppKind::Codex => "Codex",
            AppKind::Grok => "Grok Build",
        }
    }
}

/// Optional provider-level metadata (User-Agent / local-proxy overrides).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMeta {
    /// Custom User-Agent for proxy / probe paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_user_agent: Option<String>,
    /// Local proxy request header/body overrides (stored for future proxy / advanced use).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_proxy_request_overrides: Option<LocalProxyRequestOverrides>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalProxyRequestOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<serde_json::Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

/// One saved provider profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// Live-shaped settings. Codex: `{ auth, config, modelCatalog? }`; Grok: `{ config }`.
    pub settings_config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    /// `official` | `third_party` | `custom` | `aggregator` …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<ProviderMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_index: Option<usize>,
    /// When auto-failover is on, this provider is tried in queue order.
    #[serde(default, skip_serializing_if = "is_false")]
    pub in_failover_queue: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl Provider {
    pub fn new(id: String, name: String, settings_config: Value) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            id,
            name,
            settings_config,
            website_url: None,
            category: Some("custom".into()),
            notes: None,
            meta: None,
            created_at: Some(now),
            updated_at: Some(now),
            sort_index: None,
            in_failover_queue: false,
        }
    }

    pub fn is_official(&self) -> bool {
        self.category.as_deref() == Some("official")
    }
}

/// Per-app circuit breaker defaults (local routing).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CircuitConfig {
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,
    #[serde(default = "default_circuit_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_error_rate")]
    pub error_rate_threshold: f64,
    #[serde(default = "default_min_requests")]
    pub min_requests: u32,
}

fn default_failure_threshold() -> u32 {
    4
}
fn default_success_threshold() -> u32 {
    2
}
fn default_circuit_timeout() -> u64 {
    60
}
fn default_error_rate() -> f64 {
    0.6
}
fn default_min_requests() -> u32 {
    10
}

impl Default for CircuitConfig {
    fn default() -> Self {
        Self {
            failure_threshold: default_failure_threshold(),
            success_threshold: default_success_threshold(),
            timeout_seconds: default_circuit_timeout(),
            error_rate_threshold: default_error_rate(),
            min_requests: default_min_requests(),
        }
    }
}

/// On-disk store for one app kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppProviderStore {
    #[serde(default)]
    pub current: String,
    #[serde(default)]
    pub providers: Vec<Provider>,
    /// Live config base_url points at local proxy for this app.
    #[serde(default)]
    pub takeover_enabled: bool,
    /// Failover queue walk instead of single current provider.
    /// Default **on** for new installs; existing JSON without the field also enables.
    #[serde(default = "default_true")]
    pub auto_failover_enabled: bool,
    /// Explicit failover order (provider ids). Decoupled from list `sort_index`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failover_order: Vec<String>,
    /// Max extra retries after the first attempt (FO attempts = max_retries + 1).
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default)]
    pub circuit: CircuitConfig,
    #[serde(default = "default_streaming_first_byte")]
    pub streaming_first_byte_timeout: u64,
    #[serde(default = "default_streaming_idle")]
    pub streaming_idle_timeout: u64,
    #[serde(default = "default_non_streaming")]
    pub non_streaming_timeout: u64,
}

impl Default for AppProviderStore {
    fn default() -> Self {
        Self {
            current: String::new(),
            providers: Vec::new(),
            takeover_enabled: false,
            auto_failover_enabled: true,
            failover_order: Vec::new(),
            max_retries: default_max_retries(),
            circuit: CircuitConfig::default(),
            streaming_first_byte_timeout: default_streaming_first_byte(),
            streaming_idle_timeout: default_streaming_idle(),
            non_streaming_timeout: default_non_streaming(),
        }
    }
}

impl AppProviderStore {
    /// Keep `failover_order` and `in_failover_queue` consistent.
    ///
    /// **SSOT is `failover_order` once it is non-empty.** Flags alone only seed
    /// the order on first migration (empty order). This prevents remove→normalize
    /// from re-adding providers that still had a stale `in_failover_queue=true`.
    pub fn normalize_failover_order(&mut self) {
        self.failover_order
            .retain(|id| self.providers.iter().any(|p| p.id == *id));
        // Dedup while preserving order
        let mut seen = std::collections::HashSet::new();
        self.failover_order.retain(|id| seen.insert(id.clone()));

        if self.failover_order.is_empty() {
            // One-time migration from legacy flags
            for p in &self.providers {
                if p.in_failover_queue {
                    self.failover_order.push(p.id.clone());
                }
            }
        }
        let order = self.failover_order.clone();
        for p in &mut self.providers {
            p.in_failover_queue = order.iter().any(|id| id == &p.id);
        }
    }
}

#[cfg(test)]
mod failover_order_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_order_is_ssot_and_clears_stale_flags() {
        let mut s = AppProviderStore::default();
        let mut a = Provider::new("a".into(), "A".into(), json!({}));
        a.in_failover_queue = true; // stale flag — must NOT re-enter order
        let mut b = Provider::new("b".into(), "B".into(), json!({}));
        b.in_failover_queue = false;
        let mut c = Provider::new("c".into(), "C".into(), json!({}));
        c.in_failover_queue = true;
        s.providers = vec![a, b, c];
        s.failover_order = vec!["c".into(), "missing".into()];
        s.normalize_failover_order();
        assert_eq!(s.failover_order, vec!["c".to_string()]);
        assert!(!s.providers.iter().find(|p| p.id == "a").unwrap().in_failover_queue);
        assert!(!s.providers.iter().find(|p| p.id == "b").unwrap().in_failover_queue);
        assert!(s.providers.iter().find(|p| p.id == "c").unwrap().in_failover_queue);
    }

    #[test]
    fn normalize_migrates_from_flags_when_order_empty() {
        let mut s = AppProviderStore::default();
        let mut a = Provider::new("a".into(), "A".into(), json!({}));
        a.in_failover_queue = true;
        let b = Provider::new("b".into(), "B".into(), json!({}));
        s.providers = vec![a, b];
        s.failover_order.clear();
        s.normalize_failover_order();
        assert_eq!(s.failover_order, vec!["a".to_string()]);
    }
}

fn default_max_retries() -> u32 {
    3
}
fn default_streaming_first_byte() -> u64 {
    60
}
fn default_streaming_idle() -> u64 {
    120
}
fn default_non_streaming() -> u64 {
    600
}

/// Global local-proxy listen settings (shared by Codex + Grok).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalProxyConfig {
    /// Bind address — default loopback only.
    #[serde(default = "default_listen_address")]
    pub listen_address: String,
    /// Uncommon high port (avoids clash with common local tools).
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    #[serde(default = "default_true")]
    pub enable_logging: bool,
    /// How many days of proxy request logs to keep (UI-configurable, default 7).
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u32,
    /// Optional upstream egress proxy for local-routing outbound requests
    /// (http / https / socks5). Empty = direct connect (no system proxy).
    /// Only applies when local routing is on: App → local proxy → [egress] → upstream.
    #[serde(default)]
    pub egress_proxy: String,
}

fn default_log_retention_days() -> u32 {
    7
}

fn default_listen_address() -> String {
    "127.0.0.1".into()
}
/// 18964 — deliberately uncommon; avoids clash with common local / dev ports.
fn default_listen_port() -> u16 {
    18964
}

impl Default for GlobalProxyConfig {
    fn default() -> Self {
        Self {
            listen_address: default_listen_address(),
            listen_port: default_listen_port(),
            enable_logging: true,
            log_retention_days: default_log_retention_days(),
            egress_proxy: String::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// Root file: `%LOCALAPPDATA%\ChatGPTTools\providers.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersFile {
    #[serde(default)]
    pub version: u32,
    /// When true (default), enabling a third-party Codex provider only rewrites
    /// `config.toml` (provider-scoped bearer + `requires_openai_auth`) and leaves
    /// `~/.codex/auth.json` ChatGPT / Codex OAuth cache intact.
    #[serde(default = "default_true")]
    pub preserve_codex_official_auth: bool,
    #[serde(default)]
    pub proxy: GlobalProxyConfig,
    #[serde(default)]
    pub codex: AppProviderStore,
    #[serde(default)]
    pub grok: AppProviderStore,
}

impl Default for ProvidersFile {
    fn default() -> Self {
        Self {
            version: 2,
            preserve_codex_official_auth: true,
            proxy: GlobalProxyConfig::default(),
            codex: AppProviderStore::default(),
            grok: AppProviderStore::default(),
        }
    }
}

impl ProvidersFile {
    pub fn for_kind_mut(&mut self, kind: AppKind) -> &mut AppProviderStore {
        match kind {
            AppKind::Codex => &mut self.codex,
            AppKind::Grok => &mut self.grok,
        }
    }

    pub fn for_kind(&self, kind: AppKind) -> &AppProviderStore {
        match kind {
            AppKind::Codex => &self.codex,
            AppKind::Grok => &self.grok,
        }
    }
}

/// Live file status for the GUI.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub config_exists: bool,
    pub auth_exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wire_api: Option<String>,
    pub has_api_key: bool,
    /// Whether the marked-current provider matches live files.
    pub current_matches_live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// `direct` | `takeover` | `broken`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Machine-readable: ok | drift | unlinked | route_half | route_desync | missing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_code: Option<String>,
}

/// List payload for the GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderListResponse {
    pub app: String,
    pub current: String,
    pub providers: Vec<ProviderSummary>,
    pub live_paths: LivePaths,
    #[serde(default)]
    pub live_status: LiveStatus,
    /// Codex: keep ChatGPT OAuth in auth.json when enabling third-party providers.
    #[serde(default = "default_true")]
    pub preserve_codex_official_auth: bool,
    #[serde(default)]
    pub takeover_enabled: bool,
    #[serde(default)]
    pub auto_failover_enabled: bool,
    #[serde(default)]
    pub proxy: GlobalProxyConfig,
    #[serde(default)]
    pub proxy_running: bool,
    #[serde(default)]
    pub proxy_status: Option<ProxyRuntimeStatus>,
}

/// Runtime snapshot for the local routing process (GUI).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRuntimeStatus {
    pub running: bool,
    pub address: String,
    pub port: u16,
    pub active_connections: usize,
    pub total_requests: u64,
    pub success_requests: u64,
    pub failed_requests: u64,
    pub success_rate: f32,
    pub uptime_seconds: u64,
    pub failover_count: u64,
    #[serde(default)]
    pub active_targets: Vec<ProxyActiveTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProxyActiveTarget {
    pub app_type: String,
    pub provider_id: String,
    pub provider_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub is_current: bool,
    /// Live files currently match this provider's base_url/model.
    #[serde(default)]
    pub matches_live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wire_api: Option<String>,
    /// Ready to switch (has key + base_url for third-party).
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub in_failover_queue: bool,
    /// 1-based position in failover queue when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failover_priority: Option<usize>,
    /// healthy | degraded | open | unknown
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePaths {
    pub home: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    pub config: String,
}

/// Result of switch / write live.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SwitchResult {
    pub ok: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub live_paths: Option<LivePaths>,
    /// Codex: slugs projected into model_catalog_json (desktop + CLI list).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projected_models: Vec<String>,
}

/// Input for add / update from GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUpsertRequest {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub website_url: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// Codex wire_api: `responses` | `chat`
    #[serde(default)]
    pub wire_api: Option<String>,
    /// Codex top-level `model_reasoning_effort`: high | medium | low | minimal
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    /// Grok `[models].default` / `[model."<id>"]` identity (defaults to supplier name).
    #[serde(default)]
    pub profile: Option<String>,
    /// Grok `api_backend`: responses | chat_completions
    #[serde(default)]
    pub api_backend: Option<String>,
    /// Grok `context_window` (positive integer).
    #[serde(default)]
    pub context_window: Option<i64>,
    /// Grok `[model.*].name` picker label (defaults to upstream model id).
    #[serde(default)]
    pub model_display_name: Option<String>,
    /// When true (or when base_url is empty), use `config_toml` as the source of truth.
    #[serde(default)]
    pub use_config_toml: Option<bool>,
    #[serde(default)]
    pub config_toml: Option<String>,
    #[serde(default)]
    pub keep_existing_api_key: Option<bool>,
    /// When true, save then immediately switch/write live.
    #[serde(default)]
    pub activate: Option<bool>,
    /// Custom User-Agent (stored in provider.meta).
    #[serde(default)]
    pub custom_user_agent: Option<String>,
    /// Local proxy header overrides as JSON object string.
    #[serde(default)]
    pub local_proxy_headers_json: Option<String>,
    /// Local proxy body overrides as JSON object string.
    #[serde(default)]
    pub local_proxy_body_json: Option<String>,
    /// Codex model catalog rows: `[{ model, displayName?, contextWindow? }, …]`.
    #[serde(default)]
    pub model_catalog: Option<Vec<Value>>,
}

/// Full provider detail (includes secrets for edit form).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDetail {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub wire_api: String,
    #[serde(default)]
    pub reasoning_effort: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub api_backend: String,
    #[serde(default)]
    pub context_window: i64,
    /// Grok `[model.*].name` picker label.
    #[serde(default)]
    pub model_display_name: String,
    pub config_toml: String,
    #[serde(default)]
    pub custom_user_agent: String,
    #[serde(default)]
    pub local_proxy_headers_json: String,
    #[serde(default)]
    pub local_proxy_body_json: String,
    /// Codex simplified catalog rows for the form table.
    #[serde(default)]
    pub model_catalog: Vec<Value>,
    pub is_current: bool,
    pub is_official: bool,
    pub ready: bool,
    pub matches_live: bool,
}
