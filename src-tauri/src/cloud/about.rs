//! About / contact info from CDN (`/v1/about.json`).
//! Independent of app version check (`version.json`).
//!
//! Contact images (QR / banners) are remote `https://…` URLs in about.json, but
//! WebView CSP only allows `data:` / `asset:` images — same constraint as catalog
//! previews. We download allowlisted images in Rust, cache under
//! `cache/about-images/`, and rewrite field values / HTML `<img src>` to data-URLs
//! before handing JSON to the GUI.

use super::cache::{
    about_etag_path, about_path, ensure_cloud_layout, read_etag, read_json, write_etag,
    write_text_atomic,
};
use super::config::{validate_download_url, CloudConfig, MAX_PREVIEW_BYTES};
use super::http::{get_bytes_allowlisted_with_cap, get_text, join_url};
use crate::cdp;
use crate::engine::EngineError;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

/// Fetch about.json from CDN and cache to disk (original remote image URLs kept).
/// UI payload always goes through [`materialize_contact_images`].
pub fn refresh_about(cfg: &CloudConfig) -> Result<Value, EngineError> {
    if !cfg.enabled {
        return Err(EngineError::msg("云端已关闭"));
    }
    ensure_cloud_layout()?;
    let etag = read_etag(&about_etag_path());
    let urls = [
        join_url(&cfg.base_url, "about.json"),
        join_url(&cfg.base_url, "about"),
    ];
    let mut last_err = EngineError::msg("about 请求失败");
    for url in &urls {
        match get_text(cfg, url, etag.as_deref()) {
            Ok(resp) if resp.not_modified => {
                if let Some(v) = read_json(&about_path()) {
                    return Ok(materialize_contact_images(cfg, true, normalize_about(v)));
                }
            }
            Ok(resp) => {
                let body = resp
                    .body
                    .ok_or_else(|| EngineError::msg("about 空响应"))?;
                let value: Value = serde_json::from_str(&body)
                    .map_err(|e| EngineError::msg(format!("about JSON: {e}")))?;
                let protocol = value.get("protocol").and_then(|p| p.as_u64()).unwrap_or(0);
                if protocol != 0 && protocol != 1 {
                    return Err(EngineError::msg(format!(
                        "不支持的 about protocol: {protocol}"
                    )));
                }
                let normalized = normalize_about(value);
                // Disk keeps original remote URLs (small JSON); images live in about-images/.
                write_text_atomic(
                    &about_path(),
                    &format!(
                        "{}\n",
                        serde_json::to_string_pretty(&normalized).unwrap_or(body)
                    ),
                )?;
                if let Some(et) = resp.etag {
                    let _ = write_etag(&about_etag_path(), &et);
                }
                return Ok(materialize_contact_images(cfg, true, normalized));
            }
            Err(e) => last_err = e,
        }
    }
    if let Some(v) = read_json(&about_path()) {
        return Ok(materialize_contact_images(cfg, true, normalize_about(v)));
    }
    Err(last_err)
}

/// Load disk cache only (no network for about.json). Still rewrites images from
/// local about-images cache when present; optional network fill when `cfg` given.
pub fn load_about_disk() -> Option<Value> {
    let v = read_json(&about_path()).map(normalize_about)?;
    // Disk-only image resolve (no network) — use cached thumbs if any.
    Some(materialize_contact_images_disk_only(v))
}

fn about_ui_fields(v: &Value) -> Value {
    json!({
        "protocol": v.get("protocol").cloned().unwrap_or(json!(1)),
        "updatedAt": v.get("updatedAt").cloned().unwrap_or(Value::Null),
        "contact": v.get("contact").cloned().unwrap_or(json!({})),
        "ad": v.get("ad").cloned().unwrap_or(Value::Null),
    })
}

