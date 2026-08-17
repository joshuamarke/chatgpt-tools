//! Detect whether a provider's real upstream speaks Chat Completions.

use crate::providers::models::{CodexChatReasoningConfig, Provider};
use serde_json::Value as JsonValue;

fn is_chat_wire_api(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "chat"
            | "chat_completions"
            | "chat-completions"
            | "openai_chat"
            | "openai-chat"
            | "openai_chat_completions"
    )
}

fn is_chat_completions_url(url: &str) -> bool {
    let lower = url.trim().trim_end_matches('/').to_ascii_lowercase();
    lower.ends_with("/chat/completions") || lower.contains("/chat/completions?")
}

fn extract_codex_wire_api_from_toml(config_text: &str) -> Option<String> {
    crate::providers::codex::extract_wire_api(config_text)
}

fn extract_codex_base_url_from_toml(config_text: &str) -> Option<String> {
    crate::providers::codex::extract_base_url(config_text)
}

/// Whether this provider's real upstream should be called via Chat Completions.
///
/// Priority: `meta.apiFormat` 鈫?settings `apiFormat` 鈫?archive Codex `wire_api`
/// / Grok `api_backend` 鈫?base_url shape.
pub fn codex_provider_uses_chat_completions(provider: &Provider) -> bool {
    if let Some(api_format) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("api_format")
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("apiFormat")
                .and_then(|v| v.as_str())
        })
    {
        return is_chat_wire_api(api_format);
    }

    if let Some(config) = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
    {
        if let Some(wire_api) = extract_codex_wire_api_from_toml(config) {
            if is_chat_wire_api(&wire_api) {
                return true;
            }
        }
        // Grok Build: api_backend = "chat_completions"
        if config.to_ascii_lowercase().contains("api_backend") {
            if let Ok(doc) = config.parse::<toml_edit::DocumentMut>() {
                if let Some(models) = doc.get("model").and_then(|i| i.as_table_like()) {
                    for (_, item) in models.iter() {
                        if let Some(backend) = item
                            .as_table_like()
                            .and_then(|t| t.get("api_backend"))
                            .and_then(|v| v.as_str())
                        {
                            if is_chat_wire_api(backend) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(base_url) = provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .and_then(|v| v.as_str())
    {
        return is_chat_completions_url(base_url);
    }

    provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_codex_base_url_from_toml)
        .map(|url| is_chat_completions_url(&url))
        .unwrap_or(false)
}

pub fn should_convert_codex_responses_to_chat(provider: &Provider, endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map_or(endpoint, |(path, _query)| path);
    let path = path.trim_end_matches('/');

    // Codex: /v1/responses ; Grok local proxy: /grok/v1/responses
    let is_responses = path.ends_with("/responses")
        || path.ends_with("/responses/compact")
        || path == "/responses"
        || path == "/responses/compact";

    is_responses && codex_provider_uses_chat_completions(provider)
}

pub fn should_send_codex_chat_prompt_cache_key(provider: &Provider) -> bool {
    match provider
        .meta
        .as_ref()
        .and_then(|meta| meta.prompt_cache_routing.as_deref())
        .unwrap_or("auto")
    {
        "enabled" => return true,
        "disabled" => return false,
        _ => {}
    }

    let base_url = provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|value| value.as_str())
                .and_then(extract_codex_base_url_from_toml)
        });

    let Some(base_url) = base_url else {
        return false;
    };
    let Ok(url) = url::Url::parse(&base_url) else {
        return false;
    };

    match url.host_str() {
        Some("api.openai.com") => true,
        Some("api.kimi.com") => {
            let path = url.path().trim_end_matches('/');
            path == "/coding" || path.starts_with("/coding/")
        }
        _ => false,
    }
}

pub fn inject_codex_chat_prompt_cache_key(
    provider: &Provider,
    chat_body: &mut JsonValue,
    explicit_key: Option<&str>,
    client_session_id: Option<&str>,
) -> bool {
    if !should_send_codex_chat_prompt_cache_key(provider) {
        return false;
    }

    let key = explicit_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .or_else(|| {
            client_session_id
                .map(str::trim)
                .filter(|session_id| !session_id.is_empty())
        });
    let Some(key) = key else {
        return false;
    };

    chat_body["prompt_cache_key"] = JsonValue::String(key.to_string());
    true
}

fn normalize_codex_chat_reasoning_config(
    mut config: CodexChatReasoningConfig,
) -> CodexChatReasoningConfig {
    if config.supports_effort.unwrap_or(false) && config.supports_thinking.is_none() {
        config.supports_thinking = Some(true);
    }
    config
}

fn zen_catalog_effort_levels(provider: &Provider, body: &JsonValue) -> Option<Vec<String>> {
    let model = body.get("model")?.as_str()?.trim();
    if model.is_empty() {
        return None;
    }
    let entries = provider
        .settings_config
        .get("modelCatalog")?
        .get("models")?
        .as_array()?;
    let entry = entries.iter().find(|entry| {
        entry
            .get("model")
            .and_then(|value| value.as_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(model))
    })?;
    let levels_value = entry
        .get("reasoningLevels")
        .or_else(|| entry.get("reasoning_levels"))?;
    let levels: Vec<String> = levels_value
        .as_array()?
        .iter()
        .filter_map(|level| level.as_str().map(str::to_string))
        .collect();
    (!levels.is_empty()).then_some(levels)
}

fn infer_codex_chat_reasoning_config(
    provider: &Provider,
    body: &JsonValue,
) -> Option<CodexChatReasoningConfig> {
    let model = body
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let base_url = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_codex_base_url_from_toml)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = provider.name.to_ascii_lowercase();

    // DeepSeek reasoner / thinking models
    if model.contains("deepseek") && (model.contains("reasoner") || model.contains("r1"))
        || base_url.contains("deepseek.com") && model.contains("reasoner")
    {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: None,
            effort_param: None,
            effort_value_mode: None,
            output_format: Some("reasoning_content".into()),
            effort_levels: None,
        });
    }

    // Moonshot / Kimi thinking
    if name.contains("kimi") || name.contains("moonshot") || base_url.contains("moonshot") {
        if model.contains("thinking") || model.contains("k2") {
            return Some(CodexChatReasoningConfig {
                supports_thinking: Some(true),
                supports_effort: Some(true),
                thinking_param: Some("thinking".into()),
                effort_param: Some("reasoning_effort".into()),
                effort_value_mode: None,
                output_format: Some("reasoning_content".into()),
                effort_levels: None,
            });
        }
    }

    // OpenAI o-series / gpt-5 via chat (rare but supported)
    if crate::proxy::protocol::openai_helpers::supports_reasoning_effort(&model) {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: None,
            effort_param: Some("reasoning_effort".into()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".into()),
            effort_levels: None,
        });
    }

    None
}

pub fn resolve_codex_chat_reasoning_config(
    provider: &Provider,
    body: &JsonValue,
) -> Option<CodexChatReasoningConfig> {
    let mut config = if let Some(config) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.codex_chat_reasoning.clone())
    {
        normalize_codex_chat_reasoning_config(config)
    } else {
        infer_codex_chat_reasoning_config(provider, body)?
    };

    if config.effort_value_mode.as_deref() == Some("zen") {
        config.effort_levels = zen_catalog_effort_levels(provider, body);
    }

    Some(config)
}
