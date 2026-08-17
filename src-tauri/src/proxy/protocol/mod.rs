//! Codex Responses 鈫?OpenAI Chat Completions protocol bridge.
//!
//! When a third-party upstream only speaks Chat Completions, the client-facing
//! live config still uses `wire_api = "responses"`. The local proxy converts
//! Responses requests to Chat Completions and maps responses (JSON + SSE) back.

mod codex_chat_common;
mod codex_chat_history;
mod codex_responses_sse;
pub mod detect;
mod json_canonical;
mod openai_helpers;
mod sse;
mod streaming_codex_chat;
mod tool_media;
mod transform_codex_chat;

use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use http::{HeaderMap, HeaderValue, StatusCode};
use serde_json::{json, Value};
use thiserror::Error;

pub use detect::{
    inject_codex_chat_prompt_cache_key, resolve_codex_chat_reasoning_config,
    should_convert_codex_responses_to_chat,
};
use transform_codex_chat::CodexToolContext;

use streaming_codex_chat::create_responses_sse_stream_from_chat_with_context;
use transform_codex_chat::{
    chat_completion_to_response_with_context, chat_error_to_response_error,
    responses_to_chat_completions_with_reasoning,
};

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("格式转换错误: {0}")]
    TransformError(String),
    #[error("无效的请求: {0}")]
    InvalidRequest(String),
}

impl ProxyError {
    pub fn message(&self) -> String {
        self.to_string()
    }
}

fn chat_history_store() -> Arc<codex_chat_history::CodexChatHistoryStore> {
    static STORE: OnceLock<Arc<codex_chat_history::CodexChatHistoryStore>> = OnceLock::new();
    STORE
        .get_or_init(|| Arc::new(codex_chat_history::CodexChatHistoryStore::default()))
        .clone()
}

/// True when the client path is a Responses API endpoint (Codex or Grok proxy prefix).
pub fn path_is_responses_endpoint(path_and_query: &str) -> bool {
    let path = path_and_query
        .split_once('?')
        .map_or(path_and_query, |(p, _)| p)
        .trim_end_matches('/');
    path.ends_with("/responses")
        || path.ends_with("/responses/compact")
        || path == "/responses"
        || path == "/responses/compact"
}

/// Rewrite `/v1/responses` (and compact) 鈫?`/chat/completions`, preserving query.
///
/// For Grok local paths (`/grok/v1/responses`), strip the `/grok` prefix in the
/// rewritten path so `build_upstream_url` still joins correctly against a normal
/// OpenAI-compatible base (`鈥?v1` + `/chat/completions`).
pub fn rewrite_responses_endpoint_to_chat(path_and_query: &str) -> String {
    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path_and_query, None),
    };
    let path = path.trim_end_matches('/');
    // Drop local-only app prefix before rewriting.
    let path = path.strip_prefix("/grok").unwrap_or(path);
    let new_path = if path_is_responses_endpoint(path) {
        if let Some(idx) = path.rfind("/responses") {
            format!("{}{}", &path[..idx], "/chat/completions")
        } else {
            "/chat/completions".into()
        }
    } else {
        path.to_string()
    };
    match query {
        Some(q) if !q.is_empty() => format!("{new_path}?{q}"),
        _ => new_path,
    }
}

/// Heuristic: body looks like Chat Completions (has choices) rather than Responses.
pub fn body_looks_like_chat_completion(body: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    if v.get("object")
        .and_then(|o| o.as_str())
        .is_some_and(|o| o == "response")
    {
        return false;
    }
    if v.get("output").is_some() && v.get("created_at").is_some() {
        return false;
    }
    v.get("choices")
        .and_then(|c| c.as_array())
        .is_some_and(|a| !a.is_empty())
}

fn unix_now_secs_u64() -> u64 {
    transform_codex_chat::unix_now_secs().max(1)
}