/// UI-facing about payload: disk first, optional network refresh.
pub fn get_about(cfg: &CloudConfig, network: bool) -> Value {
    if !cfg.enabled {
        return json!({
            "ok": true,
            "enabled": false,
            "contact": Value::Null,
            "ad": Value::Null,
            "message": "云端已关闭",
        });
    }
    if network {
        match refresh_about(cfg) {
            Ok(v) => {
                let mut out = about_ui_fields(&v);
                if let Some(obj) = out.as_object_mut() {
                    obj.insert("ok".into(), json!(true));
                    obj.insert("enabled".into(), json!(true));
                    obj.insert("fromNetwork".into(), json!(true));
                }
                return out;
            }
            Err(e) => {
                if let Some(disk) = read_json(&about_path()).map(normalize_about) {
                    let v = materialize_contact_images(cfg, true, disk);
                    let mut out = about_ui_fields(&v);
                    if let Some(obj) = out.as_object_mut() {
                        obj.insert("ok".into(), json!(true));
                        obj.insert("enabled".into(), json!(true));
                        obj.insert("fromNetwork".into(), json!(false));
                        obj.insert("fromCache".into(), json!(true));
                        obj.insert("networkError".into(), json!(e.to_string()));
                    }
                    return out;
                }
                return json!({
                    "ok": false,
                    "enabled": true,
                    "contact": Value::Null,
                    "ad": Value::Null,
                    "error": e.to_string(),
                });
            }
        }
    }

    if let Some(disk) = read_json(&about_path()).map(normalize_about) {
        // Prefer cache; allow network fill for missing contact images so first open works offline-ish after one fetch.
        let v = materialize_contact_images(cfg, true, disk);
        let mut out = about_ui_fields(&v);
        if let Some(obj) = out.as_object_mut() {
            obj.insert("ok".into(), json!(true));
            obj.insert("enabled".into(), json!(true));
            obj.insert("fromNetwork".into(), json!(false));
            obj.insert("fromCache".into(), json!(true));
        }
        return out;
    }

    json!({
        "ok": true,
        "enabled": true,
        "contact": Value::Null,
        "ad": Value::Null,
        "message": "尚无本地 about 缓存",
    })
}

