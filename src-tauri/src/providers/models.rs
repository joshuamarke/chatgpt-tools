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
        }
    }

    pub fn is_official(&self) -> bool {
        self.category.as_deref() == Some("official")
    }
}

/// On-disk store for one app kind.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppProviderStore {
    #[serde(default)]
    pub current: String,
    #[serde(default)]
    pub providers: Vec<Provider>,
}

/// Root file: `%LOCALAPPDATA%\ChatGPTTools\providers.json`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub codex: AppProviderStore,
    #[serde(default)]
    pub grok: AppProviderStore,
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
    /// Grok `[models].default` / profile key (defaults to model id).
    #[serde(default)]
    pub profile: Option<String>,
    /// Grok `api_backend`: responses | chat_completions
    #[serde(default)]
    pub api_backend: Option<String>,
    /// Grok `context_window` (positive integer).
    #[serde(default)]
    pub context_window: Option<i64>,
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