/// Ensure a Responses JSON object has required `created_at` (Grok Build / async-openai
/// deserialize it as a required field 鈥?many third-party gateways omit it).
/// Fill common required / expected Responses fields for strict clients
/// (Grok Build / async-openai). Only inserts missing or null keys.
fn patch_response_object_fields(obj: &mut Value) -> bool {
    let Some(map) = obj.as_object_mut() else {
        return false;
    };
    let mut changed = false;

    // created_at 鈥?required by Grok Build; many gateways omit it.
    if !map.contains_key("created_at") || map.get("created_at") == Some(&Value::Null) {
        let ts = map
            .get("created")
            .and_then(|v| v.as_u64())
            .filter(|v| *v > 0)
            .unwrap_or_else(unix_now_secs_u64);
        map.insert("created_at".into(), json!(ts));
        changed = true;
    }

    // object 鈥?OpenAI Responses always uses "response".
    if !map.contains_key("object") || map.get("object") == Some(&Value::Null) {
        map.insert("object".into(), json!("response"));
        changed = true;
    }

    // status 鈥?non-stream completed bodies often omit it on gateways.
    if !map.contains_key("status") || map.get("status") == Some(&Value::Null) {
        map.insert("status".into(), json!("completed"));
        changed = true;
    }

    // output 鈥?empty array is safer than missing for deserializers expecting Vec.
    if !map.contains_key("output") || map.get("output") == Some(&Value::Null) {
        map.insert("output".into(), json!([]));
        changed = true;
    }

    // id 鈥?some clients require a non-empty id string.
    if !map.contains_key("id")
        || map
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true)
    {
        if map.get("id").is_none() || map.get("id") == Some(&Value::Null) {
            map.insert(
                "id".into(),
                json!(format!("resp_{}", unix_now_secs_u64())),
            );
            changed = true;
        }
    }

    // Nested message/output_text parts often omit annotations on gateways.
    if let Some(output) = map.get_mut("output") {
        changed |= ensure_output_text_annotations(output);
    }

    // usage — Grok Build / async-openai require nested detail objects.
    // Third-party gateways often send only input/output/total_tokens.
    if let Some(usage) = map.get_mut("usage") {
        changed |= ensure_responses_usage_fields(usage);
    }

    changed
}

/// Normalize Responses `usage` for strict clients (Grok Build / async-openai).
/// Only fills missing/null nested objects; never overwrites present values.
fn ensure_responses_usage_fields(usage: &mut Value) -> bool {
    let Some(map) = usage.as_object_mut() else {
        return false;
    };
    let mut changed = false;

    // input_tokens_details.cached_tokens — required when details object is present
    // in OpenAI Responses; async-openai expects the object itself on ResponseUsage.
    if !map.contains_key("input_tokens_details")
        || map.get("input_tokens_details") == Some(&Value::Null)
    {
        let cached = map
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                map.get("prompt_tokens_details")
                    .and_then(|d| d.get("cached_tokens"))
                    .and_then(|v| v.as_u64())
            })
            .unwrap_or(0);
        map.insert(
            "input_tokens_details".into(),
            json!({ "cached_tokens": cached }),
        );
        changed = true;
    } else if let Some(details) = map.get_mut("input_tokens_details") {
        if let Some(dmap) = details.as_object_mut() {
            if !dmap.contains_key("cached_tokens") || dmap.get("cached_tokens") == Some(&Value::Null)
            {
                dmap.insert("cached_tokens".into(), json!(0));
                changed = true;
            }
        }
    }

    // output_tokens_details.reasoning_tokens — the field Grok reports as missing
    // when gateways omit the whole object (tidalrelay etc.).
    if !map.contains_key("output_tokens_details")
        || map.get("output_tokens_details") == Some(&Value::Null)
    {
        let reasoning = map
            .get("completion_tokens_details")
            .and_then(|d| d.get("reasoning_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        map.insert(
            "output_tokens_details".into(),
            json!({ "reasoning_tokens": reasoning }),
        );
        changed = true;
    } else if let Some(details) = map.get_mut("output_tokens_details") {
        if let Some(dmap) = details.as_object_mut() {
            if !dmap.contains_key("reasoning_tokens")
                || dmap.get("reasoning_tokens") == Some(&Value::Null)
            {
                dmap.insert("reasoning_tokens".into(), json!(0));
                changed = true;
            }
        }
    }

    // Top-level token counters: fill 0 only when entirely missing (not when 0).
    for key in ["input_tokens", "output_tokens", "total_tokens"] {
        if !map.contains_key(key) || map.get(key) == Some(&Value::Null) {
            let fallback = match key {
                "total_tokens" => {
                    let inn = map.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    let out = map
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    inn.saturating_add(out)
                }
                _ => 0,
            };
            map.insert(key.into(), json!(fallback));
            changed = true;
        }
    }

    changed
}

