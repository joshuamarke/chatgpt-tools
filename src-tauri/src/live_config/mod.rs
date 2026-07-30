//! Shared live config file lock + atomic write for Codex / Grok.
//!
//! Skin theme, provider `write_live`, and proxy takeover all touch host config
//! files. This module serializes writers in-process and provides CAS-style
//! atomic replace so concurrent full-file writes do not clobber each other.
//!
//! **Locking rule:** never call `with_live_lock` / `read_modify_write` /
//! `write_text` from inside another `with_live_lock` on the same path
//! (`std::sync::Mutex` is not reentrant). Use bare `atomic_write_text` when
//! the caller already holds the lock.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

static LOCKS: Lazy<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn path_mutex(path: &Path) -> Arc<Mutex<()>> {
    let key = path_key(path);
    let mut map = LOCKS.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Run `f` while holding the exclusive lock for `path`.
/// Retries a few times if the lock is busy.
pub fn with_live_lock<T, F>(path: &Path, f: F) -> Result<T, String>
where
    F: FnOnce(&Path) -> Result<T, String>,
{
    let arc = path_mutex(path);
    for attempt in 0..10 {
        match arc.try_lock() {
            Ok(_guard) => return f(path),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(35 + attempt as u64 * 25));
            }
            Err(std::sync::TryLockError::Poisoned(p)) => {
                let _guard = p.into_inner();
                return f(path);
            }
        }
    }
    // Last resort: block (should be rare).
    let _guard = arc
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    f(path)
}

/// Atomic write with fsync. Optionally refuse if file changed since `original`.
pub fn atomic_write_text(
    path: &Path,
    text: &str,
    original: Option<&[u8]>,
) -> Result<(), String> {
    if let Some(orig) = original {
        if path.exists() {
            let now = fs::read(path).map_err(|e| format!("读取配置失败: {e}"))?;
            if strip_bom(&now) != strip_bom(orig) {
                return Err(
                    "配置文件在写入前被其他操作修改，已中止以避免覆盖（请重试）".into(),
                );
            }
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let tmp = path.with_extension(format!(
        "{}.chatgpt-tools.{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("bak"),
        std::process::id()
    ));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {e}"))?;
        f.write_all(text.as_bytes())
            .map_err(|e| format!("写入临时文件失败: {e}"))?;
        f.sync_all()
            .map_err(|e| format!("同步临时文件失败: {e}"))?;
    }
    if let Some(orig) = original {
        if path.exists() {
            let now = fs::read(path).unwrap_or_default();
            if strip_bom(&now) != strip_bom(orig) {
                let _ = fs::remove_file(&tmp);
                return Err("配置文件在替换前被修改，已中止（请重试）".into());
            }
        }
    }
    // On Windows, replace existing target via rename-over when possible.
    #[cfg(windows)]
    {
        if path.exists() {
            let bak = path.with_extension(format!(
                "{}.chatgpt-tools.{}.bak",
                path.extension().and_then(|e| e.to_str()).unwrap_or("bak"),
                std::process::id()
            ));
            if let Err(e) = fs::rename(path, &bak) {
                let _ = fs::remove_file(&tmp);
                return Err(format!("备份旧配置失败: {e}"));
            }
            if let Err(e) = fs::rename(&tmp, path) {
                let _ = fs::rename(&bak, path);
                let _ = fs::remove_file(&tmp);
                return Err(format!("替换配置文件失败: {e}"));
            }
            let _ = fs::remove_file(&bak);
            return Ok(());
        }
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("替换配置文件失败: {e}")
    })
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

/// Lock + read + transform + atomic write for a text config.
pub fn read_modify_write<F>(path: &Path, modify: F) -> Result<(), String>
where
    F: FnOnce(&str) -> Result<String, String>,
{
    with_live_lock(path, |p| {
        let original = if p.exists() {
            fs::read(p).map_err(|e| format!("读取失败: {e}"))?
        } else {
            Vec::new()
        };
        let text = if original.is_empty() {
            String::new()
        } else {
            String::from_utf8(original.clone())
                .map_err(|_| "配置文件不是合法 UTF-8".to_string())?
        };
        let next = modify(&text)?;
        if next == text {
            return Ok(());
        }
        atomic_write_text(p, &next, Some(&original))
    })
}

/// Locked full replace of text (reads original for CAS).
pub fn write_text(path: &Path, text: &str) -> Result<(), String> {
    with_live_lock(path, |p| {
        let original = if p.exists() {
            fs::read(p).ok()
        } else {
            None
        };
        atomic_write_text(p, text, original.as_deref())
    })
}

// ── Port helpers (local routing listen port) ───────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortCheckResult {
    pub host: String,
    pub port: u16,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_port: Option<u16>,
    pub message: String,
}

