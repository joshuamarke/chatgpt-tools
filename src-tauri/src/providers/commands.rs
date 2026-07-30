//! Tauri IPC for provider management.

use super::catalog;
use super::codex;
use super::grok;
use super::models::{
    AppKind, LivePaths, LiveStatus, LocalProxyRequestOverrides, Provider, ProviderDetail,
    ProviderListResponse, ProviderMeta, ProviderSummary, ProviderUpsertRequest, SwitchResult,
};
use super::presets;
use super::store;
use serde_json::Value;
use tauri::AppHandle;

/// Keep the tray menu in sync after archive / current-provider mutations.
fn notify_tray(app: &AppHandle) {
    crate::tray::notify_providers_changed(app);
}

fn parse_app(app: &str) -> Result<AppKind, String> {
    AppKind::from_str_loose(app)
        .ok_or_else(|| format!("未知应用类型: {app}（支持 codex / grok）"))
}

fn live_paths(kind: AppKind) -> LivePaths {
    match kind {
        AppKind::Codex => LivePaths {
            home: codex::codex_home_dir().display().to_string(),
            auth: Some(codex::auth_path().display().to_string()),
            config: codex::config_path().display().to_string(),
        },
        AppKind::Grok => LivePaths {
            home: grok::grok_home_dir().display().to_string(),
            auth: None,
            config: grok::config_path().display().to_string(),
        },
    }
}

fn is_ready(kind: AppKind, p: &Provider) -> bool {
    if p.is_official() {
        return true;
    }
    match kind {
        AppKind::Codex => codex::validate_for_switch(p).is_ok(),
        AppKind::Grok => grok::validate_for_switch(p).is_ok(),
    }
}

fn live_status_for(
    kind: AppKind,
    current: &str,
    providers: &[Provider],
    takeover: bool,
) -> LiveStatus {
    let proxy_cfg = store::load().map(|f| f.proxy).unwrap_or_default();
    let active_id = crate::proxy::proxy_status_snapshot()
        .active_targets
        .iter()
        .find(|t| t.app_type == kind.as_str())
        .map(|t| t.provider_id.clone());

    let build = |snap_base: Option<String>,
                 snap_model: Option<String>,
                 snap_wire: Option<String>,
                 config_exists: bool,
                 auth_exists: bool,
                 has_api_key: bool,
                 direct_matches: bool|
     -> LiveStatus {
        let proxy_live = snap_base
            .as_deref()
            .map(|u| crate::proxy::is_proxy_base_url(u, &proxy_cfg))
            .unwrap_or(false);
        let host = crate::proxy::proxy_connect_host(&proxy_cfg);
        let port = proxy_cfg.listen_port;

        let (mode, detail_code, matches, summary) = if takeover && proxy_live {
            let desync = active_id
                .as_ref()
                .map(|a| a != current && !current.is_empty())
                .unwrap_or(false);
            if desync {
                (
                    "takeover",
                    "route_desync",
                    false,
                    format!(
                        "本地路由已开，但当前上游与启用的供应商不一致，可点「修复路由」· {host}:{port}"
                    ),
                )
            } else {
                (
                    "takeover",
                    "ok",
                    true,
                    format!("本地路由已开启 · {host}:{port}"),
                )
            }
        } else if takeover && !proxy_live {
            (
                "broken",
                "route_half",
                false,
                "本地路由标记异常，请点「修复路由」".into(),
            )
        } else if !takeover && proxy_live {
            (
                "broken",
                "route_half",
                false,
                "本机仍指向本地代理，但路由已关闭，请点「修复路由」或重新启用供应商".into(),
            )
        } else if !config_exists {
            (
                "direct",
                "missing",
                false,
                if kind == AppKind::Grok {
                    "尚未检测到 Grok 本机配置".into()
                } else {
                    "尚未检测到 Codex 本机配置".into()
                },
            )
        } else if current.is_empty() {
            // No archive marked enabled — not the same as “Official drifted”.
            (
                "direct",
                "unlinked",
                false,
                "本机已有配置，可「从本机配置导入」或添加后启用".into(),
            )
        } else if direct_matches {
            (
                "direct",
                "ok",
                true,
                "当前供应商与本机配置一致".into(),
            )
        } else {
            (
                "direct",
                "drift",
                false,
                // Drift = routing (base_url / official shape), never default model alone.
                if kind == AppKind::Grok {
                    "供应商与本机配置不一致（渠道地址可能已在外部更改）".into()
                } else {
                    "供应商与本机配置不一致（渠道地址可能已在外部更改）".into()
                },
            )
        };

        LiveStatus {
            config_exists,
            auth_exists,
            base_url: snap_base,
            model: snap_model,
            wire_api: snap_wire,
            has_api_key,
            current_matches_live: matches,
            summary: Some(summary),
            mode: Some(mode.into()),
            detail_code: Some(detail_code.into()),
        }
    };

    match kind {
        AppKind::Codex => {
            let snap = codex::read_live_snapshot();
            let current_p = providers.iter().find(|p| p.id == current);
            let direct = current_p
                .map(|p| codex::matches_live(p, &snap))
                .unwrap_or(false);
            build(
                snap.base_url,
                snap.model,
                snap.wire_api,
                snap.config_exists,
                snap.auth_exists,
                snap.has_api_key,
                direct,
            )
        }
        AppKind::Grok => {
            let snap = grok::read_live_snapshot();
            let current_p = providers.iter().find(|p| p.id == current);
            let direct = current_p
                .map(|p| grok::matches_live(p, &snap))
                .unwrap_or(false);
            build(
                snap.base_url,
                snap.model,
                None,
                snap.config_exists,
                false,
                snap.has_api_key,
                direct,
            )
        }
    }
}

