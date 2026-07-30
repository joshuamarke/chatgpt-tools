//! Grok Build live config helpers (`~/.grok/config.toml`).
//! Grok Build live config read/write, with MCP section preservation.

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};
use toml_edit::DocumentMut;

use super::models::Provider;

/// Official default model (Grok Build docs + `grok models`).
pub const DEFAULT_MODEL: &str = "grok-4.5";
/// Project default for *custom* third-party profiles (Responses-friendly).
/// Official built-in models omit `api_backend` (Grok defaults to chat_completions).
pub const DEFAULT_API_BACKEND: &str = "responses";
pub const DEFAULT_CONTEXT_WINDOW: i64 = 500_000;
/// Official xAI Platform API (API-key / BYOK path).
pub const OFFICIAL_API_BASE_URL: &str = "https://api.x.ai/v1";
/// Official Grok CLI chat proxy used with SpaceXAI / grok.com OAuth.
pub const OFFICIAL_CLI_CHAT_PROXY: &str = "https://cli-chat-proxy.grok.com/v1";

/// Canonical official profile: default model only, no custom `[model.*]` base_url.
/// Auth is `~/.grok/auth.json` via `grok login` (or `XAI_API_KEY` env).
pub fn official_settings_config() -> Value {
    json!({ "config": official_config_toml() })
}

pub fn official_config_toml() -> String {
    format!(
        r#"# ChatGPT Tools · Grok Official
# Default model uses built-in SpaceXAI hosting (OAuth via grok login).
# CLI chat proxy: {proxy}
# Platform API:   {api}  (XAI_API_KEY)
# Do not add [model.*] base_url overrides here.

[models]
default = {model}
"#,
        proxy = OFFICIAL_CLI_CHAT_PROXY,
        api = OFFICIAL_API_BASE_URL,
        model = toml_string(DEFAULT_MODEL),
    )
}

pub fn is_official_base_url(url: &str) -> bool {
    let u = url.trim().trim_end_matches('/').to_ascii_lowercase();
    u == OFFICIAL_API_BASE_URL
        || u == "https://api.x.ai"
        || u == OFFICIAL_CLI_CHAT_PROXY
        || u.starts_with("https://cli-chat-proxy.grok.com")
}

pub fn grok_home_dir() -> PathBuf {
    crate::sessions::default_grok_home_dir()
}

pub fn config_path() -> PathBuf {
    grok_home_dir().join("config.toml")
}

pub fn read_config_text() -> Result<String, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(&path).map_err(|e| format!("读取 Grok config.toml 失败: {e}"))
}

pub fn validate_syntax(config_toml: &str) -> Result<(), String> {
    if config_toml.trim().is_empty() {
        return Ok(());
    }
    config_toml
        .parse::<toml::Value>()
        .map(|_| ())
        .map_err(|e| format!("Grok config.toml 格式错误: {e}"))
}

/// Full custom-model shape validation (non-official).
pub fn validate_custom(config_toml: &str) -> Result<(), String> {
    let document = config_toml
        .parse::<toml::Value>()
        .map_err(|e| format!("Grok config.toml 格式错误: {e}"))?;
    let root = document
        .as_table()
        .ok_or_else(|| "Grok 配置必须是 TOML 表结构".to_string())?;
    let models = root
        .get("models")
        .and_then(|v| v.as_table())
        .ok_or_else(|| "Grok 配置缺少 [models]".to_string())?;
    let default_model = models
        .get("default")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Grok 配置缺少 models.default".to_string())?;
    let model_entries = root
        .get("model")
        .and_then(|v| v.as_table())
        .ok_or_else(|| "Grok 配置缺少 [model.<name>]".to_string())?;
    let selected = model_entries
        .get(default_model)
        .and_then(|v| v.as_table())
        .ok_or_else(|| format!("Grok 配置缺少 [model.\"{default_model}\"]"))?;

    for key in ["model", "base_url", "name", "api_backend"] {
        let ok = selected
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        if !ok {
            return Err(format!("Grok 配置缺少有效的 {key} 字段"));
        }
    }
    let has_key = selected
        .get("api_key")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
        || selected
            .get("env_key")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
    if !has_key {
        return Err("Grok 配置缺少 api_key 或 env_key".into());
    }
    let cw = selected
        .get("context_window")
        .and_then(|v| v.as_integer())
        .filter(|v| *v > 0);
    if cw.is_none() {
        return Err("Grok context_window 必须是正整数".into());
    }
    Ok(())
}