fn str_field(obj: &Value, key: &str) -> String {
    obj.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Normalize free-form contact field item.
fn normalize_field(raw: &Value, index: usize) -> Option<Value> {
    if !raw.is_object() {
        return None;
    }
    let label = str_field(raw, "label");
    let value = str_field(raw, "value");
    let mut href = str_field(raw, "href");
    let mut ty = str_field(raw, "type").to_lowercase();
    if ty.is_empty() || !matches!(ty.as_str(), "text" | "email" | "link" | "image") {
        ty = "text".into();
    }
    let mut id = str_field(raw, "id");
    if id.is_empty() {
        id = format!("f_{index}");
    }
    if label.is_empty() && value.is_empty() && href.is_empty() {
        return None;
    }
    if ty == "email" && !value.is_empty() && href.is_empty() {
        href = format!("mailto:{value}");
    }
    if ty == "link" && !value.is_empty() && href.is_empty() && value.starts_with("http") {
        href = value.clone();
    }
    Some(json!({
        "id": id,
        "label": label,
        "value": value,
        "type": ty,
        "href": href,
    }))
}

/// Migrate legacy fixed email/website/imageUrl keys into free-form fields.
fn legacy_fields(contact: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let email = str_field(contact, "email");
    if !email.is_empty() {
        out.push(json!({
            "id": "legacy_email",
            "label": "邮箱",
            "value": email,
            "type": "email",
            "href": format!("mailto:{email}"),
        }));
    }
    let website = str_field(contact, "website");
    if !website.is_empty() {
        let mut label = str_field(contact, "websiteLabel");
        if label.is_empty() {
            label = website
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .to_string();
        }
        out.push(json!({
            "id": "legacy_website",
            "label": "网站",
            "value": label,
            "type": "link",
            "href": website,
        }));
    }
    let image_url = str_field(contact, "imageUrl");
    if !image_url.is_empty() {
        let alt = str_field(contact, "imageAlt");
        out.push(json!({
            "id": "legacy_image",
            "label": if alt.is_empty() { "图片".into() } else { alt },
            "value": image_url,
            "type": "image",
            "href": "",
        }));
    }
    out
}

/// Normalize about-page ad slot. Modes: placeholder | image | html.
/// Missing/null ad → `Null` (client hides slot). Never invent marketing copy.
fn normalize_ad(raw: &Value) -> Value {
    if raw.is_null() || !raw.is_object() {
        return Value::Null;
    }
    let enabled = raw
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let html = str_field(raw, "html");
    let css = str_field(raw, "css");
    let image_url = {
        let u = str_field(raw, "imageUrl");
        if u.is_empty() {
            str_field(raw, "image")
        } else {
            u
        }
    };
    let href = {
        let h = str_field(raw, "href");
        if h.is_empty() {
            str_field(raw, "link")
        } else {
            h
        }
    };
    let title = str_field(raw, "title");
    let subtitle = {
        let s = str_field(raw, "subtitle");
        if s.is_empty() {
            str_field(raw, "body")
        } else {
            s
        }
    };
    let mut mode = str_field(raw, "mode").to_lowercase();
    if mode != "placeholder" && mode != "image" && mode != "html" {
        mode = if !html.is_empty() {
            "html".into()
        } else if !image_url.is_empty() {
            "image".into()
        } else {
            "placeholder".into()
        };
    }
    if mode == "html" {
        return json!({
            "enabled": enabled,
            "mode": "html",
            "title": title,
            "subtitle": subtitle,
            "imageUrl": "",
            "href": href,
            "html": html,
            "css": css,
        });
    }
    if mode == "image" {
        return json!({
            "enabled": enabled,
            "mode": "image",
            "title": title,
            "subtitle": subtitle,
            "imageUrl": image_url,
            "href": href,
            "html": "",
            "css": "",
        });
    }
    // Pass through cloud title/subtitle as-is (may be empty → client hides)
    json!({
        "enabled": enabled,
        "mode": "placeholder",
        "title": title,
        "subtitle": subtitle,
        "imageUrl": "",
        "href": "",
        "html": "",
        "css": "",
    })
}

fn normalize_about(raw: Value) -> Value {
    let contact_raw = raw.get("contact").cloned().unwrap_or(json!({}));
    let ad = normalize_ad(raw.get("ad").unwrap_or(&Value::Null));
    let html = str_field(&contact_raw, "html");
    let css = str_field(&contact_raw, "css");
    let mut mode = str_field(&contact_raw, "mode").to_lowercase();
    if mode != "fields" && mode != "html" {
        // Infer: non-empty html without fields array → html mode
        let has_fields = contact_raw
            .get("fields")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        mode = if !html.is_empty() && !has_fields {
            "html".into()
        } else {
            "fields".into()
        };
    }

    if mode == "html" {
        return json!({
            "protocol": raw.get("protocol").and_then(|p| p.as_u64()).unwrap_or(1),
            "updatedAt": raw.get("updatedAt").cloned().unwrap_or(Value::Null),
            "contact": {
                "mode": "html",
                "intro": "",
                "fields": [],
                "html": html,
                "css": css,
            },
            "ad": ad,
        });
    }

    let intro = {
        let i = str_field(&contact_raw, "intro");
        if !i.is_empty() {
            i
        } else {
            str_field(&contact_raw, "note")
        }
    };

    let mut fields: Vec<Value> = Vec::new();
    if let Some(arr) = contact_raw.get("fields").and_then(|v| v.as_array()) {
        for (i, item) in arr.iter().enumerate() {
            if let Some(f) = normalize_field(item, i) {
                // Preserve optional copyable flag for QQ-style contact cards
                if let Some(obj) = f.as_object() {
                    let mut out = obj.clone();
                    if let Some(c) = item.get("copyable").and_then(|v| v.as_bool()) {
                        out.insert("copyable".into(), json!(c));
                    }
                    if let Some(cv) = item.get("copyValue").and_then(|v| v.as_str()) {
                        let t = cv.trim();
                        if !t.is_empty() {
                            out.insert("copyValue".into(), json!(t));
                        }
                    }
                    fields.push(Value::Object(out));
                    continue;
                }
                fields.push(f);
            }
        }
    }
    if fields.is_empty() {
        fields = legacy_fields(&contact_raw);
    }

    json!({
        "protocol": raw.get("protocol").and_then(|p| p.as_u64()).unwrap_or(1),
        "updatedAt": raw.get("updatedAt").cloned().unwrap_or(Value::Null),
        "contact": {
            "mode": "fields",
            "intro": intro,
            "fields": fields,
            "html": "",
            "css": "",
        },
        "ad": ad,
    })
}

// ── Contact image materialization (CSP-safe data-URLs) ─────────────────────

fn about_images_root() -> PathBuf {
    cdp::native_state_root().join("cache").join("about-images")
}

fn url_cache_key(url: &str) -> String {
    let mut h = Sha256::new();
    h.update(url.trim().as_bytes());
    hex::encode(h.finalize())
}

fn meta_path_for(key: &str) -> PathBuf {
    about_images_root().join(format!("{key}.meta.json"))
}

fn image_path_for(key: &str, ext: &str) -> PathBuf {
    about_images_root().join(format!("{key}.{ext}"))
}

fn find_cached_image(key: &str) -> Option<PathBuf> {
    let root = about_images_root();
    if !root.is_dir() {
        return None;
    }
    for ext in ["jpg", "jpeg", "png", "webp", "gif", "svg", "bin"] {
        let p = root.join(format!("{key}.{ext}"));
        if p.is_file() {
            return Some(p);
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

fn ext_from_url_or_bytes(url: &str, bytes: &[u8]) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(seg) = parsed
            .path_segments()
            .and_then(|mut s| s.next_back())
            .map(|s| s.to_ascii_lowercase())
        {
            for ext in ["jpg", "jpeg", "png", "webp", "gif", "svg"] {
                if seg.ends_with(&format!(".{ext}")) || seg.contains(&format!(".{ext}?")) {
                    return ext.to_string();
                }
            }
        }
    }
    // Magic sniff
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return "png".into();
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "jpg".into();
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "webp".into();
    }
    if bytes.starts_with(b"GIF8") {
        return "gif".into();
    }
    if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
        return "svg".into();
    }
    "bin".into()
}

