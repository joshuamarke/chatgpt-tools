//! Codex live config helpers (`~/.codex/auth.json` + `config.toml`).
//!
//! Production path (aligned with common third-party channel usage):
//! - Stored profile: `{ auth: { OPENAI_API_KEY }, config: "<toml>" }`
//! - On switch (third-party): write **both** auth.json (API key) and config.toml
//!   (model_provider + base_url + wire_api). Official: preserve live ChatGPT
//!   OAuth unless the stored profile already carries login material.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use toml_edit::DocumentMut;

use super::models::Provider;

/// Official ChatGPT / Codex desktop+CLI routing (OAuth path).
/// Built into Codex; do not confuse with third-party `base_url` proxies.
pub const OFFICIAL_CHATGPT_BACKEND: &str = "https://chatgpt.com/backend-api";
/// Official OpenAI Platform API root (API-key path / built-in `openai` provider).
pub const OFFICIAL_API_BASE_URL: &str = "https://api.openai.com/v1";
/// Built-in provider id used by Codex for OpenAI / ChatGPT auth.
pub const OFFICIAL_MODEL_PROVIDER: &str = "openai";
/// Default wire API for official OpenAI Responses path.
pub const OFFICIAL_WIRE_API: &str = "responses";
/// Display / seed default model label (Codex ships multiple gpt-5.x variants;
/// leaving model unset lets the client pick its built-in default).
pub const OFFICIAL_MODEL_HINT: &str = "gpt-5.5";

pub fn codex_home_dir() -> PathBuf {
    crate::sessions::default_codex_home_dir()
}

/// Canonical empty-ish official profile: no custom `model_providers`, no proxy
/// `base_url`. Auth is ChatGPT OAuth in `auth.json` (managed by Codex login).
pub fn official_settings_config() -> Value {
    json!({
        "auth": {},
        "config": official_config_toml(),
    })
}

/// Stored official config is intentionally free of third-party routing.
/// An empty / comment-only body means: use Codex built-in OpenAI provider +
/// ChatGPT OAuth (`https://chatgpt.com/backend-api`) or Platform API
/// (`https://api.openai.com/v1`) depending on login mode.
pub fn official_config_toml() -> String {
    format!(
        r#"# ChatGPT Tools · OpenAI Official
# Routing: built-in provider "{provider}" (ChatGPT OAuth / Platform API).
# Do not set a custom model_providers.*.base_url here.
# ChatGPT backend: {chatgpt}
# Platform API:    {api}
# wire_api:        {wire}
"#,
        provider = OFFICIAL_MODEL_PROVIDER,
        chatgpt = OFFICIAL_CHATGPT_BACKEND,
        api = OFFICIAL_API_BASE_URL,
        wire = OFFICIAL_WIRE_API,
    )
}