fn to_summary(
    kind: AppKind,
    p: &Provider,
    current: &str,
    matches_live: bool,
    app_store: &crate::providers::models::AppProviderStore,
) -> ProviderSummary {
    let (key, base, model) = match kind {
        AppKind::Codex => codex::summarize(p),
        AppKind::Grok => grok::summarize(p),
    };
    let wire = match kind {
        AppKind::Codex => codex::summarize_wire(p),
        AppKind::Grok => None,
    };
    let failover_priority =
        crate::proxy::commands::failover_priority(app_store, &p.id);
    // Circuit health is only relevant while local routing can actually walk the queue.
    let health = if app_store.takeover_enabled
        && (p.in_failover_queue || app_store.auto_failover_enabled)
    {
        Some(crate::proxy::commands::enrich_provider_health(
            kind.as_str(),
            &p.id,
        ))
    } else {
        None
    };
    ProviderSummary {
        id: p.id.clone(),
        name: p.name.clone(),
        is_current: p.id == current,
        matches_live: p.id == current && matches_live,
        website_url: p.website_url.clone(),
        category: p.category.clone(),
        notes: p.notes.clone(),
        api_key_preview: key.as_deref().and_then(store::mask_api_key),
        base_url: base,
        model,
        wire_api: wire,
        ready: is_ready(kind, p),
        in_failover_queue: p.in_failover_queue,
        failover_priority,
        health,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

/// List providers for an app (codex | grok).
#[tauri::command]
pub fn list_providers(app: String) -> Result<ProviderListResponse, String> {
    let kind = parse_app(&app)?;
    let file = store::load()?;
    let app_store = file.for_kind(kind);
    let current = app_store.current.clone();
    let takeover = app_store.takeover_enabled;
    let live = live_status_for(kind, &current, &app_store.providers, takeover);
    let matches = live.current_matches_live;

    let mut providers: Vec<ProviderSummary> = app_store
        .providers
        .iter()
        .map(|p| to_summary(kind, p, &current, matches, app_store))
        .collect();
    providers.sort_by(|a, b| {
        let ao = app_store
            .providers
            .iter()
            .find(|p| p.id == a.id)
            .and_then(|p| p.sort_index)
            .unwrap_or(999);
        let bo = app_store
            .providers
            .iter()
            .find(|p| p.id == b.id)
            .and_then(|p| p.sort_index)
            .unwrap_or(999);
        ao.cmp(&bo)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let proxy_running = crate::proxy::runtime().is_running();
    Ok(ProviderListResponse {
        app: kind.as_str().into(),
        current,
        providers,
        live_paths: live_paths(kind),
        live_status: live,
        preserve_codex_official_auth: file.preserve_codex_official_auth,
        takeover_enabled: takeover,
        auto_failover_enabled: app_store.auto_failover_enabled,
        proxy: file.proxy.clone(),
        proxy_running,
        proxy_status: Some(crate::proxy::proxy_status_snapshot()),
    })
}

/// Whether third-party Codex enables leave `auth.json` OAuth intact (default true).
#[tauri::command(rename_all = "camelCase")]
pub fn get_preserve_codex_official_auth() -> Result<bool, String> {
    Ok(store::preserve_codex_official_auth())
}

/// Toggle Codex official-login preservation on third-party switch.
#[tauri::command(rename_all = "camelCase")]
pub fn set_preserve_codex_official_auth(enabled: bool) -> Result<bool, String> {
    store::set_preserve_codex_official_auth(enabled)
}

/// Get full provider detail for the edit form (includes API key).
#[tauri::command]
pub fn get_provider(app: String, id: String) -> Result<ProviderDetail, String> {
    let kind = parse_app(&app)?;
    let file = store::load()?;
    let app_store = file.for_kind(kind);
    let p = store::find_provider(app_store, &id).ok_or_else(|| format!("供应商不存在: {id}"))?;
    let (key, base, model) = match kind {
        AppKind::Codex => codex::summarize(p),
        AppKind::Grok => grok::summarize(p),
    };
    let wire = match kind {
        AppKind::Codex => codex::summarize_wire(p).unwrap_or_else(|| "responses".into()),
        AppKind::Grok => "responses".into(),
    };
    let reasoning = match kind {
        AppKind::Codex => codex::summarize_reasoning(p).unwrap_or_else(|| "high".into()),
        AppKind::Grok => String::new(),
    };
    let (profile, api_backend, context_window) = match kind {
        AppKind::Codex => (String::new(), String::new(), 0),
        AppKind::Grok => grok::summarize_extra(p),
    };
    let archive_toml = p
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Codex: show full live-aware config in the advanced editor so users never
    // only see a short routing template and wipe MCP/desktop on save.
    let config_toml = match kind {
        AppKind::Codex => codex::config_for_editor(&archive_toml),
        AppKind::Grok => archive_toml,
    };
    let (custom_user_agent, headers_json, body_json) = meta_form_fields(p);
    let model_catalog = model_catalog_rows(p);
    let live = live_status_for(
        kind,
        &app_store.current,
        &app_store.providers,
        app_store.takeover_enabled,
    );
    let matches = p.id == app_store.current && live.current_matches_live;
    Ok(ProviderDetail {
        id: p.id.clone(),
        name: p.name.clone(),
        website_url: p.website_url.clone(),
        category: p.category.clone(),
        notes: p.notes.clone(),
        api_key: key.unwrap_or_default(),
        base_url: base.unwrap_or_default(),
        model: model.unwrap_or_default(),
        wire_api: wire,
        reasoning_effort: reasoning,
        profile,
        api_backend,
        context_window,
        config_toml,
        custom_user_agent,
        local_proxy_headers_json: headers_json,
        local_proxy_body_json: body_json,
        model_catalog,
        is_current: p.id == app_store.current,
        is_official: p.is_official(),
        ready: is_ready(kind, p),
        matches_live: matches,
    })
}

fn meta_form_fields(p: &Provider) -> (String, String, String) {
    let meta = p.meta.as_ref();
    let ua = meta
        .and_then(|m| m.custom_user_agent.clone())
        .unwrap_or_default();
    let headers = meta
        .and_then(|m| m.local_proxy_request_overrides.as_ref())
        .and_then(|o| o.headers.as_ref())
        .map(|h| serde_json::to_string_pretty(h).unwrap_or_default())
        .unwrap_or_default();
    let body = meta
        .and_then(|m| m.local_proxy_request_overrides.as_ref())
        .and_then(|o| o.body.as_ref())
        .map(|b| serde_json::to_string_pretty(b).unwrap_or_default())
        .unwrap_or_default();
    (ua, headers, body)
}

fn model_catalog_rows(p: &Provider) -> Vec<Value> {
    p.settings_config
        .get("modelCatalog")
        .and_then(|c| c.get("models"))
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default()
}

fn parse_meta_from_request(request: &ProviderUpsertRequest) -> Result<Option<ProviderMeta>, String> {
    let ua = request
        .custom_user_agent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let headers = parse_headers_json(request.local_proxy_headers_json.as_deref())?;
    let body = parse_body_json(request.local_proxy_body_json.as_deref())?;

    if ua.is_none() && headers.is_none() && body.is_none() {
        return Ok(None);
    }
    let overrides = if headers.is_some() || body.is_some() {
        Some(LocalProxyRequestOverrides { headers, body })
    } else {
        None
    };
    Ok(Some(ProviderMeta {
        custom_user_agent: ua,
        local_proxy_request_overrides: overrides,
    }))
}

fn parse_headers_json(
    raw: Option<&str>,
) -> Result<Option<serde_json::Map<String, Value>>, String> {
    let Some(text) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("Header 覆盖 JSON 无效: {e}"))?;
    let obj = value
        .as_object()
        .ok_or_else(|| "Header 覆盖必须是 JSON 对象".to_string())?
        .clone();
    for (k, v) in &obj {
        if k.trim().is_empty() {
            return Err("Header 名不能为空".into());
        }
        if !v.is_string() {
            return Err(format!("Header「{k}」的值必须是字符串"));
        }
    }
    Ok(Some(obj))
}

fn parse_body_json(raw: Option<&str>) -> Result<Option<Value>, String> {
    let Some(text) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("Body 覆盖 JSON 无效: {e}"))?;
    if !value.is_object() {
        return Err("Body 覆盖必须是 JSON 对象".into());
    }
    Ok(Some(value))
}

fn build_settings(
    kind: AppKind,
    request: &ProviderUpsertRequest,
    name: &str,
    category: Option<&str>,
    existing: Option<&Provider>,
    keep: bool,
) -> Result<serde_json::Value, String> {
    let use_toml = request.use_config_toml.unwrap_or(false);
    let mut settings = match kind {
        AppKind::Codex => codex::settings_from_form(
            request.api_key.as_deref(),
            request.base_url.as_deref(),
            request.model.as_deref(),
            request.config_toml.as_deref(),
            name,
            category,
            existing,
            keep,
            request.wire_api.as_deref(),
            request.reasoning_effort.as_deref(),
            use_toml,
        )?,
        AppKind::Grok => grok::settings_from_form(
            request.api_key.as_deref(),
            request.base_url.as_deref(),
            request.model.as_deref(),
            request.config_toml.as_deref(),
            name,
            category,
            existing,
            keep,
            request.profile.as_deref(),
            request.api_backend.as_deref(),
            request.context_window,
            use_toml,
        )?,
    };

    // Codex: attach modelCatalog for model_catalog_json projection (profile SSOT).
    // Always merge default model into mapping so enable never projects a GPT-only
    // catalog when the user only filled the single "模型" field with a third-party id.
    if kind == AppKind::Codex {
        if let Some(obj) = settings.as_object_mut() {
            if let Some(rows) = request.model_catalog.as_ref() {
                if let Some(catalog) = catalog::model_catalog_value_from_rows(rows) {
                    obj.insert("modelCatalog".into(), catalog);
                } else {
                    // Explicit empty mapping clears stored catalog (and live pointer on enable).
                    obj.remove("modelCatalog");
                }
            } else if let Some(existing) = existing {
                // Keep previous catalog when request omits the field entirely
                if let Some(prev) = existing.settings_config.get("modelCatalog") {
                    obj.insert("modelCatalog".into(), prev.clone());
                }
            }
        }

        // Merge form default model (+ any catalog rows already present).
        let mut merge = Vec::new();
        if let Some(m) = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            merge.push(m.to_string());
        }
        // Also pull model= from generated config when form used structured fields.
        if let Some(cfg) = settings.get("config").and_then(|v| v.as_str()) {
            if let Some(m) = codex::extract_model(cfg) {
                merge.push(m);
            }
        }
        let _ = catalog::merge_models_into_settings(&mut settings, merge);

        // If mapping exists but top-level model is empty, fill from first mapped model.
        let config_snapshot = settings
            .get("config")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if let Some(config) = config_snapshot {
            if let Ok(patched) = catalog::ensure_config_model_from_catalog(&config, &settings) {
                if patched != config {
                    if let Some(obj) = settings.as_object_mut() {
                        obj.insert("config".into(), Value::String(patched));
                    }
                }
            }
        }
    }

    Ok(settings)
}

/// Add a new provider profile.
#[tauri::command]
pub fn add_provider(
    app_handle: AppHandle,
    app: String,
    request: ProviderUpsertRequest,
) -> Result<ProviderSummary, String> {
    let kind = parse_app(&app)?;
    let name = request.name.trim();
    if name.is_empty() {
        return Err("供应商名称不能为空".into());
    }
    let mut file = store::load()?;
    let app_store = file.for_kind_mut(kind);

    let id = request
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            let frag = store::sanitize_id_fragment(name);
            store::next_id(&frag)
        });
    if store::find_provider(app_store, &id).is_some() {
        return Err(format!("供应商 ID 已存在: {id}"));
    }

    let category = request.category.clone().or_else(|| Some("custom".into()));
    if category.as_deref() == Some("official") {
        return Err("不能通过表单新增官方供应商".into());
    }

    let settings = build_settings(kind, &request, name, category.as_deref(), None, false)?;

    let meta = parse_meta_from_request(&request)?;
    let mut provider = Provider::new(id.clone(), name.to_string(), settings);
    provider.website_url = request
        .website_url
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    provider.category = category;
    provider.notes = request
        .notes
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    provider.meta = meta;
    provider.sort_index = Some(app_store.providers.len());

    let activate = request.activate.unwrap_or(false);
    if activate {
        validate_switch(kind, &provider)?;
    }

    let takeover_on = app_store.takeover_enabled;
    app_store.providers.push(provider.clone());
    store::save(&file)?;
    if activate {
        if takeover_on {
            // Live first via hot_switch (sets current only after success).
            crate::proxy::runtime().hot_switch_current(kind, &id)?;
            file = store::load()?;
        } else {
            let mut file2 = store::load()?;
            let app_store = file2.for_kind_mut(kind);
            let prev = app_store.current.clone();
            if !prev.is_empty() && prev != id {
                if let Some(old) = store::find_provider_mut(app_store, &prev) {
                    let _ = backfill(kind, old);
                }
            }
            write_live_for(kind, &provider)?;
            app_store.current = id;
            store::save(&file2)?;
            file = file2;
        }
    }

    let app_ref = file.for_kind(kind);
    let current = app_ref.current.clone();
    let live = live_status_for(
        kind,
        &current,
        &app_ref.providers,
        app_ref.takeover_enabled,
    );
    let summary = to_summary(
        kind,
        store::find_provider(app_ref, &provider.id).unwrap_or(&provider),
        &current,
        live.current_matches_live,
        app_ref,
    );
    notify_tray(&app_handle);
    Ok(summary)
}