/// Non-stream: patch top-level Responses JSON missing required fields.
pub fn ensure_responses_json_created_at(body: Bytes) -> Bytes {
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let mut changed = patch_response_object_fields(&mut value);
    changed |= ensure_responses_value_annotations(&mut value);
    if !changed {
        return body;
    }
    Bytes::from(serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec()))
}

/// Fill `annotations: []` on `output_text` content parts / content_part events.
/// Grok Build deserializes OutputTextContent.annotations as a required Vec.
fn ensure_output_text_annotations(node: &mut Value) -> bool {
    match node {
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= ensure_output_text_annotations(item);
            }
            changed
        }
        Value::Object(map) => {
            let mut changed = false;
            let type_name = map.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if type_name == "output_text"
                && (!map.contains_key("annotations")
                    || map.get("annotations") == Some(&Value::Null))
            {
                map.insert("annotations".into(), json!([]));
                changed = true;
            }
            for key in ["content", "part", "item", "output", "response"] {
                if let Some(child) = map.get_mut(key) {
                    changed |= ensure_output_text_annotations(child);
                }
            }
            changed
        }
        _ => false,
    }
}

fn ensure_responses_value_annotations(v: &mut Value) -> bool {
    ensure_output_text_annotations(v)
}

/// Stream: rewrite SSE `data:` JSON so nested `response.created_at` and
/// top-level `sequence_number` are present. Grok Build / async-openai fail on
/// `response.created` / `response.completed` without them (third-party gateways
/// often omit both on the first event).
pub fn ensure_responses_sse_created_at_stream<S, E>(
    stream: S,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::error::Error + Send + 'static,
{
    let mapped = stream.map(|item| {
        item.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    });
    // next_seq: next sequence_number to assign when upstream omits it.
    // After seeing an explicit N, next becomes N+1 so filled values stay monotonic.
    futures::stream::unfold(
        (Box::pin(mapped), String::new(), Vec::<u8>::new(), 0u64),
        |(mut inner, mut buffer, mut utf8_rem, mut next_seq)| async move {
            loop {
                // Emit complete SSE blocks from buffer first.
                if let Some(block) = sse::take_sse_block(&mut buffer) {
                    let patched = patch_sse_block_created_at(&block, &mut next_seq);
                    let out = Bytes::from(format!("{patched}\n\n"));
                    return Some((Ok(out), (inner, buffer, utf8_rem, next_seq)));
                }
                match inner.as_mut().next().await {
                    None => {
                        if buffer.is_empty() {
                            return None;
                        }
                        let patched = patch_sse_block_created_at(buffer.trim_end(), &mut next_seq);
                        let out = if patched.is_empty() {
                            Bytes::new()
                        } else if patched.ends_with('\n') {
                            Bytes::from(patched)
                        } else {
                            Bytes::from(format!("{patched}\n\n"))
                        };
                        buffer.clear();
                        return Some((Ok(out), (inner, buffer, utf8_rem, next_seq)));
                    }
                    Some(Err(e)) => return Some((Err(e), (inner, buffer, utf8_rem, next_seq))),
                    Some(Ok(chunk)) => {
                        sse::append_utf8_safe(&mut buffer, &mut utf8_rem, &chunk);
                    }
                }
            }
        },
    )
}

/// Patch one SSE event block. `next_seq` tracks the next sequence_number to
/// emit when the event JSON omits it (common on `response.created`).
fn patch_sse_block_created_at(block: &str, next_seq: &mut u64) -> String {
    let mut event_name = String::new();
    let mut data_lines: Vec<String> = Vec::new();
    let mut other_lines: Vec<String> = Vec::new();
    for line in block.lines() {
        let trimmed = line.trim_start();
        if let Some(evt) = sse::strip_sse_field(trimmed, "event") {
            event_name = evt.trim().to_string();
            other_lines.push(line.to_string());
        } else if let Some(d) = sse::strip_sse_field(trimmed, "data") {
            data_lines.push(d.to_string());
        } else if !trimmed.is_empty() {
            other_lines.push(line.to_string());
        }
    }
    if data_lines.is_empty() {
        return block.to_string();
    }
    let data = data_lines.join("\n");
    // Keep [DONE] / non-JSON control payloads untouched.
    if data.trim() == "[DONE]" {
        return block.to_string();
    }
    let patched_data = match serde_json::from_str::<Value>(&data) {
        Ok(mut v) => {
            let mut changed = false;
            // Top-level response object (rare in SSE data alone).
            changed |= patch_response_object_fields(&mut v);
            // Nested under "response" (response.created / completed / failed / 鈥?.
            if let Some(resp) = v.get_mut("response") {
                // Streaming in_progress events should not be forced to "completed".
                let status_before = resp
                    .get("status")
                    .and_then(|s| s.as_str())
                    .map(str::to_string);
                changed |= patch_response_object_fields(resp);
                if let Some(st) = status_before {
                    if let Some(map) = resp.as_object_mut() {
                        map.insert("status".into(), json!(st));
                    }
                } else if event_name.contains("created") || event_name.contains("in_progress") {
                    if let Some(map) = resp.as_object_mut() {
                        if map.get("status").and_then(|s| s.as_str()) == Some("completed") {
                            map.insert("status".into(), json!("in_progress"));
                        }
                    }
                }
            }
            // sequence_number 鈥?required on every Responses SSE event by Grok
            // Build / async-openai. Some gateways omit it on response.created only.
            // annotations on output_text parts (item/content/part/response paths)
            changed |= ensure_responses_value_annotations(&mut v);
            changed |= ensure_sse_event_sequence_number(&mut v, next_seq);
            if changed {
                serde_json::to_string(&v).unwrap_or(data)
            } else {
                data
            }
        }
        Err(_) => data,
    };
    let mut out = String::new();
    if !event_name.is_empty() {
        // Prefer reconstructed event line for stability.
        out.push_str("event: ");
        out.push_str(&event_name);
        out.push('\n');
        for line in &other_lines {
            let t = line.trim_start();
            if sse::strip_sse_field(t, "event").is_some() {
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
    } else {
        for line in &other_lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str("data: ");
    out.push_str(&patched_data);
    out
}

/// Ensure SSE event JSON has a numeric `sequence_number`.
///
/// - Missing / null 鈫?assign `*next_seq` then bump.
/// - Present number 鈫?adopt it and set `*next_seq = n + 1` so subsequent fills
///   stay monotonic with upstream (e.g. created filled as 0, next event has 1).
fn ensure_sse_event_sequence_number(v: &mut Value, next_seq: &mut u64) -> bool {
    let Some(map) = v.as_object_mut() else {
        return false;
    };
    // Only Responses stream events carry a type; skip bare response objects.
    let looks_like_event = map
        .get("type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| t.starts_with("response."));
    if !looks_like_event {
        return false;
    }
    match map.get("sequence_number") {
        Some(Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                *next_seq = u.saturating_add(1);
            } else if let Some(i) = n.as_i64().filter(|&i| i >= 0) {
                *next_seq = (i as u64).saturating_add(1);
            }
            false
        }
        Some(Value::Null) | None => {
            let assigned = *next_seq;
            map.insert("sequence_number".into(), json!(assigned));
            *next_seq = assigned.saturating_add(1);
            true
        }
        // Non-numeric garbage: replace so strict clients can deserialize.
        Some(_) => {
            let assigned = *next_seq;
            map.insert("sequence_number".into(), json!(assigned));
            *next_seq = assigned.saturating_add(1);
            true
        }
    }
}

/// Convert a Codex Responses request body into Chat Completions for upstream.
pub async fn convert_responses_request_body(
    provider: &crate::providers::models::Provider,
    body: &[u8],
    client_session_id: Option<&str>,
) -> Result<(Bytes, CodexToolContext), ProxyError> {
    let mut value: Value = serde_json::from_slice(body).map_err(|e| {
        ProxyError::InvalidRequest(format!("Failed to parse Responses request body: {e}"))
    })?;

    // Multi-turn tool history enrichment on the Responses body (before convert).
    let _ = chat_history_store().enrich_request(&mut value).await;

    let tool_context = transform_codex_chat::build_codex_tool_context_from_request(&value);
    let reasoning = resolve_codex_chat_reasoning_config(provider, &value);
    let mut chat_body =
        responses_to_chat_completions_with_reasoning(value, reasoning.as_ref())?;

    let explicit = provider
        .meta
        .as_ref()
        .and_then(|m| m.prompt_cache_key.as_deref());
    inject_codex_chat_prompt_cache_key(provider, &mut chat_body, explicit, client_session_id);

    let bytes = serde_json::to_vec(&chat_body).map_err(|e| {
        ProxyError::TransformError(format!("Failed to serialize Chat request: {e}"))
    })?;
    Ok((Bytes::from(bytes), tool_context))
}

/// Convert a successful non-streaming Chat Completions JSON body 鈫?Responses.
pub async fn convert_chat_json_to_responses(
    body: &[u8],
    tool_context: &CodexToolContext,
) -> Result<Bytes, ProxyError> {
    let chat: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) if body_looks_like_sse(std::str::from_utf8(body).unwrap_or("")) => {
            chat_sse_to_response_value(std::str::from_utf8(body).unwrap_or(""))?
        }
        Err(e) => {
            return Err(ProxyError::TransformError(format!(
                "Failed to parse upstream chat response: {e}"
            )));
        }
    };
    let responses = chat_completion_to_response_with_context(chat, tool_context)?;
    let _ = chat_history_store().record_response(&responses).await;
    let bytes = serde_json::to_vec(&responses).map_err(|e| {
        ProxyError::TransformError(format!("Failed to serialize Responses body: {e}"))
    })?;
    Ok(Bytes::from(bytes))
}

/// Convert a Chat error body into Responses-style `{"error":{...}}`.
pub fn convert_chat_error_to_responses(body: &[u8]) -> Bytes {
    let parsed: Value = match serde_json::from_slice::<Value>(body) {
        Ok(v) => v,
        Err(_) => {
            let lossy = String::from_utf8_lossy(body);
            let truncated: String = lossy.chars().take(1024).collect();
            Value::String(truncated)
        }
    };
    let err = chat_error_to_response_error(Some(&parsed));
    Bytes::from(serde_json::to_vec(&err).unwrap_or_else(|_| {
        br#"{"error":{"message":"upstream error","type":"upstream_error"}}"#.to_vec()
    }))
}

/// Wrap an upstream Chat Completions SSE byte stream as Responses SSE.
pub fn convert_chat_sse_stream_to_responses<S, E>(
    stream: S,
    tool_context: CodexToolContext,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::error::Error + Send + 'static,
{
    let mapped = stream.map(|item| {
        item.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    });
    let sse = create_responses_sse_stream_from_chat_with_context(mapped, tool_context);
    let recorded = codex_chat_history::record_responses_sse_stream(sse, chat_history_store());
    recorded.map(|item| {
        item.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    })
}

pub fn responses_content_type_json(headers: &mut HeaderMap) {
    headers.remove(http::header::CONTENT_TYPE);
    headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
}

pub fn responses_content_type_sse(headers: &mut HeaderMap) {
    headers.remove(http::header::CONTENT_TYPE);
    headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(
        http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
}

pub fn transform_status_from_error(err: &ProxyError) -> StatusCode {
    match err {
        ProxyError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        ProxyError::TransformError(_) => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

fn body_looks_like_sse(body: &str) -> bool {
    let trimmed = body.trim_start_matches('\u{feff}').trim_start();
    ["data:", "event:", "id:", "retry:", ":"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    sse::strip_sse_field(line, field)
}

fn take_sse_block(buffer: &mut String) -> Option<String> {
    sse::take_sse_block(buffer)
}

fn error_event_message(error: &Value) -> Option<String> {
    if let Some(msg) = error.get("message").and_then(|m| m.as_str()) {
        return (!msg.is_empty()).then(|| msg.to_string());
    }
    if let Some(s) = error.as_str() {
        return (!s.is_empty()).then(|| s.to_string());
    }
    None
}

fn envelope_value_meaningful(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => true,
    }
}

fn sse_block_parts(block: &str) -> Option<(String, String)> {
    let mut event_name = String::new();
    let mut data_lines: Vec<&str> = Vec::new();
    for line in block.lines() {
        let line = line.trim_start();
        if let Some(evt) = strip_sse_field(line, "event") {
            event_name = evt.trim().to_string();
        } else if let Some(d) = strip_sse_field(line, "data") {
            data_lines.push(d);
        }
    }
    (!data_lines.is_empty()).then(|| (event_name, data_lines.join("\n")))
}

fn merge_tool_call_delta(
    tool_calls: &mut std::collections::BTreeMap<usize, Value>,
    tc: &Value,
    pos: usize,
) {
    let index = tc
        .get("index")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(pos);
    let entry = tool_calls.entry(index).or_insert_with(|| {
        json!({
            "id": "",
            "type": "function",
            "function": { "name": "", "arguments": "" }
        })
    });
    if let Some(id) = tc.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        entry["id"] = json!(id);
    }
    if let Some(ty) = tc.get("type").and_then(|v| v.as_str()) {
        entry["type"] = json!(ty);
    }
    let func = tc.get("function").unwrap_or(tc);
    if let Some(name) = func
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        entry["function"]["name"] = json!(name);
    }
    if let Some(args) = func.get("arguments") {
        match args {
            Value::String(s) => {
                let prev = entry["function"]["arguments"].as_str().unwrap_or("").to_string();
                entry["function"]["arguments"] = json!(format!("{prev}{s}"));
            }
            other => {
                entry["function"]["arguments"] = json!(other.to_string());
            }
        }
    }
}

/// Aggregate Chat Completions SSE into a single chat.completion JSON (fallback).
fn chat_sse_to_response_value(body: &str) -> Result<Value, ProxyError> {
    let mut buffer = body.trim_start_matches('\u{feff}').to_string();

    let mut id = Value::Null;
    let mut created = Value::Null;
    let mut model = Value::Null;
    let mut content = String::new();
    let mut reasoning_content = String::new();
    let mut tool_calls: std::collections::BTreeMap<usize, Value> =
        std::collections::BTreeMap::new();
    let mut finish_reason = Value::Null;
    let mut usage = Value::Null;
    let mut saw_choice = false;
    let mut saw_done = false;

    let mut process_event =
        |event_name: &str, data_str: &str, strict: bool| -> Result<(), ProxyError> {
            let trimmed = data_str.trim();
            if trimmed == "[DONE]" {
                saw_done = true;
                return Ok(());
            }
            if trimmed.is_empty() {
                return Ok(());
            }
            let chunk: Value = match serde_json::from_str(data_str) {
                Ok(v) => v,
                Err(_) if !strict => return Ok(()),
                Err(e) => {
                    return Err(ProxyError::TransformError(format!(
                        "Failed to parse upstream SSE chunk: {e}"
                    )))
                }
            };

            if event_name.eq_ignore_ascii_case("error") {
                let message = chunk
                    .get("error")
                    .and_then(error_event_message)
                    .or_else(|| error_event_message(&chunk))
                    .unwrap_or_else(|| "upstream error event in SSE stream".to_string());
                return Err(ProxyError::TransformError(message));
            }
            if let Some(message) = chunk
                .get("error")
                .filter(|e| !e.is_null())
                .and_then(error_event_message)
            {
                return Err(ProxyError::TransformError(message));
            }

            for (slot, key) in [
                (&mut id, "id"),
                (&mut created, "created"),
                (&mut model, "model"),
            ] {
                if slot.is_null() {
                    if let Some(v) = chunk.get(key).filter(|v| envelope_value_meaningful(v)) {
                        *slot = v.clone();
                    }
                }
            }
            if let Some(u) = chunk.get("usage").filter(|u| !u.is_null()) {
                usage = u.clone();
            }

            let Some(choice) = chunk
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|ch| ch.get("index").and_then(|i| i.as_u64()).unwrap_or(0) == 0)
                })
            else {
                return Ok(());
            };

            saw_choice = true;

            if finish_reason.is_null() {
                if let Some(fr) = choice.get("finish_reason").filter(|v| !v.is_null()) {
                    finish_reason = fr.clone();
                }
            }
            let delta_nonempty = choice
                .get("delta")
                .and_then(|d| d.as_object())
                .is_some_and(|o| !o.is_empty());
            let (payload, is_full_message) = if delta_nonempty {
                (choice.get("delta").unwrap(), false)
            } else if let Some(message) = choice.get("message") {
                (message, true)
            } else if let Some(delta) = choice.get("delta") {
                (delta, false)
            } else {
                return Ok(());
            };
            if is_full_message {
                content.clear();
                reasoning_content.clear();
                tool_calls.clear();
            }
            match payload.get("content") {
                Some(Value::String(text)) => content.push_str(text),
                Some(Value::Array(parts)) => {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            content.push_str(text);
                        } else if let Some(refusal) = part.get("refusal").and_then(|r| r.as_str()) {
                            content.push_str(refusal);
                        }
                    }
                }
                _ => {}
            }
            if let Some(refusal) = payload.get("refusal").and_then(|r| r.as_str()) {
                content.push_str(refusal);
            }
            if let Some(text) = codex_chat_common::extract_reasoning_field_text(payload) {
                reasoning_content.push_str(&text);
            }
            if let Some(deltas) = payload.get("tool_calls").and_then(|t| t.as_array()) {
                for (pos, tc) in deltas.iter().enumerate() {
                    merge_tool_call_delta(&mut tool_calls, tc, pos);
                }
            } else if let Some(fc) = payload.get("function_call").filter(|v| !v.is_null()) {
                let synthetic = json!({
                    "id": fc.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                    "type": "function",
                    "function": fc,
                });
                merge_tool_call_delta(&mut tool_calls, &synthetic, 0);
            }
            Ok(())
        };

    while let Some(block) = take_sse_block(&mut buffer) {
        if let Some((event, data)) = sse_block_parts(&block) {
            process_event(&event, &data, true)?;
        }
    }
    if let Some((event, data)) = sse_block_parts(&buffer) {
        process_event(&event, &data, false)?;
    }

    if !saw_choice {
        return Err(ProxyError::TransformError(
            "No chat completion choices in upstream SSE".to_string(),
        ));
    }
    if finish_reason.is_null() && !saw_done {
        return Err(ProxyError::TransformError(
            "Upstream SSE stream appears truncated (no finish_reason or [DONE] marker)".to_string(),
        ));
    }

    let tool_calls: Vec<Value> = tool_calls
        .into_iter()
        .filter(|(_, tc)| {
            tc["id"].as_str().is_some_and(|s| !s.is_empty())
                || tc["function"]["name"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
                || tc["function"]["arguments"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
        })
        .map(|(index, mut tc)| {
            if tc["id"].as_str().is_none_or(str::is_empty) {
                tc["id"] = json!(format!("tool_call_{index}"));
            }
            if tc["function"]["name"].as_str().is_none_or(str::is_empty) {
                tc["function"]["name"] = json!("unknown_tool");
            }
            tc
        })
        .collect();

    let mut message = serde_json::Map::new();
    message.insert("role".to_string(), json!("assistant"));
    message.insert("content".to_string(), json!(content));
    if !reasoning_content.is_empty() {
        message.insert("reasoning_content".to_string(), json!(reasoning_content));
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    let id = if envelope_value_meaningful(&id) {
        id
    } else {
        json!(uuid::Uuid::new_v4().to_string())
    };

    // Ensure Chat envelope has `created` so Responses conversion always gets created_at.
    let created = if envelope_value_meaningful(&created) {
        created
    } else {
        json!(transform_codex_chat::unix_now_secs())
    };

    let mut response = json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_reason,
        }],
    });
    if !usage.is_null() {
        response["usage"] = usage;
    }
    Ok(response)
}


