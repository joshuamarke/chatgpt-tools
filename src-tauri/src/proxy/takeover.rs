//! Live config rewrite / restore for local routing takeover.

use std::fs;
use std::path::PathBuf;

use toml_edit::DocumentMut;

use super::{
    CODEX_OFFICIAL_PROXY_PROVIDER_ID, CODEX_PROXY_PROVIDER_ID, PROXY_MANAGED,
};
use crate::providers::codex;
use crate::providers::grok;
use crate::providers::models::{AppKind, GlobalProxyConfig, Provider};

fn backup_dir() -> PathBuf {
    crate::sessions::paths::app_state_dir().join("proxy-live-backups")
}

fn backup_path(app: AppKind, name: &str) -> PathBuf {
    backup_dir().join(format!("{app}-{name}.bak", app = app.as_str()))
}

pub fn proxy_connect_host(cfg: &GlobalProxyConfig) -> String {
    match cfg.listen_address.as_str() {
        "0.0.0.0" => "127.0.0.1".into(),
        "::" => "[::1]".into(),
        other => other.to_string(),
    }
}

pub fn proxy_base_url_for(kind: AppKind, cfg: &GlobalProxyConfig) -> String {
    let host = proxy_connect_host(cfg);
    match kind {
        AppKind::Codex => format!("http://{host}:{}/v1", cfg.listen_port),
        AppKind::Grok => format!("http://{host}:{}/grok/v1", cfg.listen_port),
    }
}

pub fn is_proxy_base_url(url: &str, cfg: &GlobalProxyConfig) -> bool {
    let u = url.trim().trim_end_matches('/').to_ascii_lowercase();
    let host = proxy_connect_host(cfg).to_ascii_lowercase();
    let codex = format!("http://{host}:{}/v1", cfg.listen_port).to_ascii_lowercase();
    let grok = format!("http://{host}:{}/grok/v1", cfg.listen_port).to_ascii_lowercase();
    u == codex || u == grok || u.contains(&format!(":{}/v1", cfg.listen_port))
}

fn ensure_backup_dir() -> Result<(), String> {
    fs::create_dir_all(backup_dir()).map_err(|e| format!("创建代理备份目录失败: {e}"))
}

/// Snapshot live files before first takeover for this app.
pub fn backup_live(kind: AppKind) -> Result<(), String> {
    ensure_backup_dir()?;
    match kind {
        AppKind::Codex => {
            let cfg = codex::read_config_text().unwrap_or_default();
            fs::write(backup_path(kind, "config.toml"), cfg.as_bytes())
                .map_err(|e| format!("备份 Codex config 失败: {e}"))?;
            // Auth is never rewritten by takeover; still snapshot for diagnostics.
            let auth = serde_json::to_string_pretty(&codex::read_auth())
                .unwrap_or_else(|_| "{}".into());
            let _ = fs::write(backup_path(kind, "auth.json"), auth.as_bytes());
        }
        AppKind::Grok => {
            let cfg = grok::read_config_text().unwrap_or_default();
            fs::write(backup_path(kind, "config.toml"), cfg.as_bytes())
                .map_err(|e| format!("备份 Grok config 失败: {e}"))?;
        }
    }
    Ok(())
}

