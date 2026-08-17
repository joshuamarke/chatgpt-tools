//! Small OpenAI Chat helpers shared by the Responses↔Chat bridge.

use serde_json::{json, Value};

/// Detect OpenAI o-series reasoning models (o1, o3, o4-mini, etc.)
pub fn is_openai_o_series(model: &str) -> bool {
    model.len() > 1
        && model.starts_with('o')
        && model.as_bytes().get(1).is_some_and(|b| b.is_ascii_digit())
}

/// Detect Responses-compatible models that support reasoning effort.
pub fn supports_reasoning_effort(model: &str) -> bool {
    let normalized = model.to_lowercase();
    is_openai_o_series(&normalized)
        || normalized
            .strip_prefix("gpt-")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_digit() && c >= '5')
        || normalized == "grok-4.5"
        || normalized.starts_with("grok-4.5-")
        || normalized.starts_with("grok-build-")
}

/// Inject `stream_options.include_usage` for streaming Chat requests.
pub(crate) fn inject_openai_stream_include_usage(result: &mut Value) {
    let is_stream = result
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_stream {
        return;
    }
    match result.get_mut("stream_options") {
        Some(Value::Object(opts)) => {
            opts.insert("include_usage".to_string(), json!(true));
        }
        _ => {
            result["stream_options"] = json!({ "include_usage": true });
        }
    }
}
