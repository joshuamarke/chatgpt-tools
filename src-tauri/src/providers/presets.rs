//! Built-in channel presets for quick add (Codex + Grok).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub app: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    pub base_url: String,
    pub model: String,
    /// Suggested multi-model mapping for Codex model_catalog_json (optional).
    /// When set, the add form seeds the mapping table with these ids so desktop/CLI
    /// can list third-party models without requiring an immediate /models pull.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    /// `responses` | `chat` (Codex wire_api). Grok ignores; always responses-like.
    pub wire_api: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

pub fn list_presets(app: &str) -> Vec<ProviderPreset> {
    let app = app.trim().to_ascii_lowercase();
    all_presets()
        .into_iter()
        .filter(|p| p.app == app)
        .collect()
}

fn all_presets() -> Vec<ProviderPreset> {
    vec![
        // ── Codex ─────────────────────────────────────────────
        ProviderPreset {
            id: "codex-custom".into(),
            name: "自定义渠道".into(),
            app: "codex".into(),
            website_url: None,
            base_url: "https://api.example.com/v1".into(),
            model: "gpt-5.5".into(),
            models: vec![
                "gpt-5.5".into(),
                "grok-4.5".into(),
                "deepseek-chat".into(),
                "claude-sonnet-4".into(),
                "gemini-2.5-pro".into(),
            ],
            wire_api: "responses".into(),
            notes: Some(
                "填写中转 / 聚合站 Base URL 与 API Key。模型映射已预填常见第三方 id，请按实际上游增删，或点「拉取模型」覆盖。"
                    .into(),
            ),
            category: Some("custom".into()),
        },
        ProviderPreset {
            id: "codex-openrouter".into(),
            name: "OpenRouter".into(),
            app: "codex".into(),
            website_url: Some("https://openrouter.ai".into()),
            base_url: "https://openrouter.ai/api/v1".into(),
            model: "openai/gpt-5.1".into(),
            models: vec![
                "openai/gpt-5.1".into(),
                "x-ai/grok-4.5".into(),
                "deepseek/deepseek-chat".into(),
                "anthropic/claude-sonnet-4".into(),
                "google/gemini-2.5-pro".into(),
            ],
            wire_api: "responses".into(),
            notes: Some(
                "OpenRouter 聚合；模型 id 含厂商前缀。建议「拉取模型」同步真实列表。".into(),
            ),
            category: Some("aggregator".into()),
        },
        ProviderPreset {
            id: "codex-deepseek".into(),
            name: "DeepSeek".into(),
            app: "codex".into(),
            website_url: Some("https://platform.deepseek.com".into()),
            base_url: "https://api.deepseek.com/v1".into(),
            model: "deepseek-chat".into(),
            models: vec!["deepseek-chat".into(), "deepseek-reasoner".into()],
            wire_api: "responses".into(),
            notes: Some(
                "DeepSeek 官方 API（默认 responses 协议）。启用后建议开本地路由；映射含 chat + reasoner。"
                    .into(),
            ),
            category: Some("third_party".into()),
        },
        ProviderPreset {
            id: "codex-moonshot".into(),
            name: "Kimi (Moonshot)".into(),
            app: "codex".into(),
            website_url: Some("https://platform.moonshot.cn".into()),
            base_url: "https://api.moonshot.cn/v1".into(),
            model: "kimi-k2.5".into(),
            models: vec!["kimi-k2.5".into(), "kimi-k2.5-thinking".into()],
            wire_api: "chat".into(),
            notes: Some("月之暗面 Kimi。Chat Completions 建议开本地路由。".into()),
            category: Some("third_party".into()),
        },
        ProviderPreset {
            id: "codex-siliconflow".into(),
            name: "SiliconFlow".into(),
            app: "codex".into(),
            website_url: Some("https://siliconflow.cn".into()),
            base_url: "https://api.siliconflow.cn/v1".into(),
            model: "deepseek-ai/DeepSeek-V3".into(),
            models: vec![
                "deepseek-ai/DeepSeek-V3".into(),
                "deepseek-ai/DeepSeek-R1".into(),
                "Qwen/Qwen2.5-72B-Instruct".into(),
            ],
            wire_api: "chat".into(),
            notes: Some("硅基流动聚合。建议拉取模型覆盖完整列表。".into()),
            category: Some("aggregator".into()),
        },
        ProviderPreset {
            id: "codex-azure-compat".into(),
            name: "OpenAI 兼容接口".into(),
            app: "codex".into(),
            website_url: None,
            base_url: "https://your-endpoint/v1".into(),
            model: "gpt-4o".into(),
            models: vec![
                "gpt-4o".into(),
                "gpt-5.5".into(),
                "grok-4.5".into(),
                "deepseek-chat".into(),
                "claude-sonnet-4".into(),
                "gemini-2.5-pro".into(),
            ],
            wire_api: "chat".into(),
            notes: Some(
                "任意 OpenAI-compatible /v1 网关。映射表已预填多厂商 id，请按上游实际修改。"
                    .into(),
            ),
            category: Some("custom".into()),
        },
        // ── Grok ──────────────────────────────────────────────
        ProviderPreset {
            id: "grok-custom".into(),
            name: "自定义 Grok 渠道".into(),
            app: "grok".into(),
            website_url: None,
            base_url: "https://api.example.com/v1".into(),
            model: "grok-4.5".into(),
            models: vec![],
            wire_api: "responses".into(),
            notes: Some("兼容 Grok 模型的第三方 / 中转".into()),
            category: Some("custom".into()),
        },
        ProviderPreset {
            id: "grok-openrouter".into(),
            name: "OpenRouter (Grok)".into(),
            app: "grok".into(),
            website_url: Some("https://openrouter.ai".into()),
            base_url: "https://openrouter.ai/api/v1".into(),
            model: "x-ai/grok-4.5".into(),
            models: vec![],
            wire_api: "responses".into(),
            notes: Some("经 OpenRouter 调用 xAI Grok".into()),
            category: Some("aggregator".into()),
        },
        ProviderPreset {
            id: "grok-xai".into(),
            name: "xAI 官方 API".into(),
            app: "grok".into(),
            website_url: Some("https://console.x.ai".into()),
            base_url: "https://api.x.ai/v1".into(),
            model: "grok-4.5".into(),
            models: vec![],
            wire_api: "responses".into(),
            notes: Some(
                "Platform API Key（api.x.ai/v1）。与内置「Grok Official」OAuth 订阅不同：这是 BYOK 自定义 [model.*] 档案。"
                    .into(),
            ),
            category: Some("third_party".into()),
        },
    ]
}