#[cfg(test)]
mod passthrough_fill_tests {
    use super::{
        ensure_output_text_annotations, ensure_responses_usage_fields,
        ensure_sse_event_sequence_number, patch_response_object_fields, patch_sse_block_created_at,
    };
    use serde_json::{json, Value};

    #[test]
    fn fills_missing_sequence_number_and_created_at_on_response_created() {
        let block = r#"event: response.created
data: {"type":"response.created","response":{"id":"r1","object":"response","model":"m","status":"in_progress","output":[]}}"#;
        let mut next = 0u64;
        let out = patch_sse_block_created_at(block, &mut next);
        let data = out.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
        let v: Value = serde_json::from_str(data).unwrap();
        assert_eq!(v["sequence_number"], json!(0));
        assert!(v["response"]["created_at"].as_u64().is_some());
        assert_eq!(next, 1);
    }

    #[test]
    fn fills_missing_annotations_on_output_item_added() {
        let block = r#"event: response.output_item.added
data: {"item":{"content":[{"text":"","type":"output_text"}],"id":"item_1","role":"assistant","status":"in_progress","type":"message"},"output_index":0,"sequence_number":1,"type":"response.output_item.added"}"#;
        let mut next = 0u64;
        let out = patch_sse_block_created_at(block, &mut next);
        let data = out.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
        let v: Value = serde_json::from_str(data).unwrap();
        assert_eq!(v["item"]["content"][0]["annotations"], json!([]));
        assert_eq!(v["sequence_number"], json!(1));
        assert_eq!(next, 2);
    }

