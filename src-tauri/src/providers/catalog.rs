//! Codex `model_catalog_json` generation for third-party model lists.
//!
//! Writes `~/.codex/chatgpt-tools-model-catalog.json` and points
//! `config.toml` at it so Codex `/model` lists third-party model names.
//!
//! Catalog entries MUST include parser-required fields (`base_instructions`,
//! `supports_reasoning_summaries`, …); otherwise Codex refuses to load the file.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

pub const CATALOG_FILENAME: &str = "chatgpt-tools-model-catalog.json";

/// Fields Codex's external-catalog parser has been observed to require
/// (codex ≥ 0.144.5).
const PARSER_REQUIRED_FIELDS: &[&str] = &["base_instructions", "supports_reasoning_summaries"];

const NATIVE_TEMPLATE: &str =
    include_str!("resources/codex_native_responses_template.json");

#[derive(Debug, Clone)]
pub struct CatalogModelSpec {
    pub model: String,
    pub display_name: String,
    /// Explicit per-model window from mapping UI; `None` → use native cache / fallback.
    pub context_window: Option<u64>,
}

pub fn catalog_path(codex_home: &Path) -> PathBuf {
    codex_home.join(CATALOG_FILENAME)
}

fn parse_positive_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(n)) => n
            .as_u64()
            .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok()))
            .or_else(|| n.as_f64().and_then(|f| (f > 0.0).then_some(f as u64)))
            .filter(|v| *v > 0),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok().filter(|v| *v > 0),
        _ => None,
    }
}

fn default_context_window(config_text: &str) -> u64 {
    if let Ok(doc) = config_text.parse::<toml::Value>() {
        if let Some(v) = doc
            .get("model_context_window")
            .and_then(|v| v.as_integer())
            .and_then(|v| u64::try_from(v).ok())
            .filter(|v| *v > 0)
        {
            return v;
        }
    }
    128_000
}

