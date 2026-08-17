//! Codex Responses ↔ OpenAI Chat Completions conversion.
//!
//! This module is used when the Codex client talks to CC Switch through the
//! Responses API, while the selected upstream provider only exposes an
//! OpenAI-compatible Chat Completions endpoint.

use super::codex_chat_common::{
    append_reasoning_content, extract_reasoning_field_text, extract_reasoning_summary_text,
    response_function_call_item, response_function_call_item_with_namespace,
    split_leading_think_block,
};
use crate::providers::models::CodexChatReasoningConfig;
use super::ProxyError;
use super::json_canonical::{
    canonical_json_string, canonicalize_json_string_if_parseable, canonicalize_tool_arguments,
    short_sha256_hex,
};
use super::tool_media::{
    chat_file_from_input_file, flush_pending_chat_tool_media, plan_chat_tool_output_media,
    queue_chat_tool_output_media, strip_and_clamp_media_from_tool_value,
    TOOL_RESULT_MEDIA_MOVED_MARKER,
};
use super::openai_helpers as transform;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const EXTRA_CHAT_PASSTHROUGH_FIELDS: &[&str] = &[
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "metadata",
    "n",
    "parallel_tool_calls",
    "presence_penalty",
    "response_format",
    "seed",
    "service_tier",
    "stop",
    "stream_options",
    "top_logprobs",
    "user",
];

const TOOL_SEARCH_PROXY_NAME: &str = "tool_search";
const CUSTOM_TOOL_INPUT_FIELD: &str = "input";
const CHAT_TOOL_NAME_MAX_LEN: usize = 64;
const CUSTOM_TOOL_INPUT_DESCRIPTION: &str = "Raw string input for the original custom tool. Preserve formatting exactly and follow the original tool definition embedded in the description.";
const CUSTOM_TOOL_PRESERVED_METADATA_HEADING: &str = "Original tool definition:";
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexToolKind {
    Function,
    Namespace,
    Custom,
    ToolSearch,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexToolSpec {
    pub(crate) kind: CodexToolKind,
    pub(crate) name: String,
    pub(crate) namespace: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CodexToolContext {
    chat_tools: Vec<Value>,
    seen_chat_names: HashSet<String>,
    chat_name_to_spec: HashMap<String, CodexToolSpec>,
    namespace_name_to_chat_name: HashMap<(String, String), String>,
}

impl CodexToolContext {
    pub(crate) fn chat_tools(&self) -> &[Value] {
        &self.chat_tools
    }

    pub(crate) fn lookup_chat_name(&self, chat_name: &str) -> Option<&CodexToolSpec> {
        self.chat_name_to_spec.get(chat_name)
    }

    pub(crate) fn is_custom_tool_chat_name(&self, chat_name: &str) -> bool {
        self.lookup_chat_name(chat_name)
            .is_some_and(|spec| matches!(&spec.kind, CodexToolKind::Custom))
    }

    pub(crate) fn chat_name_for_response_function(
        &self,
        name: &str,
        namespace: Option<&str>,
    ) -> String {
        if let Some(namespace) = namespace.filter(|value| !value.is_empty()) {
            if let Some(chat_name) = self
                .namespace_name_to_chat_name
                .get(&(namespace.to_string(), name.to_string()))
            {
                return chat_name.clone();
            }
            return flatten_namespace_tool_name(namespace, name);
        }

        name.to_string()
    }

    fn add_chat_tool(&mut self, chat_name: String, spec: CodexToolSpec, chat_tool: Value) {
        if chat_name.trim().is_empty() || self.seen_chat_names.contains(&chat_name) {
            return;
        }
        self.seen_chat_names.insert(chat_name.clone());
        if let Some(namespace) = spec.namespace.as_ref() {
            self.namespace_name_to_chat_name
                .insert((namespace.clone(), spec.name.clone()), chat_name.clone());
        }
        self.chat_name_to_spec.insert(chat_name, spec);
        self.chat_tools.push(chat_tool);
    }

    fn add_function_tool(&mut self, tool: &Value, namespace: Option<&str>) {
        let Some(original_name) = responses_tool_name(tool) else {
            return;
        };
        let chat_name = namespace
            .map(|namespace| flatten_namespace_tool_name(namespace, &original_name))
            .unwrap_or_else(|| original_name.clone());

        let Some(chat_tool) = responses_function_tool_to_chat_tool(tool, &chat_name) else {
            return;
        };
        let spec = CodexToolSpec {
            kind: if namespace.is_some() {
                CodexToolKind::Namespace
            } else {
                CodexToolKind::Function
            },
            name: original_name,
            namespace: namespace.map(ToString::to_string),
        };
        self.add_chat_tool(chat_name, spec, chat_tool);
    }

    fn add_custom_tool(&mut self, tool: &Value) {
        let Some(name) = responses_tool_name(tool) else {
            return;
        };
        let description = json!(responses_custom_tool_description(tool));
        let chat_tool = json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": {
                        CUSTOM_TOOL_INPUT_FIELD: {
                            "type": "string",
                            "description": CUSTOM_TOOL_INPUT_DESCRIPTION
                        }
                    },
                    "required": [CUSTOM_TOOL_INPUT_FIELD]
                }
            }
        });
        let spec = CodexToolSpec {
            kind: CodexToolKind::Custom,
            name: name.clone(),
            namespace: None,
        };
        self.add_chat_tool(name, spec, chat_tool);
    }

    fn add_tool_search_tool(&mut self) {
        let chat_tool = json!({
            "type": "function",
            "function": {
                "name": TOOL_SEARCH_PROXY_NAME,
                "description": "Search and load Codex tools, plugins, connectors, and MCP namespaces for the current task.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query for tools or connectors to load."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of tool groups to return."
                        }
                    },
                    "required": ["query"]
                }
            }
        });
        let spec = CodexToolSpec {
            kind: CodexToolKind::ToolSearch,
            name: TOOL_SEARCH_PROXY_NAME.to_string(),
            namespace: None,
        };
        self.add_chat_tool(TOOL_SEARCH_PROXY_NAME.to_string(), spec, chat_tool);
    }

    fn add_namespace_tool(&mut self, namespace_tool: &Value) {
        let Some(namespace) = namespace_tool.get("name").and_then(|v| v.as_str()) else {
            return;
        };
        let Some(children) = namespace_tool
            .get("tools")
            .or_else(|| namespace_tool.get("children"))
            .and_then(|v| v.as_array())
        else {
            return;
        };

        for child in children {
            if child.get("type").and_then(|v| v.as_str()) == Some("function") {
                self.add_function_tool(child, Some(namespace));
            }
        }
    }

    fn add_response_tool(&mut self, tool: &Value) {
        match tool {
            Value::String(name) => {
                self.add_custom_tool(&json!({
                    "type": "custom",
                    "name": name
                }));
            }
            Value::Object(_) => match tool.get("type").and_then(|v| v.as_str()) {
                Some("function") => self.add_function_tool(tool, None),
                Some("custom") => self.add_custom_tool(tool),
                Some("tool_search") => self.add_tool_search_tool(),
                Some("namespace") => self.add_namespace_tool(tool),
                _ => {}
            },
            _ => {}
        }
    }
}