/// Update an existing provider.
#[tauri::command]
pub fn update_provider(
    app_handle: AppHandle,
    app: String,
    id: String,
    request: ProviderUpsertRequest,
) -> Result<ProviderSummary, String> {
    let kind = parse_app(&app)?;
    let name = request.name.trim();
    if name.is_empty() {
        return Err("供应商名称不能为空".into());
    }
    let mut file = store::load()?;
    let app_store = file.for_kind_mut(kind);
    let existing = store::find_provider(app_store, &id)
        .ok_or_else(|| format!("供应商不存在: {id}"))?
        .clone();

    let category = existing.category.clone();
    let is_official = existing.is_official();
    let keep = request.keep_existing_api_key.unwrap_or(true);

    let settings = if is_official {
        // Re-assert canonical official defaults (never keep third-party shaped config).
        match kind {
            AppKind::Codex => {
                let mut s = codex::official_settings_config();
                if let Some(auth) = existing.settings_config.get("auth") {
                    if auth.as_object().is_some_and(|o| {
                        o.keys().any(|k| k != "OPENAI_API_KEY" && k != "auth_mode")
                    }) {
                        if let Some(obj) = s.as_object_mut() {
                            obj.insert("auth".into(), auth.clone());
                        }
                    }
                }
                s
            }
            AppKind::Grok => grok::official_settings_config(),
        }
    } else {
        build_settings(
            kind,
            &request,
            name,
            category.as_deref(),
            Some(&existing),
            keep,
        )?
    };

    let current_id = app_store.current.clone();
    let activate = request.activate.unwrap_or(false);

    let p = store::find_provider_mut(app_store, &id)
        .ok_or_else(|| format!("供应商不存在: {id}"))?;
    p.name = name.to_string();
    p.settings_config = settings;
    // Always apply optional fields from the form (empty clears).
    if !is_official {
        p.website_url = request
            .website_url
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    } else if let Some(url) = request
        .website_url
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        p.website_url = Some(url);
    }
    p.notes = request
        .notes
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if !is_official {
        p.meta = parse_meta_from_request(&request)?;
    }
    p.updated_at = Some(chrono::Utc::now().timestamp_millis());

    let provider_clone = p.clone();
    let was_current = provider_clone.id == current_id;

    let takeover_on = app_store.takeover_enabled;
    store::save(&file)?;
    if activate {
        validate_switch(kind, &provider_clone)?;
        if takeover_on {
            crate::proxy::runtime().hot_switch_current(kind, &id)?;
            file = store::load()?;
        } else {
            let mut file2 = store::load()?;
            let app_store = file2.for_kind_mut(kind);
            if !was_current && !current_id.is_empty() {
                if let Some(old) = store::find_provider_mut(app_store, &current_id) {
                    let _ = backfill(kind, old);
                }
            }
            write_live_for(kind, &provider_clone)?;
            app_store.current = id.clone();
            store::save(&file2)?;
            file = file2;
        }
    } else if was_current {
        if takeover_on {
            crate::proxy::runtime().hot_switch_current(kind, &id)?;
            file = store::load()?;
        } else if let Err(e) = write_live_for(kind, &provider_clone) {
            log_warn(&format!("更新后写 live 失败: {e}"));
        }
    }
    let app_ref = file.for_kind(kind);
    let current = app_ref.current.clone();
    let live = live_status_for(
        kind,
        &current,
        &app_ref.providers,
        app_ref.takeover_enabled,
    );
    let p = store::find_provider(app_ref, &id).unwrap();
    let summary = to_summary(
        kind,
        p,
        &current,
        live.current_matches_live,
        app_ref,
    );
    notify_tray(&app_handle);
    Ok(summary)
}