/// True when live config uses official OpenAI/ChatGPT routing (no third-party proxy).
pub fn is_official_live_config(config_text: &str) -> bool {
    let trimmed = config_text.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Comment-only official seed is official.
    if trimmed.lines().all(|l| {
        let t = l.trim();
        t.is_empty() || t.starts_with('#')
    }) {
        return true;
    }
    let Ok(doc) = config_text.parse::<toml::Value>() else {
        return false;
    };
    let Some(root) = doc.as_table() else {
        return false;
    };

    if let Some(mp) = root.get("model_provider").and_then(|v| v.as_str()) {
        let mp = mp.trim();
        if !mp.is_empty() && !is_official_provider_id(mp) {
            return false;
        }
    }

    if root
        .get("experimental_bearer_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
    {
        return false;
    }

    if let Some(providers) = root.get("model_providers").and_then(|v| v.as_table()) {
        for (name, entry) in providers {
            if !is_official_provider_id(name) {
                // Any non-openai custom provider table ⇒ third-party
                if entry.get("base_url").is_some()
                    || entry.get("experimental_bearer_token").is_some()
                {
                    return false;
                }
            }
            if let Some(url) = entry.get("base_url").and_then(|v| v.as_str()) {
                if !is_official_base_url(url) {
                    return false;
                }
            }
            if entry
                .get("experimental_bearer_token")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .is_some_and(|s| !s.is_empty())
            {
                return false;
            }
        }
    }

    // Top-level base_url (rare) must also be official.
    if let Some(url) = root.get("base_url").and_then(|v| v.as_str()) {
        if !is_official_base_url(url) {
            return false;
        }
    }

    true
}

fn is_official_provider_id(id: &str) -> bool {
    matches!(
        id.trim().to_ascii_lowercase().as_str(),
        "openai" | "chatgpt" | ""
    )
}

pub fn is_official_base_url(url: &str) -> bool {
    let u = normalize_base_url(url).to_ascii_lowercase();
    u == OFFICIAL_API_BASE_URL
        || u == "https://api.openai.com"
        || u.starts_with("https://chatgpt.com/backend-api")
        || u.starts_with("https://chat.openai.com/backend-api")
}

/// Strip third-party routing from an existing live config while preserving
/// MCP / plugins / projects / desktop / memories / etc.
pub fn strip_to_official_routing(config_text: &str) -> Result<String, String> {
    if config_text.trim().is_empty() {
        return Ok(official_config_toml());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;

    // Drop active third-party provider pointer.
    if let Some(mp) = doc
        .get("model_provider")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .map(str::to_string)
    {
        if !is_official_provider_id(&mp) {
            doc.as_table_mut().remove("model_provider");
        }
    }

    // Drop all custom model_providers (built-in openai does not need a table).
    doc.as_table_mut().remove("model_providers");
    doc.as_table_mut().remove("experimental_bearer_token");
    // Third-party model catalog projection
    doc.as_table_mut().remove("model_catalog_json");

    // If only comments / whitespace remain useful keys, keep the rest intact.
    let out = doc.to_string();
    if out.trim().is_empty() {
        return Ok(official_config_toml());
    }
    Ok(out)
}

pub fn auth_path() -> PathBuf {
    codex_home_dir().join("auth.json")
}

pub fn config_path() -> PathBuf {
    codex_home_dir().join("config.toml")
}

pub fn read_auth() -> Value {
    let path = auth_path();
    if !path.exists() {
        return json!({});
    }
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    }
}

pub fn read_config_text() -> Result<String, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(&path).map_err(|e| format!("读取 Codex config.toml 失败: {e}"))
}

pub fn validate_config_toml(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Ok(());
    }
    toml::from_str::<toml::Table>(text)
        .map(|_| ())
        .map_err(|e| format!("Codex config.toml 格式错误: {e}"))
}

/// Validate a third-party profile is complete enough to switch.
pub fn validate_for_switch(provider: &Provider) -> Result<(), String> {
    if provider.is_official() {
        return Ok(());
    }
    let obj = provider
        .settings_config
        .as_object()
        .ok_or_else(|| "Codex 供应商配置必须是 JSON 对象".to_string())?;
    let auth = obj.get("auth").cloned().unwrap_or_else(|| json!({}));
    let config = obj
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if config.trim().is_empty() {
        return Err("Codex 第三方供应商缺少 config.toml 内容".into());
    }
    validate_config_toml(&config)?;
    let base = extract_base_url(&config).filter(|s| !s.is_empty());
    if base.is_none() {
        return Err("Codex 配置缺少 base_url（请在 [model_providers.*] 中设置）".into());
    }
    let key = extract_api_key(&auth, &config);
    if key.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_none() {
        return Err("请先填写 API Key 再启用该供应商".into());
    }
    Ok(())
}

/// Build third-party Codex config.toml from form fields.
pub fn build_third_party_config(
    provider_name: &str,
    base_url: &str,
    model: &str,
    wire_api: &str,
    reasoning_effort: &str,
) -> String {
    let name = toml_string(provider_name);
    let url = toml_string(&normalize_base_url(base_url));
    let model = toml_string(if model.trim().is_empty() {
        "gpt-5.5"
    } else {
        model.trim()
    });
    let wire = normalize_wire_api(wire_api);
    let wire_q = toml_string(&wire);
    let effort = normalize_reasoning_effort(reasoning_effort);
    let effort_q = toml_string(&effort);
    format!(
        r#"model_provider = "custom"
model = {model}
model_reasoning_effort = {effort_q}
disable_response_storage = true

[model_providers.custom]
name = {name}
base_url = {url}
wire_api = {wire_q}
requires_openai_auth = true
"#
    )
}

pub fn normalize_wire_api(wire: &str) -> String {
    match wire.trim().to_ascii_lowercase().as_str() {
        "chat" | "chat_completions" | "openai_chat" | "completions" => "chat".into(),
        _ => "responses".into(),
    }
}

pub fn normalize_reasoning_effort(effort: &str) -> String {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" | "none" | "xhigh" => effort.trim().to_ascii_lowercase(),
        "low" => "low".into(),
        "medium" | "med" => "medium".into(),
        "high" | "" => "high".into(),
        other if !other.is_empty() => other.to_string(),
        _ => "high".into(),
    }
}

pub fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