    #[test]
    fn fills_annotations_on_nested_response_output() {
        let mut v = json!({
            "type": "response.completed",
            "sequence_number": 17,
            "response": {
                "id": "r1",
                "object": "response",
                "status": "completed",
                "output": [{
                    "type": "message",
                    "content": [{"type": "output_text", "text": "Hi!"}]
                }]
            }
        });
        assert!(ensure_output_text_annotations(&mut v));
        assert_eq!(
            v["response"]["output"][0]["content"][0]["annotations"],
            json!([])
        );
        assert!(!ensure_output_text_annotations(&mut v));
    }

    #[test]
    fn leaves_done_marker_untouched() {
        let mut next = 0u64;
        assert_eq!(patch_sse_block_created_at("data: [DONE]", &mut next), "data: [DONE]");
        assert_eq!(next, 0);
    }

    #[test]
    fn ensure_sequence_number_skips_bare_response_object() {
        let mut next = 5u64;
        let mut v = json!({"id":"r1","object":"response","status":"completed"});
        assert!(!ensure_sse_event_sequence_number(&mut v, &mut next));
        assert!(v.get("sequence_number").is_none());
        assert_eq!(next, 5);
    }

    #[test]
    fn fills_missing_output_tokens_details_on_usage() {
        let mut usage = json!({
            "input_tokens": 496,
            "output_tokens": 35,
            "total_tokens": 531,
            "input_tokens_details": { "cached_tokens": 384 }
        });
        assert!(ensure_responses_usage_fields(&mut usage));
        assert_eq!(
            usage["output_tokens_details"],
            json!({ "reasoning_tokens": 0 })
        );
        // Already-present details must not be overwritten.
        assert_eq!(
            usage["input_tokens_details"],
            json!({ "cached_tokens": 384 })
        );
        assert!(!ensure_responses_usage_fields(&mut usage));
    }

