//! Catalog preview thumbnails: disk cache + allowlisted fetch.
//!
//! WebView CSP only allows local `data:` / `asset:` images, so remote
//! `preview.url` from catalog must be downloaded in Rust and exposed as
//! `previewUrl` data-URLs. Package download remains separate (`download_skin`).

use super::cache::{ensure_cloud_layout, now_unix_ms, read_json, write_text_atomic};
use super::catalog::load_catalog_disk;
use super::config::{validate_download_url, CloudConfig, MAX_PREVIEW_BYTES};
use super::http::get_bytes_allowlisted_with_cap;
use crate::cdp;
use crate::engine::EngineError;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Soft cap on network fetches per `ensure_missing_previews` call.
/// Respects CDN heavy-bucket rate limits; GUI can re-request leftovers later.
const MAX_NETWORK_FETCHES_PER_PASS: usize = 6;

fn previews_root() -> PathBuf {
    cdp::native_state_root().join("cache").join("previews")
}

fn preview_dir(skin_id: &str) -> PathBuf {
    previews_root().join(skin_id)
}

fn meta_path(skin_id: &str) -> PathBuf {
    preview_dir(skin_id).join("meta.json")
}

fn image_path_for_ext(skin_id: &str, ext: &str) -> PathBuf {
    preview_dir(skin_id).join(format!("image.{ext}"))
}