pub(crate) fn build_codex_tool_context_from_request(body: &Value) -> CodexToolContext {
    let mut context = CodexToolContext::default();

    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        for tool in tools {
            context.add_response_tool(tool);
        }
    }

    if let Some(input) = body.get("input") {
        collect_tool_search_output_tools(input, &mut context);
    }

    context
}

/// Convert an OpenAI Responses request into an OpenAI Chat Completions request.
#[allow(dead_code)]
pub fn responses_to_chat_completions(body: Value) -> Result<Value, ProxyError> {
    responses_to_chat_completions_with_reasoning(body, None)
}

/// Convert an OpenAI Responses request into an OpenAI Chat Completions request,
/// using provider-declared Codex Chat reasoning capabilities when available.
pub fn responses_to_chat_completions_with_reasoning(
    body: Value,
    reasoning_config: Option<&CodexChatReasoningConfig>,
) -> Result<Value, ProxyError> {
    let mut result = json!({});
    let tool_context = build_codex_tool_context_from_request(&body);

    if let Some(model) = body.get("model") {
        result["model"] = model.clone();
    }

    let mut messages = Vec::new();
    if let Some(instructions) = body.get("instructions") {
        let instructions = instruction_text(instructions);
        if !instructions.is_empty() {
            messages.push(json!({
                "role": "system",
                "content": instructions
            }));
        }
    }

    if let Some(input) = body.get("input") {
        append_responses_input_as_chat_messages(input, &mut messages, &tool_context)?;
    }
    let messages = collapse_system_messages_to_head(messages);
    result["messages"] = json!(messages);

    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
    if let Some(max_tokens) = body.get("max_output_tokens") {
        if transform::is_openai_o_series(model) {
            result["max_completion_tokens"] = max_tokens.clone();
        } else {
            result["max_tokens"] = max_tokens.clone();
        }
    }
    if let Some(max_tokens) = body.get("max_tokens") {
        result["max_tokens"] = max_tokens.clone();
    }
    if let Some(max_tokens) = body.get("max_completion_tokens") {
        result["max_completion_tokens"] = max_tokens.clone();
    }

    for key in ["temperature", "top_p", "stream"] {
        if let Some(value) = body.get(key) {
            result[key] = value.clone();
        }
    }

    apply_reasoning_options(&mut result, &body, model, reasoning_config);

    let tools = tool_context.chat_tools();
    if !tools.is_empty() {
        result["tools"] = json!(tools);
    }

    if let Some(tool_choice) = body.get("tool_choice") {
        result["tool_choice"] = responses_tool_choice_to_chat(tool_choice, &tool_context);
    }

    for key in EXTRA_CHAT_PASSTHROUGH_FIELDS {
        if let Some(value) = body.get(*key) {
            result[*key] = value.clone();
        }
    }

    // Strict OpenAI-compatible upstreams (vLLM, enterprise gateways) reject
    // requests that carry tool_choice or parallel_tool_calls without a non-empty
    // tools array. Drop both fields when tools ended up absent or empty after
    // conversion to avoid 503/400 from such providers.
    let has_tools = result
        .get("tools")
        .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()));
    if !has_tools {
        if let Some(obj) = result.as_object_mut() {
            obj.remove("tool_choice");
            obj.remove("parallel_tool_calls");
        }
    }
    // OpenAI 兼容上游在流式下默认不在 SSE 里返回 usage，必须显式声明
    // include_usage 才会在末尾吐 usage chunk。Codex CLI 用 Responses 协议、
    // 自身不带 stream_options，缺这一注入会导致 kimi/MiniMax 等第三方流式请求的
    // token/成本/缓存命中率全部漏记（input/output/cache 全为 0）。
    // 与 Claude→openai_chat 路径共用同一 helper，保证两个客户端方向一致。
    transform::inject_openai_stream_include_usage(&mut result);

    Ok(result)
}

fn apply_reasoning_options(
    result: &mut Value,
    body: &Value,
    model: &str,
    config: Option<&CodexChatReasoningConfig>,
) {
    let Some(config) = config else {
        if transform::supports_reasoning_effort(model) {
            if let Some(effort) = body.pointer("/reasoning/effort") {
                result["reasoning_effort"] = effort.clone();
            }
        }
        return;
    };

    let supports_effort = config.supports_effort.unwrap_or(false);
    let supports_thinking = config.supports_thinking.unwrap_or(false) || supports_effort;
    let Some(reasoning_enabled) = reasoning_requested(body) else {
        return;
    };

    if supports_thinking {
        match config
            .thinking_param
            .as_deref()
            .unwrap_or("thinking")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "thinking" => {
                result["thinking"] = json!({
                    "type": if reasoning_enabled { "enabled" } else { "disabled" }
                });
            }
            "enable_thinking" => {
                result["enable_thinking"] = json!(reasoning_enabled);
            }
            "reasoning_split" => {
                result["reasoning_split"] = json!(reasoning_enabled);
            }
            _ => {}
        }
    }

    // effort_param 在 early return 之前算出：reasoning.effort 形态的「显式关闭」分支要用到。
    let effort_param = config
        .effort_param
        .as_deref()
        .unwrap_or("reasoning_effort")
        .trim()
        .to_ascii_lowercase();

    if !reasoning_enabled {
        // OpenRouter 原生 reasoning.effort 支持显式 "none"（语义：彻底关闭推理）。
        // 上游显式发 effort=none/off/disabled（或 reasoning=null）时 reasoning_enabled 为 false，
        // 直接 return 会丢失关闭意图——OpenRouter 部分模型默认开思考，不带字段无法关闭，
        // 造成行为与成本偏差；故对该形态忠实转发 {"reasoning":{"effort":"none"}}。
        // 顶层 reasoning_effort 平台的枚举不含 none，仍走上方 thinking 关闭路径、不发 effort。
        // 注意：完全不带 reasoning 字段时 reasoning_requested 返回 None 已提前 return，
        // 不会走到这里，故只有上游「显式」表达关闭才透传 none。
        if effort_param == "reasoning.effort" {
            result["reasoning"] = json!({ "effort": "none" });
        }
        return;
    }

    if !supports_effort {
        return;
    }

    let Some(effort) = body.pointer("/reasoning/effort").and_then(|v| v.as_str()) else {
        return;
    };
    let Some(mapped) = map_reasoning_effort(
        effort,
        config.effort_value_mode.as_deref(),
        config.effort_levels.as_deref(),
    ) else {
        return;
    };

    match effort_param.as_str() {
        // OpenAI 风格顶层字段（DeepSeek 官方、OpenAI o-series 等）。
        "reasoning_effort" => {
            result["reasoning_effort"] = json!(mapped);
        }
        // OpenRouter 原生归一化对象：reasoning.effort 会被 OpenRouter 翻译成各底层模型
        // （OpenAI/Grok/Gemini/Anthropic）的正确推理参数，覆盖面比顶层 OpenAI 别名更全。
        // 本转换从空对象构造、不残留原始 reasoning 对象，故不会出现 reasoning 与
        // reasoning_effort 并存触发 400 的情况（参见 openclaw#24119）。
        "reasoning.effort" => {
            result["reasoning"] = json!({ "effort": mapped });
        }
        _ => {}
    }
}

