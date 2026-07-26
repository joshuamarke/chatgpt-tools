//! App version check against catalog `minAppVersion` (+ optional remote meta).

use super::catalog::{load_catalog_disk, version_cmp};
use super::config::CloudConfig;
use super::http::{get_text, join_url};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct VersionCheckResult {
    pub ok: bool,
    pub current: String,
    pub min_app_version: Option<String>,
    pub update_required: bool,
    pub update_available: bool,
    pub latest: Option<String>,
    pub message: String,
    pub download_url: Option<String>,
    pub release_notes: Option<String>,
}

impl VersionCheckResult {
    pub fn to_json(&self) -> Value {
        json!({
            "ok": self.ok,
            "current": self.current,
            "minAppVersion": self.min_app_version,
            "updateRequired": self.update_required,
            "updateAvailable": self.update_available,
            "latest": self.latest,
            "message": self.message,
            "downloadUrl": self.download_url,
            "releaseNotes": self.release_notes,
        })
    }
}

/// Check against catalog minAppVersion.
/// Offline-first: uses provided/disk catalog only unless `network=true`.
pub fn check_app_version(cfg: &CloudConfig, catalog: Option<&Value>) -> Value {
    check_app_version_opts(cfg, catalog, false)
}

/// `network=true` may hit optional version.json when catalog lacks latest (manual check).
pub fn check_app_version_opts(cfg: &CloudConfig, catalog: Option<&Value>, network: bool) -> Value {
    let current = cfg.app_version.clone();
    if !cfg.enabled {
        return VersionCheckResult {
            ok: true,
            current,
            min_app_version: None,
            update_required: false,
            update_available: false,
            latest: None,
            message: "云端已关闭，跳过版本检查".into(),
            download_url: None,
            release_notes: None,
        }
        .to_json();
    }

    // Prefer caller-provided / disk catalog — no nested CDN fetch on boot.
    let cat = catalog.cloned().or_else(load_catalog_disk);

    let mut min_app: Option<String> = cat
        .as_ref()
        .and_then(|c| c.get("minAppVersion").and_then(|v| v.as_str()))
        .map(|s| s.to_string());

    let mut latest: Option<String> = None;
    let mut download_url: Option<String> = None;
    // Cloud admin prompt copy — only used when an update actually exists.
    let mut remote_message: Option<String> = None;
    let mut release_notes: Option<String> = None;

    if let Some(l) = cat
        .as_ref()
        .and_then(|c| c.get("latest").or_else(|| c.get("appVersion")))
        .and_then(|x| x.as_str())
    {
        let t = l.trim();
        if !t.is_empty() {
            latest = Some(t.trim_start_matches('v').trim_start_matches('V').to_string());
        }
    }

    // Manual check may fill gaps via version.json; boot path stays disk-only.
    if network && latest.is_none() && min_app.is_none() {
        for rel in ["version.json", "version", "app-version.json"] {
            let url = join_url(&cfg.base_url, rel);
            if let Ok(resp) = get_text(cfg, &url, None) {
                if let Some(body) = resp.body {
                    if let Ok(v) = serde_json::from_str::<Value>(&body) {
                        if let Some(l) = v
                            .get("latest")
                            .or_else(|| v.get("version"))
                            .and_then(|x| x.as_str())
                        {
                            let t = l.trim();
                            if !t.is_empty() {
                                latest = Some(
                                    t.trim_start_matches('v')
                                        .trim_start_matches('V')
                                        .to_string(),
                                );
                            }
                        }
                        if let Some(m) = v.get("minAppVersion").and_then(|x| x.as_str()) {
                            let t = m.trim();
                            if !t.is_empty() {
                                min_app = Some(
                                    t.trim_start_matches('v')
                                        .trim_start_matches('V')
                                        .to_string(),
                                );
                            }
                        }
                        if let Some(u) = v
                            .get("downloadUrl")
                            .or_else(|| v.get("url"))
                            .and_then(|x| x.as_str())
                        {
                            let t = u.trim();
                            if !t.is_empty() {
                                download_url = Some(t.to_string());
                            }
                        }
                        if let Some(msg) = v.get("message").and_then(|x| x.as_str()) {
                            let t = msg.trim();
                            if !t.is_empty() {
                                remote_message = Some(t.to_string());
                            }
                        }
                        if let Some(notes) = v
                            .get("releaseNotes")
                            .or_else(|| v.get("notes"))
                            .or_else(|| v.get("changelog"))
                            .and_then(|x| x.as_str())
                        {
                            let t = notes.trim();
                            if !t.is_empty() {
                                release_notes = Some(t.to_string());
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    let update_required = min_app
        .as_ref()
        .map(|m| version_cmp(&current, m) < 0)
        .unwrap_or(false);

    // Only "new version available" when remote latest is strictly newer than local.
    let update_available = latest
        .as_ref()
        .map(|l| version_cmp(&current, l) < 0)
        .unwrap_or(false);

    let has_update = update_available || update_required;

    // Prompt copy is for update UX only — never replace "已是最新版本" when up to date.
    let message = if has_update {
        if let Some(m) = remote_message.filter(|s| !s.is_empty()) {
            m
        } else if update_required {
            format!(
                "当前版本 {} 低于云端要求的最低版本 {}",
                current,
                min_app.as_deref().unwrap_or("?")
            )
        } else {
            format!(
                "发现新版本 {}（当前 {}）",
                latest.as_deref().unwrap_or("?"),
                current
            )
        }
    } else {
        "已是最新版本".into()
    };

    // Hide download / notes noise when there is nothing to update.
    let (download_url, release_notes) = if has_update {
        (download_url, release_notes)
    } else {
        (None, None)
    };

    VersionCheckResult {
        ok: true,
        current,
        min_app_version: min_app,
        update_required,
        update_available,
        latest,
        message,
        download_url,
        release_notes,
    }
    .to_json()
}