fn log_warn(msg: &str) {
    eprintln!("[providers] {msg}");
}

/// Delete a provider (cannot delete the active one or official seeds).
#[tauri::command]
pub fn delete_provider(app_handle: AppHandle, app: String, id: String) -> Result<bool, String> {
    let kind = parse_app(&app)?;
    let mut file = store::load()?;
    let app_store = file.for_kind_mut(kind);
    let p = store::find_provider(app_store, &id).ok_or_else(|| format!("供应商不存在: {id}"))?;
    if p.is_official() {
        return Err("不能删除官方供应商".into());
    }
    if p.id == app_store.current {
        return Err("不能删除当前启用的供应商，请先切换到其他供应商".into());
    }
    app_store.providers.retain(|x| x.id != id);
    store::save(&file)?;
    notify_tray(&app_handle);
    Ok(true)
}

/// Switch active provider and write live config files.
#[tauri::command]
pub fn switch_provider(
    app_handle: AppHandle,
    app: String,
    id: String,
) -> Result<SwitchResult, String> {
    let kind = parse_app(&app)?;
    let mut file = store::load()?;
    let takeover = file.for_kind(kind).takeover_enabled;

    // Local routing: hot-switch without dual-writing real upstream into live.
    if takeover {
        let warnings = crate::proxy::runtime().hot_switch_current(kind, &id)?;
        let file2 = store::load()?;
        let name = file2
            .for_kind(kind)
            .providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| id.clone());
        // Unlock / clear already handled inside proxy hot_switch / apply_takeover.
        let projected = if kind == AppKind::Codex {
            catalog::model_slugs_from_catalog_file(&codex::codex_home_dir())
        } else {
            Vec::new()
        };
        let result = SwitchResult {
            ok: true,
            warnings,
            message: format!(
                "已热切换到「{name}」（本地路由模式，通常无需重启 {}）",
                kind.display_name()
            ),
            live_paths: Some(live_paths(kind)),
            projected_models: projected,
        };
        notify_tray(&app_handle);
        return Ok(result);
    }

    let app_store = file.for_kind_mut(kind);
    let provider = store::find_provider(app_store, &id)
        .ok_or_else(|| format!("供应商不存在: {id}"))?
        .clone();

    validate_switch(kind, &provider)?;

    // Backfill outgoing current from live (skip when live is loopback proxy)
    let prev = app_store.current.clone();
    if !prev.is_empty() && prev != id {
        if let Some(old) = store::find_provider_mut(app_store, &prev) {
            let _ = backfill(kind, old);
        }
    }

    let mut warnings = write_live_for(kind, &provider)?;
    app_store.current = id.clone();

    // Persist any modelCatalog merge (default model ∪ mapping) back into the archive
    // so the next edit shows the same list that was projected to live.
    if kind == AppKind::Codex && !provider.is_official() {
        if let Some(p) = store::find_provider_mut(app_store, &id) {
            let mut settings = p.settings_config.clone();
            let mut merge = catalog::model_slugs_from_settings(&settings);
            if let Some(m) = codex::extract_model(
                settings
                    .get("config")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            ) {
                merge.push(m);
            }
            let live_slugs =
                catalog::model_slugs_from_catalog_file(&codex::codex_home_dir());
            merge.extend(live_slugs);
            if catalog::merge_models_into_settings(&mut settings, merge) {
                p.settings_config = settings;
                p.updated_at = Some(chrono::Utc::now().timestamp_millis());
            }
        }
    }

    store::save(&file)?;

    let projected = if kind == AppKind::Codex && !provider.is_official() {
        let models = catalog::model_slugs_from_catalog_file(&codex::codex_home_dir());
        if models.is_empty() {
            warnings.push("模型映射为空，请先添加可用模型。".into());
        }
        models
    } else {
        Vec::new()
    };

    // App-specific, short — no inject / catalog engineering text.
    let msg = format!("已启用「{}」", provider.name);

    let result = SwitchResult {
        ok: true,
        warnings,
        message: msg,
        live_paths: Some(live_paths(kind)),
        projected_models: projected,
    };
    notify_tray(&app_handle);
    Ok(result)
}

