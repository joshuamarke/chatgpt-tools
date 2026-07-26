//! Cloud CDN client: catalog, announcements, version check, secure skin cache.
//!
//! All package downloads run in Rust (never from WebView). Host allowlist +
//! sha256 + import-grade validation prevent arbitrary remote hooks from
//! installing skins.

mod announcements;
mod cache;
mod catalog;
mod config;
mod download;
mod http;
mod version;

pub use announcements::{get_announcements, mark_announcement_read, refresh_announcements};
pub use cache::{clear_skin_cache, list_cached_skins};
pub use catalog::{load_catalog_disk, merge_remote_into_status, refresh_catalog};
pub use config::load_cloud_config;
pub use download::download_skin;
pub use version::{check_app_version, check_app_version_opts};

use crate::engine::EngineError;
use serde_json::{json, Value};

/// Minimum interval between automatic CDN syncs (seconds).
/// Manual “刷新” can still force a network pull.
pub const CLOUD_SOFT_SYNC_TTL_SECS: u64 = 30 * 60; // 30 minutes

/// Ensure cloud directories exist under state root.
pub fn ensure_cloud_dirs() -> Result<(), EngineError> {
    cache::ensure_cloud_layout()
}

fn disk_cache_fresh(ttl_secs: u64) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Some(last) = cache::read_last_sync_unix() {
        if now.saturating_sub(last) < ttl_secs {
            return true;
        }
    }
    // Fallback: catalog file mtime still within TTL
    if let Some(mt) = cache::catalog_mtime_secs() {
        if now.saturating_sub(mt) < ttl_secs {
            return true;
        }
    }
    false
}

/// Soft network sync: skip CDN when local cache is still fresh (unless force).
/// On network failure, keeps disk cache and returns errors as soft fields.
pub fn soft_network_sync(cfg: &config::CloudConfig, force: bool) -> Value {
    if !cfg.enabled {
        return json!({
            "ok": true,
            "enabled": false,
            "synced": false,
            "reason": "cloud disabled",
        });
    }
    let _ = ensure_cloud_dirs();
    let has_disk = catalog::load_catalog_disk().is_some()
        || cache::read_json(&cache::announcements_path()).is_some();

    if !force && has_disk && disk_cache_fresh(CLOUD_SOFT_SYNC_TTL_SECS) {
        return json!({
            "ok": true,
            "enabled": true,
            "synced": false,
            "skipped": true,
            "reason": "cache-fresh",
            "ttlSecs": CLOUD_SOFT_SYNC_TTL_SECS,
        });
    }

    let mut catalog_error = Value::Null;
    let mut announcements_error = Value::Null;
    let mut any_ok = false;

    match refresh_catalog(cfg) {
        Ok(snap) => {
            any_ok = true;
            if snap.from_network || snap.not_modified {
                // network path or 304 both count as a successful check
            }
        }
        Err(e) => catalog_error = json!(e.to_string()),
    }
    match refresh_announcements(cfg) {
        Ok(_) => any_ok = true,
        Err(e) => announcements_error = json!(e.to_string()),
    }

    if any_ok && catalog_error.is_null() {
        let _ = cache::write_last_sync_now();
    } else if any_ok {
        // partial success still advances soft TTL to avoid tight retry loops offline
        let _ = cache::write_last_sync_now();
    }

    json!({
        "ok": any_ok || has_disk,
        "enabled": true,
        "synced": any_ok,
        "skipped": false,
        "fromCache": has_disk && !any_ok,
        "catalogError": catalog_error,
        "announcementsError": announcements_error,
    })
}

/// Combined cloud snapshot for GUI boot (non-fatal partials).
/// `force_refresh=true` still respects soft TTL when disk cache is warm.
pub fn cloud_status_snapshot(force_refresh: bool) -> Value {
    let cfg = load_cloud_config();
    if !cfg.enabled {
        return json!({
            "ok": true,
            "enabled": false,
            "reason": "cloud disabled",
        });
    }

    let _ = ensure_cloud_dirs();
    let mut sync_meta = json!({ "skipped": true, "reason": "disk-only" });

    if force_refresh {
        // Soft TTL: avoid hammering CDN when cache is recent.
        sync_meta = soft_network_sync(&cfg, false);
    }

    let catalog = catalog::load_catalog_disk();
    let announcements = announcements::load_announcements_for_ui(&cfg);
    // Version from disk catalog only (no nested network during status).
    let version = check_app_version(&cfg, catalog.as_ref());

    json!({
        "ok": catalog.is_some() || sync_meta.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
        "enabled": true,
        "baseUrl": cfg.base_url,
        "channel": cfg.channel,
        "protocol": cfg.protocol,
        "catalog": catalog,
        "announcements": announcements,
        "version": version,
        "sync": sync_meta,
        "catalogError": sync_meta.get("catalogError").cloned().unwrap_or(Value::Null),
        "announcementsError": sync_meta.get("announcementsError").cloned().unwrap_or(Value::Null),
        "cachedSkinIds": list_cached_skins()
            .into_iter()
            .filter_map(|s| s.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect::<Vec<_>>(),
    })
}
