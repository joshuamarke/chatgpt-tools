//! Cloud endpoint configuration + host allowlist.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use crate::cdp;
use crate::engine::EngineError;

/// Protocol version this client speaks (must match CDN `protocol: 1`).
pub const CLOUD_PROTOCOL: u32 = 1;

/// Default development preview API (local chatgpt-tools-cdn `npm run serve`).
pub const DEFAULT_DEV_BASE_URL: &str = "http://127.0.0.1:8788/v1";

/// Default production-style channel.
pub const DEFAULT_CHANNEL: &str = "stable";

/// Hard cap for a single .cgskin download (bytes).
pub const MAX_PACKAGE_BYTES: u64 = 48 * 1024 * 1024;

/// Max HTTP redirects when following package URLs.
pub const MAX_REDIRECTS: u32 = 3;

/// Default request timeout (ms).
pub const DEFAULT_TIMEOUT_MS: u64 = 15_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudConfig {
    pub enabled: bool,
    pub base_url: String,
    pub channel: String,
    pub timeout_ms: u64,
    pub allowed_hosts: Vec<String>,
    pub protocol: u32,
    /// App version string used for min/max filters (from package / constant).
    pub app_version: String,
    pub engine_protocol: u32,
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_url: DEFAULT_DEV_BASE_URL.to_string(),
            channel: DEFAULT_CHANNEL.to_string(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            allowed_hosts: default_allowed_hosts(),
            protocol: CLOUD_PROTOCOL,
            // Keep in sync with GUI `APP_VERSION` / package.json (not crate 1.0.0).
            app_version: "2.2.0".to_string(),
            engine_protocol: cdp::native_engine_protocol(),
        }
    }
}

fn default_allowed_hosts() -> Vec<String> {
    vec![
        "127.0.0.1".into(),
        "localhost".into(),
        "cdn.example.com".into(),
        "github.com".into(),
        "objects.githubusercontent.com".into(),
        "release-assets.githubusercontent.com".into(),
        "raw.githubusercontent.com".into(),
        // Cloudflare R2 public buckets (wildcard match via host_allowed)
        "r2.dev".into(),
        "cloudflarestorage.com".into(),
    ]
}

fn settings_path() -> PathBuf {
    cdp::native_state_root().join("settings.json")
}

fn read_settings_json() -> Value {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Load cloud config: defaults → settings.json → env overrides.
pub fn load_cloud_config() -> CloudConfig {
    let mut cfg = CloudConfig::default();
    // Prefer GUI/about APP_VERSION when settings pin it
    if let Ok(v) = std::env::var("CODEX_SKIN_APP_VERSION") {
        if !v.trim().is_empty() {
            cfg.app_version = v.trim().to_string();
        }
    }

    let settings = read_settings_json();
    if let Some(cloud) = settings.get("cloud") {
        if let Some(b) = cloud.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(u) = cloud.get("baseUrl").and_then(|v| v.as_str()) {
            if !u.trim().is_empty() {
                cfg.base_url = normalize_base_url(u);
            }
        }
        if let Some(c) = cloud.get("channel").and_then(|v| v.as_str()) {
            if !c.trim().is_empty() {
                cfg.channel = c.trim().to_string();
            }
        }
        if let Some(t) = cloud.get("timeoutMs").and_then(|v| v.as_u64()) {
            if t >= 1000 {
                cfg.timeout_ms = t;
            }
        }
        if let Some(hosts) = cloud.get("allowedHosts").and_then(|v| v.as_array()) {
            let mut list: Vec<String> = hosts
                .iter()
                .filter_map(|h| h.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect();
            if !list.is_empty() {
                // Always keep loopback for local CDN preview
                for h in ["127.0.0.1", "localhost"] {
                    if !list.iter().any(|x| x.eq_ignore_ascii_case(h)) {
                        list.push(h.into());
                    }
                }
                cfg.allowed_hosts = list;
            }
        }
        if let Some(av) = cloud.get("appVersion").and_then(|v| v.as_str()) {
            if !av.trim().is_empty() {
                cfg.app_version = av.trim().to_string();
            }
        }
    }

    // Env overrides (dev / CI)
    if matches!(
        std::env::var("CODEX_SKIN_CLOUD_DISABLED")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    ) {
        cfg.enabled = false;
    }
    if let Ok(url) = std::env::var("CODEX_SKIN_CLOUD_URL") {
        if !url.trim().is_empty() {
            cfg.base_url = normalize_base_url(&url);
            cfg.enabled = true;
        }
    }
    if let Ok(ch) = std::env::var("CODEX_SKIN_CLOUD_CHANNEL") {
        if !ch.trim().is_empty() {
            cfg.channel = ch.trim().to_string();
        }
    }

    cfg.base_url = normalize_base_url(&cfg.base_url);
    cfg
}

pub fn normalize_base_url(raw: &str) -> String {
    let s = raw.trim().trim_end_matches('/');
    s.to_string()
}

/// Whether a URL host is permitted for catalog/package/preview fetches.
pub fn host_allowed(host: &str, allowed: &[String]) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    for pattern in allowed {
        let p = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
        if p.is_empty() {
            continue;
        }
        if p.starts_with("*.") {
            let suffix = &p[1..]; // ".example.com"
            if host.ends_with(suffix) || host == &p[2..] {
                return true;
            }
        } else if host == p {
            return true;
        } else if host.ends_with(&format!(".{p}")) {
            // allow subdomain of listed apex (e.g. pub-xxx.r2.dev under r2.dev)
            return true;
        }
    }
    false
}

/// Validate absolute http(s) URL against scheme + host allowlist.
pub fn validate_download_url(url: &str, cfg: &CloudConfig) -> Result<url::Url, EngineError> {
    let parsed = url::Url::parse(url).map_err(|e| EngineError::msg(format!("无效下载 URL: {e}")))?;
    let scheme = parsed.scheme();
    let host = parsed
        .host_str()
        .ok_or_else(|| EngineError::msg("下载 URL 缺少 host"))?;

    let is_loopback = host.eq_ignore_ascii_case("127.0.0.1")
        || host.eq_ignore_ascii_case("localhost")
        || host == "::1";

    if scheme == "http" {
        if !is_loopback {
            return Err(EngineError::msg(
                "非本机下载仅允许 HTTPS（开发预览可用 http://127.0.0.1）",
            ));
        }
    } else if scheme != "https" {
        return Err(EngineError::msg(format!("不支持的 URL scheme: {scheme}")));
    }

    if !host_allowed(host, &cfg.allowed_hosts) {
        return Err(EngineError::msg(format!(
            "下载 host 不在白名单: {host}"
        )));
    }

    // Block userinfo / odd ports abuse lightly
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(EngineError::msg("拒绝带认证信息的下载 URL"));
    }

    Ok(parsed)
}