fn reasoning_requested(body: &Value) -> Option<bool> {
    if let Some(effort) = body.pointer("/reasoning/effort").and_then(|v| v.as_str()) {
        return Some(!matches!(
            effort.trim().to_ascii_lowercase().as_str(),
            "none" | "off" | "disabled"
        ));
    }

    body.get("reasoning").map(|value| !value.is_null())
}

fn map_reasoning_effort<'a>(
    effort: &str,
    mode: Option<&str>,
    effort_levels: Option<&'a [String]>,
) -> Option<&'a str> {
    let effort = effort.trim().to_ascii_lowercase();
    if matches!(effort.as_str(), "none" | "off" | "disabled") {
        return None;
    }

    // ultra 是 Codex 扩展档位：已知枚举的专用模式（deepseek/openrouter/low_high）
    // 钳到自身最高合法档而非丢弃——丢弃会让"选最深思考"静默退化成"不带 effort"；
    // passthrough 面向枚举未知的通用上游，档位由用户/预设的 reasoningLevels 声明
    // 背书，与 max/xhigh 一样原值透传。
    match mode.unwrap_or("passthrough") {
        "deepseek" => match effort.as_str() {
            "max" | "xhigh" | "ultra" => Some("max"),
            _ => Some("high"),
        },
        "low_high" => match effort.as_str() {
            "minimal" | "low" => Some("low"),
            _ => Some("high"),
        },
        // OpenRouter effort 枚举为 xhigh|high|medium|low|minimal（无 max）。max 是
        // Codex / 部分模型的扩展档位，对 OpenRouter 非法，会触发
        // `400 reasoning_effort: Invalid option`（见 openclaw#77350）；钳到最高合法档
        // xhigh，其余合法值透传，未知值丢弃以免被上游拒绝。
        "openrouter" => match effort.as_str() {
            "max" | "xhigh" | "ultra" => Some("xhigh"),
            "high" => Some("high"),
            "medium" => Some("medium"),
            "low" => Some("low"),
            "minimal" => Some("minimal"),
            _ => None,
        },
        // OpenCode Zen：合法档位逐模型（表数据 = 供应商 modelCatalog 各条目的
        // reasoningLevels，镜像 models.dev——glm-5.2 仅 high|max、deepseek-v4-flash
        // 为 low|high|max、kimi-k3 仅 max），opencode 客户端也严格按模型声明发值，
        // 故不能用统一并集映射。无表（模型未收录目录、或为 toggle/budget 型未声明
        // effort）→ None，完全不发 reasoning_effort；有表 → 钳到「不小于请求的
        // 最近合法档」，请求超出最高档则取最高合法档；请求值本身无法识别 → None
        // （同其他模式的未知值丢弃策略）。
        "zen" => {
            let levels = effort_levels?;
            let requested = zen_effort_rank(&effort)?;
            levels
                .iter()
                .filter_map(|level| zen_effort_rank(level).map(|rank| (rank, level.as_str())))
                .filter(|(rank, _)| *rank >= requested)
                .min_by_key(|(rank, _)| *rank)
                .or_else(|| {
                    levels
                        .iter()
                        .filter_map(|level| {
                            zen_effort_rank(level).map(|rank| (rank, level.as_str()))
                        })
                        .max_by_key(|(rank, _)| *rank)
                })
                .map(|(_, level)| level)
        }
        _ => match effort.as_str() {
            "minimal" => Some("minimal"),
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "xhigh" => Some("xhigh"),
            "max" => Some("max"),
            "ultra" => Some("ultra"),
            _ => None,
        },
    }
}

/// Codex 规范档位序（minimal < low < medium < high < xhigh < max < ultra），供 zen
/// 逐模型钳制做大小比较；目录里的非法/扩展值（如 "none"）返回 None，查表时被滤掉。
fn zen_effort_rank(effort: &str) -> Option<u8> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" => Some(0),
        "low" => Some(1),
        "medium" => Some(2),
        "high" => Some(3),
        "xhigh" => Some(4),
        "max" => Some(5),
        "ultra" => Some(6),
        _ => None,
    }
}

/// MiniMax 严格要求 messages 中只能首条出现 `role=system`，
/// 否则返回 `invalid params, chat content has invalid message role: system (2013)`。
/// 把所有 system 消息合并到首位，避免中间 system（如 Codex 的 `developer` 指令）触发该约束；
/// 该重排对 OpenAI / DeepSeek 等宽松兼容层也是无损的。
fn collapse_system_messages_to_head(messages: Vec<Value>) -> Vec<Value> {
    let mut system_chunks: Vec<String> = Vec::new();
    let mut rest: Vec<Value> = Vec::with_capacity(messages.len());

    for msg in messages {
        if msg.get("role").and_then(|v| v.as_str()) == Some("system") {
            if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    system_chunks.push(text.to_string());
                }
                continue;
            }
        }
        rest.push(msg);
    }

    let mut out: Vec<Value> = Vec::with_capacity(rest.len() + 1);
    if !system_chunks.is_empty() {
        out.push(json!({
            "role": "system",
            "content": system_chunks.join("\n\n")
        }));
    }
    out.extend(rest);
    out
}

fn instruction_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| part.as_str())
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        other => other.as_str().unwrap_or_default().to_string(),
    }
}

fn append_responses_input_as_chat_messages(
    input: &Value,
    messages: &mut Vec<Value>,
    tool_context: &CodexToolContext,
) -> Result<(), ProxyError> {
    let mut pending_tool_calls = Vec::new();
    let mut pending_media = Vec::new();
    let mut pending_reasoning: Option<String> = None;
    let mut last_assistant_index: Option<usize> = None;

    match input {
        Value::String(text) => {
            messages.push(json!({
                "role": "user",
                "content": text
            }));
        }
        Value::Array(items) => {
            for item in items {
                append_responses_item_as_chat_message(
                    item,
                    messages,
                    &mut pending_tool_calls,
                    &mut pending_media,
                    &mut pending_reasoning,
                    &mut last_assistant_index,
                    tool_context,
                )?;
            }
        }
        Value::Object(_) => {
            append_responses_item_as_chat_message(
                input,
                messages,
                &mut pending_tool_calls,
                &mut pending_media,
                &mut pending_reasoning,
                &mut last_assistant_index,
                tool_context,
            )?;
        }
        _ => {}
    }

    // If a later assistant tool-call batch was accumulated after an earlier
    // media-bearing result, the synthetic user media belongs before that next
    // assistant turn.
    flush_pending_chat_tool_media(messages, &mut pending_media);
    flush_pending_tool_calls(
        messages,
        &mut pending_tool_calls,
        &mut pending_media,
        &mut pending_reasoning,
        &mut last_assistant_index,
    );
    // 整个 input 处理完毕后仍剩余的 pending reasoning 属于「真正的尾部」思考
    // （其后已没有任何可前向附挂的 message / function_call），回溯附挂到最后一条
    // assistant；目标已有 reasoning_content 时追加，以保留同一 turn 的 embedded
    // reasoning 与 trailing reasoning。
    attach_pending_reasoning_to_previous_assistant(
        messages,
        last_assistant_index,
        &mut pending_reasoning,
    );
    backfill_tool_call_reasoning_placeholders(messages);
    Ok(())
}

