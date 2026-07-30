//! About / contact info from CDN (`/v1/about.json`).
//! Independent of app version check (`version.json`).

use super::cache::{
    about_etag_path, about_path, ensure_cloud_layout, read_etag, read_json, write_etag,
    write_text_atomic,
};
use super::config::CloudConfig;
use super::http::{get_text, join_url};
use crate::engine::EngineError;
use serde_json::{json, Value};

/// Fetch about.json from CDN and cache to disk.
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
                    return Ok(normalize_about(v));
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
                return Ok(normalized);
            }
            Err(e) => last_err = e,
        }
    }
    if let Some(v) = read_json(&about_path()) {
        return Ok(normalize_about(v));
    }
    Err(last_err)
}

/// Load disk cache only (no network).
pub fn load_about_disk() -> Option<Value> {
    read_json(&about_path()).map(normalize_about)
}

/// UI-facing about payload: disk first, optional network refresh.
pub fn get_about(cfg: &CloudConfig, network: bool) -> Value {
    if !cfg.enabled {
        return json!({
            "ok": true,
            "enabled": false,
            "contact": Value::Null,
            "message": "云端已关闭",
        });
    }
    if network {
        match refresh_about(cfg) {
            Ok(v) => {
                return json!({
                    "ok": true,
                    "enabled": true,
                    "fromNetwork": true,
                    "protocol": v.get("protocol").cloned().unwrap_or(json!(1)),
                    "updatedAt": v.get("updatedAt").cloned().unwrap_or(Value::Null),
                    "contact": v.get("contact").cloned().unwrap_or(json!({})),
                });
            }
            Err(e) => {
                if let Some(disk) = load_about_disk() {
                    return json!({
                        "ok": true,
                        "enabled": true,
                        "fromNetwork": false,
                        "fromCache": true,
                        "networkError": e.to_string(),
                        "protocol": disk.get("protocol").cloned().unwrap_or(json!(1)),
                        "updatedAt": disk.get("updatedAt").cloned().unwrap_or(Value::Null),
                        "contact": disk.get("contact").cloned().unwrap_or(json!({})),
                    });
                }
                return json!({
                    "ok": false,
                    "enabled": true,
                    "contact": Value::Null,
                    "error": e.to_string(),
                });
            }
        }
    }

    if let Some(disk) = load_about_disk() {
        return json!({
            "ok": true,
            "enabled": true,
            "fromNetwork": false,
            "fromCache": true,
            "protocol": disk.get("protocol").cloned().unwrap_or(json!(1)),
            "updatedAt": disk.get("updatedAt").cloned().unwrap_or(Value::Null),
            "contact": disk.get("contact").cloned().unwrap_or(json!({})),
        });
    }

    json!({
        "ok": true,
        "enabled": true,
        "contact": Value::Null,
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
    if ty.is_empty()
        || !matches!(ty.as_str(), "text" | "email" | "link" | "image")
    {
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

fn normalize_about(raw: Value) -> Value {
    let contact_raw = raw.get("contact").cloned().unwrap_or(json!({}));
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
            }
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
        }
    })
}
