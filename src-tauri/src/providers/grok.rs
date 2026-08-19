//! Grok Build live config helpers (`~/.grok/config.toml`).
//! Enabling a supplier patches `[model."chatgpt-tools-proxy"]` and
//! `[models].default`. The supplier display name is written to the `name` field
//! in `[model.*]`. Previous GUI supplier tables are pruned so switches do not stack identities.

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};
use toml_edit::DocumentMut;

use super::models::Provider;

/// Official default model (Grok Build docs + `grok models`).
pub const DEFAULT_MODEL: &str = "grok-4.5";
/// Canonical third-party `[model."<id>"]` / `[models].default` identity.
/// Same public name as Codex local-routing; supplier titles stay in the GUI.
pub const CUSTOM_MODEL_ID: &str = "chatgpt-tools-proxy";
/// Live-only `[model."<id>"]` identity while local routing is on.
/// Direct-connect third-party uses [`CUSTOM_MODEL_ID`]; the proxy table is
/// itself a supplier named `localproxy` and must not be stored in archives.
pub const LOCAL_PROXY_MODEL_ID: &str = "localproxy";
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

fn is_reserved_identity(id: &str) -> bool {
    matches!(id.trim(), CUSTOM_MODEL_ID | LOCAL_PROXY_MODEL_ID | "")
}

/// Custom `[model."<id>"]` / `[models].default` identity.
/// Always [`CUSTOM_MODEL_ID`]. `profile` / `provider_name` are ignored
/// (kept for call-site compatibility); they must never become a table key.
pub fn resolve_model_identity(_profile: &str, _provider_name: &str) -> String {
    CUSTOM_MODEL_ID.to_string()
}

/// Resolve supplier name for `[model.<id>].name`.
/// Prefers `provider_name` if non-empty; falls back to `picker_name` if non-empty and non-reserved;
/// otherwise falls back to `model` (or DEFAULT_MODEL).
pub fn resolve_supplier_name(provider_name: &str, picker_name: &str, model: &str) -> String {
    let p = provider_name.trim();
    if !p.is_empty() && !is_reserved_identity(p) {
        return p.to_string();
    }
    let picker = picker_name.trim();
    if !picker.is_empty() && !is_reserved_identity(picker) {
        return picker.to_string();
    }
    let model = model.trim();
    if !model.is_empty() {
        return model.to_string();
    }
    DEFAULT_MODEL.to_string()
}

pub fn resolve_picker_name(picker: &str, model: &str) -> String {
    resolve_supplier_name(picker, "", model)
}

/// Supplier display name for the edit form: return stored name if valid, otherwise supplier name or model.
pub fn picker_name_for_form(stored: &str, supplier_name: &str, model: &str) -> String {
    let stored = stored.trim();
    if !stored.is_empty() && !is_reserved_identity(stored) {
        return stored.to_string();
    }
    let supplier = supplier_name.trim();
    if !supplier.is_empty() && !is_reserved_identity(supplier) {
        return supplier.to_string();
    }
    resolve_supplier_name("", "", model)
}

fn canonicalize_archive_fields(mut fields: GrokFields) -> GrokFields {
    fields.profile = CUSTOM_MODEL_ID.to_string();
    fields.name = resolve_supplier_name(&fields.name, "", &fields.model);
    fields
}

/// Rewrite a stored third-party fragment onto [`CUSTOM_MODEL_ID`] with supplier name and canonical section ordering.
/// No-op for official / unreadable / live-only `localproxy` archives,
/// or when identity + name + order are already canonical. Used on providers.json load.
pub fn migrate_archive_identity(config: &str, supplier_name: &str) -> Option<String> {
    let fields = extract_fields(config)?;
    if fields.profile == LOCAL_PROXY_MODEL_ID {
        return None;
    }
    let target_name = resolve_supplier_name(supplier_name, &fields.name, &fields.model);
    let needs_id = fields.profile != CUSTOM_MODEL_ID;
    let needs_name = fields.name != target_name;
    let needs_order = !is_model_before_models(config);
    if !needs_id && !needs_name && !needs_order {
        return None;
    }
    Some(build_custom_config_with_picker(
        CUSTOM_MODEL_ID,
        &fields.model,
        &fields.base_url,
        &target_name,
        "",
        fields.api_key.as_deref().unwrap_or(""),
        &fields.api_backend,
        fields.context_window,
    ))
}

