//! Cloud endpoint configuration + host allowlist.
//!
//! Production base URL / extra hosts are **not** hardcoded. They are embedded at
//! package time via `scripts/inject-release-config.mjs` → `gen/release-config.json`
//! → `build.rs` (`CHATGPT_TOOLS_CLOUD_*` rustc-env). Treat those values like
//! signing private keys: CI Secrets or `keys/release.env` only.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use crate::cdp;
use crate::engine::EngineError;

/// Protocol version this client speaks (must match CDN `protocol: 1`).
pub const CLOUD_PROTOCOL: u32 = 1;

/// Local CDN preview (or local-only mode) when no package-time cloud URL was embedded.
pub const DEFAULT_DEV_BASE_URL: &str = "";  // empty = local-only mode (no cloud URL)

/// Default production-style channel.
pub const DEFAULT_CHANNEL: &str = "stable";

/// Hard cap for a single .skin download (bytes).
pub const MAX_PACKAGE_BYTES: u64 = 48 * 1024 * 1024;

/// Hard cap for a single catalog preview / screenshot thumbnail (bytes).
/// Keep small so list enrich + IPC stay snappy (full art stays in the package).
pub const MAX_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;

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

/// Package-time cloud API base (empty in pure dev builds).
fn embedded_cloud_base_url() -> String {
    option_env!("CHATGPT_TOOLS_CLOUD_BASE_URL")
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Package-time extra allowlist hosts (comma-separated).
fn embedded_cloud_extra_hosts() -> Vec<String> {
    option_env!("CHATGPT_TOOLS_CLOUD_EXTRA_HOSTS")
        .unwrap_or("")
        .split([',', ';', '\n'])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn default_base_url() -> String {
    let embedded = embedded_cloud_base_url();
    if !embedded.is_empty() {
        return normalize_base_url(&embedded);
    }
    "".to_string()  // empty = local-only: no cloud catalog (skin list still from library / workspace)
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_url: default_base_url(),
            channel: DEFAULT_CHANNEL.to_string(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            allowed_hosts: default_allowed_hosts(),
            protocol: CLOUD_PROTOCOL,
            // Keep in sync with GUI `APP_VERSION` / package.json (not crate 1.0.0).
            app_version: "1.1.12".to_string(),
            engine_protocol: cdp::native_engine_protocol(),
        }
    }
}

fn default_allowed_hosts() -> Vec<String> {
    let mut hosts: Vec<String> = vec![
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
    ];
    for h in embedded_cloud_extra_hosts() {
        if !hosts.iter().any(|x| x.eq_ignore_ascii_case(&h)) {
            hosts.push(h);
        }
    }
    // Always allow host of the effective default base URL (embedded or dev).
    if let Ok(parsed) = url::Url::parse(&default_base_url()) {
        if let Some(host) = parsed.host_str() {
            let h = host.trim().to_string();
            if !h.is_empty() && !hosts.iter().any(|x| x.eq_ignore_ascii_case(&h)) {
                hosts.push(h);
            }
        }
    }
    hosts
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

fn push_host_unique(hosts: &mut Vec<String>, host: &str) {
    let h = host.trim();
    if h.is_empty() {
        return;
    }
    if !hosts.iter().any(|x| x.eq_ignore_ascii_case(h)) {
        hosts.push(h.to_string());
    }
}

fn allow_url_host(cfg: &mut CloudConfig, base_url: &str) {
    if let Ok(parsed) = url::Url::parse(base_url) {
        if let Some(host) = parsed.host_str() {
            push_host_unique(&mut cfg.allowed_hosts, host);
        }
    }
}

/// Load cloud config: package-time defaults → settings.json → env overrides.
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
                let base = cfg.base_url.clone();
                allow_url_host(&mut cfg, &base);
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
                // Keep package-time hosts so production CDN still works if settings
                // only lists a subset.
                for h in embedded_cloud_extra_hosts() {
                    push_host_unique(&mut list, &h);
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

    // Env overrides (dev / CI) — runtime only, never committed defaults
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
            let base = cfg.base_url.clone();
            allow_url_host(&mut cfg, &base);
        }
    }
    if let Ok(ch) = std::env::var("CODEX_SKIN_CLOUD_CHANNEL") {
        if !ch.trim().is_empty() {
            cfg.channel = ch.trim().to_string();
        }
    }

    cfg.base_url = normalize_base_url(&cfg.base_url);

    // Local-only mode: no cloud URL provided at startup → treat as dev mode, disable cloud catalog for skin list
    if cfg.base_url.is_empty() || cfg.base_url == DEFAULT_DEV_BASE_URL {
        cfg.enabled = false;
    }

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
    let parsed =
        url::Url::parse(url).map_err(|e| EngineError::msg(format!("无效下载 URL: {e}")))?;
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
        return Err(EngineError::msg(format!("下载 host 不在白名单: {host}")));
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(EngineError::msg("拒绝带认证信息的下载 URL"));
    }

    Ok(parsed)
}