pub fn extract_api_key(auth: &Value, config_text: &str) -> Option<String> {
    auth.get("OPENAI_API_KEY")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| extract_bearer_token(config_text))
}

pub fn extract_base_url(config_text: &str) -> Option<String> {
    if config_text.trim().is_empty() {
        return None;
    }
    let doc = config_text.parse::<toml::Value>().ok()?;
    if let Some(active) = doc.get("model_provider").and_then(|v| v.as_str()) {
        if let Some(url) = doc
            .get("model_providers")
            .and_then(|p| p.get(active))
            .and_then(|p| p.get("base_url"))
            .and_then(|v| v.as_str())
        {
            return Some(normalize_base_url(url));
        }
    }
    doc.get("base_url")
        .and_then(|v| v.as_str())
        .map(normalize_base_url)
}

pub fn extract_model(config_text: &str) -> Option<String> {
    if config_text.trim().is_empty() {
        return None;
    }
    let doc = config_text.parse::<toml::Value>().ok()?;
    doc.get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn extract_wire_api(config_text: &str) -> Option<String> {
    if config_text.trim().is_empty() {
        return None;
    }
    let doc = config_text.parse::<toml::Value>().ok()?;
    if let Some(active) = doc.get("model_provider").and_then(|v| v.as_str()) {
        if let Some(w) = doc
            .get("model_providers")
            .and_then(|p| p.get(active))
            .and_then(|p| p.get("wire_api"))
            .and_then(|v| v.as_str())
        {
            return Some(normalize_wire_api(w));
        }
    }
    None
}

pub fn extract_reasoning_effort(config_text: &str) -> Option<String> {
    if config_text.trim().is_empty() {
        return None;
    }
    let doc = config_text.parse::<toml::Value>().ok()?;
    doc.get("model_reasoning_effort")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalize_reasoning_effort)
}

/// Overlay every top-level key from `overlay` onto `base`, preserving keys that
/// only exist in `base` (MCP / desktop / features / notify / …).
///
/// This is the Codex++-style "preserve unmanaged" merge used when:
/// - the archive only stores a routing fragment, or
/// - the advanced editor shows a partial template but live still has full config.
pub fn merge_toml_overlay(base: &str, overlay: &str) -> Result<String, String> {
    let overlay = overlay.trim();
    if overlay.is_empty() {
        return Ok(base.to_string());
    }
    let overlay_doc = overlay
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;
    if base.trim().is_empty() {
        return Ok(overlay_doc.to_string());
    }
    let mut base_doc = base
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;
    for (key, item) in overlay_doc.as_table().iter() {
        base_doc[key] = item.clone();
    }
    Ok(base_doc.to_string())
}

/// Pick the richer of archive vs live config as the merge base (more top-level keys wins;
/// tie-break on length so a full live file beats a short routing template).
pub fn richer_config_base(archive: &str, live: &str) -> String {
    let a = archive.trim();
    let l = live.trim();
    if a.is_empty() {
        return live.to_string();
    }
    if l.is_empty() {
        return archive.to_string();
    }
    let count = |text: &str| -> usize {
        text.parse::<DocumentMut>()
            .ok()
            .map(|d| d.as_table().len())
            .unwrap_or(0)
    };
    let ca = count(a);
    let cl = count(l);
    if cl > ca {
        live.to_string()
    } else if ca > cl {
        archive.to_string()
    } else if l.len() >= a.len() {
        live.to_string()
    } else {
        archive.to_string()
    }
}

/// Config text shown in the advanced editor: prefer full live file so users never
/// only see a short routing template and accidentally wipe MCP on save.
pub fn config_for_editor(archive_config: &str) -> String {
    let live = read_config_text().unwrap_or_default();
    let base = richer_config_base(archive_config, &live);
    if base.trim().is_empty() {
        return archive_config.to_string();
    }
    // If archive carries newer routing, overlay it onto the rich base for display.
    if archive_config.trim().is_empty() || archive_config.trim() == base.trim() {
        return base;
    }
    merge_toml_overlay(&base, archive_config).unwrap_or(base)
}