/// Project current provider into live as local-proxy routing (config only; never touch OAuth auth.json).
/// Used when **first enabling** local routing or repairing a broken route shell.
pub fn apply_takeover_live(kind: AppKind, provider: &Provider, cfg: &GlobalProxyConfig) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    match kind {
        AppKind::Codex => {
            let path = codex::config_path();
            let base = proxy_base_url_for(kind, cfg);
            let provider = provider.clone();
            let home = codex::codex_home_dir();
            crate::live_config::read_modify_write(&path, |live| {
                let rewritten = if provider.is_official() {
                    apply_codex_official_proxy_route(live, &base)?
                } else {
                    apply_codex_third_party_proxy_route(live, &provider, &base)?
                };
                let mut final_cfg = rewritten;
                // Never clobber skin-managed [desktop] appearance* pins.
                final_cfg = codex::preserve_live_desktop_appearance(live, &final_cfg)?;
                if !provider.is_official() {
                    final_cfg = crate::providers::catalog::ensure_config_model_from_catalog(
                        &final_cfg,
                        &provider.settings_config,
                    )?;
                    let model = codex::extract_model(&final_cfg);
                    final_cfg = crate::providers::catalog::prepare_config_with_catalog(
                        &home,
                        &provider.settings_config,
                        &final_cfg,
                        model.as_deref(),
                    )?;
                    final_cfg = codex::preserve_live_desktop_appearance(live, &final_cfg)?;
                }
                Ok(final_cfg)
            })?;
            warnings.push("已开启 Codex 本地路由。".into());
            if provider.is_official() {
                crate::providers::model_unlock::schedule_official_activated();
            } else {
                push_codex_catalog_warnings(&mut warnings, &home, &provider);
            }
        }
        AppKind::Grok => {
            if provider.is_official() {
                return Err("Grok 官方渠道不支持本地路由，请改用自定义供应商。".into());
            }
            let path = grok::config_path();
            let base = proxy_base_url_for(kind, cfg);
            let provider = provider.clone();
            crate::live_config::read_modify_write(&path, |live| {
                apply_grok_proxy_route(live, &provider, &base)
            })?;
            warnings.push("已开启 Grok 本地路由。".into());
        }
    }
    Ok(warnings)
}

/// Hot-switch under an **already-active** local route.
///
/// Does **not** rewrite `model_providers` when the proxy shell is already in place:
/// only updates logical current (caller's job), top-level `model`, and model catalog.
/// Falls back to full [`apply_takeover_live`] if live is not yet proxy-shaped
/// (or official ↔ third-party kind changed).
pub fn hot_switch_live(
    kind: AppKind,
    provider: &Provider,
    cfg: &GlobalProxyConfig,
) -> Result<Vec<String>, String> {
    if !live_matches_proxy(kind, cfg) || codex_proxy_kind_mismatch(kind, provider) {
        return apply_takeover_live(kind, provider, cfg);
    }

    let mut warnings = Vec::new();
    match kind {
        AppKind::Codex => {
            let path = codex::config_path();
            let home = codex::codex_home_dir();
            let provider = provider.clone();
            crate::live_config::read_modify_write(&path, |live| {
                // Preserve entire live document including model_providers; only model + catalog.
                hot_switch_codex_model_and_catalog(live, &provider, &home)
            })?;
            warnings.push(format!("已切换到「{}」。", provider.name));
            if provider.is_official() {
                crate::providers::model_unlock::schedule_official_activated();
            } else {
                push_codex_catalog_warnings(&mut warnings, &home, &provider);
            }
        }
        AppKind::Grok => {
            if provider.is_official() {
                return Err("本地路由下不能启用 Grok 官方渠道。".into());
            }
            // Live already points at local proxy (`/grok/v1`). Upstream selection is
            // entirely inside the proxy (current + failover); do **not** rewrite
            // ~/.grok/config.toml on hot-switch — only full apply_takeover sets
            // base_url / name=localproxy once when routing is turned on.
            warnings.push(format!(
                "已切换到「{}」（本地路由热切，未改 live 配置）。",
                provider.name
            ));
        }
    }
    Ok(warnings)
}

fn push_codex_catalog_warnings(warnings: &mut Vec<String>, home: &std::path::Path, provider: &Provider) {
    // Silent unlock / clear off-thread — never block hot-switch on CDP.
    if provider.is_official() {
        crate::providers::model_unlock::schedule_official_activated();
    } else {
        crate::providers::model_unlock::schedule_desktop_unlock(Some(
            provider.settings_config.clone(),
        ));
    }
    let projected = crate::providers::catalog::model_slugs_from_catalog_file(home);
    if projected.is_empty() && !provider.is_official() {
        warnings.push("模型映射为空，请先在供应商中添加可用模型。".into());
    }
}

