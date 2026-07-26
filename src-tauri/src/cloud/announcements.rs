//! Announcements fetch, filter, read-state.

use super::cache::{
    announcements_etag_path, announcements_path, ensure_cloud_layout, read_etag, read_json,
    read_state_path, write_etag, write_text_atomic,
};
use super::catalog::version_cmp;
use super::config::CloudConfig;
use super::http::{get_text, join_url};
use crate::engine::EngineError;
use serde_json::{json, Value};
use std::fs;

pub fn refresh_announcements(cfg: &CloudConfig) -> Result<Value, EngineError> {
    if !cfg.enabled {
        return Err(EngineError::msg("云端已关闭"));
    }
    ensure_cloud_layout()?;
    let etag = read_etag(&announcements_etag_path());
    let urls = [
        join_url(
            &cfg.base_url,
            &format!("{}/announcements.json", cfg.channel),
        ),
        join_url(&cfg.base_url, &format!("{}/announcements", cfg.channel)),
    ];
    let mut last_err = EngineError::msg("announcements 请求失败");
    for url in &urls {
        match get_text(cfg, url, etag.as_deref()) {
            Ok(resp) if resp.not_modified => {
                if let Some(v) = read_json(&announcements_path()) {
                    return Ok(v);
                }
            }
            Ok(resp) => {
                let body = resp
                    .body
                    .ok_or_else(|| EngineError::msg("announcements 空响应"))?;
                let value: Value = serde_json::from_str(&body)
                    .map_err(|e| EngineError::msg(format!("announcements JSON: {e}")))?;
                let protocol = value.get("protocol").and_then(|p| p.as_u64()).unwrap_or(0);
                if protocol != 1 {
                    return Err(EngineError::msg(format!(
                        "不支持的 announcements protocol: {protocol}"
                    )));
                }
                write_text_atomic(
                    &announcements_path(),
                    &format!(
                        "{}\n",
                        serde_json::to_string_pretty(&value).unwrap_or(body)
                    ),
                )?;
                if let Some(et) = resp.etag {
                    let _ = write_etag(&announcements_etag_path(), &et);
                }
                return Ok(value);
            }
            Err(e) => last_err = e,
        }
    }
    if let Some(v) = read_json(&announcements_path()) {
        return Ok(v);
    }
    Err(last_err)
}

fn read_ids() -> std::collections::HashSet<String> {
    let v = read_json(&read_state_path()).unwrap_or(json!({}));
    v.get("readIds")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn mark_announcement_read(id: &str) -> Result<Value, EngineError> {
    ensure_cloud_layout()?;
    let id = id.trim();
    if id.is_empty() || id.len() > 128 {
        return Err(EngineError::msg("无效公告 id"));
    }
    let mut set = read_ids();
    set.insert(id.to_string());
    let mut ids: Vec<String> = set.into_iter().collect();
    ids.sort();
    // Cap growth
    if ids.len() > 200 {
        ids = ids.split_off(ids.len() - 200);
    }
    let body = json!({ "readIds": ids });
    write_text_atomic(
        &read_state_path(),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        ),
    )?;
    Ok(json!({ "ok": true, "id": id }))
}

fn parse_iso_ms(s: &str) -> Option<i64> {
    // Accept full ISO8601 with Z; minimal parser via chrono-less heuristic:
    // Use time crate? not in deps. Simple: rely on starts/ends being comparable as strings
    // for ISO8601 UTC — lexicographic works for same format.
    // Also allow empty.
    if s.is_empty() {
        return None;
    }
    // Convert "2026-07-01T00:00:00.000Z" → rough epoch via manual parse for filter only
    let clean = s.trim().trim_end_matches('Z');
    let (date, time) = clean.split_once('T')?;
    let mut dp = date.split('-');
    let y: i64 = dp.next()?.parse().ok()?;
    let mo: i64 = dp.next()?.parse().ok()?;
    let d: i64 = dp.next()?.parse().ok()?;
    let time = time.split('.').next().unwrap_or(time);
    let mut tp = time.split(':');
    let h: i64 = tp.next()?.parse().ok()?;
    let mi: i64 = tp.next()?.parse().ok()?;
    let se: i64 = tp.next()?.parse().ok()?;
    // Days from civil date (Howard Hinnant algorithm)
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if mo > 2 { mo - 3 } else { mo + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Active, version-matched, unread-first list for UI.
pub fn load_announcements_for_ui(cfg: &CloudConfig) -> Value {
    let raw = read_json(&announcements_path()).unwrap_or(json!({
        "protocol": 1,
        "channel": cfg.channel,
        "items": []
    }));
    filter_announcements(&raw, cfg)
}

pub fn get_announcements(cfg: &CloudConfig, refresh: bool) -> Result<Value, EngineError> {
    if refresh && cfg.enabled {
        let _ = refresh_announcements(cfg);
    }
    Ok(load_announcements_for_ui(cfg))
}

pub fn filter_announcements(raw: &Value, cfg: &CloudConfig) -> Value {
    let read = read_ids();
    let now = now_unix();
    let mut items: Vec<Value> = Vec::new();
    let empty: Vec<Value> = Vec::new();
    let arr = raw
        .get("items")
        .and_then(|a| a.as_array())
        .unwrap_or(&empty);

    for it in arr {
        let id = it.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        if let Some(starts) = it.get("startsAt").and_then(|v| v.as_str()) {
            if let Some(t) = parse_iso_ms(starts) {
                if now < t {
                    continue;
                }
            }
        }
        if let Some(ends) = it.get("endsAt").and_then(|v| v.as_str()) {
            if let Some(t) = parse_iso_ms(ends) {
                if now > t {
                    continue;
                }
            }
        }
        if let Some(min_v) = it.get("minAppVersion").and_then(|v| v.as_str()) {
            if version_cmp(&cfg.app_version, min_v) < 0 {
                continue;
            }
        }
        if let Some(max_v) = it.get("maxAppVersion").and_then(|v| v.as_str()) {
            if version_cmp(&cfg.app_version, max_v) > 0 {
                continue;
            }
        }

        let is_read = read.contains(id);
        let mut item = it.clone();
        if let Some(obj) = item.as_object_mut() {
            obj.insert("read".into(), json!(is_read));
        }
        items.push(item);
    }

    items.sort_by(|a, b| {
        let pr = |x: &Value| x.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
        let rd = |x: &Value| x.get("read").and_then(|v| v.as_bool()).unwrap_or(false);
        // unread first, then higher priority
        rd(a)
            .cmp(&rd(b))
            .then_with(|| pr(b).cmp(&pr(a)))
    });

    json!({
        "ok": true,
        "channel": raw.get("channel").and_then(|v| v.as_str()).unwrap_or(&cfg.channel),
        "generatedAt": raw.get("generatedAt"),
        "items": items,
        "readCount": read.len(),
        "fromDisk": fs::metadata(announcements_path()).is_ok(),
    })
}