fn host_equiv(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let t = s.trim().to_ascii_lowercase();
        match t.as_str() {
            "localhost" | "127.0.0.1" | "::1" | "[::1]" => "loopback".to_string(),
            "0.0.0.0" | "::" => "any".to_string(),
            other => other.to_string(),
        }
    };
    let na = norm(a);
    let nb = norm(b);
    na == nb
        || na == "any"
        || nb == "any"
        || (na == "loopback" && nb == "loopback")
}

/// True when our local-routing proxy already owns this listen endpoint.
fn port_owned_by_our_proxy(host: &str, port: u16) -> bool {
    let rt = crate::proxy::runtime();
    if !rt.is_running() {
        // Also treat configured listen port as "ours" when status matches config
        // even if runtime flag races — prefer live status.
        return false;
    }
    let st = rt.status_snapshot();
    st.port == port && host_equiv(host, &st.address)
}

/// Check whether `host:port` can be bound (and suggest a free port nearby).
///
/// If **this app's** local-routing proxy is already listening on the same
/// endpoint, report **available** (in use by us — not a foreign conflict).
pub fn check_listen_port(host: &str, port: u16) -> PortCheckResult {
    let host = host.trim();
    let host = if host.is_empty() { "127.0.0.1" } else { host };
    if port < 1024 {
        return PortCheckResult {
            host: host.into(),
            port,
            available: false,
            suggested_port: find_free_port(host, 18964),
            message: "端口需 ≥ 1024".into(),
        };
    }

    if port_owned_by_our_proxy(host, port) {
        return PortCheckResult {
            host: host.into(),
            port,
            available: true,
            suggested_port: None,
            message: format!(
                "{host}:{port} 当前由本机路由占用（可继续使用；改端口需先关闭本地路由）"
            ),
        };
    }

    // Configured port while proxy reports same port (status address may lag).
    if let Ok(file) = crate::providers::store::load() {
        if crate::proxy::runtime().is_running()
            && file.proxy.listen_port == port
            && host_equiv(host, &file.proxy.listen_address)
        {
            return PortCheckResult {
                host: host.into(),
                port,
                available: true,
                suggested_port: None,
                message: format!("{host}:{port} 为当前路由监听端口（本进程）"),
            };
        }
    }

    let addr = format!("{host}:{port}");
    match TcpListener::bind(&addr) {
        Ok(listener) => {
            drop(listener);
            PortCheckResult {
                host: host.into(),
                port,
                available: true,
                suggested_port: None,
                message: format!("{addr} 可用"),
            }
        }
        Err(e) => {
            let suggested = find_free_port(host, port.saturating_add(1).max(18964));
            PortCheckResult {
                host: host.into(),
                port,
                available: false,
                suggested_port: suggested,
                message: format!("{addr} 已被其他程序占用: {e}"),
            }
        }
    }
}

fn find_free_port(host: &str, start: u16) -> Option<u16> {
    let start = start.max(1024);
    for p in start..start.saturating_add(200).min(65535) {
        let addr = format!("{host}:{p}");
        if TcpListener::bind(&addr).is_ok() {
            return Some(p);
        }
    }
    // Try high ephemeral range
    for p in (20000..30000).step_by(7) {
        let addr = format!("{host}:{p}");
        if TcpListener::bind(&addr).is_ok() {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn atomic_write_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "chatgpt-tools-live-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cfg.toml");
        write_text(&path, "a = 1\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 1\n");
        read_modify_write(&path, |t| Ok(format!("{t}b = 2\n"))).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("a = 1"));
        assert!(text.contains("b = 2"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cas_rejects_stale_original() {
        let dir = std::env::temp_dir().join(format!(
            "chatgpt-tools-live-cas-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cfg.toml");
        fs::write(&path, b"old\n").unwrap();
        let err = atomic_write_text(&path, "new\n", Some(b"stale\n")).unwrap_err();
        assert!(err.contains("修改") || err.contains("中止"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn port_check_localhost() {
        // port 0 is invalid for our check (< 1024 → structured failure + suggestion)
        let low = check_listen_port("127.0.0.1", 0);
        assert!(!low.available);
        // high port: may or may not be free; just ensure API returns structured result
        let r = check_listen_port("127.0.0.1", 58421);
        assert_eq!(r.port, 58421);
    }
}