fn validate_switch(kind: AppKind, provider: &Provider) -> Result<(), String> {
    match kind {
        AppKind::Codex => codex::validate_for_switch(provider),
        AppKind::Grok => grok::validate_for_switch(provider),
    }
}

fn backfill(kind: AppKind, provider: &mut Provider) -> Result<(), String> {
    match kind {
        AppKind::Codex => codex::backfill_from_live(provider),
        AppKind::Grok => grok::backfill_from_live(provider),
    }
}

fn write_live_for(kind: AppKind, provider: &Provider) -> Result<Vec<String>, String> {
    match kind {
        AppKind::Codex => codex::write_live(provider),
        AppKind::Grok => grok::write_live(provider),
    }
}

/// Import current live config as a new named provider.
#[tauri::command]
pub fn import_live_as_provider(
    app_handle: AppHandle,
    app: String,
    name: Option<String>,
) -> Result<ProviderSummary, String> {
    let kind = parse_app(&app)?;
    let display = name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("从本机配置导入 · {}", kind.display_name()));

    let settings = match kind {
        AppKind::Codex => {
            let auth = codex::read_auth();
            let config = codex::read_config_text()?;
            codex::validate_config_toml(&config)?;
            // Lift bearer into auth for storage
            let key = codex::extract_api_key(&auth, &config).unwrap_or_default();
            let cleaned = {
                // Use stored shape without bearer in config when possible
                let mut c = config.clone();
                if let Ok(stripped) = strip_codex_bearer(&c) {
                    c = stripped;
                }
                c
            };
            serde_json::json!({
                "auth": { "OPENAI_API_KEY": key },
                "config": cleaned
            })
        }
        AppKind::Grok => {
            let config = grok::read_config_text()?;
            grok::validate_syntax(&config)?;
            if !config.trim().is_empty() && !grok::is_official_live_config(&config) {
                grok::validate_custom(&config).map_err(|e| {
                    format!("当前 live 配置无法作为自定义供应商导入: {e}")
                })?;
            }
            serde_json::json!({ "config": config })
        }
    };

    let mut file = store::load()?;
    let app_store = file.for_kind_mut(kind);
    let frag = store::sanitize_id_fragment(&display);
    let id = store::next_id(&frag);
    let mut provider = Provider::new(id, display, settings);
    // Detect category
    let is_official_like = match kind {
        AppKind::Codex => provider
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .map(|c| c.trim().is_empty())
            .unwrap_or(true),
        AppKind::Grok => provider
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .map(grok::is_official_live_config)
            .unwrap_or(true),
    };
    // Always custom: Official seeds are fixed defaults, never promoted from live import.
    provider.category = Some("custom".into());
    provider.notes = Some(if is_official_like {
        "从本机配置导入（官方形态副本；内置 Official 种子保持独立）".into()
    } else {
        "从本机正在使用的配置导入".into()
    });
    provider.sort_index = Some(app_store.providers.len());

    let current = app_store.current.clone();
    // Snapshot store for summary before move
    let summary = {
        let mut tmp_store = app_store.clone();
        tmp_store.providers.push(provider.clone());
        to_summary(kind, &provider, &current, false, &tmp_store)
    };
    app_store.providers.push(provider);
    store::save(&file)?;
    notify_tray(&app_handle);
    Ok(summary)
}

