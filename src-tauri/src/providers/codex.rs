//! Codex live config helpers (`~/.codex/auth.json` + `config.toml`).
//!
//! Production path:
//! - Stored profile: `{ auth: { OPENAI_API_KEY }, config: "<toml>" }`
//! - On switch (third-party, default): **config-only** — provider-scoped
//!   `experimental_bearer_token` + `requires_openai_auth = true` in config.toml;
//!   leave `auth.json` ChatGPT / Codex OAuth cache intact.
//! - On switch (third-party, preserve off): dual-write auth.json API key + config
//!   (legacy / edge environments).
//! - Official: strip third-party routing; write auth only when archive carries
//!   real OAuth material, otherwise keep live auth.json.

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
/// Codex built-in default model restored when enabling OpenAI Official.
/// Third-party profiles often leave `model = "grok-4.5"` (etc.); that must not
/// survive a switch back to official routing.
pub const OFFICIAL_DEFAULT_MODEL: &str = "gpt-5.6-terra";
/// Display / seed default model label for official provider summaries.
pub const OFFICIAL_MODEL_HINT: &str = OFFICIAL_DEFAULT_MODEL;
/// Canonical `model_providers.*.name` for Codex (UI label; not the supplier archive title).
/// Always write this — never the ChatGPT Tools provider display name.
pub const PROVIDER_UI_NAME: &str = "OpenAI";

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

    let active = root
        .get("model_provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(id) = active {
        if let Some(entry) = root
            .get("model_providers")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get(id))
            .and_then(|v| v.as_table())
        {
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

/// Write the Codex OpenAI Official default model into a live document.
pub fn apply_official_default_model(doc: &mut DocumentMut) {
    doc["model"] = toml_edit::value(OFFICIAL_DEFAULT_MODEL);
}

/// Ensure top-level `model = OFFICIAL_DEFAULT_MODEL` in a config.toml string.
pub fn ensure_official_default_model_in_config(config_text: &str) -> Result<String, String> {
    let mut doc = if config_text.trim().is_empty() {
        DocumentMut::new()
    } else {
        config_text
            .parse::<DocumentMut>()
            .map_err(|e| format!("Invalid Codex config.toml: {e}"))?
    };
    apply_official_default_model(&mut doc);
    Ok(doc.to_string())
}

/// Strip third-party routing from an existing live config while preserving
/// MCP / plugins / projects / desktop / memories / etc.
///
/// Also resets top-level `model` to [`OFFICIAL_DEFAULT_MODEL`]: third-party
/// enables leave values like `grok-4.5` / `deepseek-chat` which are invalid
/// once routing is back on ChatGPT / Platform API.
pub fn strip_to_official_routing(config_text: &str) -> Result<String, String> {
    if config_text.trim().is_empty() {
        return Ok(official_config_toml());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;

    let active = doc
        .get("model_provider")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(mp) = active {
        if !is_official_provider_id(&mp) {
            doc.as_table_mut().remove("model_provider");
            if let Some(providers) = doc
                .get_mut("model_providers")
                .and_then(|i| i.as_table_like_mut())
            {
                providers.remove(mp.as_str());
            }
        }
    }
    doc.as_table_mut().remove("experimental_bearer_token");
    doc.as_table_mut().remove("model_catalog_json");
    apply_official_default_model(&mut doc);

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
/// `model_providers.*.name` is always [`PROVIDER_UI_NAME`] (`"OpenAI"`).
pub fn build_third_party_config(
    _provider_name: &str,
    base_url: &str,
    model: &str,
    wire_api: &str,
    reasoning_effort: &str,
) -> String {
    let name = toml_string(PROVIDER_UI_NAME);
    let url = toml_string(&normalize_base_url(base_url));
    let model = toml_string(if model.trim().is_empty() {
        "gpt-5.5"
    } else {
        model.trim()
    });
    // Client-facing protocol is always Responses. Upstream Chat Completions is
    // expressed via meta.apiFormat and converted by the local proxy.
    let _upstream = normalize_wire_api(wire_api);
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
wire_api = "responses"
requires_openai_auth = true
"#
    )
}

/// Normalize form / legacy wire labels to the two canonical upstream modes.
/// Archive TOML still always writes client `wire_api = "responses"`.
pub fn normalize_wire_api(wire: &str) -> String {
    match wire.trim().to_ascii_lowercase().as_str() {
        "chat" | "chat_completions" | "openai_chat" | "completions" | "openai-chat" => {
            "chat".into()
        }
        _ => "responses".into(),
    }
}

/// Force every `[model_providers.*].wire_api` in a routing fragment to `responses`.
pub fn force_client_wire_api_responses(config_text: &str) -> Result<String, String> {
    if config_text.trim().is_empty() {
        return Ok(String::new());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;
    if let Some(providers) = doc
        .get_mut("model_providers")
        .and_then(|i| i.as_table_like_mut())
    {
        let keys: Vec<String> = providers.iter().map(|(k, _)| k.to_string()).collect();
        for k in keys {
            if let Some(table) = providers.get_mut(&k).and_then(|i| i.as_table_like_mut()) {
                table.insert("wire_api", toml_edit::value("responses"));
            }
        }
    }
    Ok(doc.to_string())
}

/// Whether the form/upstream mode needs the local Responses↔Chat bridge.
pub fn upstream_is_chat(wire_or_format: &str) -> bool {
    matches!(
        normalize_wire_api(wire_or_format).as_str(),
        "chat"
    )
}

/// Map form wire selection → `meta.apiFormat` stored on the provider archive.
pub fn api_format_from_wire(wire: &str) -> &'static str {
    if upstream_is_chat(wire) {
        "openai_chat"
    } else {
        "openai_responses"
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

/// TOML basic-string encoding. Never interpolates raw user text (quotes,
/// backslashes, and control chars are escaped) so a hostile model id
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

/// Provider-owned top-level scalars. Everything else in live stays put.
const ROUTING_SCALAR_KEYS: &[&str] = &[
    "model",
    "model_provider",
    "model_reasoning_effort",
    "disable_response_storage",
];

fn active_provider_id(doc: &DocumentMut) -> String {
    doc.get("model_provider")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            doc.get("model_providers")
                .and_then(|i| i.as_table())
                .and_then(|t| t.iter().next().map(|(k, _)| k.to_string()))
        })
        .unwrap_or_else(|| "custom".into())
}

/// Archive / editor fragment: only `model` / `model_provider` /
/// `model_reasoning_effort` / `disable_response_storage` plus
/// `[model_providers.<id>]`. MCP / desktop / other provider ids are dropped.
pub fn extract_routing_fragment(config: &str) -> Result<String, String> {
    if config.trim().is_empty() {
        return Ok(String::new());
    }
    let src = config
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;
    let mut out = DocumentMut::new();
    for key in ROUTING_SCALAR_KEYS {
        if let Some(item) = src.get(key) {
            out[key] = item.clone();
        }
    }
    let provider_id = active_provider_id(&src);
    if let Some(table) = src
        .get("model_providers")
        .and_then(|i| i.as_table())
        .and_then(|t| t.get(provider_id.as_str()))
    {
        let mut providers = toml_edit::Table::new();
        let mut cloned = table.clone();
        if let Some(tbl) = cloned.as_table_mut() {
            tbl.remove("experimental_bearer_token");
        }
        providers.insert(provider_id.as_str(), cloned);
        out.as_table_mut()
            .insert("model_providers", toml_edit::Item::Table(providers));
    }
    out.as_table_mut().remove("experimental_bearer_token");
    Ok(out.to_string())
}

/// Patch only Codex routing nodes onto an existing live document.
///
/// Touches `model`, `model_provider`, `model_reasoning_effort`,
/// `disable_response_storage`, and `[model_providers.<id>]`. Leaves MCP,
/// desktop, features, projects, and other `model_providers.*` ids intact.
pub fn apply_routing_to_live(live: &str, archive: &str) -> Result<String, String> {
    if archive.trim().is_empty() {
        return Ok(live.to_string());
    }
    let fragment = extract_routing_fragment(archive)?;
    if live.trim().is_empty() {
        return Ok(fragment);
    }
    if fragment.trim().is_empty() {
        return Ok(live.to_string());
    }
    let archive_doc = fragment
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;
    let mut live_doc = live
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;

    for key in ROUTING_SCALAR_KEYS {
        if let Some(item) = archive_doc.get(key) {
            live_doc[key] = item.clone();
        }
    }

    let provider_id = active_provider_id(&archive_doc);
    if let Some(src_table) = archive_doc
        .get("model_providers")
        .and_then(|i| i.as_table())
        .and_then(|t| t.get(provider_id.as_str()))
    {
        let root = live_doc.as_table_mut();
        if !root.contains_key("model_providers") {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            root.insert("model_providers", toml_edit::Item::Table(t));
        }
        let providers = root
            .get_mut("model_providers")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Codex config.toml 中 model_providers 非法".to_string())?;
        if providers.get(provider_id.as_str()).is_none() {
            providers.insert(
                provider_id.as_str(),
                toml_edit::Item::Table(toml_edit::Table::new()),
            );
        }
        let dest = providers
            .get_mut(provider_id.as_str())
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| {
                format!("Codex config.toml 中 model_providers.{provider_id} 非法")
            })?;
        if let Some(src) = src_table.as_table() {
            for (key, item) in src.iter() {
                dest.insert(key, item.clone());
            }
        } else {
            providers.insert(provider_id.as_str(), src_table.clone());
        }
    }

    Ok(live_doc.to_string())
}

const DESKTOP_APPEARANCE_KEYS: &[&str] = &[
    "appearanceTheme",
    "appearanceLightCodeThemeId",
    "appearanceLightChromeTheme",
    "appearanceDarkCodeThemeId",
    "appearanceDarkChromeTheme",
];

/// After a provider overlay, restore live `[desktop]` appearance* scalars so
/// enabling a supplier does not clobber skin-managed chrome / code themes.
pub fn preserve_live_desktop_appearance(live: &str, merged: &str) -> Result<String, String> {
    if live.trim().is_empty() || merged.trim().is_empty() {
        return Ok(merged.to_string());
    }
    let live_doc = match live.parse::<DocumentMut>() {
        Ok(d) => d,
        Err(_) => return Ok(merged.to_string()),
    };
    let Some(live_desktop) = live_doc.get("desktop").and_then(|i| i.as_table()) else {
        return Ok(merged.to_string());
    };
    let mut out = merged
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;
    {
        let root = out.as_table_mut();
        if !root.contains_key("desktop") {
            root.insert("desktop", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let desktop = root
            .get_mut("desktop")
            .and_then(|i| i.as_table_like_mut())
            .ok_or_else(|| "Codex config.toml 中 [desktop] 非法".to_string())?;
        for key in DESKTOP_APPEARANCE_KEYS {
            if let Some(item) = live_desktop.get(key) {
                desktop.insert(key, item.clone());
            }
        }
    }
    Ok(out.to_string())
}

/// Config text shown in the advanced editor: routing fragment only.
/// Enabling a supplier patches these nodes onto live; MCP / desktop stay live-only.
pub fn config_for_editor(archive_config: &str) -> String {
    extract_routing_fragment(archive_config).unwrap_or_else(|_| archive_config.to_string())
}

/// Patch structured form fields into an existing Codex config while preserving
/// unrelated keys (MCP, extra model_providers, etc.).
///
/// `model_providers.*.name` is always [`PROVIDER_UI_NAME`] (`"OpenAI"`); the
/// `provider_name` argument is ignored (kept for call-site compatibility).
pub fn patch_config_from_form(
    existing: &str,
    _provider_name: &str,
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
            PROVIDER_UI_NAME,
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
        // Always OpenAI — never the ChatGPT Tools supplier display name.
        table.insert("name", toml_edit::value(PROVIDER_UI_NAME));
        if let Some(url) = base_url.map(str::trim).filter(|s| !s.is_empty()) {
            table.insert("base_url", toml_edit::value(normalize_base_url(url)));
        }
        // Always client-facing Responses; upstream Chat is meta.apiFormat + proxy.
        table.insert("wire_api", toml_edit::value("responses"));
        let _ = wire_api;
        if table.get("requires_openai_auth").is_none() {
            table.insert("requires_openai_auth", toml_edit::value(true));
        }
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
/// Also normalizes active `model_providers.*.name` to [`PROVIDER_UI_NAME`].
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
                table.insert("requires_openai_auth", toml_edit::value(true));
                table.insert("name", toml_edit::value(PROVIDER_UI_NAME));
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

/// Whether stored provider roughly matches live routing.
///
/// Compares **base_url only** (or official shape). Do **not** compare `model`:
/// users freely switch models in the Codex GUI; treating archive default model
/// as a match key causes false「本机漂移」.
pub fn matches_live(provider: &Provider, live: &LiveSnapshot) -> bool {
    let (_, base, _model) = summarize(provider);
    if provider.is_official() {
        let live_config = read_config_text().unwrap_or_default();
        return is_official_live_config(&live_config);
    }
    // Local-routing shell: live points at proxy; archive still holds real upstream.
    // live_status takeover path handles that; here only direct-mode base_url identity.
    match (base.as_deref(), live.base_url.as_deref()) {
        (Some(a), Some(b)) => normalize_base_url(a) == normalize_base_url(b),
        (None, None) => true,
        _ => false,
    }
}

/// Options for projecting a provider into live Codex files.
#[derive(Debug, Clone, Copy)]
pub struct WriteLiveOptions {
    /// When true, third-party switches only rewrite config.toml.
    pub preserve_official_auth: bool,
}

impl Default for WriteLiveOptions {
    fn default() -> Self {
        Self {
            preserve_official_auth: true,
        }
    }
}

/// Write live Codex files for the selected provider.
pub fn write_live(provider: &Provider) -> Result<Vec<String>, String> {
    write_live_with_options(
        provider,
        WriteLiveOptions {
            preserve_official_auth: super::store::preserve_codex_official_auth(),
        },
    )
}

/// Write live Codex files with explicit options (tests / takeover hooks).
pub fn write_live_with_options(
    provider: &Provider,
    options: WriteLiveOptions,
) -> Result<Vec<String>, String> {
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
        let _ = backup_live_files();
        crate::live_config::read_modify_write(&config_path(), |live_existing| {
            let cleaned = if live_existing.trim().is_empty() {
                if config_text.trim().is_empty() {
                    official_config_toml()
                } else {
                    config_text.clone()
                }
            } else {
                strip_to_official_routing(live_existing)?
            };
            ensure_official_default_model_in_config(&cleaned)
        })?;

        if auth_has_login_material(&auth) && !is_api_key_only_auth(&auth) {
            write_json_file(&auth_path(), &auth)?;
            warnings.push("已切换为 OpenAI 官方，并写入 ChatGPT 登录态。".into());
        } else {
            let live_auth = read_auth();
            if is_api_key_only_auth(&live_auth) {
                warnings.push(
                    "已切换为 OpenAI 官方。当前登录态仅为 API Key，订阅功能请在 Codex 内重新登录。"
                        .into(),
                );
            } else {
                warnings.push("已切换为 OpenAI 官方（保留现有登录态）。".into());
            }
        }
        // Official never needs third-party model whitelist; clear CDP hooks off-thread
        // so switch/save returns immediately (CDP open/evaluate can take seconds).
        super::model_unlock::schedule_official_activated();
        return Ok(warnings);
    }

    // Third-party: API key lives in archive auth; live prefers config-scoped bearer
    // so ChatGPT OAuth in auth.json can survive provider switches.
    let api_key = extract_api_key(&auth, &config_text)
        .ok_or_else(|| "请先填写 API Key 再启用该供应商".to_string())?;

    // Normalize modelCatalog SSOT: always include default model + mapping rows
    // so third-party slugs (DeepSeek / Claude / Gemini / Grok / …) are not dropped.
    let mut settings_for_catalog = provider.settings_config.clone();
    let default_from_form = extract_model(&config_text);
    let mut merge_ids = Vec::new();
    if let Some(m) = default_from_form {
        merge_ids.push(m);
    }
    let _ = super::catalog::merge_models_into_settings(&mut settings_for_catalog, merge_ids);

    // Backup previous live files under app state (best-effort)
    let _ = backup_live_files();

    let preserve_auth = options.preserve_official_auth;
    let catalog_settings = settings_for_catalog.clone();
    crate::live_config::read_modify_write(&config_path(), |live_existing| {
        // Only patch routing nodes — never overlay the whole archive onto live.
        let applied = apply_routing_to_live(live_existing, &config_text)?;
        let mut live_config = set_bearer_token(&applied, &api_key)?;
        live_config = ensure_requires_openai_auth(&live_config)?;

        live_config = super::catalog::ensure_config_model_from_catalog(
            &live_config,
            &catalog_settings,
        )?;
        let default_model = extract_model(&live_config);
        live_config = super::catalog::prepare_config_with_catalog(
            &home,
            &catalog_settings,
            &live_config,
            default_model.as_deref(),
        )?;
        Ok(live_config)
    })?;

    let projected = super::catalog::model_slugs_from_catalog_file(&home);
    // User-facing: only warn when mapping is empty (actionable). No catalog dump / inject toasts.
    if projected.is_empty() {
        warnings.push("模型映射为空，请在「模型映射」中添加可用模型后再用。".into());
    }

    if !preserve_auth {
        // Legacy dual-write for environments that only honor auth.json keys.
        let live_auth = json!({ "OPENAI_API_KEY": api_key });
        write_json_file(&auth_path(), &live_auth)?;
        warnings.push(
            "已写入 API Key 到登录文件（未保留官方登录）。如需保留 ChatGPT 登录，请开启「切换第三方时保留官方登录」。"
                .into(),
        );
    }

    // Desktop whitelist unlock — off-thread so GUI switch/save is not blocked by CDP.
    super::model_unlock::schedule_desktop_unlock(Some(settings_for_catalog));

    Ok(warnings)
}

/// Ensure active `model_providers.<id>.requires_openai_auth = true` so Codex
/// still loads ChatGPT login material from auth.json while using a custom base_url.
fn ensure_requires_openai_auth(config_text: &str) -> Result<String, String> {
    if config_text.trim().is_empty() {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| format!("Invalid Codex config.toml: {e}"))?;
    let provider_id = doc
        .get("model_provider")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "custom".into());
    if let Some(providers) = doc
        .get_mut("model_providers")
        .and_then(|i| i.as_table_like_mut())
    {
        if let Some(table) = providers
            .get_mut(provider_id.as_str())
            .and_then(|i| i.as_table_like_mut())
        {
            table.insert("requires_openai_auth", toml_edit::value(true));
        }
    }
    Ok(doc.to_string())
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
    // Never backfill loopback proxy URLs into archives.
    if let Some(base) = extract_base_url(&live_config) {
        let cfg = super::store::load().map(|f| f.proxy).unwrap_or_default();
        if crate::proxy::is_proxy_base_url(&base, &cfg) || base.contains("127.0.0.1") {
            return Ok(());
        }
    }
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
    // Prefer provider-scoped bearer from config so we never pull ChatGPT OAuth
    // tokens into a third-party archive's auth blob.
    let key = extract_bearer_token(&live_config)
        .or_else(|| extract_api_key(&live_auth, &live_config))
        .filter(|k| {
            // Ignore OAuth-shaped live auth when it only has login material.
            !k.is_empty()
        })
        .unwrap_or_default();
    // If live auth is OAuth-only and config has no bearer, keep existing archive key.
    let key = if key.is_empty() {
        extract_api_key(
            provider
                .settings_config
                .get("auth")
                .unwrap_or(&json!({})),
            provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        )
        .unwrap_or_default()
    } else {
        key
    };
    // Store routing fragment only — MCP / desktop stay live-only.
    let cleaned = strip_bearer_token(&live_config).unwrap_or(live_config);
    let fragment = extract_routing_fragment(&cleaned).unwrap_or(cleaned);
    if let Some(obj) = provider.settings_config.as_object_mut() {
        if !key.is_empty() {
            obj.insert("auth".into(), json!({ "OPENAI_API_KEY": key }));
        }
        obj.insert("config".into(), Value::String(fragment));
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

fn write_json_file(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("序列化 JSON 失败: {e}"))?;
    write_text_file(path, &text)
}

fn write_text_file(path: &Path, text: &str) -> Result<(), String> {
    // Serialize with skin/proxy writers when touching the live Codex config.
    if path == config_path() {
        return crate::live_config::write_text(path, text);
    }
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
    // Archives store a routing fragment only — never pull live MCP/desktop in.
    let existing_fragment =
        extract_routing_fragment(existing_cfg).unwrap_or_else(|_| existing_cfg.to_string());

    // Upstream mode for meta.apiFormat (archive TOML always keeps client responses).
    let mut upstream_wire = wire_api
        .map(normalize_wire_api)
        .unwrap_or_else(|| "responses".into());

    let config = if advanced {
        let raw = raw_toml.ok_or_else(|| "高级模式需要填写 config.toml".to_string())?;
        validate_config_toml(raw)?;
        let fragment = extract_routing_fragment(raw)?;
        if extract_base_url(&fragment).is_none() {
            return Err("高级 config.toml 中缺少 base_url".into());
        }
        // Capture legacy wire_api=chat from advanced TOML before rewriting to responses.
        if let Some(w) = extract_wire_api(&fragment) {
            if upstream_is_chat(&w) {
                upstream_wire = "chat".into();
            }
        }
        let stripped = strip_bearer_token(&fragment).unwrap_or(fragment);
        // Client protocol must stay Responses; Chat upstream is meta.apiFormat.
        force_client_wire_api_responses(&stripped).unwrap_or(stripped)
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
        if existing_fragment.trim().is_empty() {
            build_third_party_config(name, url, model_val, wire, effort)
        } else {
            patch_config_from_form(
                &existing_fragment,
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
        "config": config,
        // Convenience mirror for detection / migration (meta.apiFormat is authoritative).
        "apiFormat": api_format_from_wire(&upstream_wire)
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
    // Prefer meta.apiFormat (upstream truth); archive TOML is always client responses.
    if let Some(fmt) = provider
        .meta
        .as_ref()
        .and_then(|m| m.api_format.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(if upstream_is_chat(fmt) {
            "chat".into()
        } else {
            "responses".into()
        });
    }
    let config = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // Legacy archives may still have wire_api=chat in TOML.
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
