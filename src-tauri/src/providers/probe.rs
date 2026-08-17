//! Provider connectivity probe + OpenAI-compatible model list fetch.
//!
//! - Lightweight base_url reachability
//! - GET /v1/models with candidate URL fallbacks
//!
//! Uses `ureq` (same HTTP stack as cloud/CDP) — no extra deps.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::time::{Duration, Instant};


const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 8;
const MIN_CONNECT_TIMEOUT_SECS: u64 = 2;
const MAX_CONNECT_TIMEOUT_SECS: u64 = 30;
/// Reachable but TTFB above this → "degraded" (ms).
const DEGRADED_THRESHOLD_MS: u64 = 6000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Operational,
    Degraded,
    Failed,
}

/// Result of a lightweight base_url reachability check.
///
/// Any HTTP response (including 4xx/5xx) counts as reachable — only DNS /
/// connect / TLS / timeout failures count as unreachable. This does **not**
/// validate auth or model availability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityResult {
    pub success: bool,
    pub status: HealthStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    /// Echo of the URL that was probed (normalized).
    pub url: String,
}

fn sanitize_timeout(timeout_secs: Option<u64>) -> u64 {
    timeout_secs
        .unwrap_or(DEFAULT_CONNECT_TIMEOUT_SECS)
        .clamp(MIN_CONNECT_TIMEOUT_SECS, MAX_CONNECT_TIMEOUT_SECS)
}

fn normalize_probe_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err("Base URL 不能为空".into());
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("Base URL 必须以 http:// 或 https:// 开头".into());
    }
    // Basic parse check
    url::Url::parse(&trimmed).map_err(|e| format!("Base URL 无效: {e}"))?;
    Ok(trimmed)
}

/// Probe base_url reachability (GET, any HTTP status = success).
/// Optional `custom_user_agent` is applied when providers require a specific UA.
pub fn test_connectivity_with_ua(
    base_url: &str,
    timeout_secs: Option<u64>,
    custom_user_agent: Option<&str>,
) -> ConnectivityResult {
    let url = match normalize_probe_url(base_url) {
        Ok(u) => u,
        Err(msg) => {
            return ConnectivityResult {
                success: false,
                status: HealthStatus::Failed,
                message: msg,
                response_time_ms: None,
                http_status: None,
                url: base_url.trim().to_string(),
            };
        }
    };

    let timeout = Duration::from_secs(sanitize_timeout(timeout_secs));
    let ua = sanitize_user_agent(custom_user_agent)
        .unwrap_or_else(|| "ChatGPTTools/provider-probe".into());
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout.min(Duration::from_secs(10)))
        .timeout_read(timeout)
        .redirects(3)
        .user_agent(&ua)
        .build();

    let start = Instant::now();
    match agent
        .get(&url)
        .set("Accept", "*/*")
        .set("Accept-Encoding", "identity")
        .call()
    {
        Ok(resp) => {
            let ms = start.elapsed().as_millis() as u64;
            let code = resp.status();
            // Drain a tiny bit of body so the connection closes cleanly; ignore content.
            let _ = resp.into_reader().take(256).read_to_end(&mut Vec::new());
            let status = if ms >= DEGRADED_THRESHOLD_MS {
                HealthStatus::Degraded
            } else {
                HealthStatus::Operational
            };
            let label = if status == HealthStatus::Degraded {
                "可达（较慢）"
            } else {
                "可达"
            };
            ConnectivityResult {
                success: true,
                status,
                message: format!("{label} · HTTP {code} · {ms} ms"),
                response_time_ms: Some(ms),
                http_status: Some(code),
                url,
            }
        }
        Err(ureq::Error::Status(code, resp)) => {
            // ureq returns Status for non-2xx — still reachable.
            let ms = start.elapsed().as_millis() as u64;
            let _ = resp.into_reader().take(256).read_to_end(&mut Vec::new());
            let status = if ms >= DEGRADED_THRESHOLD_MS {
                HealthStatus::Degraded
            } else {
                HealthStatus::Operational
            };
            let label = if status == HealthStatus::Degraded {
                "可达（较慢）"
            } else {
                "可达"
            };
            ConnectivityResult {
                success: true,
                status,
                message: format!("{label} · HTTP {code} · {ms} ms（任意 HTTP 响应即视为可达）"),
                response_time_ms: Some(ms),
                http_status: Some(code),
                url,
            }
        }
        Err(e) => {
            let ms = start.elapsed().as_millis() as u64;
            let msg = map_network_error(&e);
            ConnectivityResult {
                success: false,
                status: HealthStatus::Failed,
                message: msg,
                response_time_ms: Some(ms),
                http_status: None,
                url,
            }
        }
    }
}

fn map_network_error(err: &ureq::Error) -> String {
    let s = err.to_string();
    let lower = s.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "请求超时：主机无响应或网络过慢".into()
    } else if lower.contains("dns") || lower.contains("name resolution") || lower.contains("resolve")
    {
        "DNS 解析失败：请检查域名是否正确".into()
    } else if lower.contains("connection refused") || lower.contains("actively refused") {
        "连接被拒绝：端口未开放或服务未启动".into()
    } else if lower.contains("certificate") || lower.contains("ssl") || lower.contains("tls") {
        format!("TLS/证书错误: {s}")
    } else if lower.contains("connect") {
        format!("连接失败: {s}")
    } else {
        format!("网络错误: {s}")
    }
}


const FETCH_TIMEOUT_SECS: u64 = 15;
const ERROR_BODY_MAX_CHARS: usize = 512;