fn strip_codex_bearer(config: &str) -> Result<String, String> {
    use toml_edit::DocumentMut;
    if !config.contains("experimental_bearer_token") {
        return Ok(config.to_string());
    }
    let mut doc = config
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid config: {e}"))?;
    if let Some(pid) = doc
        .get("model_provider")
        .and_then(|i| i.as_str())
        .map(str::to_string)
    {
        if let Some(t) = doc
            .get_mut("model_providers")
            .and_then(|i| i.as_table_like_mut())
            .and_then(|t| t.get_mut(pid.as_str()))
            .and_then(|i| i.as_table_like_mut())
        {
            t.remove("experimental_bearer_token");
        }
    }
    doc.as_table_mut().remove("experimental_bearer_token");
    Ok(doc.to_string())
}

/// Paths + current pointer (for status bar / diagnostics).
#[tauri::command]
pub fn provider_paths_info(app: String) -> Result<serde_json::Value, String> {
    let kind = parse_app(&app)?;
    let file = store::load()?;
    let app_store = file.for_kind(kind);
    let paths = live_paths(kind);
    let live = live_status_for(
        kind,
        &app_store.current,
        &app_store.providers,
        app_store.takeover_enabled,
    );
    Ok(serde_json::json!({
        "app": kind.as_str(),
        "current": app_store.current,
        "storePath": store::providers_file_path().display().to_string(),
        "livePaths": paths,
        "liveStatus": live,
        "count": app_store.providers.len(),
        "takeoverEnabled": app_store.takeover_enabled,
        "proxyRunning": crate::proxy::runtime().is_running(),
    }))
}