/// Find cached image file under preview dir (any common extension).
fn find_image_file(skin_id: &str) -> Option<PathBuf> {
    let dir = preview_dir(skin_id);
    if !dir.is_dir() {
        return None;
    }
    for name in [
        "image.jpg",
        "image.jpeg",
        "image.png",
        "image.webp",
        "image.gif",
        "image.bin",
    ] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(entries) = fs::read_dir(&dir) {
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_file() {
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    if name.starts_with("image.") {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

fn mime_from_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
}

fn ext_from_url(url: &str) -> String {
    let path = url::Url::parse(url)
        .ok()
        .map(|u| u.path().to_string())
        .unwrap_or_else(|| url.to_string());
    let lower = path.to_ascii_lowercase();
    for ext in ["jpeg", "jpg", "png", "webp", "gif"] {
        if lower.ends_with(&format!(".{ext}")) {
            return if ext == "jpeg" {
                "jpg".into()
            } else {
                ext.into()
            };
        }
    }
    "jpg".into()
}

fn bytes_to_data_url(path: &Path, bytes: &[u8]) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let mime = mime_from_ext(ext);
    format!("data:{mime};base64,{}", B64.encode(bytes))
}

/// Read a previously cached preview as data-URL (no network).
pub fn load_cached_preview_data_url(skin_id: &str) -> Option<String> {
    let id = cdp::native_safe_skin_id(skin_id);
    if id.is_empty() {
        return None;
    }
    let path = find_image_file(&id)?;
    let bytes = fs::read(&path).ok()?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_PREVIEW_BYTES {
        return None;
    }
    Some(bytes_to_data_url(&path, &bytes))
}

/// Whether disk cache is still valid for the given catalog preview source URL.
fn cache_matches_url(skin_id: &str, source_url: &str) -> bool {
    if source_url.is_empty() {
        return false;
    }
    let Some(meta) = read_json(&meta_path(skin_id)) else {
        return false;
    };
    let cached = meta
        .get("sourceUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    !cached.is_empty() && cached == source_url && find_image_file(skin_id).is_some()
}

fn preview_urls_from_entry(preview: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(u) = preview.get("url").and_then(|v| v.as_str()) {
        if !u.trim().is_empty() {
            urls.push(u.trim().to_string());
        }
    }
    if let Some(mirrors) = preview.get("mirrors").and_then(|v| v.as_array()) {
        for m in mirrors {
            if let Some(u) = m.as_str() {
                let t = u.trim();
                if !t.is_empty() && !urls.iter().any(|x| x == t) {
                    urls.push(t.to_string());
                }
            }
        }
    }
    urls
}

/// Keep only host-allowlisted URLs; skip bad mirrors instead of failing the whole set.
fn filter_allowlisted_urls(cfg: &CloudConfig, urls: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for u in urls {
        if validate_download_url(u, cfg).is_ok() {
            out.push(u.clone());
        }
    }
    out
}

fn clear_old_images(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for ent in entries.flatten() {
            let p = ent.path();
            if p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("image."))
                .unwrap_or(false)
            {
                let _ = fs::remove_file(&p);
            }
        }
    }
}

/// Ensure one skin's catalog preview is on disk; return data-URL.
///
/// `allow_network`: when false, only return disk hit (used to stay under rate budget).
pub fn ensure_preview_cached(
    cfg: &CloudConfig,
    skin_id: &str,
    preview: &Value,
    allow_network: bool,
) -> Result<String, EngineError> {
    let id = cdp::native_safe_skin_id(skin_id);
    if id.is_empty() {
        return Err(EngineError::msg("无效皮肤 id"));
    }
    ensure_cloud_layout()?;

    let urls = preview_urls_from_entry(preview);
    if urls.is_empty() {
        return Err(EngineError::msg("catalog preview 无 url"));
    }
    let primary = urls[0].clone();

    // Fast path: valid disk cache for same primary URL
    if cache_matches_url(&id, &primary) {
        if let Some(data) = load_cached_preview_data_url(&id) {
            return Ok(data);
        }
    }

    if !allow_network {
        return Err(EngineError::msg("预览未缓存（本轮跳过网络）"));
    }

    let size = preview.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
    if size > MAX_PREVIEW_BYTES {
        return Err(EngineError::msg(format!(
            "preview.size 超过硬限 {} 字节",
            MAX_PREVIEW_BYTES
        )));
    }
    let expected = if size > 0 { Some(size) } else { None };

    let sha_expected = preview
        .get("sha256")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let check_sha = sha_expected.len() == 64
        && sha_expected.chars().all(|c| c.is_ascii_hexdigit())
        && !sha_expected.chars().all(|c| c == '0');

    let allowed = filter_allowlisted_urls(cfg, &urls);
    if allowed.is_empty() {
        return Err(EngineError::msg("preview url 均不在 host 白名单"));
    }

    let mut last_err = EngineError::msg("预览图下载失败");
    for u in &allowed {
        match get_bytes_allowlisted_with_cap(cfg, u, expected, MAX_PREVIEW_BYTES) {
            Ok(resp) => {
                if resp.bytes.is_empty() {
                    last_err = EngineError::msg("预览图为空");
                    continue;
                }
                if check_sha {
                    let mut h = Sha256::new();
                    h.update(&resp.bytes);
                    let got = hex::encode(h.finalize());
                    if got != sha_expected {
                        last_err = EngineError::msg("preview sha256 校验失败");
                        continue;
                    }
                }
                let ext = ext_from_url(u);
                let dir = preview_dir(&id);
                fs::create_dir_all(&dir)
                    .map_err(|e| EngineError::msg(format!("mkdir preview: {e}")))?;
                clear_old_images(&dir);
                let img_path = image_path_for_ext(&id, &ext);
                fs::write(&img_path, &resp.bytes)
                    .map_err(|e| EngineError::msg(format!("写预览图: {e}")))?;
                let meta = json!({
                    "skinId": id,
                    "sourceUrl": primary,
                    "finalUrl": resp.final_url,
                    "sha256": if check_sha { json!(sha_expected) } else { Value::Null },
                    "size": resp.bytes.len(),
                    "ext": ext,
                    "fetchedAt": now_unix_ms().to_string(),
                    "channel": cfg.channel,
                });
                let _ = write_text_atomic(
                    &meta_path(&id),
                    &format!(
                        "{}\n",
                        serde_json::to_string_pretty(&meta).unwrap_or_default()
                    ),
                );
                return Ok(bytes_to_data_url(&img_path, &resp.bytes));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// Fill `previewUrl` from disk cache for skins that still lack a local preview.
/// No network — safe to call on every `status()`.
pub fn attach_disk_previews(status: &mut Value) {
    let Some(skins) = status.get_mut("skins").and_then(|s| s.as_array_mut()) else {
        return;
    };
    for skin in skins.iter_mut() {
        let has_preview = skin
            .get("previewUrl")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if has_preview {
            continue;
        }
        let id = skin
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        if let Some(data) = load_cached_preview_data_url(&id) {
            if let Some(obj) = skin.as_object_mut() {
                obj.insert("previewUrl".into(), json!(data));
                obj.insert("previewKind".into(), json!("cloud-cache"));
            }
        }
    }
}

/// Attach catalog `remotePreviewUrl` / `remotePreview` onto status skins
/// (including local skins missing package assets, for ensure/fetch fallback).
pub fn attach_remote_preview_meta(status: &mut Value, catalog: Option<&Value>) {
    let Some(catalog) = catalog else {
        return;
    };
    let Some(remote_skins) = catalog.get("skins").and_then(|s| s.as_array()) else {
        return;
    };
    let Some(local_skins) = status.get_mut("skins").and_then(|s| s.as_array_mut()) else {
        return;
    };
    for skin in local_skins.iter_mut() {
        let id = skin
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        let Some(remote) = remote_skins.iter().find(|r| {
            r.get("id").and_then(|v| v.as_str()) == Some(id.as_str())
        }) else {
            continue;
        };
        let preview_url = remote
            .pointer("/preview/url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if preview_url.is_empty() {
            continue;
        }
        if let Some(obj) = skin.as_object_mut() {
            obj.insert("remotePreviewUrl".into(), json!(preview_url));
            if let Some(prev) = remote.get("preview").cloned() {
                obj.insert("remotePreview".into(), prev);
            }
        }
    }
}

fn catalog_preview_for_id(catalog: &Value, skin_id: &str) -> Option<Value> {
    catalog
        .get("skins")
        .and_then(|s| s.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(skin_id))
                .and_then(|s| s.get("preview").cloned())
        })
}

fn preview_has_url(prev: &Value) -> bool {
    prev.get("url")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// Fetch missing previews for catalog skins.
/// Prefer disk; network only on cache miss, capped per pass.
/// Returns data-URLs for GUI progressive fill.
pub fn ensure_missing_previews(cfg: &CloudConfig, skin_ids: Option<Vec<String>>) -> Value {
    if !cfg.enabled {
        return json!({
            "ok": true,
            "enabled": false,
            "previews": {},
            "fetched": 0,
            "cached": 0,
            "skipped": 0,
            "failed": [],
            "pending": [],
        });
    }
    let _ = ensure_cloud_layout();

    let catalog = match load_catalog_disk() {
        Some(c) => c,
        None => {
            return json!({
                "ok": false,
                "enabled": true,
                "previews": {},
                "fetched": 0,
                "cached": 0,
                "skipped": 0,
                "failed": [{ "id": "", "error": "无本地 catalog，无法解析 preview.url" }],
                "pending": [],
            });
        }
    };

    let mut targets: Vec<(String, Value)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(ids) = skin_ids {
        for raw in ids {
            let id = cdp::native_safe_skin_id(&raw);
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            if let Some(prev) = catalog_preview_for_id(&catalog, &id) {
                if preview_has_url(&prev) {
                    targets.push((id, prev));
                }
            }
        }
    } else if let Some(arr) = catalog.get("skins").and_then(|s| s.as_array()) {
        for remote in arr {
            let id = remote
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            if remote.get("status").and_then(|v| v.as_str()) == Some("deprecated") {
                continue;
            }
            let safe = cdp::native_safe_skin_id(&id);
            if safe.is_empty() || !seen.insert(safe.clone()) {
                continue;
            }
            if let Some(prev) = remote.get("preview").cloned() {
                if preview_has_url(&prev) {
                    targets.push((safe, prev));
                }
            }
        }
    }

    let mut previews = serde_json::Map::new();
    let mut fetched = 0u32;
    let mut cached = 0u32;
    let mut skipped = 0u32;
    let mut failed: Vec<Value> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut network_used = 0usize;

    for (id, preview) in targets {
        let primary = preview
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let had_valid_cache = cache_matches_url(&id, &primary);

        // Always try disk first without spending network budget.
        if had_valid_cache {
            match ensure_preview_cached(cfg, &id, &preview, false) {
                Ok(data_url) => {
                    cached += 1;
                    previews.insert(id, json!(data_url));
                    continue;
                }
                Err(_) => { /* fall through to network */ }
            }
        } else if let Some(data) = load_cached_preview_data_url(&id) {
            // File on disk but meta missing/stale: still show for UX.
            // If catalog primary URL changed, re-fetch when budget allows.
            if primary.is_empty() || cache_matches_url(&id, &primary) {
                cached += 1;
                previews.insert(id, json!(data));
                continue;
            }
            // URL drift: serve stale thumb now, still queue network refresh below.
            cached += 1;
            previews.insert(id.clone(), json!(data));
            // Fall through to refresh when budget remains; if budget exhausted, UI already has image.
        }

        if network_used >= MAX_NETWORK_FETCHES_PER_PASS {
            skipped += 1;
            pending.push(id);
            continue;
        }

        // Reaching here always attempts network (disk paths `continue` above).
        match ensure_preview_cached(cfg, &id, &preview, true) {
            Ok(data_url) => {
                fetched += 1;
                network_used += 1;
                previews.insert(id, json!(data_url));
            }
            Err(e) => {
                network_used += 1;
                failed.push(json!({
                    "id": id,
                    "error": e.to_string(),
                }));
            }
        }
    }

    json!({
        "ok": true,
        "enabled": true,
        "previews": previews,
        "fetched": fetched,
        "cached": cached,
        "skipped": skipped,
        "failed": failed,
        "pending": pending,
        "count": previews.len(),
        "networkBudget": MAX_NETWORK_FETCHES_PER_PASS,
    })
}

/// Remove preview cache for one skin or all (optional maintenance).
#[allow(dead_code)]
pub fn clear_preview_cache(skin_id: Option<&str>) -> Result<Value, EngineError> {
    let root = previews_root();
    if let Some(id) = skin_id {
        let safe = cdp::native_safe_skin_id(id);
        if safe.is_empty() {
            return Err(EngineError::msg("无效皮肤 id"));
        }
        let dir = root.join(&safe);
        if dir.is_dir() {
            fs::remove_dir_all(&dir)
                .map_err(|e| EngineError::msg(format!("删除预览缓存: {e}")))?;
            return Ok(json!({ "ok": true, "removed": [safe] }));
        }
        return Ok(json!({ "ok": true, "removed": [] }));
    }
    let mut removed = Vec::new();
    if root.is_dir() {
        if let Ok(entries) = fs::read_dir(&root) {
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
    }
    Ok(json!({ "ok": true, "removed": removed }))
}