fn append_responses_item_as_chat_message(
    item: &Value,
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_media: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    last_assistant_index: &mut Option<usize>,
    tool_context: &CodexToolContext,
) -> Result<(), ProxyError> {
    let item_type = item.get("type").and_then(|v| v.as_str());
    match item_type {
        Some("function_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_function_call_to_chat_tool_call(
                item,
                tool_context,
            ));
        }
        Some("custom_tool_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_custom_tool_call_to_chat_tool_call(item));
        }
        Some("tool_search_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_tool_search_call_to_chat_tool_call(item));
        }
        Some("function_call_output") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_media,
                pending_reasoning,
                last_assistant_index,
            );
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let media_plan = item
                .get("output")
                .cloned()
                .and_then(plan_chat_tool_output_media);
            let output = if let Some(media_plan) = media_plan {
                queue_chat_tool_output_media(pending_media, call_id, media_plan.media_parts);
                media_plan.tool_content
            } else {
                // Cache-sensitive no-media fallback: keep these expressions
                // byte-for-byte equivalent to the pre-fix conversion.
                match item.get("output") {
                    Some(Value::String(s)) => canonicalize_json_string_if_parseable(s),
                    Some(v) => canonical_json_string(v),
                    None => String::new(),
                }
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output
            }));
        }
        Some("custom_tool_call_output") | Some("tool_search_output") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_media,
                pending_reasoning,
                last_assistant_index,
            );
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let mut transformed_item = item.clone();
            let replacement_block = json!({
                "type": "text",
                "text": TOOL_RESULT_MEDIA_MOVED_MARKER
            });
            let mut media_parts = Vec::new();
            let replaced = transformed_item
                .get_mut("output")
                .map(|output| {
                    strip_and_clamp_media_from_tool_value(
                        output,
                        &mut media_parts,
                        &replacement_block,
                        TOOL_RESULT_MEDIA_MOVED_MARKER,
                    )
                })
                .unwrap_or(0);
            let output = if replaced > 0 {
                queue_chat_tool_output_media(pending_media, call_id, media_parts);
                canonical_json_string(&transformed_item)
            } else {
                // Preserve the legacy whole-item representation exactly.
                canonical_json_string(item)
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output
            }));
        }
        Some("reasoning") => {
            // reasoning 一律先进入 pending_reasoning，前向附挂到其后的
            // message / function_call（后者经 flush_pending_tool_calls 消费）。
            // 此前这里在 pending_tool_calls 为空时直接回溯附挂到上一条 assistant，
            // 会把新一轮的思考错拼进旧消息，导致紧跟的纯文本 assistant 丢失
            // reasoning_content，思考型模型（kimi 等）多轮对话因此中途"断片"。
            // 真正的尾部剩余由 input 结束时的收尾逻辑、或回合边界消息（user 等）
            // 到达时回溯附挂，见 attach_pending_reasoning_to_previous_assistant。
            append_pending_reasoning(pending_reasoning, responses_reasoning_item_text(item));
        }
        Some("input_text" | "input_image" | "input_file" | "input_audio") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_media,
                pending_reasoning,
                last_assistant_index,
            );
            // `flush_pending_tool_calls` intentionally returns early when
            // there is no new assistant batch. A previous tool result may
            // still have media waiting, so flush it before this new message.
            flush_pending_chat_tool_media(messages, pending_media);
            let role = item
                .get("role")
                .and_then(|v| v.as_str())
                .map(responses_role_to_chat_role)
                .unwrap_or("user");
            let message = json!({
                "role": role,
                "content": responses_content_to_chat_content(role, &Value::Array(vec![item.clone()]))
            });
            if role == "assistant" {
                let mut message = message;
                attach_pending_reasoning_to_assistant(&mut message, pending_reasoning);
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
                return Ok(());
            } else {
                // 非 assistant 的回合边界消息（user 等）：pending reasoning 不再直接
                // 丢弃，优先回溯附挂到上一条 assistant；其已有 reasoning_content 时
                // 追加尾部 reasoning。reasoning 不允许跨 user 回合泄漏到之后的
                // assistant 消息；无上一条 assistant 可附挂时自然丢弃（等同原行为）。
                attach_pending_reasoning_to_previous_assistant(
                    messages,
                    *last_assistant_index,
                    pending_reasoning,
                );
            }
            update_last_assistant_index(messages, &message, last_assistant_index);
            messages.push(message);
        }
        Some("message") | None => {
            if item.get("role").is_some() || item.get("content").is_some() {
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
                flush_pending_chat_tool_media(messages, pending_media);
                let message = responses_message_item_to_chat_message(
                    item,
                    pending_reasoning,
                    messages,
                    *last_assistant_index,
                );
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
            } else if pending_media.is_empty() {
                // Preserve legacy no-media ordering: inert message-like items
                // used to close a pending tool-call batch.
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
            }
        }
        _ => {
            if item.get("role").is_some() || item.get("content").is_some() {
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
                flush_pending_chat_tool_media(messages, pending_media);
                let message = responses_message_item_to_chat_message(
                    item,
                    pending_reasoning,
                    messages,
                    *last_assistant_index,
                );
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
            } else if pending_media.is_empty() {
                // Preserve legacy no-media ordering without letting an inert
                // unknown item flush a media-bearing result batch.
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
            }
        }
    }

    Ok(())
}

fn flush_pending_tool_calls(
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_media: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    last_assistant_index: &mut Option<usize>,
) {
    if pending_tool_calls.is_empty() {
        return;
    }

    // Media from the preceding tool-result batch must be presented before a
    // new assistant tool-call turn. Consecutive outputs do not enter here
    // because `pending_tool_calls` is empty after the first output.
    flush_pending_chat_tool_media(messages, pending_media);
    let mut message = json!({
        "role": "assistant",
        "content": null,
        "tool_calls": std::mem::take(pending_tool_calls)
    });
    attach_pending_reasoning_to_assistant(&mut message, pending_reasoning);
    *last_assistant_index = Some(messages.len());
    messages.push(message);
}

fn responses_message_item_to_chat_message(
    item: &Value,
    pending_reasoning: &mut Option<String>,
    messages: &mut [Value],
    last_assistant_index: Option<usize>,
) -> Value {
    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    let chat_role = responses_role_to_chat_role(role);
    let content = item
        .get("content")
        .map(|value| responses_content_to_chat_content(chat_role, value))
        .unwrap_or(Value::Null);

    let mut message = json!({
        "role": chat_role,
        "content": content
    });

    if chat_role == "assistant" {
        append_pending_reasoning(pending_reasoning, responses_message_reasoning_text(item));
        attach_pending_reasoning_to_assistant(&mut message, pending_reasoning);
    } else {
        // 非 assistant 的回合边界消息（user 等）：pending reasoning 不再直接丢弃，
        // 回溯附挂到上一条 assistant；其已有 reasoning_content 时追加尾部
        // reasoning，同时防止 reasoning 跨 user 回合泄漏到之后的 assistant 消息。
        attach_pending_reasoning_to_previous_assistant(
            messages,
            last_assistant_index,
            pending_reasoning,
        );
    }

    message
}