/// Official vs third-party use different proxy table ids — switching kinds needs full apply.
fn codex_proxy_kind_mismatch(kind: AppKind, provider: &Provider) -> bool {
    if kind != AppKind::Codex {
        return false;
    }
    let live = codex::read_config_text().unwrap_or_default();
    let active = live
        .parse::<toml::Value>()
        .ok()
        .and_then(|d| {
            d.get("model_provider")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default();
    if provider.is_official() {
        active != CODEX_OFFICIAL_PROXY_PROVIDER_ID
    } else {
        active != CODEX_PROXY_PROVIDER_ID
    }
}

/// Codex hot-switch: update `model` + catalog only — never touch `model_providers`.
fn hot_switch_codex_model_and_catalog(
    live: &str,
    provider: &Provider,
    home: &std::path::Path,
) -> Result<String, String> {
    let archive = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut doc = if live.trim().is_empty() {
        DocumentMut::new()
    } else {
        live.parse::<DocumentMut>()
            .map_err(|e| format!("Invalid Codex config: {e}"))?
    };

    if provider.is_official() {
        // Official archive is comment-only (no model). Always restore the Codex
        // built-in default so third-party slugs (grok / deepseek / …) do not stick.
        codex::apply_official_default_model(&mut doc);
        // Drop third-party catalog pointer if a previous hot path left it.
        doc.as_table_mut().remove("model_catalog_json");
    } else if let Some(model) = codex::extract_model(archive) {
        // Prefer archive default model; otherwise leave live model alone.
        doc["model"] = toml_edit::value(model);
    }

    let mut final_cfg = doc.to_string();
    final_cfg = codex::preserve_live_desktop_appearance(live, &final_cfg)?;

    if !provider.is_official() {
        final_cfg = crate::providers::catalog::ensure_config_model_from_catalog(
            &final_cfg,
            &provider.settings_config,
        )?;
        let model = codex::extract_model(&final_cfg);
        final_cfg = crate::providers::catalog::prepare_config_with_catalog(
            home,
            &provider.settings_config,
            &final_cfg,
            model.as_deref(),
        )?;
        final_cfg = codex::preserve_live_desktop_appearance(live, &final_cfg)?;
    }
    Ok(final_cfg)
}

/// Initial third-party takeover: keep live as base (MCP / desktop / other tables).
/// **Never** overlay archive `model_providers` onto live — only ensure the stable
/// `chatgpt-tools-proxy` shell and top-level `model`.
fn apply_codex_third_party_proxy_route(
    live: &str,
    provider: &Provider,
    proxy_base: &str,
) -> Result<String, String> {
    let archive = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Prefer existing live so MCP / unmanaged keys survive. Empty live → bare doc
    // (do not merge archive model_providers.custom into live).
    let mut doc = if live.trim().is_empty() {
        DocumentMut::new()
    } else {
        live.parse::<DocumentMut>()
            .map_err(|e| format!("Invalid Codex config: {e}"))?
    };

    // Drop PROXY_MANAGED tokens from non-proxy tables only.
    strip_proxy_placeholders(&mut doc);

    doc["model_provider"] = toml_edit::value(CODEX_PROXY_PROVIDER_ID);

    let wire = codex::extract_wire_api(archive).unwrap_or_else(|| "responses".into());
    ensure_codex_proxy_provider_table(&mut doc, proxy_base, &wire)?;

    // Prefer archive model if present.
    if let Some(model) = codex::extract_model(archive) {
        doc["model"] = toml_edit::value(model);
    } else if doc.get("model").is_none() {
        if let Some(model) = codex::extract_model(live) {
            doc["model"] = toml_edit::value(model);
        }
    }
    Ok(doc.to_string())
}

/// Idempotently ensure `[model_providers.chatgpt-tools-proxy]` has the stable shell.
/// Does not create or rewrite other provider tables.
fn ensure_codex_proxy_provider_table(
    doc: &mut DocumentMut,
    proxy_base: &str,
    wire_api: &str,
) -> Result<(), String> {
    let root = doc.as_table_mut();
    if !root.contains_key("model_providers") {
        root.insert(
            "model_providers",
            toml_edit::Item::Table(toml_edit::Table::new()),
        );
    }
    let providers = root
        .get_mut("model_providers")
        .and_then(|i| i.as_table_like_mut())
        .ok_or_else(|| "model_providers 非法".to_string())?;
    if providers.get(CODEX_PROXY_PROVIDER_ID).is_none() {
        providers.insert(
            CODEX_PROXY_PROVIDER_ID,
            toml_edit::Item::Table(toml_edit::Table::new()),
        );
    }
    let table = providers
        .get_mut(CODEX_PROXY_PROVIDER_ID)
        .and_then(|i| i.as_table_like_mut())
        .ok_or_else(|| "proxy provider table 非法".to_string())?;
    table.insert("name", toml_edit::value(codex::PROVIDER_UI_NAME));
    table.insert("base_url", toml_edit::value(proxy_base));
    table.insert(
        "wire_api",
        toml_edit::value(codex::normalize_wire_api(wire_api)),
    );
    table.insert("requires_openai_auth", toml_edit::value(true));
    table.insert(
        "experimental_bearer_token",
        toml_edit::value(PROXY_MANAGED),
    );
    Ok(())
}

fn apply_codex_official_proxy_route(live: &str, proxy_base: &str) -> Result<String, String> {
    let base = if live.trim().is_empty() {
        codex::official_config_toml()
    } else {
        // Keep MCP etc., strip third-party routing + reset default model first.
        codex::strip_to_official_routing(live).unwrap_or_else(|_| live.to_string())
    };
    let mut doc = base
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config: {e}"))?;
    strip_proxy_placeholders(&mut doc);
    doc.as_table_mut().remove("experimental_bearer_token");
    // Empty live / seed path may still lack model — always pin official default.
    codex::apply_official_default_model(&mut doc);
    doc["model_provider"] = toml_edit::value(CODEX_OFFICIAL_PROXY_PROVIDER_ID);
    {
        let root = doc.as_table_mut();
        if !root.contains_key("model_providers") {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            root.insert("model_providers", toml_edit::Item::Table(t));
        }
        let providers = root
            .get_mut("model_providers")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "model_providers 非法".to_string())?;
        // Remove old custom proxy tables that would confuse Codex.
        let stale: Vec<String> = providers
            .iter()
            .map(|(k, _)| k.to_string())
            .filter(|k| k == CODEX_PROXY_PROVIDER_ID || k == "custom" || k == "cliproxyapi")
            .collect();
        for k in stale {
            providers.remove(&k);
        }
        if providers.get(CODEX_OFFICIAL_PROXY_PROVIDER_ID).is_none() {
            providers.insert(
                CODEX_OFFICIAL_PROXY_PROVIDER_ID,
                toml_edit::Item::Table(toml_edit::Table::new()),
            );
        }
        let table = providers
            .get_mut(CODEX_OFFICIAL_PROXY_PROVIDER_ID)
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "official proxy table 非法".to_string())?;
        table.insert("name", toml_edit::value(codex::PROVIDER_UI_NAME));
        table.insert("base_url", toml_edit::value(proxy_base));
        table.insert("wire_api", toml_edit::value("responses"));
        table.insert("requires_openai_auth", toml_edit::value(true));
        table.insert("supports_websockets", toml_edit::value(true));
        table.remove("experimental_bearer_token");
    }
    Ok(doc.to_string())
}