/// Extract simplified model list from stored settings (`settings.modelCatalog.models`).
pub fn specs_from_settings(settings: &Value) -> Vec<CatalogModelSpec> {
    let Some(models) = settings
        .get("modelCatalog")
        .and_then(|c| c.get("models"))
        .and_then(|m| m.as_array())
    else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for m in models {
        let Some(model) = m
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if !seen.insert(model.to_string()) {
            continue;
        }
        let display_name = m
            .get("displayName")
            .or_else(|| m.get("display_name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(model)
            .to_string();
        // Only keep an explicit row window; otherwise leave None so native cache
        // windows (e.g. gpt-5.6-sol = 372000) are not clobbered by 128k defaults.
        let context_window = parse_positive_u64(
            m.get("contextWindow")
                .or_else(|| m.get("context_window")),
        );
        out.push(CatalogModelSpec {
            model: model.to_string(),
            display_name,
            context_window,
        });
    }
    out
}

/// First mapped model id (if any) — used to fill top-level `model` when empty.
pub fn first_catalog_model(settings: &Value) -> Option<String> {
    specs_from_settings(settings)
        .into_iter()
        .next()
        .map(|s| s.model)
}

/// All mapped model slugs (order preserved). Used for diagnostics / desktop unlock.
pub fn model_slugs_from_settings(settings: &Value) -> Vec<String> {
    specs_from_settings(settings)
        .into_iter()
        .map(|s| s.model)
        .collect()
}

/// Read slugs from the live generated catalog file (if present).
pub fn model_slugs_from_catalog_file(codex_home: &Path) -> Vec<String> {
    let path = catalog_path(codex_home);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    if let Some(models) = value.get("models").and_then(|m| m.as_array()) {
        for m in models {
            let slug = m
                .get("slug")
                .or_else(|| m.get("model"))
                .or_else(|| m.get("id"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let Some(slug) = slug else { continue };
            if seen.insert(slug.to_string()) {
                out.push(slug.to_string());
            }
        }
    }
    out
}

/// Ensure `model` appears in specs (merge default into catalog list).
/// - Empty specs → seed one row
/// - Non-empty but missing → **prepend** so default stays first
pub fn ensure_model_in_specs(specs: &mut Vec<CatalogModelSpec>, model: &str) {
    let model = model.trim();
    if model.is_empty() {
        return;
    }
    if specs.iter().any(|s| s.model == model) {
        return;
    }
    specs.insert(
        0,
        CatalogModelSpec {
            model: model.to_string(),
            display_name: model.to_string(),
            context_window: None,
        },
    );
}

/// Merge default `model` (and optional extra ids) into settings.modelCatalog SSOT.
/// Returns true when settings were mutated.
pub fn merge_models_into_settings(
    settings: &mut Value,
    models: impl IntoIterator<Item = String>,
) -> bool {
    let mut specs = specs_from_settings(settings);
    let before_ids: Vec<String> = specs.iter().map(|s| s.model.clone()).collect();
    for m in models {
        ensure_model_in_specs(&mut specs, &m);
    }
    if specs.is_empty() {
        return false;
    }
    let after_ids: Vec<String> = specs.iter().map(|s| s.model.clone()).collect();
    let had_catalog = settings.get("modelCatalog").is_some();
    if had_catalog && before_ids == after_ids {
        return false;
    }
    let rows: Vec<Value> = specs
        .iter()
        .map(|s| {
            let mut obj = serde_json::Map::new();
            obj.insert("model".into(), json!(s.model));
            obj.insert("displayName".into(), json!(s.display_name));
            if let Some(cw) = s.context_window {
                obj.insert("contextWindow".into(), json!(cw));
            }
            Value::Object(obj)
        })
        .collect();
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("modelCatalog".into(), json!({ "models": rows }));
        return true;
    }
    false
}

/// Collect every model id that must appear in the projected catalog:
/// settings.modelCatalog ∪ default_model ∪ top-level config model.
pub fn collect_projection_specs(
    settings: &Value,
    config_text: &str,
    default_model: Option<&str>,
) -> Vec<CatalogModelSpec> {
    let mut specs = specs_from_settings(settings);
    if let Some(m) = default_model {
        ensure_model_in_specs(&mut specs, m);
    }
    if let Some(m) = extract_top_level_model(config_text) {
        ensure_model_in_specs(&mut specs, &m);
    }
    specs
}

fn load_static_template() -> Value {
    serde_json::from_str(NATIVE_TEMPLATE)
        .expect("bundled codex native responses template must be valid JSON")
}

/// Prefer the **exact slug** from `models_cache.json` (native
/// metadata), else a generic GPT-like cache entry, else the bundled template.
/// `is_native` is true when the entry came from a slug-matched cache row — those
/// keep native tool fields; pure third-party clones strip freeform tools.
fn load_template_for_slug(codex_home: Option<&Path>, slug: &str) -> (Value, bool) {
    if let Some(home) = codex_home {
        if let Some(exact) = load_models_cache_entry(home, Some(slug)) {
            let mut t = exact;
            fill_required_fields_from_static(&mut t);
            ensure_list_visibility(&mut t);
            return (t, true);
        }
        if let Some(generic) = load_models_cache_entry(home, None) {
            let mut t = generic;
            fill_required_fields_from_static(&mut t);
            strip_native_freeform_tools(&mut t);
            ensure_list_visibility(&mut t);
            return (t, false);
        }
    }
    let mut t = load_static_template();
    fill_required_fields_from_static(&mut t);
    strip_native_freeform_tools(&mut t);
    ensure_list_visibility(&mut t);
    (t, false)
}

fn ensure_list_visibility(template: &mut Value) {
    let Some(obj) = template.as_object_mut() else {
        return;
    };
    if !obj.contains_key("shell_type") {
        obj.insert("shell_type".into(), json!("shell_command"));
    }
    if !obj.contains_key("visibility") {
        obj.insert("visibility".into(), json!("list"));
    }
    if !obj.contains_key("supported_in_api") {
        obj.insert("supported_in_api".into(), json!(true));
    }
}

/// `slug_filter = Some(id)` → exact models_cache match; `None` → first usable GPT-like row.
fn load_models_cache_entry(codex_home: &Path, slug_filter: Option<&str>) -> Option<Value> {
    let path = codex_home.join("models_cache.json");
    if !path.exists() {
        return None;
    }
    let text = fs::read_to_string(&path).ok()?;
    let catalog: Value = serde_json::from_str(&text).ok()?;
    let models = catalog.get("models")?.as_array()?;
    if let Some(want) = slug_filter.map(str::trim).filter(|s| !s.is_empty()) {
        return models
            .iter()
            .find(|m| m.get("slug").and_then(|v| v.as_str()) == Some(want))
            .cloned();
    }
    // Prefer a GPT-like entry with base_instructions; else first usable model.
    models
        .iter()
        .find(|m| {
            m.get("base_instructions")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .is_some_and(|s| !s.is_empty())
                && m.get("supports_reasoning_summaries").is_some()
        })
        .or_else(|| models.first())
        .cloned()
}

fn fill_required_fields_from_static(template: &mut Value) {
    let static_t = load_static_template();
    let (Some(obj), Some(static_obj)) = (template.as_object_mut(), static_t.as_object()) else {
        return;
    };
    for key in PARSER_REQUIRED_FIELDS {
        if !obj.contains_key(*key) {
            if let Some(v) = static_obj.get(*key) {
                obj.insert((*key).to_string(), v.clone());
            }
        }
    }
    // base_instructions must be non-empty string
    let bi_ok = obj
        .get("base_instructions")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if !bi_ok {
        if let Some(v) = static_obj.get("base_instructions") {
            obj.insert("base_instructions".into(), v.clone());
        }
    }
}

/// Native `/responses` third-party gateways reject freeform apply_patch tools.
fn strip_native_freeform_tools(template: &mut Value) {
    let Some(obj) = template.as_object_mut() else {
        return;
    };
    for key in [
        "apply_patch_tool_type",
        "web_search_tool_type",
        "tools",
        "model_messages",
    ] {
        obj.remove(key);
    }
    obj.insert("shell_type".into(), json!("shell_command"));
}

fn entry_from_spec(
    template: &Value,
    spec: &CatalogModelSpec,
    priority: usize,
    is_native: bool,
    fallback_window: u64,
) -> Value {
    let mut entry = template.clone();
    let Some(obj) = entry.as_object_mut() else {
        return json!({});
    };
    let metadata_window = obj.get("context_window").and_then(|v| v.as_u64());
    // Prefer explicit mapping → native cache → config/default fallback.
    let context_window = spec
        .context_window
        .or(metadata_window)
        .unwrap_or(fallback_window);

    obj.insert("slug".into(), json!(spec.model));
    // Always set a concrete display_name for third-party slugs so desktop inject
    // and CLI never fall back to a generic「自定义 / Custom」label.
    // Native cache rows keep official display only when user did not customize.
    let custom_display = spec.display_name != spec.model;
    if !is_native || custom_display {
        let dn = if spec.display_name.trim().is_empty() {
            spec.model.as_str()
        } else {
            spec.display_name.as_str()
        };
        obj.insert("display_name".into(), json!(dn));
        obj.insert("description".into(), json!(dn));
    } else if obj
        .get("display_name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        obj.insert("display_name".into(), json!(spec.model));
        obj.insert("description".into(), json!(spec.model));
    }
    obj.insert("context_window".into(), json!(context_window));
    obj.insert("max_context_window".into(), json!(context_window));
    // 100 so UI shows the real window (default 95 shrinks 1M → 950K).
    obj.insert("effective_context_window_percent".into(), json!(100));
    obj.insert("auto_compact_token_limit".into(), Value::Null);
    obj.insert("priority".into(), json!(1000 + priority));
    obj.insert("visibility".into(), json!("list"));
    obj.insert("supported_in_api".into(), json!(true));
    if !is_native {
        // Keep list clean for pure third-party models
        obj.insert("additional_speed_tiers".into(), json!([]));
        obj.insert("service_tiers".into(), json!([]));
    }
    obj.insert("availability_nux".into(), Value::Null);
    obj.insert("upgrade".into(), Value::Null);
    // Fail-open image input for unknown third-party models
    if !obj.contains_key("input_modalities") {
        obj.insert("input_modalities".into(), json!(["text", "image"]));
    }
    // Re-assert required fields after clone mutations
    if obj
        .get("base_instructions")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        obj.insert(
            "base_instructions".into(),
            json!("You are Codex, a coding agent. You and the user share the same workspace and collaborate to achieve the user's goals."),
        );
    }
    if obj.get("supports_reasoning_summaries").is_none() {
        obj.insert("supports_reasoning_summaries".into(), json!(true));
    }
    entry
}

fn build_catalog_json_with_home(
    specs: &[CatalogModelSpec],
    codex_home: Option<&Path>,
    fallback_window: u64,
) -> Value {
    let fallback = if fallback_window > 0 {
        fallback_window
    } else {
        272_000
    };
    let models: Vec<Value> = specs
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (template, is_native) = load_template_for_slug(codex_home, &s.model);
            entry_from_spec(&template, s, i, is_native, fallback)
        })
        .collect();
    json!({ "models": models })
}