fn responses_role_to_chat_role(role: &str) -> &'static str {
    match role {
        "system" | "developer" => "system",
        "assistant" => "assistant",
        "tool" => "tool",
        "user" | "latest_reminder" => "user",
        _ => "user",
    }
}

fn update_last_assistant_index(
    messages: &[Value],
    message: &Value,
    last_assistant_index: &mut Option<usize>,
) {
    match message.get("role").and_then(|v| v.as_str()) {
        Some("assistant") => {
            *last_assistant_index = Some(messages.len());
        }
        Some("tool") => {}
        _ => {
            *last_assistant_index = None;
        }
    }
}

fn append_pending_reasoning(pending_reasoning: &mut Option<String>, reasoning: Option<String>) {
    let Some(reasoning) = reasoning else {
        return;
    };
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }

    match pending_reasoning {
        Some(existing) if !existing.is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(reasoning);
        }
        _ => {
            *pending_reasoning = Some(reasoning.to_string());
        }
    }
}

fn append_unique_pending_reasoning(
    pending_reasoning: &mut Option<String>,
    reasoning: Option<String>,
) {
    let Some(reasoning) = reasoning else {
        return;
    };
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }

    match pending_reasoning {
        Some(existing) if existing.contains(reasoning) => {}
        Some(existing) if !existing.is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(reasoning);
        }
        _ => {
            *pending_reasoning = Some(reasoning.to_string());
        }
    }
}

fn attach_pending_reasoning_to_assistant(
    message: &mut Value,
    pending_reasoning: &mut Option<String>,
) {
    let Some(reasoning) = pending_reasoning.take() else {
        return;
    };
    if reasoning.trim().is_empty() {
        return;
    }

    if let Some(obj) = message.as_object_mut() {
        append_reasoning_content(obj, &reasoning);
    }
}

/// 在所有 input 处理完毕后，对仍缺 `reasoning_content` 的 assistant tool-call 消息补占位。
/// 必须作为管线末端的最终兜底执行：真实 reasoning 可能以尾随 `reasoning` item 的形式经
/// `attach_pending_reasoning_to_previous_assistant` 回填，过早注入占位会被
/// `append_reasoning_content` 追加而污染真实思考。
fn backfill_tool_call_reasoning_placeholders(messages: &mut [Value]) {
    for message in messages.iter_mut() {
        let is_assistant_tool_call = message.get("role").and_then(|value| value.as_str())
            == Some("assistant")
            && message
                .get("tool_calls")
                .and_then(|value| value.as_array())
                .is_some_and(|calls| !calls.is_empty());
        if is_assistant_tool_call {
            ensure_tool_call_reasoning_content(message);
        }
    }
}

/// kimi/Moonshot、DeepSeek 等 thinking 模型要求每条带 `tool_calls` 的 assistant
/// 消息都必须携带非空 `reasoning_content`。跨轮历史恢复 miss（如代理重启丢失内存缓存、
/// call_id 歧义无法恢复、上游某轮未产出思考）时，这里补一个占位，避免上游返回
/// `reasoning_content is missing in assistant tool call message`。
/// 与 `transform::anthropic_to_openai_with_reasoning_content` 的占位行为保持对称。
fn ensure_tool_call_reasoning_content(message: &mut Value) {
    let Some(obj) = message.as_object_mut() else {
        return;
    };
    let has_reasoning = obj
        .get("reasoning_content")
        .and_then(|value| value.as_str())
        .is_some_and(|text| !text.trim().is_empty());
    if !has_reasoning {
        obj.insert(
            "reasoning_content".to_string(),
            Value::String("tool call".to_string()),
        );
    }
}

/// 将仍未消费的 pending reasoning 回溯附挂到上一条 assistant 消息。
///
/// 只允许两种「真正的尾部」场景调用：
/// 1. 整个 input 处理完毕后 pending_reasoning 仍有剩余——其后已没有任何可
///    前向附挂的 message / function_call；
/// 2. user 等回合边界消息到达时 pending_reasoning 非空——reasoning 不允许
///    跨 user 回合泄漏到之后的 assistant 消息，也不能直接丢弃可归属的思考。
///
/// 这里已经处于尾部/边界收尾点，不是普通 reasoning 的前向归属路径；
/// 若目标已有 reasoning_content，追加尾部 reasoning 以保留同一 assistant turn
/// 中同时出现的 embedded reasoning 与尾随 reasoning。无论是否附挂成功，
/// pending 都会被消费（拿走），绝不留到下一条 assistant。
fn attach_pending_reasoning_to_previous_assistant(
    messages: &mut [Value],
    last_assistant_index: Option<usize>,
    pending_reasoning: &mut Option<String>,
) {
    let Some(reasoning) = pending_reasoning.take() else {
        return;
    };
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }
    let Some(message) = last_assistant_index.and_then(|index| messages.get_mut(index)) else {
        return;
    };
    if message.get("role").and_then(|v| v.as_str()) != Some("assistant") {
        return;
    }
    if let Some(obj) = message.as_object_mut() {
        append_reasoning_content(obj, reasoning);
    }
}

fn responses_message_reasoning_text(item: &Value) -> Option<String> {
    responses_item_reasoning_text(item)
}

fn responses_item_reasoning_text(item: &Value) -> Option<String> {
    extract_reasoning_field_text(item)
}

fn responses_reasoning_item_text(item: &Value) -> Option<String> {
    extract_reasoning_summary_text(item)
}

fn responses_content_to_chat_content(_role: &str, content: &Value) -> Value {
    if content.is_null() || content.is_string() {
        return content.clone();
    }

    let Some(parts) = content.as_array() else {
        return content.clone();
    };

    let mut chat_parts: Vec<Value> = Vec::new();
    let mut has_non_text_part = false;

    for part in parts {
        let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match part_type {
            "input_text" | "output_text" | "text" => {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        chat_parts.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                }
            }
            "refusal" => {
                if let Some(text) = part.get("refusal").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        chat_parts.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                }
            }
            "input_image" => {
                if let Some(image_url) = part.get("image_url") {
                    let image_url = if image_url.is_object() {
                        image_url.clone()
                    } else {
                        json!({ "url": image_url.as_str().unwrap_or_default() })
                    };
                    chat_parts.push(json!({
                        "type": "image_url",
                        "image_url": image_url
                    }));
                    has_non_text_part = true;
                }
            }
            "input_file" => {
                if let Some(file) = responses_input_file_to_chat_file(part) {
                    chat_parts.push(json!({
                        "type": "file",
                        "file": file
                    }));
                    has_non_text_part = true;
                }
            }
            "input_audio" => {
                if let Some(input_audio) = part.get("input_audio") {
                    chat_parts.push(json!({
                        "type": "input_audio",
                        "input_audio": input_audio.clone()
                    }));
                    has_non_text_part = true;
                }
            }
            _ => {}
        }
    }

    if !has_non_text_part {
        return Value::String(
            chat_parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    Value::Array(chat_parts)
}

fn responses_input_file_to_chat_file(part: &Value) -> Option<Value> {
    chat_file_from_input_file(part)
}

fn collect_tool_search_output_tools(value: &Value, context: &mut CodexToolContext) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_tool_search_output_tools(item, context);
            }
        }
        Value::Object(obj) => {
            if obj.get("type").and_then(|v| v.as_str()) == Some("tool_search_output") {
                if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
                    for tool in tools {
                        context.add_response_tool(tool);
                    }
                }
            }
            for value in obj.values() {
                collect_tool_search_output_tools(value, context);
            }
        }
        _ => {}
    }
}

