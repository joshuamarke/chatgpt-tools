//! Token usage extraction from upstream API responses.
//!
//! Used for request-log stats and (with first-byte priming) for
//! judging whether a 2xx response is a real success under failover.

use serde_json::Value;

fn openai_cache_read_tokens(usage: &Value) -> u32 {
    usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32
}

fn openai_cache_write_tokens(usage: &Value) -> u32 {
    usage
        .get("cache_creation_input_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
        .or_else(|| usage.pointer("/prompt_tokens_details/cache_write_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32
}

fn response_id(body: &Value, field: &str) -> Option<String> {
    body.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Token usage extracted from an API response body / SSE events.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    pub model: Option<String>,
}

impl TokenUsage {
    /// Any billable dimension is non-zero (incl. cache-only hits).
    pub fn has_billable_tokens(&self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_read_tokens > 0
            || self.cache_creation_tokens > 0
    }

    /// Auto-detect OpenAI chat, Codex Responses, Claude, or Gemini JSON bodies.
    pub fn from_json_body(body: &Value) -> Option<Self> {
        if body.get("usageMetadata").is_some() {
            return Self::from_gemini_response(body);
        }
        let usage = body.get("usage")?;
        if usage.get("prompt_tokens").is_some() {
            return Self::from_openai_response(body);
        }
        if usage.get("input_tokens").is_some() {
            // Claude or Codex Responses share input_tokens/output_tokens names.
            return Self::from_codex_or_claude_response(body);
        }
        None
    }

    /// Parse SSE `data:` JSON lines from a stream buffer / first chunk(s).
    pub fn from_sse_text(text: &str) -> Option<Self> {
        let mut events: Vec<Value> = Vec::new();
        for block in text.split("\n\n") {
            for line in block.lines() {
                let data = line
                    .strip_prefix("data:")
                    .or_else(|| line.strip_prefix("data: "))
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && *s != "[DONE]");
                if let Some(data) = data {
                    if let Ok(v) = serde_json::from_str::<Value>(data) {
                        events.push(v);
                    }
                }
            }
        }
        if events.is_empty() {
            // Maybe a raw JSON document (gateway ignored stream:true).
            if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
                return Self::from_json_body(&v);
            }
            return None;
        }
        Self::from_stream_events(&events)
    }

    pub fn from_stream_events(events: &[Value]) -> Option<Self> {
        // Codex Responses: response.completed
        for event in events {
            if event.get("type").and_then(Value::as_str) == Some("response.completed") {
                if let Some(response) = event.get("response") {
                    if let Some(u) = Self::from_json_body(response) {
                        return Some(u);
                    }
                }
            }
        }
        // Claude SSE
        if let Some(u) = Self::from_claude_stream_events(events) {
            return Some(u);
        }
        // OpenAI chat completions: last chunk with usage
        Self::from_openai_stream_events(events)
    }

    fn from_openai_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
        let prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_u64)? as u32;
        let completion_tokens = usage.get("completion_tokens").and_then(Value::as_u64)? as u32;
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string);
        Some(Self {
            input_tokens: prompt_tokens,
            output_tokens: completion_tokens,
            cache_read_tokens: openai_cache_read_tokens(usage),
            cache_creation_tokens: openai_cache_write_tokens(usage),
            model,
        })
    }

    fn from_codex_or_claude_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
        let input_tokens = usage.get("input_tokens").and_then(Value::as_u64)? as u32;
        let output_tokens = usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string);
        let cache_read = usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32)
            .unwrap_or_else(|| openai_cache_read_tokens(usage));
        let cache_write = usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32)
            .unwrap_or_else(|| openai_cache_write_tokens(usage));
        Some(Self {
            input_tokens,
            output_tokens,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_write,
            model,
        })
    }

    fn from_openai_stream_events(events: &[Value]) -> Option<Self> {
        for event in events.iter().rev() {
            if let Some(usage) = event.get("usage") {
                if !usage.is_null() {
                    return Self::from_openai_response(event).or_else(|| {
                        // usage may sit on the chunk without model; build directly
                        let prompt = usage.get("prompt_tokens").and_then(Value::as_u64)? as u32;
                        let completion =
                            usage.get("completion_tokens").and_then(Value::as_u64)? as u32;
                        Some(Self {
                            input_tokens: prompt,
                            output_tokens: completion,
                            cache_read_tokens: openai_cache_read_tokens(usage),
                            cache_creation_tokens: openai_cache_write_tokens(usage),
                            model: events
                                .iter()
                                .find_map(|c| c.get("model").and_then(Value::as_str))
                                .map(str::to_string),
                        })
                    });
                }
            }
        }
        None
    }

    fn from_claude_stream_events(events: &[Value]) -> Option<Self> {
        let mut usage = Self::default();
        let mut model: Option<String> = None;
        let mut saw = false;
        for event in events {
            let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
            match event_type {
                "message_start" => {
                    if let Some(message) = event.get("message") {
                        if model.is_none() {
                            model = message
                                .get("model")
                                .and_then(Value::as_str)
                                .map(str::to_string);
                        }
                        if let Some(msg_usage) = message.get("usage") {
                            saw = true;
                            usage.input_tokens = msg_usage
                                .get("input_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as u32;
                            usage.cache_read_tokens = msg_usage
                                .get("cache_read_input_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as u32;
                            usage.cache_creation_tokens = msg_usage
                                .get("cache_creation_input_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as u32;
                        }
                    }
                }
                "message_delta" => {
                    if let Some(delta_usage) = event.get("usage") {
                        saw = true;
                        if let Some(output) =
                            delta_usage.get("output_tokens").and_then(Value::as_u64)
                        {
                            usage.output_tokens = output as u32;
                        }
                        if let Some(input) =
                            delta_usage.get("input_tokens").and_then(Value::as_u64)
                        {
                            if input > 0 {
                                usage.input_tokens = input as u32;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if saw && usage.has_billable_tokens() {
            usage.model = model;
            Some(usage)
        } else {
            None
        }
    }

    fn from_gemini_response(body: &Value) -> Option<Self> {
        let usage = body.get("usageMetadata")?;
        let prompt_tokens = usage.get("promptTokenCount").and_then(Value::as_u64)? as u32;
        let total_tokens = usage
            .get("totalTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(prompt_tokens as u64) as u32;
        Some(Self {
            input_tokens: prompt_tokens,
            output_tokens: total_tokens.saturating_sub(prompt_tokens),
            cache_read_tokens: usage
                .get("cachedContentTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            cache_creation_tokens: 0,
            model: body
                .get("modelVersion")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}

/// Detect 2xx JSON bodies that are semantic failures (must fail over).
pub fn semantic_error_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    // Anthropic error envelope
    if value.get("type").and_then(Value::as_str) == Some("error") || value.get("error").is_some() {
        // OpenAI-style error: {"error":{"message":"..."}}
        if let Some(error) = value.get("error") {
            if error.is_object() || error.is_string() {
                let error_type = error
                    .get("type")
                    .and_then(Value::as_str)
                    .or_else(|| error.get("code").and_then(Value::as_str))
                    .unwrap_or("error");
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| error.as_str())
                    .unwrap_or_else(|| "upstream error");
                // Anthropic type:error OR OpenAI error object without choices/output
                let looks_anthropic = value.get("type").and_then(Value::as_str) == Some("error");
                let looks_openai_err = value.get("choices").is_none()
                    && value.get("output").is_none()
                    && value.get("content").is_none()
                    && value.get("candidates").is_none();
                if looks_anthropic || looks_openai_err {
                    return Some(format!("{error_type}: {message}"));
                }
            }
        }
    }
    // Codex / OpenAI Responses failed|cancelled
    let status = value.get("status").and_then(Value::as_str);
    let has_error = value.get("error").is_some_and(|e| !e.is_null());
    if matches!(status, Some("failed" | "cancelled")) || (has_error && status.is_some()) {
        let error = value.get("error").unwrap_or(&value);
        let error_type = error
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| error.get("code").and_then(Value::as_str))
            .unwrap_or_else(|| status.unwrap_or("error"));
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .filter(|m| !m.trim().is_empty())
            .unwrap_or(match status {
                Some("cancelled") => "response generation was cancelled",
                _ => "response generation failed",
            });
        return Some(format!("{error_type}: {message}"));
    }
    let _ = response_id(&value, "id"); // keep helper used in tests via from_json
    None
}

/// True if the first stream chunk already looks like a terminal Responses failure.
pub fn sse_early_failure_message(text: &str) -> Option<String> {
    for block in text.split("\n\n") {
        let mut data_lines = Vec::new();
        let mut named_event = None::<String>;
        for line in block.lines() {
            if let Some(ev) = line.strip_prefix("event:") {
                named_event = Some(ev.trim().to_string());
            } else if let Some(data) = line.strip_prefix("data:") {
                data_lines.push(data.trim());
            }
        }
        if data_lines.is_empty() {
            continue;
        }
        let joined = data_lines.join("\n");
        if joined == "[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&joined) else {
            continue;
        };
        let ty = value
            .get("type")
            .and_then(Value::as_str)
            .or(named_event.as_deref())
            .unwrap_or("");
        if matches!(
            ty,
            "error" | "response.failed" | "response.error" | "response.incomplete"
        ) {
            if let Some(msg) = value
                .pointer("/error/message")
                .or_else(|| value.pointer("/response/error/message"))
                .and_then(Value::as_str)
            {
                return Some(format!("{ty}: {msg}"));
            }
            if let Some(msg) = semantic_error_message(joined.as_bytes()) {
                return Some(msg);
            }
            return Some(format!("{ty}: stream failure"));
        }
        // Embedded failed response object
        if let Some(resp) = value.get("response") {
            if let Some(msg) = semantic_error_message(
                &serde_json::to_vec(resp).unwrap_or_default(),
            ) {
                return Some(msg);
            }
        }
    }
    // Whole-buffer JSON document (non-SSE 2xx failure)
    if text.trim().starts_with('{') {
        return semantic_error_message(text.trim().as_bytes());
    }
    None
}