/// Patch structured form fields into an existing Codex config while preserving
/// unrelated keys (MCP, extra model_providers, etc.).
pub fn patch_config_from_form(
    existing: &str,
    provider_name: &str,
    base_url: Option<&str>,
    model: Option<&str>,
    wire_api: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Result<String, String> {
    if existing.trim().is_empty() {
        let url = base_url.unwrap_or("").trim();
        if url.is_empty() {
            return Err("请填写 Base URL".into());
        }
        return Ok(build_third_party_config(
            provider_name,
            url,
            model.unwrap_or("gpt-5.5"),
            wire_api.unwrap_or("responses"),
            reasoning_effort.unwrap_or("high"),
        ));
    }
    let mut doc = existing
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;

    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        doc["model"] = toml_edit::value(m);
    }
    if let Some(effort) = reasoning_effort.map(str::trim).filter(|s| !s.is_empty()) {
        doc["model_reasoning_effort"] = toml_edit::value(normalize_reasoning_effort(effort));
    }

    let provider_id = doc
        .get("model_provider")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "custom".into());

    if doc.get("model_provider").is_none() {
        doc["model_provider"] = toml_edit::value(provider_id.as_str());
    }

    // Ensure model_providers.<id> table exists
    {
        let root = doc.as_table_mut();
        if !root.contains_key("model_providers") {
            root.insert("model_providers", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let providers = root
            .get_mut("model_providers")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Codex config.toml 中 model_providers 非法".to_string())?;
        if providers.get(&provider_id).is_none() {
            providers.insert(&provider_id, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let table = providers
            .get_mut(&provider_id)
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| format!("Codex config.toml 中 model_providers.{provider_id} 非法"))?;
        table.insert("name", toml_edit::value(provider_name));
        if let Some(url) = base_url.map(str::trim).filter(|s| !s.is_empty()) {
            table.insert("base_url", toml_edit::value(normalize_base_url(url)));
        }
        if let Some(wire) = wire_api.map(str::trim).filter(|s| !s.is_empty()) {
            table.insert("wire_api", toml_edit::value(normalize_wire_api(wire)));
        } else if table.get("wire_api").is_none() {
            table.insert("wire_api", toml_edit::value("responses"));
        }
        if table.get("requires_openai_auth").is_none() {
            table.insert("requires_openai_auth", toml_edit::value(true));
        }
        // Canonical key lives in auth.json — strip provider-scoped bearer if present
        table.remove("experimental_bearer_token");
    }
    doc.as_table_mut().remove("experimental_bearer_token");

    let out = doc.to_string();
    if extract_base_url(&out).is_none() {
        return Err("请填写 Base URL".into());
    }
    Ok(out)
}

fn extract_bearer_token(config_text: &str) -> Option<String> {
    if !config_text.contains("experimental_bearer_token") {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let provider_id = doc
        .get("model_provider")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let from_provider = provider_id.and_then(|id| {
        doc.get("model_providers")
            .and_then(|i| i.as_table())
            .and_then(|t| t.get(id))
            .and_then(|i| i.as_table())
            .and_then(|t| t.get("experimental_bearer_token"))
            .and_then(|i| i.as_str())
    });
    let top = doc
        .get("experimental_bearer_token")
        .and_then(|i| i.as_str());
    from_provider
        .or(top)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Inject API key into config as provider-scoped experimental_bearer_token (belt & suspenders).
fn set_bearer_token(config_text: &str, token: &str) -> Result<String, String> {
    if config_text.trim().is_empty() {
        return Err("Codex 第三方供应商缺少 config.toml，无法写入 API Key".into());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;
    let provider_id = doc
        .get("model_provider")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if let Some(id) = provider_id {
        if let Some(providers) = doc
            .get_mut("model_providers")
            .and_then(|i| i.as_table_like_mut())
        {
            if let Some(table) = providers.get_mut(&id).and_then(|i| i.as_table_like_mut()) {
                table.insert("experimental_bearer_token", toml_edit::value(token));
                return Ok(doc.to_string());
            }
        }
    }
    doc["experimental_bearer_token"] = toml_edit::value(token);
    Ok(doc.to_string())
}

fn auth_has_login_material(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };
    obj.iter().any(|(key, value)| {
        if key == "auth_mode" {
            return false;
        }
        if key == "OPENAI_API_KEY" {
            return value
                .as_str()
                .map(str::trim)
                .is_some_and(|t| !t.is_empty());
        }
        match value {
            Value::Null => false,
            Value::String(t) => !t.trim().is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Object(m) => !m.is_empty(),
            _ => true,
        }
    })
}

/// Snapshot of live files for UI status.
#[derive(Debug, Clone)]
pub struct LiveSnapshot {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub wire_api: Option<String>,
    pub has_api_key: bool,
    pub config_exists: bool,
    pub auth_exists: bool,
}

pub fn read_live_snapshot() -> LiveSnapshot {
    let config = read_config_text().unwrap_or_default();
    let auth = read_auth();
    let key = extract_api_key(&auth, &config);
    LiveSnapshot {
        base_url: extract_base_url(&config),
        model: extract_model(&config),
        wire_api: extract_wire_api(&config),
        has_api_key: key.is_some(),
        config_exists: config_path().exists(),
        auth_exists: auth_path().exists(),
    }
}

/// Whether stored provider roughly matches live files (base_url + model).
pub fn matches_live(provider: &Provider, live: &LiveSnapshot) -> bool {
    let (_, base, model) = summarize(provider);
    if provider.is_official() {
        let live_config = read_config_text().unwrap_or_default();
        return is_official_live_config(&live_config);
    }
    match (base.as_deref(), live.base_url.as_deref()) {
        (Some(a), Some(b)) if normalize_base_url(a) == normalize_base_url(b) => {
            match (model.as_deref(), live.model.as_deref()) {
                (Some(m1), Some(m2)) => m1 == m2,
                (None, None) => true,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Write live Codex files for the selected provider.
pub fn write_live(provider: &Provider) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    let obj = provider
        .settings_config
        .as_object()
        .ok_or_else(|| "Codex 供应商配置必须是 JSON 对象".to_string())?;
    let auth = obj.get("auth").cloned().unwrap_or_else(|| json!({}));
    let config_text = obj
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    validate_config_toml(&config_text)?;

    let is_official = provider.is_official();
    let home = codex_home_dir();
    fs::create_dir_all(&home).map_err(|e| format!("创建 ~/.codex 失败: {e}"))?;

    if is_official {
        // Official: strip third-party routing from live, keep MCP/plugins/projects.
        // Never overwrite ChatGPT OAuth tokens with empty/API-key-only auth.
        let live_existing = read_config_text().unwrap_or_default();
        let _ = backup_live_files();
        let cleaned = strip_to_official_routing(&live_existing)?;
        // Prefer cleaned live over stored seed comments when live had real content.
        let final_config = if live_existing.trim().is_empty() {
            if config_text.trim().is_empty() {
                official_config_toml()
            } else {
                config_text
            }
        } else {
            cleaned
        };
        validate_config_toml(&final_config)?;

        if auth_has_login_material(&auth) && !is_api_key_only_auth(&auth) {
            write_atomic_auth_and_config(&auth, &final_config)?;
            warnings.push(
                "已恢复 OpenAI 官方路由（清除第三方 base_url / model_providers），并写入档案中的 ChatGPT 登录态。"
                    .into(),
            );
        } else {
            // Keep existing auth.json. If it is only a third-party API key, warn.
            let live_auth = read_auth();
            write_text_file(&config_path(), &final_config)?;
            if is_api_key_only_auth(&live_auth) {
                warnings.push(
                    "已清除第三方路由。当前 auth.json 仅有 OPENAI_API_KEY（非 ChatGPT OAuth）。若要用订阅登录请在 Codex 执行登录；Platform API Key 可继续走 api.openai.com。"
                        .into(),
                );
            } else {
                warnings.push(
                    "已恢复 OpenAI 官方路由（ChatGPT backend / api.openai.com），保留现有 auth.json。请重启 Codex 后生效。"
                        .into(),
                );
            }
        }
        return Ok(warnings);
    }

    // Third-party: dual-write so both desktop and CLI paths work.
    // 1) auth.json OPENAI_API_KEY — primary for requires_openai_auth
    // 2) config.toml + experimental_bearer_token — backup for some Codex builds
    let api_key = extract_api_key(&auth, &config_text)
        .ok_or_else(|| "请先填写 API Key 再启用该供应商".to_string())?;

    // Merge archive routing into live so MCP / desktop / features are never wiped
    // when the archive only holds a short third-party template.
    let live_existing = read_config_text().unwrap_or_default();
    let merged_config = if live_existing.trim().is_empty() {
        config_text.clone()
    } else {
        merge_toml_overlay(&live_existing, &config_text)?
    };

    let live_auth = json!({ "OPENAI_API_KEY": api_key });
    let mut live_config = set_bearer_token(&merged_config, &api_key)?;

    // Align top-level model with first mapped model when needed, then project
    // model_catalog_json so Codex `/model` lists third-party model names
    // (cc-switch: DB modelCatalog is SSOT → live catalog file + pointer).
    live_config = super::catalog::ensure_config_model_from_catalog(
        &live_config,
        &provider.settings_config,
    )?;
    let default_model = extract_model(&live_config);
    live_config = super::catalog::prepare_config_with_catalog(
        &home,
        &provider.settings_config,
        &live_config,
        default_model.as_deref(),
    )?;

    // Backup previous live files under app state (best-effort)
    let _ = backup_live_files();

    write_atomic_auth_and_config(&live_auth, &live_config)?;
    let has_catalog = live_config.contains("model_catalog_json");
    if has_catalog {
        warnings.push(
            "已写入 ~/.codex/auth.json、config.toml，并生成 model_catalog_json（/model 第三方模型名）。请完全重启 Codex / CLI 后生效。"
                .into(),
        );
    } else {
        warnings.push(
            "已写入 ~/.codex/auth.json 与 config.toml。未配置模型映射时不会生成 model_catalog_json；可在编辑页添加映射后再次启用。"
                .into(),
        );
    }
    Ok(warnings)
}

fn is_api_key_only_auth(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };
    let has_key = obj
        .get("OPENAI_API_KEY")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if !has_key {
        return false;
    }
    // Only OPENAI_API_KEY (+ maybe auth_mode) → treat as third-party key, not OAuth blob
    obj.keys().all(|k| k == "OPENAI_API_KEY" || k == "auth_mode")
}

fn backup_live_files() -> Result<(), String> {
    let dir = crate::sessions::paths::app_state_dir().join("provider-live-backups");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    if auth_path().exists() {
        let _ = fs::copy(auth_path(), dir.join(format!("codex-auth-{stamp}.json")));
    }
    if config_path().exists() {
        let _ = fs::copy(config_path(), dir.join(format!("codex-config-{stamp}.toml")));
    }
    Ok(())
}

/// Before switching away, refresh the outgoing provider from live (so edits
/// made outside this app are not lost).
pub fn backfill_from_live(provider: &mut Provider) -> Result<(), String> {
    if provider.is_official() {
        // Capture live auth only if it looks like OAuth material
        let live_auth = read_auth();
        if auth_has_login_material(&live_auth) && !is_api_key_only_auth(&live_auth) {
            if let Some(obj) = provider.settings_config.as_object_mut() {
                obj.insert("auth".into(), live_auth);
            }
        }
        return Ok(());
    }
    let live_config = read_config_text()?;
    let live_auth = read_auth();
    // Only backfill when live base_url still matches this provider
    let live_base = extract_base_url(&live_config);
    let stored_base = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_base_url);
    if live_base.is_none() || live_base != stored_base {
        return Ok(());
    }
    let key = extract_api_key(&live_auth, &live_config).unwrap_or_default();
    // Strip bearer from config for storage (canonical key lives in auth)
    let cleaned = strip_bearer_token(&live_config).unwrap_or(live_config);
    if let Some(obj) = provider.settings_config.as_object_mut() {
        obj.insert("auth".into(), json!({ "OPENAI_API_KEY": key }));
        obj.insert("config".into(), Value::String(cleaned));
    }
    provider.updated_at = Some(chrono::Utc::now().timestamp_millis());
    Ok(())
}

fn strip_bearer_token(config_text: &str) -> Result<String, String> {
    if !config_text.contains("experimental_bearer_token") {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;
    if let Some(provider_id) = doc
        .get("model_provider")
        .and_then(|i| i.as_str())
        .map(str::to_string)
    {
        if let Some(table) = doc
            .get_mut("model_providers")
            .and_then(|i| i.as_table_like_mut())
            .and_then(|t| t.get_mut(provider_id.as_str()))
            .and_then(|i| i.as_table_like_mut())
        {
            table.remove("experimental_bearer_token");
        }
    }
    doc.as_table_mut().remove("experimental_bearer_token");
    Ok(doc.to_string())
}

fn write_atomic_auth_and_config(auth: &Value, config_text: &str) -> Result<(), String> {
    let auth_path = auth_path();
    let config_path = config_path();
    let old_auth = if auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|e| format!("读取 auth.json 失败: {e}"))?)
    } else {
        None
    };

    write_json_file(&auth_path, auth)?;
    if let Err(e) = write_text_file(&config_path, config_text) {
        if let Some(bytes) = old_auth {
            let _ = write_bytes(&auth_path, &bytes);
        } else {
            let _ = fs::remove_file(&auth_path);
        }
        return Err(e);
    }
    Ok(())
}

fn write_json_file(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("序列化 JSON 失败: {e}"))?;
    write_text_file(path, &text)
}

fn write_text_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    write_bytes(path, text.as_bytes())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("bak")
    ));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("写入临时文件失败: {e}"))?;
        f.write_all(bytes)
            .map_err(|e| format!("写入临时文件失败: {e}"))?;
        f.sync_all().map_err(|e| format!("同步文件失败: {e}"))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("替换文件失败: {e}")
    })
}