pub(crate) fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
    let full_name = format!("{namespace}__{name}");
    if full_name.len() <= CHAT_TOOL_NAME_MAX_LEN {
        return full_name;
    }

    let hash = short_sha256_hex(full_name.as_bytes());
    let suffix = format!("__{hash}");
    let prefix_len = CHAT_TOOL_NAME_MAX_LEN.saturating_sub(suffix.len());
    let mut prefix = String::new();
    for ch in full_name.chars() {
        if prefix.len() + ch.len_utf8() > prefix_len {
            break;
        }
        prefix.push(ch);
    }
    format!("{prefix}{suffix}")
}

fn responses_tool_name(tool: &Value) -> Option<String> {
    tool.get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| tool.get("name"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn responses_custom_tool_description(tool: &Value) -> String {
    let mut description = String::new();
    description.push_str(CUSTOM_TOOL_PRESERVED_METADATA_HEADING);
    description.push_str("\n```json\n");
    description.push_str(&serialize_tool_definition_for_description(tool));
    description.push_str("\n```");
    description
}

fn serialize_tool_definition_for_description(tool: &Value) -> String {
    // Keep the embedded definition compact to reduce tool-description token
    // overhead for chat-only upstreams, while remaining stable across map
    // storage order.
    canonical_json_string(tool)
}

/// Normalize a function's `parameters` JSON Schema so `type` is always `"object"`.
///
/// Some Responses tools carry `parameters: null` or `parameters: {"type": null}`,
/// but OpenAI Chat Completions strictly requires `{"type": "object", "properties": {...}}`.
fn normalize_function_parameters(params: Option<&Value>) -> Value {
    let mut params = match params {
        Some(Value::Object(obj)) => Value::Object(obj.clone()),
        _ => json!({"type": "object", "properties": {}}),
    };
    if let Some(obj) = params.as_object_mut() {
        match obj.get("type").and_then(|v| v.as_str()) {
            Some("object") => {}
            _ => {
                obj.insert("type".to_string(), json!("object"));
            }
        }
    }
    params
}

fn responses_function_tool_to_chat_tool(tool: &Value, chat_name: &str) -> Option<Value> {
    if tool.get("type").and_then(|v| v.as_str()) != Some("function") {
        return None;
    }

    if let Some(function) = tool.get("function") {
        let mut chat_tool = json!({
            "type": "function",
            "function": function.clone()
        });
        if let Some(obj) = chat_tool
            .get_mut("function")
            .and_then(|value| value.as_object_mut())
        {
            // Ensure parameters.type is "object" for strict OpenAI-compatible providers
            let parameters = normalize_function_parameters(obj.get("parameters"));
            obj.insert("parameters".to_string(), parameters);

            obj.insert("name".to_string(), json!(chat_name));
            if let Some(strict) = tool.get("strict").cloned() {
                obj.entry("strict".to_string()).or_insert(strict);
            }
        }
        return Some(chat_tool);
    }

    let mut function = json!({
        "name": chat_name,
        "description": tool.get("description").cloned().unwrap_or(Value::Null),
        "parameters": normalize_function_parameters(tool.get("parameters"))
    });
    if let Some(strict) = tool.get("strict") {
        function["strict"] = strict.clone();
    }

    Some(json!({
        "type": "function",
        "function": function
    }))
}

fn responses_function_call_to_chat_tool_call(
    item: &Value,
    tool_context: &CodexToolContext,
) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let namespace = item.get("namespace").and_then(|v| v.as_str());
    let chat_name = tool_context.chat_name_for_response_function(name, namespace);
    let arguments = canonicalize_tool_arguments(item.get("arguments"));

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": chat_name,
            "arguments": arguments
        }
    })
}

fn responses_custom_tool_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let input = item.get("input").cloned().unwrap_or_else(|| json!(""));

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": canonical_json_string(&json!({ CUSTOM_TOOL_INPUT_FIELD: input }))
        }
    })
}

fn responses_tool_search_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let arguments = item
        .get("arguments")
        .map(canonical_json_string)
        .unwrap_or_else(|| "{}".to_string());

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": TOOL_SEARCH_PROXY_NAME,
            "arguments": arguments
        }
    })
}

fn responses_tool_choice_to_chat(tool_choice: &Value, tool_context: &CodexToolContext) -> Value {
    match tool_choice {
        Value::Object(obj) if obj.get("type").and_then(|v| v.as_str()) == Some("function") => {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let namespace = obj.get("namespace").and_then(|v| v.as_str());
            let chat_name = tool_context.chat_name_for_response_function(name, namespace);
            json!({
                "type": "function",
                "function": {
                    "name": chat_name
                }
            })
        }
        Value::Object(obj) if obj.get("type").and_then(|v| v.as_str()) == Some("tool_search") => {
            json!({
                "type": "function",
                "function": {
                    "name": TOOL_SEARCH_PROXY_NAME
                }
            })
        }
        Value::Object(obj) if obj.get("type").and_then(|v| v.as_str()) == Some("custom") => {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
            json!({
                "type": "function",
                "function": {
                    "name": name
                }
            })
        }
        _ => tool_choice.clone(),
    }
}

/// Convert a non-streaming Chat Completions response into a Responses response.
#[allow(dead_code)]
pub fn chat_completion_to_response(body: Value) -> Result<Value, ProxyError> {
    chat_completion_to_response_with_context(body, &CodexToolContext::default())
}