fn set_model_catalog_json_field(config_text: &str, enable: bool) -> Result<String, String> {
    // Comment-only / empty configs still need a valid TOML document.
    let text = if config_text.trim().is_empty() {
        ""
    } else {
        config_text
    };
    let mut doc = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>()
            .map_err(|e| format!("Invalid Codex config.toml: {e}"))?
    };
    if enable {
        doc["model_catalog_json"] = toml_edit::value(CATALOG_FILENAME);
    } else {
        let should_remove = doc
            .get("model_catalog_json")
            .and_then(|i| i.as_str())
            .map(|path| {
                Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    == Some(CATALOG_FILENAME)
            })
            .unwrap_or(false);
        if should_remove {
            doc.as_table_mut().remove("model_catalog_json");
        }
    }
    Ok(doc.to_string())
}

/// Ensure top-level `model = "…"` matches the first catalog entry when missing.
pub fn ensure_config_model_from_catalog(
    config_text: &str,
    settings: &Value,
) -> Result<String, String> {
    if extract_top_level_model(config_text).is_some() {
        return Ok(config_text.to_string());
    }
    let Some(model) = first_catalog_model(settings) else {
        return Ok(config_text.to_string());
    };
    let mut doc = if config_text.trim().is_empty() {
        DocumentMut::new()
    } else {
        config_text
            .parse::<DocumentMut>()
            .map_err(|e| format!("Invalid Codex config.toml: {e}"))?
    };
    doc["model"] = toml_edit::value(model.as_str());
    Ok(doc.to_string())
}