/// Known Anthropic-compat path suffixes. Longer first so longest match wins.
const KNOWN_COMPAT_SUFFIXES: &[&str] = &[
    "/api/claudecode",
    "/api/anthropic",
    "/apps/anthropic",
    "/api/coding",
    "/claudecode",
    "/anthropic",
    "/step_plan",
    "/coding",
    "/claude",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedModel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Option<Vec<ModelEntry>>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    owned_by: Option<String>,
}

/// Reject control characters in custom User-Agent strings.
pub fn sanitize_user_agent(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    if s.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(s.to_string())
}

/// Fetch available models via OpenAI-compatible `GET …/models` candidates.
pub fn fetch_models_with_ua(
    base_url: &str,
    api_key: &str,
    models_url_override: Option<&str>,
    custom_user_agent: Option<&str>,
) -> Result<Vec<FetchedModel>, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("拉取模型需要 API Key".into());
    }

    let candidates = build_models_url_candidates(base_url, models_url_override)?;
    let timeout = Duration::from_secs(FETCH_TIMEOUT_SECS);
    let ua = sanitize_user_agent(custom_user_agent)
        .unwrap_or_else(|| "ChatGPTTools/model-fetch".into());
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(timeout)
        .redirects(3)
        .user_agent(&ua)
        .build();

    let mut last_err: Option<String> = None;

    for url in &candidates {
        let result = agent
            .get(url)
            .set("Authorization", &format!("Bearer {key}"))
            .set("Accept", "application/json")
            .call();

        match result {
            Ok(resp) => {
                let status = resp.status();
                let mut body = String::new();
                resp.into_reader()
                    .take(2 * 1024 * 1024)
                    .read_to_string(&mut body)
                    .map_err(|e| format!("读取响应失败: {e}"))?;

                if (200..300).contains(&status) {
                    return parse_models_body(&body);
                }
                if status == 404 || status == 405 {
                    last_err = Some(format!("HTTP {status}: {}", truncate_body(&body)));
                    continue;
                }
                return Err(format!("HTTP {status}: {}", truncate_body(&body)));
            }
            Err(ureq::Error::Status(status, resp)) => {
                let mut body = String::new();
                let _ = resp.into_reader().take(8192).read_to_string(&mut body);
                if status == 404 || status == 405 {
                    last_err = Some(format!("HTTP {status}: {}", truncate_body(&body)));
                    continue;
                }
                if status == 401 || status == 403 {
                    return Err(format!(
                        "鉴权失败 (HTTP {status})：请检查 API Key 是否正确。{}",
                        truncate_body(&body)
                    ));
                }
                return Err(format!("HTTP {status}: {}", truncate_body(&body)));
            }
            Err(e) => {
                // Transport errors (timeout/DNS) fail the whole attempt — no point trying
                // other path candidates on the same host when the host is unreachable.
                return Err(map_network_error(&e));
            }
        }
    }

    Err(format!(
        "所有候选端点均失败: {}",
        last_err.unwrap_or_else(|| "无候选".into())
    ))
}

fn parse_models_body(body: &str) -> Result<Vec<FetchedModel>, String> {
    let resp: ModelsResponse =
        serde_json::from_str(body).map_err(|e| format!("解析模型列表失败: {e}"))?;
    let mut models: Vec<FetchedModel> = resp
        .data
        .unwrap_or_default()
        .into_iter()
        .filter(|m| !m.id.trim().is_empty())
        .map(|m| FetchedModel {
            id: m.id,
            owned_by: m.owned_by,
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

fn truncate_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= ERROR_BODY_MAX_CHARS {
        trimmed.to_string()
    } else {
        let mut s: String = trimmed.chars().take(ERROR_BODY_MAX_CHARS).collect();
        s.push('…');
        s
    }
}

/// Build candidate model-list endpoints (order preserved, deduped).
///
/// 1. non-empty `models_url_override` → only that URL  
/// 2. if base ends with `/v{N}` → `{base}/models` (+ `/v1/models` fallback when not `/v1`)  
/// 3. else `{base}/v1/models`  
/// 4. if base hits a known Anthropic-compat suffix, also try stripped root + `/v1/models` / `/models`
pub fn build_models_url_candidates(
    base_url: &str,
    models_url_override: Option<&str>,
) -> Result<Vec<String>, String> {
    if let Some(raw) = models_url_override {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(vec![trimmed.to_string()]);
        }
    }

    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Base URL 不能为空".into());
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("Base URL 必须以 http:// 或 https:// 开头".into());
    }

    let mut candidates: Vec<String> = Vec::new();

    if ends_with_version_segment(trimmed) {
        candidates.push(format!("{trimmed}/models"));
        if !trimmed.ends_with("/v1") {
            candidates.push(format!("{trimmed}/v1/models"));
        }
    } else {
        candidates.push(format!("{trimmed}/v1/models"));
    }

    if let Some(stripped) = strip_compat_suffix(trimmed) {
        let root = stripped.trim_end_matches('/');
        if !root.is_empty() && root.contains("://") {
            candidates.push(format!("{root}/v1/models"));
            candidates.push(format!("{root}/models"));
        }
    }

    let mut unique: Vec<String> = Vec::with_capacity(candidates.len());
    for url in candidates {
        if !unique.iter().any(|u| u == &url) {
            unique.push(url);
        }
    }
    Ok(unique)
}

fn strip_compat_suffix(base_url: &str) -> Option<&str> {
    for suffix in KNOWN_COMPAT_SUFFIXES {
        if base_url.ends_with(*suffix) {
            return Some(&base_url[..base_url.len() - suffix.len()]);
        }
    }
    None
}

fn ends_with_version_segment(url: &str) -> bool {
    let last = url.rsplit('/').next().unwrap_or("");
    last.strip_prefix('v')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}