fn is_model_before_models(config: &str) -> bool {
    let pos_model = config.find("[model.");
    let pos_models = config.find("[models]");
    match (pos_model, pos_models) {
        (Some(m), Some(ms)) => m < ms,
        _ => true,
    }
}

pub fn build_custom_config_with_picker(
    _profile: &str,
    model: &str,
    base_url: &str,
    provider_name: &str,
    picker_name: &str,
    api_key: &str,
    api_backend: &str,
    context_window: i64,
) -> String {
    let profile = CUSTOM_MODEL_ID;
    let model = if model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        model.trim()
    };
    let base_url = base_url.trim().trim_end_matches('/');
    let name = resolve_supplier_name(provider_name, picker_name, model);
    let backend = normalize_api_backend(api_backend);
    let cw = if context_window > 0 {
        context_window
    } else {
        DEFAULT_CONTEXT_WINDOW
    };
    format!(
        r#"[model.{profile_q}]
model = {model_q}
base_url = {url_q}
name = {name_q}
api_key = {key_q}
api_backend = {backend_q}
context_window = {cw}

[models]
default = {profile_q}
"#,
        profile_q = toml_string(&profile),
        model_q = toml_string(model),
        url_q = toml_string(base_url),
        name_q = toml_string(&name),
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

/// TOML basic-string encoding. Never interpolates raw user text (quotes,
/// backslashes, and control chars are escaped) so a hostile model / name
/// cannot inject extra TOML lines.
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let n = c as u32;
                if n <= 0xFFFF {
                    out.push_str(&format!("\\u{n:04x}"));
                } else {
                    out.push_str(&format!("\\U{n:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
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

/// `[models].default` is the catalog identity (table key), not the API model id.
fn read_default_identity(config: &str) -> Option<String> {
    config
        .parse::<toml::Value>()
        .ok()?
        .get("models")?
        .as_table()?
        .get("default")?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn extract_fields(config_toml: &str) -> Option<GrokFields> {
    let document = config_toml.parse::<toml::Value>().ok()?;
    let root = document.as_table()?;
    let model_entries = root.get("model")?.as_table()?;
    let profile = root
        .get("models")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("default"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let selected = model_entries.get(&profile)?.as_table()?;
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
    let model = selected
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(profile.as_str())
        .to_string();
    Some(GrokFields {
        profile: profile.clone(),
        model,
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
            .unwrap_or(profile.as_str())
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
    patch_config_from_form_with_picker(
        existing,
        profile,
        model,
        base_url,
        name,
        "",
        api_key,
        api_backend,
        context_window,
    )
}

pub fn patch_config_from_form_with_picker(
    existing: &str,
    profile: &str,
    model: &str,
    base_url: &str,
    provider_name: &str,
    picker_name: &str,
    api_key: &str,
    api_backend: &str,
    context_window: i64,
) -> Result<String, String> {
    let identity = CUSTOM_MODEL_ID.to_string();
    let _ = profile;
    let model = if model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        model.trim()
    };
    let base_url = base_url.trim().trim_end_matches('/');
    let name = resolve_supplier_name(provider_name, picker_name, model);
    let backend = normalize_api_backend(api_backend);
    let cw = if context_window > 0 {
        context_window
    } else {
        DEFAULT_CONTEXT_WINDOW
    };

    if existing.trim().is_empty() {
        return Ok(build_custom_config_with_picker(
            &identity,
            model,
            base_url,
            provider_name,
            &name,
            api_key,
            &backend,
            cw,
        ));
    }

    let old_default = read_default_identity(existing);

    let mut doc = existing
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Grok config.toml: {e}"))?;

    {
        let root = doc.as_table_mut();
        if !root.contains_key("model") {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            root.insert("model", toml_edit::Item::Table(t));
        }
        let model_root = root
            .get_mut("model")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Grok config.toml 中 [model] 非法".to_string())?;
        if model_root.get(&identity).is_none() {
            model_root.insert(&identity, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let table = model_root
            .get_mut(&identity)
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| format!("Grok config.toml 中 [model.\"{identity}\"] 非法"))?;
        table.insert("model", toml_edit::value(model));
        table.insert("base_url", toml_edit::value(base_url));
        table.insert("name", toml_edit::value(name.as_str()));
        table.insert("api_backend", toml_edit::value(backend.as_str()));
        table.insert("context_window", toml_edit::value(cw));
        if !api_key.trim().is_empty() {
            table.insert("api_key", toml_edit::value(api_key.trim()));
        }

        let mut drop = vec![LOCAL_PROXY_MODEL_ID.to_string()];
        if let Some(old) = old_default {
            if old != identity {
                drop.push(old);
            }
        }
        if model != identity {
            drop.push(model.to_string());
        }
        prune_model_tables(model_root, &identity, &drop);
    }

    {
        let root = doc.as_table_mut();
        if !root.contains_key("models") {
            root.insert("models", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let models = root
            .get_mut("models")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Grok config.toml 中 [models] 非法".to_string())?;
        models.insert("default", toml_edit::value(identity.as_str()));
    }

    Ok(doc.to_string())
}

fn table_has_third_party_base_url(table: &dyn toml_edit::TableLike) -> bool {
    table
        .get("base_url")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some_and(|url| !is_official_base_url(url))
}

fn prune_model_tables(
    model_root: &mut dyn toml_edit::TableLike,
    keep: &str,
    extra_drop: &[String],
) {
    let keys: Vec<String> = model_root.iter().map(|(k, _)| k.to_string()).collect();
    for key in keys {
        if key == keep {
            continue;
        }
        if extra_drop.iter().any(|d| d == &key) {
            model_root.remove(&key);
            continue;
        }
        let is_stale_route = model_root
            .get(&key)
            .and_then(|i| i.as_table_like())
            .is_some_and(table_has_third_party_base_url);
        if is_stale_route {
            model_root.remove(&key);
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiveSnapshot {
    /// `[models].default` catalog identity (table key), even when no matching table exists.
    pub identity: Option<String>,
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
        identity: read_default_identity(&config),
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

    let default = root
        .get("models")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("default"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(id) = default {
        if let Some(url) = root
            .get("model")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get(id))
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str())
        {
            if !is_official_base_url(url) {
                return false;
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

    {
        let root = doc.as_table_mut();
        if !root.contains_key("models") {
            root.insert("models", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        if let Some(models) = root.get_mut("models").and_then(|i| i.as_table_like_mut()) {
            models.insert("default", toml_edit::value(DEFAULT_MODEL));
        }
    }
    if let Some(model_root) = doc.get_mut("model").and_then(|i| i.as_table_like_mut()) {
        prune_model_tables(
            model_root,
            DEFAULT_MODEL,
            &[
                LOCAL_PROXY_MODEL_ID.to_string(),
                CUSTOM_MODEL_ID.to_string(),
            ],
        );
        let drop_official = model_root
            .get(DEFAULT_MODEL)
            .and_then(|i| i.as_table_like())
            .and_then(|t| t.get("base_url"))
            .and_then(|i| i.as_str())
            .is_some_and(|url| !is_official_base_url(url));
        if drop_official {
            model_root.remove(DEFAULT_MODEL);
        }
    }

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

    // Clear leftover proxy/third-party fork pins so official default wins.
    if let Some(ui) = doc.get_mut("ui").and_then(|i| i.as_table_like_mut()) {
        if let Some(fork) = ui
            .get("fork_secondary_model")
            .and_then(|i| i.as_str())
            .map(str::trim)
        {
            if fork == LOCAL_PROXY_MODEL_ID
                || fork == CUSTOM_MODEL_ID
                || fork.is_empty()
            {
                ui.insert("fork_secondary_model", toml_edit::value(DEFAULT_MODEL));
            }
        }
    }

    let out = doc.to_string();
    if out.trim().is_empty() {
        return Ok(official_config_toml());
    }
    Ok(out)
}

/// Whether stored provider matches live routing.
///
/// Official: live has no third-party endpoint on the *active* `[models].default`.
/// Custom: `base_url` must match. Identity may be the public
/// [`CUSTOM_MODEL_ID`] or a leftover pre-migration table name — both are
/// the same channel. Changing `.model` (API id) on the same table is not
/// drift. Live-only `localproxy` is not a direct-connect match.
pub fn matches_live(provider: &Provider, live: &LiveSnapshot) -> bool {
    if provider.is_official() {
        return live.is_official_shape;
    }
    if live.is_official_shape {
        return false;
    }
    if live.identity.as_deref() == Some(LOCAL_PROXY_MODEL_ID) {
        return false;
    }
    let (key, base, _model) = summarize(provider);
    match (base.as_deref(), live.base_url.as_deref()) {
        (Some(a), Some(b)) if a.trim_end_matches('/') == b.trim_end_matches('/') => {
            key.is_some() == live.has_api_key || live.has_api_key || key.is_some()
        }
        (None, None) => true,
        _ => false,
    }
}

/// Archive / editor fragment: `[model."<identity>"]` + `[models].default` only.
pub fn extract_routing_fragment(config: &str) -> Result<String, String> {
    if config.trim().is_empty() {
        return Ok(String::new());
    }
    if let Some(f) = extract_fields(config) {
        let f = canonicalize_archive_fields(f);
        return Ok(build_custom_config_with_picker(
            &f.profile,
            &f.model,
            &f.base_url,
            &f.name,
            "",
            f.api_key.as_deref().unwrap_or(""),
            &f.api_backend,
            f.context_window,
        ));
    }
    let src = config
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Grok config.toml: {e}"))?;
    let Some(default) = src
        .get("models")
        .and_then(|i| i.as_table())
        .and_then(|t| t.get("default"))
        .cloned()
    else {
        return Ok(String::new());
    };
    let mut out = DocumentMut::new();
    let mut models = toml_edit::Table::new();
    models.insert("default", default);
    out.as_table_mut()
        .insert("models", toml_edit::Item::Table(models));
    Ok(out.to_string())
}

/// Patch only `[models].default` and `[model."<identity>"]` onto live.
/// MCP / ui / features / tools stay as-is. Previous GUI supplier tables
/// (and other leftover third-party routes) are removed so switching
/// providers does not stack `[model."A"]` + `[model."B"]` in the picker.
pub fn apply_routing_to_live(live: &str, archive: &str) -> Result<String, String> {
    if archive.trim().is_empty() {
        return Ok(live.to_string());
    }
    let Some(fields) = extract_fields(archive) else {
        return Err("Grok 供应商档案缺少 [models].default / [model.\"…\"]".into());
    };
    let fields = canonicalize_archive_fields(fields);
    if live.trim().is_empty() {
        return extract_routing_fragment(archive);
    }
    let mut doc = live
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid live Grok config.toml: {e}"))?;
    apply_identity_nodes(&mut doc, &fields)?;
    Ok(doc.to_string())
}

fn apply_identity_nodes(doc: &mut DocumentMut, fields: &GrokFields) -> Result<(), String> {
    let old_default = doc
        .get("models")
        .and_then(|i| i.as_table())
        .and_then(|t| t.get("default"))
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    {
        let identity = fields.profile.as_str();
        let root = doc.as_table_mut();
        if !root.contains_key("model") {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            root.insert("model", toml_edit::Item::Table(t));
        }
        let model_root = root
            .get_mut("model")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Grok config.toml 中 [model] 非法".to_string())?;
        if model_root.get(identity).is_none() {
            model_root.insert(identity, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let table = model_root
            .get_mut(identity)
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| format!("Grok config.toml 中 [model.\"{identity}\"] 非法"))?;
        table.insert("model", toml_edit::value(fields.model.as_str()));
        table.insert("base_url", toml_edit::value(fields.base_url.as_str()));
        table.insert("name", toml_edit::value(fields.name.as_str()));
        table.insert(
            "api_backend",
            toml_edit::value(fields.api_backend.as_str()),
        );
        table.insert("context_window", toml_edit::value(fields.context_window));
        if let Some(key) = fields
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            table.insert("api_key", toml_edit::value(key));
        }

        let mut drop = vec![LOCAL_PROXY_MODEL_ID.to_string()];
        if let Some(old) = old_default {
            if old != identity {
                drop.push(old);
            }
        }
        // Drop a same-named built-in override table if we previously mis-keyed
        // default/table as the upstream slug (e.g. [model."grok-4.6"]).
        if fields.model != identity {
            drop.push(fields.model.clone());
        }
        prune_model_tables(model_root, identity, &drop);
    }
    {
        let root = doc.as_table_mut();
        if !root.contains_key("models") {
            root.insert("models", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let models = root
            .get_mut("models")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Grok config.toml 中 [models] 非法".to_string())?;
        // Must equal the [model."<id>"] table key — never the upstream API slug.
        // default = "grok-4.6" with [model."chatgpt-tools-proxy"] still selects the
        // built-in grok-4.6 catalog entry (exact key match wins over slug match).
        models.insert("default", toml_edit::value(fields.profile.as_str()));
    }
    // Keep fork secondary on the active catalog identity so a leftover
    // fork_secondary_model = "grok-4.5" does not keep selecting built-ins.
    sync_fork_secondary_model(doc, fields.profile.as_str());
    Ok(())
}

/// Align `[ui].fork_secondary_model` with the active catalog identity when set.
/// Leaves the key alone when absent (Grok treats empty as no-opinion).
fn sync_fork_secondary_model(doc: &mut DocumentMut, identity: &str) {
    let identity = identity.trim();
    if identity.is_empty() {
        return;
    }
    let Some(ui) = doc.get_mut("ui").and_then(|i| i.as_table_like_mut()) else {
        return;
    };
    let Some(current) = ui
        .get("fork_secondary_model")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
    else {
        return;
    };
    if current != identity {
        ui.insert("fork_secondary_model", toml_edit::value(identity));
    }
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
                strip_to_official_routing(live_existing)?
            };
            Ok(base)
        } else {
            apply_routing_to_live(live_existing, &archive)
        }
    })?;

    if is_official {
        warnings.push("已切换为 Grok 官方默认渠道。".into());
    }
    Ok(warnings)
}

fn backup_live(live_config: &str) -> Result<(), String> {
    super::backup_utils::save_live_backup("grok-config", "toml", live_config.as_bytes())
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
    let fragment = extract_routing_fragment(&live).unwrap_or(live);
    if let Some(obj) = provider.settings_config.as_object_mut() {
        obj.insert("config".into(), Value::String(fragment));
    }
    provider.updated_at = Some(chrono::Utc::now().timestamp_millis());
    Ok(())
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
    display_name: Option<&str>,
) -> Result<Value, String> {
    let is_official = category == Some("official");
    if is_official {
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
        let mut text = extract_routing_fragment(raw)?;
        if text.trim().is_empty() {
            return Err("高级 config.toml 中缺少 [models].default / [model.\"…\"]".into());
        }
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
        if extract_fields(&text)
            .and_then(|f| f.api_key)
            .filter(|k| !k.is_empty())
            .is_some()
        {
            validate_custom(&text)?;
        } else {
            validate_syntax(&text)?;
        }
        let api_format = extract_fields(&text)
            .map(|f| {
                if matches!(
                    normalize_api_backend(&f.api_backend).as_str(),
                    "chat_completions"
                ) {
                    "openai_chat"
                } else {
                    "openai_responses"
                }
            })
            .unwrap_or("openai_responses");
        return Ok(json!({ "config": text, "apiFormat": api_format }));
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
    let profile_val = resolve_model_identity(profile.unwrap_or(""), name);
    let backend = api_backend.unwrap_or(DEFAULT_API_BACKEND);
    let cw = context_window
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_CONTEXT_WINDOW);

    let existing_cfg = existing
        .and_then(|p| p.settings_config.get("config"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let existing_fragment =
        extract_routing_fragment(existing_cfg).unwrap_or_else(|_| existing_cfg.to_string());

    let picker = resolve_supplier_name(name, display_name.unwrap_or(""), model_val);
    let text = if existing_fragment.trim().is_empty() {
        build_custom_config_with_picker(
            &profile_val,
            model_val,
            url,
            name,
            &picker,
            &key,
            backend,
            cw,
        )
    } else {
        patch_config_from_form_with_picker(
            &existing_fragment,
            &profile_val,
            model_val,
            url,
            name,
            &picker,
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
    let api_format = if matches!(normalize_api_backend(backend).as_str(), "chat_completions") {
        "openai_chat"
    } else {
        "openai_responses"
    };
    Ok(json!({ "config": text, "apiFormat": api_format }))
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

pub fn summarize_picker_name(provider: &Provider) -> String {
    if provider.is_official() {
        return DEFAULT_MODEL.into();
    }
    let config = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if let Some(f) = extract_fields(config) {
        picker_name_for_form(&f.name, &provider.name, &f.model)
    } else if !provider.name.trim().is_empty() {
        provider.name.trim().to_string()
    } else {
        DEFAULT_MODEL.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_field_is_supplier_name() {
        let toml = build_custom_config_with_picker(
            "ignored-profile",
            "grok-4.6",
            "https://relay.example/v1",
            "中转api",
            "",
            "sk-test",
            "responses",
            500_000,
        );
        assert!(
            toml.contains("default = \"chatgpt-tools-proxy\""),
            "default must be catalog identity, got:\n{toml}"
        );
        assert!(
            toml.contains("[model.\"chatgpt-tools-proxy\"]")
                || toml.contains("[model.chatgpt-tools-proxy]"),
            "table key must be public identity, got:\n{toml}"
        );
        assert!(
            toml.contains("model = \"grok-4.6\""),
            "model must be upstream API id, got:\n{toml}"
        );
        assert!(
            toml.contains("name = \"中转api\""),
            "name must be supplier name with correct utf8 Chinese support, got:\n{toml}"
        );
        let pos_model = toml.find("[model.").expect("has [model.]");
        let pos_models = toml.find("[models]").expect("has [models]");
        assert!(
            pos_model < pos_models,
            "[models] must be placed below [model.*], got:\n{toml}"
        );
    }

    #[test]
    fn apply_routing_forces_default_to_table_key_not_api_slug() {
        let live = r#"
[model.chatgpt-tools-proxy]
model = "grok-4.6"
base_url = "https://old.example/v1"
name = "旧供应商"
api_key = "sk-old"
api_backend = "responses"
context_window = 500000

[models]
default = "grok-4.6"

[ui]
fork_secondary_model = "grok-4.5"
"#;
        let archive = build_custom_config_with_picker(
            CUSTOM_MODEL_ID,
            "glm-5.2",
            "https://new.example/v1",
            "新智谱渠道",
            "",
            "sk-new",
            "responses",
            500_000,
        );
        let out = apply_routing_to_live(live, &archive).expect("apply");
        let doc: toml::Value = out.parse().expect("toml");
        assert_eq!(
            doc.get("models")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some(CUSTOM_MODEL_ID),
            "default must equal table key:\n{out}"
        );
        let entry = doc
            .get("model")
            .and_then(|m| m.get(CUSTOM_MODEL_ID))
            .and_then(|t| t.as_table())
            .expect("proxy table");
        assert_eq!(entry.get("model").and_then(|v| v.as_str()), Some("glm-5.2"));
        assert_eq!(
            entry.get("base_url").and_then(|v| v.as_str()),
            Some("https://new.example/v1")
        );
        assert_eq!(
            entry.get("name").and_then(|v| v.as_str()),
            Some("新智谱渠道"),
            "name is supplier name in chinese"
        );
        assert_eq!(
            doc.get("ui")
                .and_then(|u| u.get("fork_secondary_model"))
                .and_then(|v| v.as_str()),
            Some(CUSTOM_MODEL_ID),
            "stale fork pin must follow active identity:\n{out}"
        );
        // Must not keep a built-in-key third-party override hanging around.
        assert!(
            doc.get("model")
                .and_then(|m| m.get("grok-4.6"))
                .is_none(),
            "must not leave [model.grok-4.6] override:\n{out}"
        );
    }

    #[test]
    fn resolve_supplier_name_handles_chinese_and_fallbacks() {
        assert_eq!(resolve_supplier_name("中转api", "", "grok-4.6"), "中转api");
        assert_eq!(
            resolve_supplier_name("智谱清言", "ignored", "grok-4.6"),
            "智谱清言"
        );
        // reserved identities fall back to model
        assert_eq!(
            resolve_supplier_name(CUSTOM_MODEL_ID, "", "grok-4.6"),
            "grok-4.6"
        );
        assert_eq!(
            resolve_supplier_name(LOCAL_PROXY_MODEL_ID, "", "grok-4.6"),
            "grok-4.6"
        );
        assert_eq!(resolve_supplier_name("", "", "grok-4.6"), "grok-4.6");
        assert_eq!(resolve_supplier_name("", "", ""), DEFAULT_MODEL);
    }

    #[test]
    fn migrate_archive_identity_migrates_order_and_name() {
        let old_format = r#"
[models]
default = "chatgpt-tools-proxy"

[model.chatgpt-tools-proxy]
model = "grok-4.5"
base_url = "https://api.example.com/v1"
name = "grok-4.5"
api_key = "sk-123"
api_backend = "responses"
context_window = 500000
"#;
        let migrated =
            migrate_archive_identity(old_format, "我的Grok供应商").expect("should migrate");
        assert!(migrated.contains("name = \"我的Grok供应商\""));
        let pos_model = migrated.find("[model.").expect("has [model.]");
        let pos_models = migrated.find("[models]").expect("has [models]");
        assert!(pos_model < pos_models, "[models] must be placed below [model.*]");
    }
}