/// Project catalog file + inject/remove `model_catalog_json` in config text.
/// Returns updated config text.
///
/// Important (Codex catalog semantics): once `model_catalog_json` is set,
/// Codex uses that file as the **complete** model list. Every selectable model
/// (DeepSeek / Claude / Gemini / Grok / …) must be present — not only the
/// default `model =` line.
pub fn prepare_config_with_catalog(
    codex_home: &Path,
    settings: &Value,
    config_text: &str,
    default_model: Option<&str>,
) -> Result<String, String> {
    let default_cw = default_context_window(config_text);
    let specs = collect_projection_specs(settings, config_text, default_model);

    // Align top-level model with first mapped model when unset.
    // Prefer an explicit default_model when provided.
    let mut config_text = config_text.to_string();
    if extract_top_level_model(&config_text).is_none() {
        if let Some(m) = default_model
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| specs.first().map(|s| s.model.clone()))
        {
            let mut doc = if config_text.trim().is_empty() {
                DocumentMut::new()
            } else {
                config_text
                    .parse::<DocumentMut>()
                    .map_err(|e| format!("Invalid Codex config.toml: {e}"))?
            };
            doc["model"] = toml_edit::value(m.as_str());
            config_text = doc.to_string();
        }
    } else {
        // Still run the catalog-based fill when model empty in edge cases.
        config_text = ensure_config_model_from_catalog(&config_text, settings)?;
    }

    if specs.is_empty() {
        // Remove our pointer only; leave user-owned catalog paths alone.
        return set_model_catalog_json_field(&config_text, false);
    }

    let catalog = build_catalog_json_with_home(&specs, Some(codex_home), default_cw);
    // Sanity: every entry must carry required fields + list visibility
    if let Some(models) = catalog.get("models").and_then(|v| v.as_array()) {
        for (i, m) in models.iter().enumerate() {
            let bi = m
                .get("base_instructions")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or("");
            if bi.is_empty() {
                return Err(format!(
                    "生成的 model catalog 第 {i} 项缺少 base_instructions（Codex 无法加载）"
                ));
            }
            if m.get("supports_reasoning_summaries").is_none() {
                return Err(format!(
                    "生成的 model catalog 第 {i} 项缺少 supports_reasoning_summaries（Codex 无法加载）"
                ));
            }
            let vis = m.get("visibility").and_then(|v| v.as_str()).unwrap_or("");
            if vis != "list" {
                return Err(format!(
                    "生成的 model catalog 第 {i} 项 visibility 必须为 list（当前 {vis:?}）"
                ));
            }
        }
    }

    let path = catalog_path(codex_home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 ~/.codex 失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&catalog)
        .map_err(|e| format!("序列化 model catalog 失败: {e}"))?;
    // Atomic-ish write
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &text).map_err(|e| format!("写入 model catalog 临时文件失败: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("替换 model catalog 失败: {e}")
    })?;

    set_model_catalog_json_field(&config_text, true)
}

/// OpenAI-compatible `/v1/models` body built from the live catalog file.
/// Used by local routing so desktop/CLI can list third-party slugs over HTTP.
pub fn openai_models_list_from_catalog(codex_home: &Path) -> Value {
    let slugs = model_slugs_from_catalog_file(codex_home);
    let data: Vec<Value> = slugs
        .into_iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "owned_by": "chatgpt-tools",
            })
        })
        .collect();
    json!({
        "object": "list",
        "data": data,
    })
}

fn extract_top_level_model(config_text: &str) -> Option<String> {
    if config_text.trim().is_empty() {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    doc.get("model")
        .and_then(|i| i.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Normalize frontend catalog rows into settings shape.
pub fn model_catalog_value_from_rows(rows: &[Value]) -> Option<Value> {
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let model = row
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let Some(model) = model else { continue };
        if !seen.insert(model.clone()) {
            continue;
        }
        let display_name = row
            .get("displayName")
            .or_else(|| row.get("display_name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| model.clone());
        let context_window = parse_positive_u64(
            row.get("contextWindow")
                .or_else(|| row.get("context_window")),
        );
        let mut obj = serde_json::Map::new();
        obj.insert("model".into(), json!(model));
        obj.insert("displayName".into(), json!(display_name));
        if let Some(cw) = context_window {
            obj.insert("contextWindow".into(), json!(cw));
        }
        models.push(Value::Object(obj));
    }
    if models.is_empty() {
        None
    } else {
        Some(json!({ "models": models }))
    }
}