fn bytes_to_data_url(ext: &str, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", mime_from_ext(ext), B64.encode(bytes))
}

fn load_cached_data_url(source_url: &str) -> Option<String> {
    let key = url_cache_key(source_url);
    // Prefer meta match so URL changes bust the cache
    if let Some(meta) = read_json(&meta_path_for(&key)) {
        let cached = meta
            .get("sourceUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if cached != source_url.trim() {
            return None;
        }
    }
    let path = find_cached_image(&key)?;
    let bytes = fs::read(&path).ok()?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_PREVIEW_BYTES {
        return None;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    Some(bytes_to_data_url(ext, &bytes))
}

fn fetch_and_cache_image(cfg: &CloudConfig, source_url: &str) -> Result<String, EngineError> {
    let url = source_url.trim();
    if url.is_empty() {
        return Err(EngineError::msg("空图片 URL"));
    }
    if url.starts_with("data:") {
        return Ok(url.to_string());
    }
    // Validate host allowlist (same as skin previews)
    let _ = validate_download_url(url, cfg)?;

    if let Some(cached) = load_cached_data_url(url) {
        return Ok(cached);
    }

    let resp = get_bytes_allowlisted_with_cap(cfg, url, None, MAX_PREVIEW_BYTES)?;
    if resp.bytes.is_empty() {
        return Err(EngineError::msg("联系图片为空"));
    }
    let ext = ext_from_url_or_bytes(&resp.final_url, &resp.bytes);
    let key = url_cache_key(url);
    let root = about_images_root();
    fs::create_dir_all(&root)
        .map_err(|e| EngineError::msg(format!("mkdir about-images: {e}")))?;
    // Drop previous extensions for this key
    for old_ext in ["jpg", "jpeg", "png", "webp", "gif", "svg", "bin"] {
        let p = image_path_for(&key, old_ext);
        let _ = fs::remove_file(p);
    }
    let img_path = image_path_for(&key, &ext);
    fs::write(&img_path, &resp.bytes)
        .map_err(|e| EngineError::msg(format!("写联系图片: {e}")))?;
    let meta = json!({
        "sourceUrl": url,
        "finalUrl": resp.final_url,
        "size": resp.bytes.len(),
        "ext": ext,
    });
    let _ = write_text_atomic(
        &meta_path_for(&key),
        &format!("{}\n", serde_json::to_string_pretty(&meta).unwrap_or_default()),
    );
    Ok(bytes_to_data_url(&ext, &resp.bytes))
}

fn resolve_image_src(cfg: Option<&CloudConfig>, allow_network: bool, src: &str) -> String {
    let s = src.trim();
    if s.is_empty() || s.starts_with("data:") || s.starts_with("asset:") || s.starts_with("blob:") {
        return s.to_string();
    }
    if !(s.starts_with("http://") || s.starts_with("https://")) {
        return s.to_string();
    }
    if let Some(cached) = load_cached_data_url(s) {
        return cached;
    }
    if allow_network {
        if let Some(cfg) = cfg {
            if let Ok(data) = fetch_and_cache_image(cfg, s) {
                return data;
            }
        }
    }
    // Leave remote URL — WebView CSP will block; UI still has alt text.
    s.to_string()
}

/// Rewrite remote `src="http(s)://…"` in contact HTML to data-URLs when possible.
fn rewrite_html_img_srcs(cfg: Option<&CloudConfig>, allow_network: bool, html: &str) -> String {
    // Simple attribute scan — contact HTML is small and already sanitized on the frontend.
    let mut out = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(rel) = lower[i..].find("src=") {
            let start = i + rel;
            out.push_str(&html[i..start]);
            let after = start + 4; // past "src="
            if after >= bytes.len() {
                out.push_str(&html[start..]);
                break;
            }
            let quote = html.as_bytes()[after] as char;
            if quote == '"' || quote == '\'' {
                if let Some(end_rel) = html[after + 1..].find(quote) {
                    let val_start = after + 1;
                    let val_end = val_start + end_rel;
                    let raw_src = &html[val_start..val_end];
                    let resolved = resolve_image_src(cfg, allow_network, raw_src);
                    out.push_str("src=");
                    out.push(quote);
                    out.push_str(&resolved);
                    out.push(quote);
                    i = val_end + 1;
                    continue;
                }
            }
            // Unquoted or malformed — copy through
            out.push_str(&html[start..start + 4]);
            i = after;
            continue;
        }
        out.push_str(&html[i..]);
        break;
    }
    out
}