fn strip_proxy_placeholders(doc: &mut DocumentMut) {
    doc.as_table_mut().remove("experimental_bearer_token");
    if let Some(providers) = doc
        .get_mut("model_providers")
        .and_then(|i| i.as_table_like_mut())
    {
        let keys: Vec<String> = providers.iter().map(|(k, _)| k.to_string()).collect();
        for k in keys {
            if let Some(table) = providers.get_mut(&k).and_then(|i| i.as_table_like_mut()) {
                if table
                    .get("experimental_bearer_token")
                    .and_then(|i| i.as_str())
                    == Some(PROXY_MANAGED)
                {
                    table.remove("experimental_bearer_token");
                }
            }
        }
    }
}

fn apply_grok_proxy_route(
    live: &str,
    provider: &Provider,
    proxy_base: &str,
) -> Result<String, String> {
    let archive = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // Start from richer of live vs archive, then force selected model base_url/api_key.
    let base_text = if live.trim().len() >= archive.trim().len() && !live.trim().is_empty() {
        live
    } else if !archive.trim().is_empty() {
        archive
    } else {
        live
    };
    let mut doc = if base_text.trim().is_empty() {
        DocumentMut::new()
    } else {
        base_text
            .parse::<DocumentMut>()
            .map_err(|e| format!("Invalid Grok config: {e}"))?
    };

    let (profile, model, _backend, _cw) = {
        let (p, b, c) = grok::summarize_extra(provider);
        let (_k, _base, m) = grok::summarize(provider);
        let model = m.filter(|s| !s.is_empty()).unwrap_or_else(|| p.clone());
        (p, model, b, c)
    };
    let model_key = if model.is_empty() { profile.clone() } else { model.clone() };
    if model_key.is_empty() {
        return Err("Grok 供应商缺少 model / profile".into());
    }

    if doc.get("models").is_none() {
        doc["models"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    if let Some(models) = doc.get_mut("models").and_then(|i| i.as_table_like_mut()) {
        models.insert("default", toml_edit::value(model_key.as_str()));
    }

    // Ensure [model."<id>"]
    if doc.get("model").is_none() {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        doc.as_table_mut()
            .insert("model", toml_edit::Item::Table(t));
    }
    let model_root = doc
        .get_mut("model")
        .and_then(|i| i.as_table_like_mut())
        .ok_or_else(|| "Grok [model] 非法".to_string())?;
    if model_root.get(&model_key).is_none() {
        model_root.insert(&model_key, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    let entry = model_root
        .get_mut(&model_key)
        .and_then(|i| i.as_table_like_mut())
        .ok_or_else(|| format!("Grok model.{model_key} 非法"))?;
    entry.insert("base_url", toml_edit::value(proxy_base));
    entry.insert("api_key", toml_edit::value(PROXY_MANAGED));
    // Grok local routing: always stamp model entry name for UI / client identity.
    entry.insert("name", toml_edit::value("localproxy"));
    if entry.get("api_backend").is_none() {
        entry.insert("api_backend", toml_edit::value("responses"));
    }
    Ok(doc.to_string())
}

/// Disable takeover: re-project current provider with normal write_live (direct upstream).
pub fn restore_direct_live(kind: AppKind, provider: &Provider) -> Result<Vec<String>, String> {
    match kind {
        AppKind::Codex => codex::write_live(provider),
        AppKind::Grok => grok::write_live(provider),
    }
}

/// Whether live currently points at our proxy for this app.
pub fn live_matches_proxy(kind: AppKind, cfg: &GlobalProxyConfig) -> bool {
    match kind {
        AppKind::Codex => {
            let text = codex::read_config_text().unwrap_or_default();
            codex::extract_base_url(&text)
                .map(|u| is_proxy_base_url(&u, cfg))
                .unwrap_or(false)
        }
        AppKind::Grok => {
            let text = grok::read_config_text().unwrap_or_default();
            if let Some(f) = extract_grok_fields_loose(&text) {
                is_proxy_base_url(&f, cfg)
            } else {
                false
            }
        }
    }
}

fn extract_grok_fields_loose(config: &str) -> Option<String> {
    let doc = config.parse::<toml::Value>().ok()?;
    let default = doc
        .get("models")?
        .get("default")?
        .as_str()?
        .to_string();
    let url = doc
        .get("model")?
        .get(&default)?
        .get("base_url")?
        .as_str()?
        .to_string();
    Some(url)
}

/// Resolve real upstream base_url + api key from provider archive (not live).
pub fn upstream_from_provider(kind: AppKind, provider: &Provider) -> Result<(String, Option<String>, bool), String> {
    if provider.is_official() {
        return match kind {
            AppKind::Codex => Ok((
                codex::OFFICIAL_API_BASE_URL.to_string(),
                None,
                true, // oauth passthrough
            )),
            AppKind::Grok => Err("Grok Official 不能作为代理上游".into()),
        };
    }
    match kind {
        AppKind::Codex => {
            let (key, base, _model) = codex::summarize(provider);
            let base = base.ok_or_else(|| "供应商缺少 base_url".to_string())?;
            let key = key.ok_or_else(|| "供应商缺少 API Key".to_string())?;
            Ok((base, Some(key), false))
        }
        AppKind::Grok => {
            let (key, base, _model) = grok::summarize(provider);
            let base = base.ok_or_else(|| "供应商缺少 base_url".to_string())?;
            Ok((base, key, false))
        }
    }
}
