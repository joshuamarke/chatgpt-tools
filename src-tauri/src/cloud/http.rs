//! Minimal HTTP GET for cloud JSON / binary (ureq, allowlisted hosts).

use super::config::{validate_download_url, CloudConfig, MAX_PACKAGE_BYTES, MAX_REDIRECTS};
use crate::engine::EngineError;
use std::io::Read;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HttpTextResponse {
    pub body: Option<String>,
    pub etag: Option<String>,
    pub not_modified: bool,
}

#[derive(Debug, Clone)]
pub struct HttpBytesResponse {
    pub bytes: Vec<u8>,
    pub final_url: String,
}

fn agent(cfg: &CloudConfig) -> ureq::Agent {
    // No automatic redirects — we re-validate host on each hop.
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(cfg.timeout_ms.min(10_000)))
        .timeout_read(Duration::from_millis(cfg.timeout_ms))
        .redirects(0)
        .user_agent(&format!(
            "ChatGPTTools/{} (Windows; engine-protocol/{}; cloud-protocol/{})",
            cfg.app_version, cfg.engine_protocol, cfg.protocol
        ))
        .build()
}

fn apply_common_headers(req: ureq::Request, cfg: &CloudConfig) -> ureq::Request {
    req.set("Accept", "application/json, application/octet-stream, */*")
        .set("X-App-Version", &cfg.app_version)
        .set("X-Engine-Protocol", &cfg.engine_protocol.to_string())
        .set("X-Cloud-Protocol", &cfg.protocol.to_string())
}

fn is_redirect(code: u16) -> bool {
    matches!(code, 301 | 302 | 303 | 307 | 308)
}

/// GET JSON/text with optional If-None-Match. Manual redirect with host allowlist.
pub fn get_text(
    cfg: &CloudConfig,
    url: &str,
    if_none_match: Option<&str>,
) -> Result<HttpTextResponse, EngineError> {
    let mut current = validate_download_url(url, cfg)?.as_str().to_string();
    let ag = agent(cfg);

    for _hop in 0..=MAX_REDIRECTS {
        let mut req = apply_common_headers(ag.get(&current), cfg);
        if let Some(etag) = if_none_match {
            if !etag.is_empty() {
                req = req.set("If-None-Match", etag);
            }
        }
        match req.call() {
            Ok(resp) => {
                let status = resp.status();
                if is_redirect(status) {
                    let loc = resp
                        .header("location")
                        .ok_or_else(|| EngineError::msg("重定向缺少 Location"))?
                        .to_string();
                    current = resolve_redirect(&current, &loc, cfg)?;
                    continue;
                }
                let etag = resp.header("etag").map(|s| s.to_string());
                let mut body = String::new();
                resp.into_reader()
                    .take(8 * 1024 * 1024)
                    .read_to_string(&mut body)
                    .map_err(|e| EngineError::msg(format!("read body: {e}")))?;
                let _ = status;
                return Ok(HttpTextResponse {
                    body: Some(body),
                    etag,
                    not_modified: false,
                });
            }
            Err(ureq::Error::Status(304, resp)) => {
                let etag = resp.header("etag").map(|s| s.to_string());
                return Ok(HttpTextResponse {
                    body: None,
                    etag,
                    not_modified: true,
                });
            }
            Err(ureq::Error::Status(code, resp)) if is_redirect(code) => {
                let loc = resp
                    .header("location")
                    .ok_or_else(|| EngineError::msg("重定向缺少 Location"))?
                    .to_string();
                current = resolve_redirect(&current, &loc, cfg)?;
            }
            Err(ureq::Error::Status(code, resp)) => {
                let mut body = String::new();
                let _ = resp.into_reader().take(4096).read_to_string(&mut body);
                return Err(EngineError::msg(format!(
                    "HTTP {code}: {}",
                    body.chars().take(200).collect::<String>()
                )));
            }
            Err(e) => return Err(EngineError::msg(format!("请求失败: {e}"))),
        }
    }
    Err(EngineError::msg("重定向次数过多"))
}