fn materialize_ad_images(cfg: Option<&CloudConfig>, allow_network: bool, about: &mut Value) {
    let Some(ad) = about.get_mut("ad") else {
        return;
    };
    if !ad.is_object() {
        return;
    }
    let mode = ad
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("placeholder")
        .to_ascii_lowercase();
    if mode == "html" {
        if let Some(html) = ad.get("html").and_then(|v| v.as_str()).map(|s| s.to_string()) {
            let rewritten = rewrite_html_img_srcs(cfg, allow_network, &html);
            if let Some(obj) = ad.as_object_mut() {
                obj.insert("html".into(), json!(rewritten));
            }
        }
        return;
    }
    if mode == "image" {
        let value = ad
            .get("imageUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if value.is_empty() {
            return;
        }
        let resolved = resolve_image_src(cfg, allow_network, &value);
        if let Some(obj) = ad.as_object_mut() {
            if resolved.starts_with("data:") && !value.starts_with("data:") {
                obj.insert("sourceUrl".into(), json!(value));
            }
            obj.insert("imageUrl".into(), json!(resolved));
        }
    }
}

fn materialize_contact_images(cfg: &CloudConfig, allow_network: bool, mut about: Value) -> Value {
    materialize_ad_images(Some(cfg), allow_network, &mut about);

    let Some(contact) = about.get_mut("contact") else {
        return about;
    };
    if !contact.is_object() {
        return about;
    }

    let mode = contact
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("fields")
        .to_ascii_lowercase();

    if mode == "html" {
        if let Some(html) = contact.get("html").and_then(|v| v.as_str()).map(|s| s.to_string()) {
            let rewritten = rewrite_html_img_srcs(Some(cfg), allow_network, &html);
            if let Some(obj) = contact.as_object_mut() {
                obj.insert("html".into(), json!(rewritten));
            }
        }
        return about;
    }

    // fields mode — rewrite type=image values
    let fields = match contact.get_mut("fields").and_then(|v| v.as_array_mut()) {
        Some(arr) => arr,
        None => return about,
    };
    for field in fields.iter_mut() {
        let ty = field
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ty != "image" {
            continue;
        }
        let value = field
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if value.is_empty() {
            continue;
        }
        let resolved = resolve_image_src(Some(cfg), allow_network, &value);
        if let Some(obj) = field.as_object_mut() {
            // Keep original for debugging / re-fetch; GUI uses value.
            if resolved.starts_with("data:") && !value.starts_with("data:") {
                obj.insert("sourceUrl".into(), json!(value));
            }
            obj.insert("value".into(), json!(resolved));
        }
    }
    about
}

fn materialize_contact_images_disk_only(mut about: Value) -> Value {
    materialize_ad_images(None, false, &mut about);

    let Some(contact) = about.get_mut("contact") else {
        return about;
    };
    if !contact.is_object() {
        return about;
    }
    let mode = contact
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("fields")
        .to_ascii_lowercase();

    if mode == "html" {
        if let Some(html) = contact.get("html").and_then(|v| v.as_str()).map(|s| s.to_string()) {
            let rewritten = rewrite_html_img_srcs(None, false, &html);
            if let Some(obj) = contact.as_object_mut() {
                obj.insert("html".into(), json!(rewritten));
            }
        }
        return about;
    }

    let fields = match contact.get_mut("fields").and_then(|v| v.as_array_mut()) {
        Some(arr) => arr,
        None => return about,
    };
    for field in fields.iter_mut() {
        let ty = field
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ty != "image" {
            continue;
        }
        let value = field
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if value.is_empty() {
            continue;
        }
        let resolved = resolve_image_src(None, false, &value);
        if let Some(obj) = field.as_object_mut() {
            if resolved.starts_with("data:") && !value.starts_with("data:") {
                obj.insert("sourceUrl".into(), json!(value));
            }
            obj.insert("value".into(), json!(resolved));
        }
    }
    about
}