/// List built-in channel presets for the add form.
#[tauri::command]
pub fn list_provider_presets(app: String) -> Result<Vec<presets::ProviderPreset>, String> {
    let kind = parse_app(&app)?;
    Ok(presets::list_presets(kind.as_str()))
}

/// Re-apply current provider to live (repair / force sync).
#[tauri::command]
pub fn reapply_current_provider(app_handle: AppHandle, app: String) -> Result<SwitchResult, String> {
    let kind = parse_app(&app)?;
    let file = store::load()?;
    let app_store = file.for_kind(kind);
    let id = app_store.current.clone();
    if id.is_empty() {
        return Err("没有当前启用的供应商".into());
    }
    let takeover = app_store.takeover_enabled;
    let provider = store::find_provider(app_store, &id)
        .ok_or_else(|| format!("当前供应商不存在: {id}"))?
        .clone();
    if takeover {
        let warnings = crate::proxy::runtime().hot_switch_current(kind, &id)?;
        let projected = if kind == AppKind::Codex {
            let models = catalog::model_slugs_from_catalog_file(&codex::codex_home_dir());
            // Silent unlock / clear — do not toast inject diagnostics.
            if provider.is_official() {
                let _ = super::model_unlock::on_official_activated();
            } else {
                let _ = super::model_unlock::notify_provider_or_catalog_changed();
            }
            models
        } else {
            Vec::new()
        };
        let result = SwitchResult {
            ok: true,
            warnings,
            message: format!("已重新应用「{}」", provider.name),
            live_paths: Some(live_paths(kind)),
            projected_models: projected,
        };
        notify_tray(&app_handle);
        return Ok(result);
    }
    validate_switch(kind, &provider)?;
    let mut warnings = write_live_for(kind, &provider)?;
    let projected = if kind == AppKind::Codex {
        catalog::model_slugs_from_catalog_file(&codex::codex_home_dir())
    } else {
        Vec::new()
    };
    if kind == AppKind::Codex && projected.is_empty() && !provider.is_official() {
        warnings.push(
            "模型映射为空：请拉取/添加第三方模型到映射表后重新应用。".into(),
        );
    }
    let result = SwitchResult {
        ok: true,
        warnings,
        message: format!("已重新应用「{}」", provider.name),
        live_paths: Some(live_paths(kind)),
        projected_models: projected,
    };
    notify_tray(&app_handle);
    Ok(result)
}