/// Parse form fields into settings_config for storage.
///
/// Priority:
/// 1. Advanced mode (`use_config_toml` or only config_toml without base_url) → raw TOML
/// 2. Structured form fields → patch existing config when possible, else build fresh
pub fn settings_from_form(
    api_key: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
    config_toml: Option<&str>,
    name: &str,
    category: Option<&str>,
    existing: Option<&Provider>,
    keep_existing_key: bool,
    wire_api: Option<&str>,
    reasoning_effort: Option<&str>,
    use_config_toml: bool,
) -> Result<Value, String> {
    let is_official = category == Some("official");
    if is_official {
        // Official profiles always keep canonical routing metadata — never
        // promote a third-party live import into the official seed.
        let auth = existing
            .and_then(|p| p.settings_config.get("auth").cloned())
            .unwrap_or_else(|| json!({}));
        let mut settings = official_settings_config();
        if auth_has_login_material(&auth) && !is_api_key_only_auth(&auth) {
            if let Some(obj) = settings.as_object_mut() {
                obj.insert("auth".into(), auth);
            }
        }
        return Ok(settings);
    }

    let mut key = api_key.map(str::trim).unwrap_or("").to_string();
    if key.is_empty() && keep_existing_key {
        if let Some(p) = existing {
            let auth = p.settings_config.get("auth").cloned().unwrap_or(json!({}));
            let cfg = p
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if let Some(prev) = extract_api_key(&auth, cfg) {
                key = prev;
            }
        }
    }

    let raw_toml = config_toml.map(str::trim).filter(|s| !s.is_empty());
    let form_url = base_url.map(str::trim).filter(|s| !s.is_empty());
    let advanced = use_config_toml || (raw_toml.is_some() && form_url.is_none());

    let existing_cfg = existing
        .and_then(|p| p.settings_config.get("config"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let live_cfg = read_config_text().unwrap_or_default();
    // Always start from the richest document so MCP / desktop / features survive
    // both structured patches and advanced partial edits (Codex++ preserve unmanaged).
    let merge_base = richer_config_base(existing_cfg, &live_cfg);

    let config = if advanced {
        let raw = raw_toml.ok_or_else(|| "高级模式需要填写 config.toml".to_string())?;
        validate_config_toml(raw)?;
        // Overlay user TOML onto full base — never replace a rich live/archive
        // file with a short routing-only template.
        let merged = if merge_base.trim().is_empty() {
            raw.to_string()
        } else {
            merge_toml_overlay(&merge_base, raw)?
        };
        if extract_base_url(&merged).is_none() {
            return Err("高级 config.toml 中缺少 base_url".into());
        }
        // Strip bearer from stored config; key lives in auth
        strip_bearer_token(&merged).unwrap_or(merged)
    } else {
        let url = form_url.unwrap_or("");
        if url.is_empty() {
            return Err("请填写 Base URL".into());
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("Base URL 必须以 http:// 或 https:// 开头".into());
        }
        let model_val = model.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("gpt-5.5");
        let wire = wire_api.unwrap_or("responses");
        let effort = reasoning_effort.unwrap_or("high");
        if merge_base.trim().is_empty() {
            build_third_party_config(name, url, model_val, wire, effort)
        } else {
            patch_config_from_form(
                &merge_base,
                name,
                Some(url),
                Some(model_val),
                Some(wire),
                Some(effort),
            )?
        }
    };

    Ok(json!({
        "auth": { "OPENAI_API_KEY": key },
        "config": config
    }))
}

pub fn summarize(
    provider: &Provider,
) -> (Option<String>, Option<String>, Option<String>) {
    if provider.is_official() {
        // Display official endpoints — not whatever was last imported from live.
        return (
            None,
            Some(OFFICIAL_CHATGPT_BACKEND.into()),
            Some(OFFICIAL_MODEL_HINT.into()),
        );
    }
    let auth = provider
        .settings_config
        .get("auth")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let config = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let key = extract_api_key(&auth, config);
    let base = extract_base_url(config);
    let model = extract_model(config);
    (key, base, model)
}

pub fn summarize_wire(provider: &Provider) -> Option<String> {
    if provider.is_official() {
        return Some(OFFICIAL_WIRE_API.into());
    }
    let config = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    extract_wire_api(config)
}

pub fn summarize_reasoning(provider: &Provider) -> Option<String> {
    let config = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    extract_reasoning_effort(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_live_rejects_third_party_proxy() {
        let third = r#"
model_provider = "cliproxyapi"
model = "gpt-5.6-sol"

[model_providers.cliproxyapi]
name = "OpenAI"
base_url = "https://oai.livein.eu.org/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        assert!(!is_official_live_config(third));

        let official_empty = "";
        assert!(is_official_live_config(official_empty));

        let official_builtin = r#"
model = "gpt-5.5"
approval_policy = "on-request"
"#;
        assert!(is_official_live_config(official_builtin));

        let openai_api = r#"
model_provider = "openai"

[model_providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
wire_api = "responses"
"#;
        assert!(is_official_live_config(openai_api));
    }

    #[test]
    fn strip_to_official_keeps_mcp_and_drops_proxy() {
        let live = r#"
model = "gpt-5.6-sol"
model_provider = "cliproxyapi"

[model_providers.cliproxyapi]
base_url = "https://oai.livein.eu.org/v1"
experimental_bearer_token = "sk-test"

[mcp_servers.node_repl]
command = "node"

[projects.'e:\demo']
trust_level = "trusted"
"#;
        let cleaned = strip_to_official_routing(live).expect("strip");
        assert!(!cleaned.contains("cliproxyapi"));
        assert!(!cleaned.contains("oai.livein.eu.org"));
        assert!(!cleaned.contains("experimental_bearer_token"));
        assert!(cleaned.contains("mcp_servers") || cleaned.contains("node_repl"));
        assert!(cleaned.contains("projects") || cleaned.contains("trust_level"));
        assert!(is_official_live_config(&cleaned));
    }

    #[test]
    fn merge_toml_overlay_preserves_unmanaged_keys() {
        let base = r#"
model = "old"
model_provider = "custom"

[model_providers.custom]
base_url = "https://old.example/v1"

[mcp_servers.demo]
command = "demo"

[desktop]
appearanceTheme = "dark"
"#;
        let overlay = r#"
model = "new-model"
model_provider = "custom"

[model_providers.custom]
name = "New"
base_url = "https://new.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let out = merge_toml_overlay(base, overlay).expect("merge");
        assert!(out.contains("new-model"));
        assert!(out.contains("https://new.example/v1"));
        assert!(out.contains("[mcp_servers.demo]"), "MCP must survive: {out}");
        assert!(out.contains("appearanceTheme"), "desktop must survive: {out}");
    }

    #[test]
    fn richer_config_prefers_live_with_more_sections() {
        let archive = "model = \"x\"\nmodel_provider = \"custom\"\n";
        let live = r#"
model = "y"
[mcp_servers.a]
command = "a"
[desktop]
followUpQueueMode = "queue"
"#;
        let base = richer_config_base(archive, live);
        assert!(base.contains("mcp_servers"));
    }

    #[test]
    fn edit_form_fields_override_stale_config_without_advanced() {
        let existing = Provider::new(
            "p1".into(),
            "Old".into(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-old" },
                "config": build_third_party_config(
                    "Old",
                    "https://old.example.com/v1",
                    "gpt-old",
                    "responses",
                    "high",
                ),
            }),
        );

        // Simulate frontend: form fields only, no configToml / use_config_toml=false
        let settings = settings_from_form(
            None, // keep existing key
            Some("https://new.example.com/v1"),
            Some("gpt-new"),
            None,
            "New Name",
            Some("custom"),
            Some(&existing),
            true,
            Some("chat"),
            Some("medium"),
            false,
        )
        .expect("edit save should succeed");

        let config = settings.get("config").and_then(|v| v.as_str()).unwrap();
        assert!(config.contains("https://new.example.com/v1"));
        assert!(config.contains("gpt-new"));
        assert!(config.contains("wire_api = \"chat\"") || config.contains("wire_api=\"chat\""));
        assert!(config.contains("medium"));
        let auth = settings.get("auth").unwrap();
        assert_eq!(
            auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()),
            Some("sk-old")
        );
    }

    #[test]
    fn advanced_toml_used_when_flagged() {
        let toml = r#"
model_provider = "custom"
model = "from-toml"

[model_providers.custom]
name = "TOML"
base_url = "https://toml.example.com/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let settings = settings_from_form(
            Some("sk-toml"),
            Some("https://should-be-ignored.example.com/v1"),
            Some("ignored-model"),
            Some(toml),
            "TOML",
            Some("custom"),
            None,
            false,
            Some("chat"),
            Some("low"),
            true, // advanced
        )
        .expect("advanced save");
        let config = settings.get("config").and_then(|v| v.as_str()).unwrap();
        assert!(config.contains("from-toml"));
        assert!(config.contains("https://toml.example.com/v1"));
        assert!(!config.contains("should-be-ignored"));
    }
}