pub fn validate_for_switch(provider: &Provider) -> Result<(), String> {
    if provider.is_official() {
        return Ok(());
    }
    let config = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Grok 配置缺少 config 字段".to_string())?;
    validate_custom(config)
}

pub fn build_custom_config(
    profile: &str,
    model: &str,
    base_url: &str,
    name: &str,
    api_key: &str,
    api_backend: &str,
    context_window: i64,
) -> String {
    let profile = if profile.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        profile.trim()
    };
    let model = if model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        model.trim()
    };
    let base_url = base_url.trim().trim_end_matches('/');
    let name = if name.trim().is_empty() {
        profile
    } else {
        name.trim()
    };
    let backend = normalize_api_backend(api_backend);
    let cw = if context_window > 0 {
        context_window
    } else {
        DEFAULT_CONTEXT_WINDOW
    };
    format!(
        r#"[models]
default = {profile_q}

[model.{profile_q}]
model = {model_q}
base_url = {url_q}
name = {name_q}
api_key = {key_q}
api_backend = {backend_q}
context_window = {cw}
"#,
        profile_q = toml_string(profile),
        model_q = toml_string(model),
        url_q = toml_string(base_url),
        name_q = toml_string(name),
        key_q = toml_string(api_key),
        backend_q = toml_string(&backend),
        cw = cw,
    )
}

pub fn normalize_api_backend(backend: &str) -> String {
    match backend.trim().to_ascii_lowercase().as_str() {
        "chat" | "chat_completions" | "openai_chat" | "completions" => {
            "chat_completions".into()
        }
        "messages" | "anthropic" => "messages".into(),
        _ => DEFAULT_API_BACKEND.into(),
    }
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

struct GrokFields {
    profile: String,
    model: String,
    base_url: String,
    name: String,
    api_key: Option<String>,
    api_backend: String,
    context_window: i64,
}

fn extract_fields(config_toml: &str) -> Option<GrokFields> {
    let document = config_toml.parse::<toml::Value>().ok()?;
    let root = document.as_table()?;
    let default_model = root
        .get("models")?
        .as_table()?
        .get("default")?
        .as_str()?
        .trim();
    let selected = root
        .get("model")?
        .as_table()?
        .get(default_model)?
        .as_table()?;
    let context_window = selected
        .get("context_window")
        .and_then(|v| v.as_integer())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_CONTEXT_WINDOW);
    let api_backend = selected
        .get("api_backend")
        .and_then(|v| v.as_str())
        .map(normalize_api_backend)
        .unwrap_or_else(|| DEFAULT_API_BACKEND.into());
    Some(GrokFields {
        profile: default_model.to_string(),
        model: selected.get("model")?.as_str()?.trim().to_string(),
        base_url: selected
            .get("base_url")?
            .as_str()?
            .trim_end_matches('/')
            .to_string(),
        name: selected
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(default_model)
            .to_string(),
        api_key: selected
            .get("api_key")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        api_backend,
        context_window,
    })
}