/// Convert a non-streaming Chat Completions response into a Responses response,
/// restoring Codex-specific tool names using the original Responses request.
pub(crate) fn chat_completion_to_response_with_context(
    body: Value,
    tool_context: &CodexToolContext,
) -> Result<Value, ProxyError> {
    let choices = body
        .get("choices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ProxyError::TransformError("No choices in chat response".to_string()))?;
    let choice = choices
        .first()
        .ok_or_else(|| ProxyError::TransformError("Empty choices in chat response".to_string()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| ProxyError::TransformError("No message in chat choice".to_string()))?;

    let response_id = response_id_from_chat_id(body.get("id").and_then(|v| v.as_str()));
    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
    // Codex/Grok require `created_at` on Response; never omit. Prefer upstream
    // Chat `created`, else wall-clock seconds (0 can confuse some deserializers).
    let created_at = body
        .get("created")
        .and_then(|v| v.as_u64())
        .filter(|v| *v > 0)
        .unwrap_or_else(unix_now_secs);
    let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());

    let reasoning = chat_reasoning_text(message);
    let mut output = Vec::new();
    if let Some(reasoning_item) =
        chat_reasoning_to_response_output_item(reasoning.as_deref(), &response_id)
    {
        output.push(reasoning_item);
    }
    if let Some(message_item) = chat_message_to_response_output_item(message, &response_id) {
        output.push(message_item);
    }
    let tool_calls =
        chat_tool_calls_to_response_output_items(message, reasoning.as_deref(), tool_context);

    // 丢弃过工具调用、且最终一个工具调用都没剩下时，Codex 会收到一个
    // "status=completed 但 output 里没有任何工具调用" 的回合，agent loop 必然静默
    // 收尾（#4341）。此时如实报错，而不是谎报成功。只要还剩下任何一个合法工具
    // 调用，Codex 本来就会继续，判据不成立，行为保持不变。
    //
    // 🔴 与流式分支一致，只对本应 `completed` 的回合生效：`finish_reason=length`
    // 是截断，工具调用缺 name 是截断的后果而非上游发了畸形数据，报成
    // tool_call_dropped 会给出错误的归因。
    if response_status_from_finish_reason(finish_reason) == "completed"
        && tool_calls.dropped > 0
        && tool_calls.items.is_empty()
    {
        return Err(ProxyError::TransformError(format!(
            "Upstream returned {} tool call(s) without a function name, \
             leaving no usable tool call in this turn",
            tool_calls.dropped
        )));
    }
    output.extend(tool_calls.items);

    let mut response = json!({
        "id": response_id,
        "object": "response",
        "created_at": created_at,
        "status": response_status_from_finish_reason(finish_reason),
        "model": model,
        "output": output,
        "usage": chat_usage_to_responses_usage(body.get("usage"))
    });

    if finish_reason == Some("length") {
        response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
    }

    Ok(response)
}

fn chat_reasoning_to_response_output_item(
    reasoning: Option<&str>,
    response_id: &str,
) -> Option<Value> {
    let reasoning = reasoning?;
    if reasoning.is_empty() {
        return None;
    }

    Some(json!({
        "id": format!("rs_{response_id}"),
        "type": "reasoning",
        "summary": [{
            "type": "summary_text",
            "text": reasoning
        }]
    }))
}

fn chat_reasoning_text(message: &Value) -> Option<String> {
    if let Some(reasoning) = extract_reasoning_field_text(message) {
        return Some(reasoning);
    }

    if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
        if let Some((reasoning, _answer)) = split_leading_think_block(content) {
            if !reasoning.is_empty() {
                return Some(reasoning);
            }
        }
    }

    None
}

