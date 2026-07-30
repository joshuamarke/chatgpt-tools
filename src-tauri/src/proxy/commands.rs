//! Tauri IPC for local routing.

use super::runtime::{proxy_status_snapshot, runtime};
use super::types::*;
use crate::providers::models::{AppKind, AppProviderStore, GlobalProxyConfig, Provider};
use crate::providers::store;

fn parse_app(app: &str) -> Result<AppKind, String> {
    AppKind::from_str_loose(app).ok_or_else(|| format!("未知应用: {app}"))
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_proxy_status() -> Result<ProxyRuntimeStatus, String> {
    Ok(proxy_status_snapshot())
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_proxy_config() -> Result<GlobalProxyConfig, String> {
    Ok(store::load()?.proxy)
}

#[tauri::command(rename_all = "camelCase")]
pub fn update_proxy_config(config: GlobalProxyConfig) -> Result<GlobalProxyConfig, String> {
    // When proxy is running, listen address/port cannot change (runtime enforces).
    // Port check treats our own listen port as available so saving logging / egress
    // fields is not blocked by a false "occupied" result.
    let running = runtime().is_running();
    let current = store::load()?.proxy;
    let mut config = config;
    config.log_retention_days =
        super::log_store::clamp_retention_days(if config.log_retention_days == 0 {
            super::log_store::default_retention_days()
        } else {
            config.log_retention_days
        });
    let listen_changed =
        current.listen_address != config.listen_address || current.listen_port != config.listen_port;
    if !running || listen_changed {
        let check =
            crate::live_config::check_listen_port(&config.listen_address, config.listen_port);
        if !check.available {
            let hint = check
                .suggested_port
                .map(|p| format!("建议改用端口 {p}"))
                .unwrap_or_else(|| "请更换端口".into());
            return Err(format!("{}。{hint}", check.message));
        }
    }
    runtime().update_listen_config(config)?;
    let saved = store::load()?.proxy;
    let _ = super::log_store::set_retention_days(saved.log_retention_days);
    Ok(saved)
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_proxy_takeover_status() -> Result<TakeoverStatus, String> {
    let file = store::load()?;
    Ok(TakeoverStatus {
        codex: file.codex.takeover_enabled,
        grok: file.grok.takeover_enabled,
        proxy_running: runtime().is_running(),
        proxy: file.proxy,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_proxy_takeover(
    app_handle: tauri::AppHandle,
    app: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    let kind = parse_app(&app)?;
    let warnings = runtime().set_takeover(kind, enabled)?;
    // Tray submenu title shows routing marker (⚡) — refresh after takeover toggle.
    crate::tray::notify_providers_changed(&app_handle);
    Ok(serde_json::json!({
        "ok": true,
        "warnings": warnings,
        "status": get_proxy_takeover_status()?,
        "proxyStatus": proxy_status_snapshot(),
    }))
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_app_proxy_settings(app: String) -> Result<AppProxySettings, String> {
    let kind = parse_app(&app)?;
    let file = store::load()?;
    let s = file.for_kind(kind);
    Ok(AppProxySettings {
        app: kind.as_str().into(),
        takeover_enabled: s.takeover_enabled,
        auto_failover_enabled: s.auto_failover_enabled,
        max_retries: s.max_retries,
        circuit: s.circuit.clone(),
        streaming_first_byte_timeout: s.streaming_first_byte_timeout,
        streaming_idle_timeout: s.streaming_idle_timeout,
        non_streaming_timeout: s.non_streaming_timeout,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn update_app_proxy_settings(settings: AppProxySettings) -> Result<AppProxySettings, String> {
    let kind = parse_app(&settings.app)?;
    let mut file = store::load()?;
    {
        let s = file.for_kind_mut(kind);
        s.max_retries = settings.max_retries.min(10);
        s.circuit = settings.circuit.clone();
        s.streaming_first_byte_timeout = settings.streaming_first_byte_timeout.clamp(1, 120);
        s.streaming_idle_timeout = settings.streaming_idle_timeout.min(600);
        s.non_streaming_timeout = settings.non_streaming_timeout.clamp(60, 1200);
        // auto_failover / takeover go through dedicated commands
    }
    store::save(&file)?;
    get_app_proxy_settings(kind.as_str().into())
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_auto_failover(app: String, enabled: bool) -> Result<serde_json::Value, String> {
    let kind = parse_app(&app)?;
    let mut file = store::load()?;
    {
        let s = file.for_kind_mut(kind);
        if enabled && !s.takeover_enabled {
            return Err("请先开启本地路由，再启用自动故障转移".into());
        }
        s.normalize_failover_order();
        if enabled {
            let current = s.current.clone();
            if !current.is_empty() && !s.failover_order.iter().any(|id| id == &current) {
                if s.providers.iter().any(|p| p.id == current) {
                    s.failover_order.insert(0, current.clone());
                }
            }
            if s.failover_order.is_empty() {
                return Err("故障转移队列为空：请先在路由设置中添加备用供应商".into());
            }
            s.normalize_failover_order();
        }
        s.auto_failover_enabled = enabled;
    }
    store::save(&file)?;
    Ok(serde_json::json!({
        "ok": true,
        "autoFailoverEnabled": enabled,
        "failoverOrder": store::load()?.for_kind(kind).failover_order,
    }))
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_failover_queue(app: String) -> Result<Vec<FailoverQueueItem>, String> {
    let kind = parse_app(&app)?;
    let mut file = store::load()?;
    file.for_kind_mut(kind).normalize_failover_order();
    let _ = store::save(&file);
    let s = file.for_kind(kind);
    let rt = runtime();
    let mut items = Vec::new();
    for (i, id) in s.failover_order.iter().enumerate() {
        let Some(p) = s.providers.iter().find(|p| p.id == *id) else {
            continue;
        };
        items.push(FailoverQueueItem {
            provider_id: p.id.clone(),
            provider_name: p.name.clone(),
            sort_index: i,
            is_current: p.id == s.current,
            health: rt.circuits().health(kind.as_str(), &p.id),
        });
    }
    Ok(items)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_to_failover_queue(app: String, provider_id: String) -> Result<bool, String> {
    let kind = parse_app(&app)?;
    let mut file = store::load()?;
    {
        let s = file.for_kind_mut(kind);
        let p = s
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("供应商不存在: {provider_id}"))?
            .clone();
        if p.is_official() && kind == AppKind::Grok {
            return Err("Grok Official 不能加入故障转移队列".into());
        }
        s.normalize_failover_order();
        if !s.failover_order.iter().any(|id| id == &provider_id) {
            s.failover_order.push(provider_id.clone());
        }
        s.normalize_failover_order();
    }
    store::save(&file)?;
    Ok(true)
}

#[tauri::command(rename_all = "camelCase")]
pub fn remove_from_failover_queue(app: String, provider_id: String) -> Result<bool, String> {
    let kind = parse_app(&app)?;
    let mut file = store::load()?;
    {
        let s = file.for_kind_mut(kind);
        s.failover_order.retain(|id| id != &provider_id);
        // Clear flag first so normalize (SSOT=order) cannot re-add from stale flag.
        if let Some(p) = s.providers.iter_mut().find(|p| p.id == provider_id) {
            p.in_failover_queue = false;
        }
        s.normalize_failover_order();
    }
    runtime()
        .circuits()
        .reset_provider(kind.as_str(), &provider_id);
    store::save(&file)?;
    Ok(true)
}

#[tauri::command(rename_all = "camelCase")]
pub fn reorder_failover_queue(app: String, provider_ids: Vec<String>) -> Result<bool, String> {
    let kind = parse_app(&app)?;
    let mut file = store::load()?;
    {
        let s = file.for_kind_mut(kind);
        // Only keep known provider ids, preserve given order
        let mut order = Vec::new();
        for id in provider_ids {
            if s.providers.iter().any(|p| p.id == id) && !order.iter().any(|x| x == &id) {
                order.push(id);
            }
        }
        s.failover_order = order;
        s.normalize_failover_order();
    }
    store::save(&file)?;
    Ok(true)
}

#[tauri::command(rename_all = "camelCase")]
pub fn repair_proxy_takeover(app: String) -> Result<serde_json::Value, String> {
    let kind = parse_app(&app)?;
    let warnings = runtime().repair_takeover(kind)?;
    Ok(serde_json::json!({
        "ok": true,
        "warnings": warnings,
        "status": get_proxy_takeover_status()?,
    }))
}

/// Probe whether the local-routing listen port can be bound.
#[tauri::command(rename_all = "camelCase")]
pub fn check_proxy_listen_port(host: String, port: u16) -> Result<crate::live_config::PortCheckResult, String> {
    Ok(crate::live_config::check_listen_port(&host, port))
}

#[tauri::command(rename_all = "camelCase")]
pub fn reset_provider_circuit(app: String, provider_id: String) -> Result<bool, String> {
    let kind = parse_app(&app)?;
    runtime()
        .circuits()
        .reset_provider(kind.as_str(), &provider_id);
    Ok(true)
}

#[tauri::command(rename_all = "camelCase")]
pub fn stop_proxy_with_restore() -> Result<bool, String> {
    runtime().shutdown_all()?;
    Ok(true)
}

// ── Request logs ──────────────────────────────────────────────────────────

#[tauri::command(rename_all = "camelCase")]
pub fn list_proxy_request_logs(
    filters: Option<super::log_store::RequestLogFilters>,
) -> Result<super::log_store::RequestLogPage, String> {
    super::log_store::list(filters.unwrap_or_default())
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_proxy_request_log(
    id: String,
) -> Result<Option<super::log_store::RequestLogEntry>, String> {
    super::log_store::get(&id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn clear_proxy_request_logs() -> Result<serde_json::Value, String> {
    let deleted = super::log_store::clear_all()?;
    Ok(serde_json::json!({ "ok": true, "deleted": deleted }))
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_proxy_log_retention_days() -> Result<u32, String> {
    // Prefer providers.json value; fall back to log DB meta.
    let from_cfg = store::load()
        .ok()
        .map(|f| f.proxy.log_retention_days)
        .filter(|&d| d > 0);
    if let Some(d) = from_cfg {
        let d = super::log_store::clamp_retention_days(d);
        let _ = super::log_store::set_retention_days(d);
        return Ok(d);
    }
    super::log_store::get_retention_days()
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_proxy_log_retention_days(days: u32) -> Result<u32, String> {
    let days = super::log_store::clamp_retention_days(days);
    // Persist on GlobalProxyConfig so it survives with other route settings.
    let mut file = store::load()?;
    file.proxy.log_retention_days = days;
    store::save(&file)?;
    super::log_store::set_retention_days(days)
}

/// Shared helper used by list_providers enrichment.
pub fn enrich_provider_health(app: &str, provider_id: &str) -> String {
    runtime().circuits().health(app, provider_id)
}

pub fn failover_priority(store: &AppProviderStore, provider_id: &str) -> Option<usize> {
    // List P1/P2 badges only when local routing is on. Queue can still be curated
    // in route settings without takeover; show priority even if auto-FO is off.
    if !store.takeover_enabled {
        return None;
    }
    if store.failover_order.is_empty() {
        if !store.auto_failover_enabled {
            return None;
        }
        let mut queued: Vec<&Provider> = store
            .providers
            .iter()
            .filter(|p| p.in_failover_queue)
            .collect();
        queued.sort_by_key(|p| p.sort_index.unwrap_or(9999));
        return queued
            .iter()
            .position(|p| p.id == provider_id)
            .map(|i| i + 1);
    }
    store
        .failover_order
        .iter()
        .position(|id| id == provider_id)
        .map(|i| i + 1)
}