/// Download binary with size cap; each redirect hop re-checked against allowlist.
pub fn get_bytes_allowlisted(
    cfg: &CloudConfig,
    url: &str,
    expected_size: Option<u64>,
) -> Result<HttpBytesResponse, EngineError> {
    let mut current = validate_download_url(url, cfg)?.as_str().to_string();

    if let Some(sz) = expected_size {
        if sz > MAX_PACKAGE_BYTES {
            return Err(EngineError::msg(format!(
                "声明体积 {} 超过硬限 {} 字节",
                sz, MAX_PACKAGE_BYTES
            )));
        }
    }

    let ag = agent(cfg);
    for _hop in 0..=MAX_REDIRECTS {
        let req = apply_common_headers(ag.get(&current), cfg)
            .set("Accept", "application/zip, application/octet-stream, */*");

        match req.call() {
            Ok(resp) => {
                let status = resp.status();
                if is_redirect(status) {
                    let loc = resp
                        .header("location")
                        .ok_or_else(|| EngineError::msg("重定向缺少 Location"))?
                        .to_string();
                    current = resolve_redirect(&current, &loc, cfg)?;
                    continue;
                }
                if !(200..300).contains(&status) {
                    return Err(EngineError::msg(format!("下载 HTTP {status}")));
                }

                if let Some(cl) = resp
                    .header("content-length")
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    if cl > MAX_PACKAGE_BYTES {
                        return Err(EngineError::msg(format!(
                            "Content-Length {cl} 超过硬限 {} 字节",
                            MAX_PACKAGE_BYTES
                        )));
                    }
                }

                let limit = expected_size
                    .filter(|s| *s > 0 && *s <= MAX_PACKAGE_BYTES)
                    .unwrap_or(MAX_PACKAGE_BYTES)
                    .saturating_add(1024);

                let mut buf = Vec::new();
                resp.into_reader()
                    .take(limit + 1)
                    .read_to_end(&mut buf)
                    .map_err(|e| EngineError::msg(format!("读取下载内容: {e}")))?;

                if buf.len() as u64 > MAX_PACKAGE_BYTES {
                    return Err(EngineError::msg(format!(
                        "下载超过硬限 {} 字节",
                        MAX_PACKAGE_BYTES
                    )));
                }
                if let Some(sz) = expected_size {
                    if sz > 0 && buf.len() as u64 != sz {
                        return Err(EngineError::msg(format!(
                            "体积不符：期望 {sz}，实际 {}",
                            buf.len()
                        )));
                    }
                }

                return Ok(HttpBytesResponse {
                    bytes: buf,
                    final_url: current,
                });
            }
            Err(ureq::Error::Status(code, resp)) if is_redirect(code) => {
                let loc = resp
                    .header("location")
                    .ok_or_else(|| EngineError::msg("重定向缺少 Location"))?
                    .to_string();
                current = resolve_redirect(&current, &loc, cfg)?;
            }
            Err(ureq::Error::Status(code, r)) => {
                let mut body = String::new();
                let _ = r.into_reader().take(1024).read_to_string(&mut body);
                return Err(EngineError::msg(format!(
                    "下载 HTTP {code}: {}",
                    body.chars().take(160).collect::<String>()
                )));
            }
            Err(e) => return Err(EngineError::msg(format!("下载失败: {e}"))),
        }
    }
    Err(EngineError::msg("重定向次数过多"))
}

fn resolve_redirect(current: &str, location: &str, cfg: &CloudConfig) -> Result<String, EngineError> {
    let base = url::Url::parse(current).map_err(|e| EngineError::msg(format!("URL: {e}")))?;
    let next = base
        .join(location)
        .map_err(|e| EngineError::msg(format!("重定向 URL: {e}")))?;
    let checked = validate_download_url(next.as_str(), cfg)?;
    Ok(checked.as_str().to_string())
}

/// Build URL under base + relative path (handles trailing slashes).
pub fn join_url(base: &str, rel: &str) -> String {
    let b = base.trim_end_matches('/');
    let r = rel.trim_start_matches('/');
    format!("{b}/{r}")
}
