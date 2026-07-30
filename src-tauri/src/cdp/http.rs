//! Loopback HTTP helpers for Chromium `/json/*` endpoints.

use serde_json::Value;
use std::time::Duration;
use thiserror::Error;
use url::Url;

const LOOPBACK_HOSTS: &[&str] = &["127.0.0.1", "localhost", "[::1]", "::1"];

#[derive(Debug, Error)]
pub enum CdpHttpError {
    #[error("{0}")]
    Message(String),
}

impl CdpHttpError {
    fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BrowserIdentity {
    pub browser_id: String,
    pub web_socket_debugger_url: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CdpTarget {
    pub id: String,
    pub url: String,
    pub web_socket_debugger_url: String,
    pub target_type: String,
}

/// GET `http://127.0.0.1:{port}{path}` and parse JSON.
pub fn fetch_json(port: u16, path: &str, timeout_ms: u64) -> Result<Value, CdpHttpError> {
    if !(1024..=65535).contains(&port) {
        return Err(CdpHttpError::msg(format!("invalid CDP port {port}")));
    }
    let pathname = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let url = format!("http://127.0.0.1:{port}{pathname}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(timeout_ms.min(3000)))
        .timeout_read(Duration::from_millis(timeout_ms))
        .timeout_write(Duration::from_millis(timeout_ms.min(3000)))
        .build();
    let resp = agent
        .get(&url)
        .call()
        .map_err(|e| CdpHttpError::msg(format!("CDP HTTP {pathname}: {e}")))?;
    if !(200..300).contains(&resp.status()) {
        return Err(CdpHttpError::msg(format!(
            "CDP HTTP {pathname} status {}",
            resp.status()
        )));
    }
    resp.into_json::<Value>()
        .map_err(|e| CdpHttpError::msg(format!("CDP JSON {pathname}: {e}")))
}

pub fn is_debug_port_open(port: u16, timeout_ms: u64) -> bool {
    fetch_json(port, "/json/version", timeout_ms).is_ok()
}

fn browser_id_from_ws_url(ws: &str, port: u16) -> Result<String, CdpHttpError> {
    let url = Url::parse(ws).map_err(|_| CdpHttpError::msg("invalid browser WS URL"))?;
    if url.scheme() != "ws" && url.scheme() != "wss" {
        return Err(CdpHttpError::msg("debugger URL must be ws/wss"));
    }
    let host = url.host_str().unwrap_or("");
    if !LOOPBACK_HOSTS.contains(&host) {
        return Err(CdpHttpError::msg(format!(
            "debugger host is not loopback: {host}"
        )));
    }
    let url_port = url.port().unwrap_or(if url.scheme() == "wss" { 443 } else { 80 });
    if url_port != port {
        return Err(CdpHttpError::msg(format!(
            "debugger port mismatch: expected {port}, got {url_port}"
        )));
    }
    let path = url.path();
    let prefix = "/devtools/browser/";
    if let Some(rest) = path.strip_prefix(prefix) {
        if !rest.is_empty()
            && rest.len() <= 200
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            return Ok(rest.to_string());
        }
    }
    Err(CdpHttpError::msg(format!(
        "rejected invalid CDP browser identity URL: {path}"
    )))
}

pub fn read_browser_identity(port: u16) -> Result<BrowserIdentity, CdpHttpError> {
    let version = fetch_json(port, "/json/version", 2500)?;
    let ws = version
        .get("webSocketDebuggerUrl")
        .or_else(|| version.get("webSocketDebuggerUrl"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let Some(ref ws_url) = ws else {
        return Err(CdpHttpError::msg("CDP version missing webSocketDebuggerUrl"));
    };
    let browser_id = browser_id_from_ws_url(ws_url, port)?;
    Ok(BrowserIdentity {
        browser_id,
        web_socket_debugger_url: ws,
    })
}

/// Rank app:// targets so the main shell is first.
///
/// Newer Codex builds also expose aux pages (e.g. `?initialRoute=/avatar-overlay`)
/// that share `app://` but never host the full chat UI. Probing/injecting the first
/// CDP entry used to miss the main window and fail cold apply with「宿主尚未就绪」.
fn app_target_rank(url: &str) -> u8 {
    let lower = url.to_ascii_lowercase();
    if lower.contains("avatar-overlay")
        || lower.contains("initialroute=%2favatar")
        || lower.contains("initialroute=/avatar")
    {
        return 90;
    }
    // Other compact / overlay routes (mascot, mini windows).
    if lower.contains("compact")
        || lower.contains("overlay")
        || lower.contains("initialroute=")
    {
        return 50;
    }
    // Query-less main shell (`app://-/index.html`) ranks highest.
    if !lower.contains('?') {
        return 0;
    }
    20
}

pub fn list_app_targets(port: u16) -> Result<Vec<CdpTarget>, CdpHttpError> {
    let list = fetch_json(port, "/json/list", 3000)?;
    let arr = list
        .as_array()
        .ok_or_else(|| CdpHttpError::msg("Invalid /json/list payload"))?;
    let mut out = Vec::new();
    for item in arr {
        let ty = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if ty != "page" {
            continue;
        }
        let url = item
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !url.starts_with("app://") {
            continue;
        }
        let ws = item
            .get("webSocketDebuggerUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if ws.is_empty() {
            continue;
        }
        // Enforce loopback on page WS URLs
        if let Ok(parsed) = Url::parse(&ws) {
            let host = parsed.host_str().unwrap_or("");
            if !LOOPBACK_HOSTS.contains(&host) {
                continue;
            }
        } else {
            continue;
        }
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(CdpTarget {
            id,
            url,
            web_socket_debugger_url: ws,
            target_type: ty,
        });
    }
    // Stable sort: primary shell first, aux windows last.
    out.sort_by(|a, b| {
        app_target_rank(&a.url)
            .cmp(&app_target_rank(&b.url))
            .then_with(|| a.url.cmp(&b.url))
    });
    Ok(out)
}

pub fn is_renderer_ready(port: u16) -> bool {
    list_app_targets(port).map(|t| !t.is_empty()).unwrap_or(false)
}

/// Soft wait until at least one app:// target exists.
pub fn wait_for_app_targets(
    port: u16,
    timeout_ms: u64,
) -> Result<Vec<CdpTarget>, CdpHttpError> {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    let mut last = CdpHttpError::msg("No app:// page targets");
    while std::time::Instant::now() < deadline {
        match list_app_targets(port) {
            Ok(list) if !list.is_empty() => return Ok(list),
            Ok(_) => last = CdpHttpError::msg("No app:// page targets"),
            Err(e) => last = e,
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Err(CdpHttpError::msg(format!(
        "No Codex renderer target on 127.0.0.1:{port}: {last}"
    )))
}