/// Best-effort: re-inject Codex desktop model whitelist from live catalog.
#[tauri::command]
pub fn refresh_codex_model_unlock() -> Result<Value, String> {
    let result = super::model_unlock::try_inject_from_live_catalog();
    Ok(serde_json::json!({
        "attempted": result.attempted,
        "ok": result.ok,
        "models": result.models,
        "message": result.message,
        "skippedUnchanged": result.skipped_unchanged,
    }))
}

// ── Connectivity / model list (add-provider form helpers) ───────────────────

/// Lightweight base_url reachability probe.
/// Any HTTP response = reachable; only network-level failures count as down.
#[tauri::command(rename_all = "camelCase")]
pub async fn test_provider_connectivity(
    base_url: String,
    timeout_secs: Option<u64>,
    custom_user_agent: Option<String>,
) -> Result<super::probe::ConnectivityResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        super::probe::test_connectivity_with_ua(
            &base_url,
            timeout_secs,
            custom_user_agent.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("连通测试任务失败: {e}"))
}

/// Fetch OpenAI-compatible model list for the add/edit form.
/// Tries `{base}/v1/models` and known fallbacks (versioned paths, Anthropic subpaths).
#[tauri::command(rename_all = "camelCase")]
pub async fn fetch_provider_models(
    base_url: String,
    api_key: String,
    models_url: Option<String>,
    custom_user_agent: Option<String>,
) -> Result<Vec<super::probe::FetchedModel>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        super::probe::fetch_models_with_ua(
            &base_url,
            &api_key,
            models_url.as_deref(),
            custom_user_agent.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("拉取模型任务失败: {e}"))?
}