    #[test]
    fn fills_usage_details_on_response_completed_sse() {
        let block = r#"event: response.completed
data: {"type":"response.completed","sequence_number":17,"response":{"id":"r1","object":"response","model":"m","status":"completed","output":[{"type":"message","id":"i1","role":"assistant","content":[{"type":"output_text","text":"Hi!"}],"status":"completed"}],"usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}}"#;
        let mut next = 0u64;
        let out = patch_sse_block_created_at(block, &mut next);
        let data = out.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
        let v: Value = serde_json::from_str(data).unwrap();
        assert_eq!(
            v["response"]["usage"]["output_tokens_details"],
            json!({ "reasoning_tokens": 0 })
        );
        assert_eq!(
            v["response"]["usage"]["input_tokens_details"],
            json!({ "cached_tokens": 0 })
        );
        assert!(v["response"]["created_at"].as_u64().is_some());
        assert_eq!(
            v["response"]["output"][0]["content"][0]["annotations"],
            json!([])
        );
    }

    #[test]
    fn patch_response_preserves_existing_usage_details() {
        let mut resp = json!({
            "id": "r1",
            "object": "response",
            "status": "completed",
            "output": [],
            "usage": {
                "input_tokens": 1,
                "output_tokens": 2,
                "total_tokens": 3,
                "input_tokens_details": { "cached_tokens": 9 },
                "output_tokens_details": { "reasoning_tokens": 7 }
            }
        });
        let _ = patch_response_object_fields(&mut resp);
        assert_eq!(
            resp["usage"]["output_tokens_details"]["reasoning_tokens"],
            json!(7)
        );
        assert_eq!(
            resp["usage"]["input_tokens_details"]["cached_tokens"],
            json!(9)
        );
    }
}