fn chat_message_to_response_output_item(message: &Value, response_id: &str) -> Option<Value> {
    let mut content = Vec::new();

    if let Some(text) = message.get("content").and_then(|v| v.as_str()) {
        let text = split_leading_think_block(text)
            .map(|(_reasoning, answer)| answer)
            .unwrap_or_else(|| text.to_string());
        if !text.is_empty() {
            content.push(json!({
                "type": "output_text",
                "text": text,
                "annotations": []
            }));
        }
    } else if let Some(parts) = message.get("content").and_then(|v| v.as_array()) {
        for part in parts {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match part_type {
                "text" | "output_text" => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(json!({
                                "type": "output_text",
                                "text": text,
                                "annotations": []
                            }));
                        }
                    }
                }
                "refusal" => {
                    if let Some(text) = part.get("refusal").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(json!({
                                "type": "refusal",
                                "refusal": text
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(refusal) = message.get("refusal").and_then(|v| v.as_str()) {
        if !refusal.is_empty() {
            content.push(json!({
                "type": "refusal",
                "refusal": refusal
            }));
        }
    }

    if content.is_empty() {
        return None;
    }

    Some(json!({
        "id": format!("{response_id}_msg"),
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": content
    }))
}

/// 非流式工具调用转换结果。`dropped` 记录因缺少合法函数名而被丢弃的条数，
/// 供调用方判断本回合是否已经不可能让 Codex 继续（见 #4341）。
struct ChatToolCallItems {
    items: Vec<Value>,
    dropped: usize,
}

fn chat_tool_calls_to_response_output_items(
    message: &Value,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> ChatToolCallItems {
    let mut output = Vec::new();
    let mut dropped = 0usize;

    if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for (index, tool_call) in tool_calls.iter().enumerate() {
            // Skip tool calls with missing function names (defensive: some models
            // may generate tool calls without providing a valid name)
            let function = tool_call.get("function").unwrap_or(&Value::Null);
            let name = function.get("name").and_then(|v| v.as_str()).unwrap_or("");
            // 纯空白名同样对应不到任何已发布工具，与空名同等对待。
            if name.trim().is_empty() {
                dropped += 1;
                // 只记结构信息，不记 arguments 内容（可能包含用户代码）。
                let call_id_empty = tool_call
                    .get("id")
                    .and_then(|v| v.as_str())
                    .is_none_or(str::is_empty);
                let args_bytes = function
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .map(str::len)
                    .unwrap_or(0);
                eprintln!(
                    "[Codex] dropped tool call: index={index} call_id_empty={call_id_empty} \
                     args_bytes={args_bytes} tools_total={}",
                    tool_calls.len()
                );
                continue;
            }
            output.push(chat_tool_call_to_response_item(
                tool_call,
                index,
                reasoning,
                tool_context,
            ));
        }
    } else if let Some(function_call) = message.get("function_call") {
        match chat_legacy_function_call_to_response_item(function_call, reasoning, tool_context) {
            Some(item) => output.push(item),
            None => dropped += 1,
        }
    }

    ChatToolCallItems {
        items: output,
        dropped,
    }
}

fn chat_tool_call_to_response_item(
    tool_call: &Value,
    index: usize,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Value {
    let call_id = tool_call
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("call_{index}"));
    let function = tool_call.get("function").unwrap_or(&Value::Null);
    let name = function.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = canonicalize_tool_arguments(function.get("arguments"));

    let item_id = response_tool_call_item_id_from_chat_name(&call_id, name, tool_context);
    response_tool_call_item_from_chat_name(
        &item_id,
        "completed",
        &call_id,
        name,
        &arguments,
        reasoning,
        tool_context,
    )
}

fn chat_legacy_function_call_to_response_item(
    function_call: &Value,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Option<Value> {
    let call_id = function_call
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("call_0");
    let name = function_call
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Skip legacy function calls with missing names (defensive: some models
    // may generate function_call without providing a valid name)。
    // 纯空白名同样对应不到任何已发布工具，与空名同等对待。
    if name.trim().is_empty() {
        // 只记结构信息，不记 arguments 内容（可能包含用户代码）。
        let args_bytes = function_call
            .get("arguments")
            .and_then(|v| v.as_str())
            .map(str::len)
            .unwrap_or(0);
        eprintln!(
            "[Codex] dropped legacy function_call: call_id={call_id} args_bytes={args_bytes}"
        );
        return None;
    }

    let arguments = canonicalize_tool_arguments(function_call.get("arguments"));

    let item_id = response_tool_call_item_id_from_chat_name(call_id, name, tool_context);
    Some(response_tool_call_item_from_chat_name(
        &item_id,
        "completed",
        call_id,
        name,
        &arguments,
        reasoning,
        tool_context,
    ))
}

pub(crate) fn response_tool_call_item_id_from_chat_name(
    call_id: &str,
    chat_name: &str,
    tool_context: &CodexToolContext,
) -> String {
    if tool_context.is_custom_tool_chat_name(chat_name) {
        format!("ctc_{call_id}")
    } else {
        format!("fc_{call_id}")
    }
}

pub(crate) fn response_tool_call_item_from_chat_name(
    item_id: &str,
    status: &str,
    call_id: &str,
    chat_name: &str,
    arguments: &str,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Value {
    match tool_context.lookup_chat_name(chat_name) {
        Some(spec) if spec.kind == CodexToolKind::ToolSearch => {
            response_tool_search_call_item(call_id, status, arguments, reasoning)
        }
        Some(spec) if spec.kind == CodexToolKind::Custom => response_custom_tool_call_item(
            item_id, status, call_id, &spec.name, arguments, reasoning,
        ),
        Some(spec) => response_function_call_item_with_namespace(
            item_id,
            status,
            call_id,
            &spec.name,
            spec.namespace.as_deref(),
            arguments,
            reasoning,
        ),
        None => {
            response_function_call_item(item_id, status, call_id, chat_name, arguments, reasoning)
        }
    }
}

fn response_tool_search_call_item(
    call_id: &str,
    status: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let parsed_arguments = parse_tool_arguments_object(arguments);
    let mut item = json!({
        "type": "tool_search_call",
        "call_id": call_id,
        "status": status,
        "execution": "client",
        "arguments": parsed_arguments
    });
    super::codex_chat_common::attach_optional_reasoning_content_field(&mut item, reasoning);
    item
}

fn response_custom_tool_call_item(
    item_id: &str,
    status: &str,
    call_id: &str,
    name: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let input = custom_tool_input_from_chat_arguments(arguments);
    let mut item = json!({
        "id": item_id,
        "type": "custom_tool_call",
        "status": status,
        "call_id": call_id,
        "name": name,
        "input": input
    });
    super::codex_chat_common::attach_optional_reasoning_content_field(&mut item, reasoning);
    item
}

fn parse_tool_arguments_object(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str::<Value>(arguments)
        .ok()
        .filter(|value| value.is_object())
        .unwrap_or_else(|| json!({ "query": arguments }))
}

pub(crate) fn custom_tool_input_from_chat_arguments(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(obj)) => obj
            .get(CUSTOM_TOOL_INPUT_FIELD)
            .and_then(|value| value.as_str())
            .unwrap_or(arguments)
            .to_string(),
        _ => arguments.to_string(),
    }
}

pub(crate) fn chat_usage_to_responses_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage.filter(|value| value.is_object() && !value.is_null()) else {
        return json!({
            "input_tokens": 0,
            "input_tokens_details": { "cached_tokens": 0 },
            "output_tokens": 0,
            "total_tokens": 0,
            "output_tokens_details": { "reasoning_tokens": 0 }
        });
    };

    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);

    let mut result = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_tokens
    });

    let direct_cache_read = usage.get("cache_read_input_tokens").and_then(Value::as_u64);
    let cached = direct_cache_read
        .or_else(|| {
            usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(Value::as_u64)
        })
        .or_else(|| {
            usage
                .pointer("/input_tokens_details/cached_tokens")
                .and_then(Value::as_u64)
        })
        // DeepSeek Chat 的文档化缓存命中字段（与 usage/parser.rs 的处理对应），末位兜底。
        // 官方端点目前把同值镜像进未文档化的 prompt_tokens_details.cached_tokens（上面的
        // 标准字段已命中），故仅当上游只发文档字段、不发镜像时此兜底生效（如部分中转），
        // 并防御未文档化镜像将来消失；上游发任一标准字段时行为零变化。
        .or_else(|| usage.get("prompt_cache_hit_tokens").and_then(Value::as_u64))
        .unwrap_or(0);
    let cache_write = usage
        .pointer("/prompt_tokens_details/cache_write_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
        .and_then(|v| v.as_u64())
        .or_else(|| {
            usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0);
    if cached > 0 || cache_write > 0 {
        result["input_tokens_details"] = json!({
            "cached_tokens": cached,
            "cache_write_tokens": cache_write
        });
    } else {
        result["input_tokens_details"] = json!({ "cached_tokens": 0 });
    }

    if let Some(details) = usage
        .get("completion_tokens_details")
        .filter(|v| v.is_object())
    {
        let mut details = details.clone();
        if details.get("reasoning_tokens").is_none() {
            details["reasoning_tokens"] = json!(0);
        }
        result["output_tokens_details"] = details;
    } else {
        result["output_tokens_details"] = json!({ "reasoning_tokens": 0 });
    }

    if let Some(cache_read) = direct_cache_read {
        result["cache_read_input_tokens"] = json!(cache_read);
    }
    if cache_write > 0 {
        result["cache_creation_input_tokens"] = json!(cache_write);
    }

    result
}

pub(crate) fn response_id_from_chat_id(id: Option<&str>) -> String {
    let id = id.unwrap_or("ccswitch");
    if id.starts_with("resp_") {
        id.to_string()
    } else {
        format!("resp_{id}")
    }
}

/// Unix seconds for Responses `created_at` when upstream Chat omits `created`.
pub(crate) fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn response_status_from_finish_reason(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("length") => "incomplete",
        _ => "completed",
    }
}

/// 把 Chat Completions 上游的错误体规整成 OpenAI Responses API 风格的错误对象。
///
/// 兼容三类输入：
/// 1. 标准 OpenAI 形式 `{"error": {"message": "...", "type": "...", "code": ...}}`
/// 2. MiniMax 等非标形式（如 `{"base_resp": {"status_code": 2013, "status_msg": "..."}}`）
/// 3. 顶层只有 `message` / `detail` / 裸字符串的最小错误
///
/// 输出统一为 `{"error": {"message", "type", "code", "param"}}`，与 OpenAI Responses
/// API 错误响应一致；Codex 客户端的错误处理只识别这个形状。
pub fn chat_error_to_response_error(body: Option<&Value>) -> Value {
    let Some(value) = body else {
        return json!({
            "error": {
                "message": "Upstream returned an empty error response",
                "type": "upstream_error",
                "code": serde_json::Value::Null,
                "param": serde_json::Value::Null,
            }
        });
    };

    if let Some(text) = value.as_str() {
        return json!({
            "error": {
                "message": text,
                "type": "upstream_error",
                "code": serde_json::Value::Null,
                "param": serde_json::Value::Null,
            }
        });
    }

    let source = value.get("error").unwrap_or(value);

    let message = source
        .get("message")
        .or_else(|| source.get("detail"))
        .or_else(|| source.get("status_msg"))
        .or_else(|| source.pointer("/base_resp/status_msg"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| source.as_str().map(ToString::to_string))
        .unwrap_or_else(|| {
            // 没法从字段提取出文本，就把整个 JSON 序列化回去，方便用户排查。
            serde_json::to_string(source).unwrap_or_else(|_| "Upstream error".to_string())
        });

    let error_type = source
        .get("type")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "upstream_error".to_string());

    let code = source
        .get("code")
        .cloned()
        .or_else(|| source.pointer("/base_resp/status_code").cloned())
        .unwrap_or(serde_json::Value::Null);

    let param = source
        .get("param")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    json!({
        "error": {
            "message": message,
            "type": error_type,
            "code": code,
            "param": param,
        }
    })
}