/// Patch form fields into existing Grok config (preserve MCP / extra keys when present).
pub fn patch_config_from_form(
    existing: &str,
    profile: &str,
    model: &str,
    base_url: &str,
    name: &str,
    api_key: &str,
    api_backend: &str,
    context_window: i64,
) -> Result<String, String> {
    let profile = if profile.trim().is_empty() {
        model.trim()
    } else {
        profile.trim()
    };
    let profile = if profile.is_empty() { DEFAULT_MODEL } else { profile };
    let model = if model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        model.trim()
    };
    let base_url = base_url.trim().trim_end_matches('/');
    let name = if name.trim().is_empty() { profile } else { name.trim() };
    let backend = normalize_api_backend(api_backend);
    let cw = if context_window > 0 {
        context_window
    } else {
        DEFAULT_CONTEXT_WINDOW
    };

    if existing.trim().is_empty() {
        return Ok(build_custom_config(
            profile, model, base_url, name, api_key, &backend, cw,
        ));
    }

    let mut doc = existing
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Grok config.toml: {e}"))?;

    // [models].default
    {
        let root = doc.as_table_mut();
        if !root.contains_key("models") {
            root.insert("models", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let models = root
            .get_mut("models")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Grok config.toml 中 [models] 非法".to_string())?;
        models.insert("default", toml_edit::value(profile));
    }

    // [model.<profile>]
    {
        let root = doc.as_table_mut();
        if !root.contains_key("model") {
            root.insert("model", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let model_root = root
            .get_mut("model")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Grok config.toml 中 [model] 非法".to_string())?;
        if model_root.get(profile).is_none() {
            model_root.insert(profile, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let table = model_root
            .get_mut(profile)
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| format!("Grok config.toml 中 [model.\"{profile}\"] 非法"))?;
        table.insert("model", toml_edit::value(model));
        table.insert("base_url", toml_edit::value(base_url));
        table.insert("name", toml_edit::value(name));
        table.insert("api_backend", toml_edit::value(backend.as_str()));
        table.insert("context_window", toml_edit::value(cw));
        if api_key.trim().is_empty() {
            // leave existing key if present
            if table
                .get("api_key")
                .and_then(|i| i.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none()
                && table
                    .get("env_key")
                    .and_then(|i| i.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .is_none()
            {
                // no key yet — allow draft; validate_custom will catch on activate
            }
        } else {
            table.insert("api_key", toml_edit::value(api_key.trim()));
        }
    }

    Ok(doc.to_string())
}

#[derive(Debug, Clone)]
pub struct LiveSnapshot {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub has_api_key: bool,
    pub config_exists: bool,
    pub is_official_shape: bool,
}

pub fn read_live_snapshot() -> LiveSnapshot {
    let config = read_config_text().unwrap_or_default();
    let fields = extract_fields(&config);
    LiveSnapshot {
        base_url: fields.as_ref().map(|f| f.base_url.clone()),
        model: fields.as_ref().map(|f| f.model.clone()),
        has_api_key: fields.as_ref().and_then(|f| f.api_key.as_ref()).is_some(),
        config_exists: config_path().exists(),
        is_official_shape: is_official_live_config(&config),
    }
}

/// Official live shape: no third-party `[model.*].base_url` / `[endpoints].models_base_url`.
/// Having `[models] default = "grok-4.5"` alone is still official (matches Grok docs).
pub fn is_official_live_config(config_toml: &str) -> bool {
    let trimmed = config_toml.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.lines().all(|l| {
        let t = l.trim();
        t.is_empty() || t.starts_with('#')
    }) {
        return true;
    }
    let Ok(document) = config_toml.parse::<toml::Value>() else {
        return false;
    };
    let Some(root) = document.as_table() else {
        return false;
    };

    // Corporate / custom models endpoint override
    if let Some(url) = root
        .get("endpoints")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("models_base_url"))
        .and_then(|v| v.as_str())
    {
        if !is_official_base_url(url) {
            return false;
        }
    }

    // Any [model.<id>] with a non-official base_url ⇒ third-party / BYOK proxy
    if let Some(models) = root.get("model").and_then(|v| v.as_table()) {
        for (_name, entry) in models {
            let Some(table) = entry.as_table() else {
                continue;
            };
            if let Some(url) = table.get("base_url").and_then(|v| v.as_str()) {
                if !is_official_base_url(url) {
                    return false;
                }
            }
        }
    }

    true
}

/// Remove custom model endpoint overrides; keep UI / MCP / features / etc.
pub fn strip_to_official_routing(config_toml: &str) -> Result<String, String> {
    if config_toml.trim().is_empty() {
        return Ok(official_config_toml());
    }
    let mut doc = config_toml
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Grok config.toml: {e}"))?;

    // Drop entire [model] table (custom endpoints). Built-in models need no table.
    doc.as_table_mut().remove("model");

    // Drop non-official endpoints.models_base_url
    if let Some(endpoints) = doc
        .get_mut("endpoints")
        .and_then(|i| i.as_table_like_mut())
    {
        if let Some(url) = endpoints
            .get("models_base_url")
            .and_then(|i| i.as_str())
            .map(str::to_string)
        {
            if !is_official_base_url(&url) {
                endpoints.remove("models_base_url");
            }
        }
    }

    // Ensure [models].default points at the official default when missing.
    {
        let root = doc.as_table_mut();
        if !root.contains_key("models") {
            root.insert("models", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        if let Some(models) = root.get_mut("models").and_then(|i| i.as_table_like_mut()) {
            let has_default = models
                .get("default")
                .and_then(|i| i.as_str())
                .map(str::trim)
                .is_some_and(|s| !s.is_empty());
            if !has_default {
                models.insert("default", toml_edit::value(DEFAULT_MODEL));
            }
        }
    }

    let out = doc.to_string();
    if out.trim().is_empty() {
        return Ok(official_config_toml());
    }
    Ok(out)
}

/// Whether stored provider roughly matches live routing.
///
/// Compares **base_url** (and loose API-key presence). Do **not** compare
/// default `model` — users switch models in Grok freely; that must not mark drift.
pub fn matches_live(provider: &Provider, live: &LiveSnapshot) -> bool {
    if provider.is_official() {
        return live.is_official_shape;
    }
    let (key, base, _model) = summarize(provider);
    match (base.as_deref(), live.base_url.as_deref()) {
        (Some(a), Some(b)) if a.trim_end_matches('/') == b.trim_end_matches('/') => {
            // Key presence is soft: live may keep env/login material.
            key.is_some() == live.has_api_key || live.has_api_key || key.is_some()
        }
        (None, None) => true,
        _ => false,
    }
}

/// Merge provider archive into live while preserving unmanaged live sections.
/// Routing keys owned by the archive: models / model / endpoints.
fn merge_preserve_mcp(new_config: &str, live_config: &str) -> Result<String, String> {
    // Prefer live as base so UI/tools/features survive; overlay archive routing.
    merge_preserve_sections(new_config, live_config)
}

pub fn write_live(provider: &Provider) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    let config = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Grok 配置缺少 config 字段".to_string())?;

    if !provider.is_official() {
        validate_custom(config)?;
    } else {
        validate_syntax(config)?;
    }

    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 ~/.grok 失败: {e}"))?;
    }

    // Locked read-modify-write so proxy/skin (Codex) style races are avoided on Grok too.
    let is_official = provider.is_official();
    let archive = config.to_string();
    crate::live_config::read_modify_write(&path, |live_existing| {
        let _ = backup_live(live_existing);
        if is_official {
            let base = if live_existing.trim().is_empty() {
                if archive.trim().is_empty() {
                    official_config_toml()
                } else {
                    archive.clone()
                }
            } else {
                let merged = merge_preserve_sections(&archive, live_existing)?;
                strip_to_official_routing(&merged)?
            };
            Ok(base)
        } else {
            merge_preserve_mcp(&archive, live_existing)
        }
    })?;

    if is_official {
        warnings.push("已切换为 Grok 官方默认渠道。".into());
    }
    // Silent path write — GUI already confirms enable; avoid engineering paths in toast.
    Ok(warnings)
}

/// Merge provider config into live while preserving non-model user sections
/// (mcp, ui, features, tools, …) from live when the new config omits them.
fn merge_preserve_sections(new_config: &str, live_config: &str) -> Result<String, String> {
    let mut new_doc = if new_config.trim().is_empty() {
        DocumentMut::new()
    } else {
        new_config
            .parse::<DocumentMut>()
            .map_err(|e| format!("Invalid Grok config.toml: {e}"))?
    };
    if live_config.trim().is_empty() {
        return Ok(new_doc.to_string());
    }
    let live_doc = live_config
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid live Grok config.toml: {e}"))?;

    // Keys that official seed owns (routing). Everything else can be kept from live.
    const SEED_KEYS: &[&str] = &["models", "model", "endpoints"];
    for (key, item) in live_doc.as_table().iter() {
        if SEED_KEYS.contains(&key) {
            continue;
        }
        if new_doc.get(key).is_none() {
            new_doc[key] = item.clone();
        }
    }
    // Prefer live [models] extras but we'll re-assert default in strip.
    if new_doc.get("models").is_none() {
        if let Some(item) = live_doc.get("models") {
            new_doc["models"] = item.clone();
        }
    }
    Ok(new_doc.to_string())
}

fn backup_live(live_config: &str) -> Result<(), String> {
    if live_config.is_empty() {
        return Ok(());
    }
    let dir = crate::sessions::paths::app_state_dir().join("provider-live-backups");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("grok-config-{stamp}.toml"));
    fs::write(path, live_config).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn backfill_from_live(provider: &mut Provider) -> Result<(), String> {
    if provider.is_official() {
        return Ok(());
    }
    let live = read_config_text()?;
    if live.trim().is_empty() || is_official_live_config(&live) {
        return Ok(());
    }
    let live_fields = match extract_fields(&live) {
        Some(f) => f,
        None => return Ok(()),
    };
    // Never backfill loopback proxy URLs into archives.
    {
        let cfg = super::store::load().map(|f| f.proxy).unwrap_or_default();
        if crate::proxy::is_proxy_base_url(&live_fields.base_url, &cfg)
            || live_fields.base_url.contains("127.0.0.1")
        {
            return Ok(());
        }
    }
    let stored_base = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_fields)
        .map(|f| f.base_url);
    if stored_base.as_deref() != Some(live_fields.base_url.as_str()) {
        return Ok(());
    }
    // Strip MCP from stored profile (MCP stays live-only)
    let stripped = strip_mcp(&live).unwrap_or(live);
    if let Some(obj) = provider.settings_config.as_object_mut() {
        obj.insert("config".into(), Value::String(stripped));
    }
    provider.updated_at = Some(chrono::Utc::now().timestamp_millis());
    Ok(())
}

fn strip_mcp(config: &str) -> Result<String, String> {
    let mut doc = config
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Grok config.toml: {e}"))?;
    doc.as_table_mut().remove("mcp_servers");
    doc.as_table_mut().remove("mcp");
    Ok(doc.to_string())
}

pub fn settings_from_form(
    api_key: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
    config_toml: Option<&str>,
    name: &str,
    category: Option<&str>,
    existing: Option<&Provider>,
    keep_existing_key: bool,
    profile: Option<&str>,
    api_backend: Option<&str>,
    context_window: Option<i64>,
    use_config_toml: bool,
) -> Result<Value, String> {
    let is_official = category == Some("official");
    if is_official {
        // Always keep canonical official defaults — never promote live imports.
        return Ok(official_settings_config());
    }

    let mut key = api_key.map(str::trim).unwrap_or("").to_string();
    if key.is_empty() && keep_existing_key {
        if let Some(p) = existing {
            if let Some(prev) = p
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .and_then(extract_fields)
                .and_then(|f| f.api_key)
            {
                key = prev;
            }
        }
    }

    let raw_toml = config_toml.map(str::trim).filter(|s| !s.is_empty());
    let form_url = base_url.map(str::trim).filter(|s| !s.is_empty());
    let advanced = use_config_toml || (raw_toml.is_some() && form_url.is_none());

    if advanced {
        let raw = raw_toml.ok_or_else(|| "高级模式需要填写 config.toml".to_string())?;
        let mut text = raw.to_string();
        // If key provided and differs, inject into TOML
        if let Some(fields) = extract_fields(&text) {
            if !key.is_empty() && fields.api_key.as_deref() != Some(key.as_str()) {
                text = patch_config_from_form(
                    &text,
                    &fields.profile,
                    &fields.model,
                    &fields.base_url,
                    &fields.name,
                    &key,
                    &fields.api_backend,
                    fields.context_window,
                )?;
            }
        }
        // Drafts without key: syntax only; complete configs: full validate
        if extract_fields(&text)
            .and_then(|f| f.api_key)
            .filter(|k| !k.is_empty())
            .is_some()
        {
            validate_custom(&text)?;
        } else {
            validate_syntax(&text)?;
        }
        return Ok(json!({ "config": text }));
    }

    let url = form_url.unwrap_or("");
    if url.is_empty() {
        return Err("请填写 Base URL".into());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("Base URL 必须以 http:// 或 https:// 开头".into());
    }
    let model_val = model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_MODEL);
    let profile_val = profile
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(model_val);
    let backend = api_backend.unwrap_or(DEFAULT_API_BACKEND);
    let cw = context_window
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_CONTEXT_WINDOW);

    let existing_cfg = existing
        .and_then(|p| p.settings_config.get("config"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let text = if existing_cfg.trim().is_empty() {
        build_custom_config(profile_val, model_val, url, name, &key, backend, cw)
    } else {
        patch_config_from_form(
            existing_cfg,
            profile_val,
            model_val,
            url,
            name,
            &key,
            backend,
            cw,
        )?
    };

    if key.is_empty() {
        validate_syntax(&text)?;
    } else {
        validate_custom(&text)?;
    }
    Ok(json!({ "config": text }))
}

pub fn summarize(provider: &Provider) -> (Option<String>, Option<String>, Option<String>) {
    if provider.is_official() {
        return (
            None,
            Some(OFFICIAL_CLI_CHAT_PROXY.into()),
            Some(DEFAULT_MODEL.into()),
        );
    }
    let config = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if let Some(f) = extract_fields(config) {
        (f.api_key, Some(f.base_url), Some(f.model))
    } else {
        (None, None, None)
    }
}

pub fn summarize_extra(provider: &Provider) -> (String, String, i64) {
    if provider.is_official() {
        return (DEFAULT_MODEL.into(), "built-in".into(), 0);
    }
    let config = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if let Some(f) = extract_fields(config) {
        (f.profile, f.api_backend, f.context_window)
    } else {
        (DEFAULT_MODEL.into(), DEFAULT_API_BACKEND.into(), DEFAULT_CONTEXT_WINDOW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_live_ignores_default_model_switch() {
        // Archive default model A; live user switched to model B — same base_url.
        let archive_cfg = r#"
[models]
default = "model-a"

[model."model-a"]
model = "model-a"
base_url = "https://proxy.example.com/v1"
api_key = "sk-test"
api_backend = "responses"
context_window = 200000
"#;
        let live_cfg = r#"
[models]
default = "model-b"

[model."model-b"]
model = "model-b"
base_url = "https://proxy.example.com/v1"
api_key = "sk-test"
api_backend = "responses"
context_window = 200000

[model."model-a"]
model = "model-a"
base_url = "https://proxy.example.com/v1"
api_key = "sk-test"
api_backend = "responses"
context_window = 200000
"#;
        let provider = Provider::new(
            "g-custom".into(),
            "Custom".into(),
            json!({ "config": archive_cfg }),
        );
        // Live snapshot as if user switched models in Grok GUI.
        let live = LiveSnapshot {
            base_url: Some("https://proxy.example.com/v1".into()),
            model: Some("model-b".into()),
            has_api_key: true,
            config_exists: true,
            is_official_shape: is_official_live_config(live_cfg),
        };
        assert!(
            matches_live(&provider, &live),
            "switching models.default must not mark 本机漂移 when base_url is unchanged"
        );

        // Different base_url should still be drift.
        let live_other = LiveSnapshot {
            base_url: Some("https://other.example.com/v1".into()),
            model: Some("model-a".into()),
            has_api_key: true,
            config_exists: true,
            is_official_shape: false,
        };
        assert!(!matches_live(&provider, &live_other));
    }

    #[test]
    fn official_live_allows_models_default_without_custom_base() {
        let official = r#"
[models]
default = "grok-4.5"

[ui]
max_thoughts_width = 120
"#;
        assert!(is_official_live_config(official));
        assert!(is_official_live_config(""));
    }

    #[test]
    fn official_live_rejects_third_party_model_base() {
        let third = r#"
[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://proxy.example.com/v1"
api_key = "sk-test"
api_backend = "responses"
context_window = 500000
"#;
        assert!(!is_official_live_config(third));
        let cleaned = strip_to_official_routing(third).expect("strip");
        assert!(is_official_live_config(&cleaned));
        assert!(!cleaned.contains("proxy.example.com"));
        assert!(cleaned.contains("grok-4.5"));
    }

    #[test]
    fn edit_form_fields_update_profile_backend_and_window() {
        let existing_cfg = build_custom_config(
            "grok-4.5",
            "grok-4.5",
            "https://old.example.com/v1",
            "Old",
            "sk-old",
            "responses",
            500_000,
        );
        let existing = Provider::new(
            "g1".into(),
            "Old".into(),
            json!({ "config": existing_cfg }),
        );

        let settings = settings_from_form(
            None,
            Some("https://new.example.com/v1"),
            Some("grok-new"),
            None,
            "New Grok",
            Some("custom"),
            Some(&existing),
            true,
            Some("my-profile"),
            Some("chat_completions"),
            Some(256_000),
            false,
        )
        .expect("grok edit save");

        let config = settings.get("config").and_then(|v| v.as_str()).unwrap();
        assert!(config.contains("https://new.example.com/v1"));
        assert!(config.contains("grok-new"));
        assert!(config.contains("my-profile"));
        assert!(config.contains("chat_completions"));
        assert!(config.contains("256000") || config.contains("256_000"));
        assert!(config.contains("sk-old"));
    }
}
