//! Disk layout for cloud JSON caches and installed skin packages.

use crate::cdp;
use crate::engine::EngineError;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn cloud_root() -> PathBuf {
    cdp::native_state_root().join("cloud")
}

pub fn cache_skins_dir() -> PathBuf {
    cdp::native_state_root().join("cache").join("skins")
}

pub fn cache_tmp_dir() -> PathBuf {
    cdp::native_state_root().join("cache").join("tmp")
}

pub fn ensure_cloud_layout() -> Result<(), EngineError> {
    for d in [
        cloud_root(),
        cache_skins_dir(),
        cache_tmp_dir(),
        // Catalog screenshot thumbnails (independent of full .skin package cache)
        cdp::native_state_root().join("cache").join("previews"),
        // Contact / about images (QR etc.) rewritten to data-URLs for CSP
        cdp::native_state_root().join("cache").join("about-images"),
        cloud_root().join("meta"),
    ] {
        fs::create_dir_all(&d)
            .map_err(|e| EngineError::msg(format!("create {}: {e}", d.display())))?;
    }
    Ok(())
}

pub fn catalog_path() -> PathBuf {
    cloud_root().join("catalog.json")
}

pub fn catalog_etag_path() -> PathBuf {
    cloud_root().join("catalog.etag")
}

pub fn announcements_path() -> PathBuf {
    cloud_root().join("announcements.json")
}

pub fn announcements_etag_path() -> PathBuf {
    cloud_root().join("announcements.etag")
}

/// About / contact (independent of version.json).
pub fn about_path() -> PathBuf {
    cloud_root().join("about.json")
}

pub fn about_etag_path() -> PathBuf {
    cloud_root().join("about.etag")
}

/// Last successful network sync timestamp (unix seconds, plain text).
pub fn last_sync_path() -> PathBuf {
    cloud_root().join("meta").join("last-network-sync.txt")
}

pub fn read_last_sync_unix() -> Option<u64> {
    read_text(&last_sync_path())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&t| t > 0)
}

pub fn write_last_sync_now() -> Result<(), EngineError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    write_text_atomic(&last_sync_path(), &format!("{now}\n"))
}

pub fn catalog_mtime_secs() -> Option<u64> {
    fs::metadata(catalog_path())
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

pub fn read_state_path() -> PathBuf {
    cloud_root().join("read-state.json")
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), EngineError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| EngineError::msg(format!("mkdir {}: {e}", parent.display())))?;
    }
    let tmp = path.with_extension(format!(
        "tmp.{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    fs::write(&tmp, bytes).map_err(|e| EngineError::msg(format!("write temp: {e}")))?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        EngineError::msg(format!("rename {}: {e}", path.display()))
    })
}

pub fn write_text_atomic(path: &Path, text: &str) -> Result<(), EngineError> {
    write_atomic(path, text.as_bytes())
}

pub fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

pub fn read_json(path: &Path) -> Option<Value> {
    let t = read_text(path)?;
    serde_json::from_str(&t).ok()
}

pub fn read_etag(path: &Path) -> Option<String> {
    read_text(path).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn write_etag(path: &Path, etag: &str) -> Result<(), EngineError> {
    write_text_atomic(path, &format!("{}\n", etag.trim()))
}

/// Read `.cache-meta.json` for a cached skin id.
pub fn read_cache_meta(skin_id: &str) -> Option<Value> {
    let id = cdp::native_safe_skin_id(skin_id);
    if id.is_empty() {
        return None;
    }
    read_json(&cache_skins_dir().join(&id).join(".cache-meta.json"))
}

/// List skins installed under cache/skins (source=cache).
pub fn list_cached_skins() -> Vec<Value> {
    let root = cache_skins_dir();
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("skin.json");
        if !manifest_path.is_file() {
            continue;
        }
        let Ok(text) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(mut manifest) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if manifest.get("id").and_then(|v| v.as_str()).is_none() {
            if let Some(obj) = manifest.as_object_mut() {
                obj.insert(
                    "id".into(),
                    json!(path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()),
                );
            }
        }
        let meta = read_json(&path.join(".cache-meta.json"));
        if let Some(obj) = manifest.as_object_mut() {
            obj.insert("dir".into(), json!(path.to_string_lossy()));
            obj.insert("source".into(), json!("cache"));
            obj.insert("builtin".into(), json!(false));
            obj.insert("installState".into(), json!("ready"));
            if let Some(m) = meta {
                if let Some(v) = m.get("version").cloned() {
                    obj.insert("cacheVersion".into(), v);
                }
                if let Some(v) = m.get("sha256").cloned() {
                    obj.insert("cacheSha256".into(), v);
                }
            }
        }
        out.push(manifest);
    }
    out
}

/// Remove one cached skin or all if `skin_id` is None.
pub fn clear_skin_cache(skin_id: Option<&str>) -> Result<Value, EngineError> {
    ensure_cloud_layout()?;
    if let Some(id) = skin_id {
        let safe = cdp::native_safe_skin_id(id);
        if safe.is_empty() {
            return Err(EngineError::msg("无效皮肤 id"));
        }
        let dir = cache_skins_dir().join(&safe);
        if dir.is_dir() {
            fs::remove_dir_all(&dir)
                .map_err(|e| EngineError::msg(format!("删除缓存: {e}")))?;
            return Ok(json!({ "ok": true, "removed": [safe] }));
        }
        return Ok(json!({ "ok": true, "removed": [] }));
    }
    let mut removed = Vec::new();
    if let Ok(entries) = fs::read_dir(cache_skins_dir()) {
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_dir() {
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let _ = fs::remove_dir_all(&p);
                removed.push(name);
            }
        }
    }
    // also wipe tmp leftovers
    if let Ok(entries) = fs::read_dir(cache_tmp_dir()) {
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_file() || p.is_dir() {
                if p.is_dir() {
                    let _ = fs::remove_dir_all(&p);
                } else {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
    Ok(json!({ "ok": true, "removed": removed }))
}

pub fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
