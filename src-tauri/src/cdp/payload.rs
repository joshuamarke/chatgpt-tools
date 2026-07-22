//! Skin payload builder: fingerprint, staged shell/art/delta assembly.
//! Port of `engine/payload.mjs` (protocol 2).

use super::image::{
    assert_art_bytes, detect_mime_from_bytes, read_image_metadata, ImageMetadata,
    RECOMMENDED_ART_BYTES,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use thiserror::Error;

const PLACEHOLDERS: &[&str] = &[
    "__SKIN_CSS_JSON__",
    "__SKIN_ART_JSON__",
    "__SKIN_THEME_JSON__",
    "__SKIN_MARKERS_JSON__",
    "__SKIN_PLUGIN_JSON__",
    "__SKIN_REVISION_JSON__",
];

static PAYLOAD_CACHE: OnceLock<Mutex<HashMap<String, StagedPayload>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, StagedPayload>> {
    PAYLOAD_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Error)]
pub enum PayloadError {
    #[error("{0}")]
    Message(String),
}

impl PayloadError {
    fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

#[derive(Debug, Clone)]
pub struct SkinBundle {
    pub manifest: Value,
    pub markers: Value,
    pub theme: Value,
    pub plugin: Value,
    pub css: String,
    pub core_template: String,
    pub art_bytes: Vec<u8>,
    pub mime: String,
    pub art_metadata: ImageMetadata,
    pub revision: String,
    pub core_revision: String,
    pub fingerprint: String,
    pub recommended: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields mirror Node staged payload for diagnostics / future watch
pub struct StagedPayload {
    pub shell_payload: String,
    pub delta_shell_payload: String,
    pub art_payload: String,
    pub art_data_url: String,
    pub fingerprint: String,
    pub revision: String,
    pub core_revision: String,
    pub markers: Value,
    pub theme: Value,
    pub manifest: Value,
    pub art_metadata: ImageMetadata,
    pub shell_bytes: usize,
    pub delta_shell_bytes: usize,
    pub art_payload_bytes: usize,
    pub art_bytes: usize,
    pub total_bytes: usize,
    pub recommended: bool,
    pub deferred_art: bool,
    pub supports_delta: bool,
    pub cache_hit: bool,
}

fn sha_hex_prefix(data: impl AsRef<[u8]>, n: usize) -> String {
    let mut h = Sha256::new();
    h.update(data.as_ref());
    let full = hex::encode(h.finalize());
    full.chars().take(n).collect()
}

fn sha_hex_multi(parts: &[&[u8]], n: usize) -> String {
    let mut h = Sha256::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            h.update(b"\0");
        }
        h.update(p);
    }
    let full = hex::encode(h.finalize());
    full.chars().take(n).collect()
}

fn normalized_choice(
    value: Option<&Value>,
    field: &str,
    allowed: &[&str],
    fallback: &str,
) -> Result<String, PayloadError> {
    match value {
        None | Some(Value::Null) => Ok(fallback.into()),
        Some(v) => {
            let text = v
                .as_str()
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| v.to_string());
            if text.is_empty() {
                return Ok(fallback.into());
            }
            if allowed.contains(&text.as_str()) {
                Ok(text)
            } else {
                Err(PayloadError::msg(format!(
                    "{field} must be one of {}",
                    allowed.join("|")
                )))
            }
        }
    }
}

fn normalized_unit(value: Option<&Value>, field: &str) -> Result<Option<f64>, PayloadError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            if let Some(s) = v.as_str() {
                if s.trim().is_empty() {
                    return Ok(None);
                }
            }
            let number = v
                .as_f64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .ok_or_else(|| PayloadError::msg(format!("{field} must be a number in 0..1")))?;
            if !(0.0..=1.0).contains(&number) {
                return Err(PayloadError::msg(format!("{field} must be a number in 0..1")));
            }
            Ok(Some(number))
        }
    }
}

pub fn normalize_theme_config(manifest: &Value) -> Result<Value, PayloadError> {
    let art = manifest.get("art").cloned().unwrap_or(json!({}));
    let theme_block = manifest.get("theme").cloned().unwrap_or(json!({}));
    let theme_art = theme_block.get("art").cloned().unwrap_or(json!({}));
    let mut merged_art = art.as_object().cloned().unwrap_or_default();
    if let Some(obj) = theme_art.as_object() {
        for (k, v) in obj {
            merged_art.insert(k.clone(), v.clone());
        }
    }
    let appearance = normalized_choice(
        theme_block
            .get("appearance")
            .or_else(|| manifest.get("appearance")),
        "appearance",
        &["auto", "light", "dark"],
        "auto",
    )?;
    let focus_x = normalized_unit(merged_art.get("focusX"), "art.focusX")?;
    let focus_y = normalized_unit(merged_art.get("focusY"), "art.focusY")?;
    let safe_area = normalized_choice(
        merged_art.get("safeArea"),
        "art.safeArea",
        &["auto", "left", "right", "center", "none"],
        "auto",
    )?;
    let task_mode = normalized_choice(
        merged_art.get("taskMode"),
        "art.taskMode",
        &["auto", "ambient", "banner", "off"],
        "auto",
    )?;
    let skip = theme_block
        .get("skipAnalysis")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || manifest
            .get("skipAnalysis")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

    Ok(json!({
        "id": manifest.get("id").and_then(|v| v.as_str()).unwrap_or("custom"),
        "name": manifest.get("name").and_then(|v| v.as_str())
            .or_else(|| manifest.get("id").and_then(|v| v.as_str()))
            .unwrap_or("Skin"),
        "version": manifest.get("version").map(|v| v.as_str().unwrap_or("2.0.0").to_string()).unwrap_or_else(|| "2.0.0".into()),
        "appearance": appearance,
        "accent": manifest.get("accent").or_else(|| theme_block.get("accent")).cloned().unwrap_or(Value::Null),
        "palette": theme_block.get("palette").cloned().unwrap_or(json!({})),
        "art": {
            "focusX": focus_x,
            "focusY": focus_y,
            "safeArea": safe_area,
            "taskMode": task_mode,
        },
        "skipAnalysis": skip,
    }))
}

pub fn normalize_markers(manifest: &Value) -> Result<Value, PayloadError> {
    let m = manifest
        .get("markers")
        .cloned()
        .unwrap_or(json!({}));
    let root = m
        .get("rootClass")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PayloadError::msg("skin.json markers require rootClass, styleId, stateKey"))?;
    let style_id = m
        .get("styleId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PayloadError::msg("skin.json markers require rootClass, styleId, stateKey"))?;
    let state_key = m
        .get("stateKey")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PayloadError::msg("skin.json markers require rootClass, styleId, stateKey"))?;
    let home_class = m
        .get("homeClass")
        .and_then(|v| v.as_str())
        .unwrap_or("skin-home");
    let disabled = m
        .get("disabledKey")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| state_key.replacen("_STATE__", "_DISABLED__", 1));
    let chrome = m
        .get("chromeId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| style_id.replacen("-style", "-chrome", 1));
    Ok(json!({
        "id": manifest.get("id").and_then(|v| v.as_str()).unwrap_or("custom"),
        "rootClass": root,
        "homeClass": home_class,
        "homeShellClass": m.get("homeShellClass").and_then(|v| v.as_str()).unwrap_or(&format!("{home_class}-shell")),
        "homeUtilityClass": m.get("homeUtilityClass").and_then(|v| v.as_str()).unwrap_or(&format!("{home_class}-utility")),
        "styleId": style_id,
        "chromeId": chrome,
        "stateKey": state_key,
        "disabledKey": disabled,
        "artVar": m.get("artVar").and_then(|v| v.as_str()).unwrap_or("--skin-art"),
    }))
}

fn load_plugin(skin_dir: &Path, manifest: &Value) -> Result<Value, PayloadError> {
    let plugin_rel = manifest
        .pointer("/assets/plugin")
        .and_then(|v| v.as_str())
        .unwrap_or("assets/plugin.json");
    let plugin_path = skin_dir.join(plugin_rel);
    let text = std::fs::read_to_string(&plugin_path).map_err(|_| {
        PayloadError::msg(format!(
            "skin requires {plugin_rel} (shared runtime; per-skin inject is no longer supported)"
        ))
    })?;
    let json: Value = serde_json::from_str(&text)
        .map_err(|e| PayloadError::msg(format!("Invalid plugin.json: {e}")))?;
    if !json.get("chromeHtml").and_then(|v| v.as_str()).is_some() {
        return Err(PayloadError::msg(
            "plugin.json requires string field chromeHtml",
        ));
    }
    Ok(json!({
        "version": json.get("version").map(|v| v.as_str().unwrap_or("2.0.0").to_string())
            .or_else(|| manifest.get("version").map(|v| v.as_str().unwrap_or("2.0.0").to_string()))
            .unwrap_or_else(|| "2.0.0".into()),
        "chromeHtml": json.get("chromeHtml").and_then(|v| v.as_str()).unwrap_or(""),
        "skipAnalysis": json.get("skipAnalysis").and_then(|v| v.as_bool()).unwrap_or(false),
        "labels": json.get("labels").cloned().unwrap_or(json!({})),
    }))
}

pub fn assemble_payload(
    core_template: &str,
    replacements: &HashMap<&str, String>,
) -> Result<String, PayloadError> {
    let mut payload = core_template.to_string();
    for (token, value) in replacements {
        if !payload.contains(token) {
            return Err(PayloadError::msg(format!(
                "renderer-core missing placeholder {token}"
            )));
        }
        payload = payload.replace(token, value);
    }
    for token in PLACEHOLDERS {
        if payload.contains(token) {
            return Err(PayloadError::msg(format!(
                "payload still contains unresolved placeholder {token}"
            )));
        }
    }
    Ok(payload)
}

/// Lightweight JS sanity: balanced braces / no empty script.
pub fn assert_payload_syntax(payload: &str) -> Result<(), PayloadError> {
    if payload.trim().is_empty() {
        return Err(PayloadError::msg(
            "assembled payload is not valid JavaScript: empty",
        ));
    }
    let mut depth = 0i32;
    let mut in_str: Option<char> = None;
    let mut escape = false;
    for c in payload.chars() {
        if let Some(q) = in_str {
            if escape {
                escape = false;
                continue;
            }
            if c == '\\' {
                escape = true;
                continue;
            }
            if c == q {
                in_str = None;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => in_str = Some(c),
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return Err(PayloadError::msg(
                        "assembled payload is not valid JavaScript: unbalanced brackets",
                    ));
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(PayloadError::msg(
            "assembled payload is not valid JavaScript: unbalanced brackets",
        ));
    }
    Ok(())
}

pub fn art_data_url(mime: &str, art_bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", B64.encode(art_bytes))
}

pub fn assemble_art_payload(markers: &Value, art_data_url: &str, revision: &str) -> String {
    let state_key = markers
        .get("stateKey")
        .and_then(|v| v.as_str())
        .unwrap_or("__CODEX_SKIN_STATE__");
    let disabled_key = markers
        .get("disabledKey")
        .and_then(|v| v.as_str())
        .unwrap_or("__CODEX_SKIN_DISABLED__");
    format!(
        r#"(() => {{
  const hostKey = "__CHATGPT_TOOLS_SKIN_HOST__";
  const stateKey = {state_key};
  const disabledKey = {disabled_key};
  const revision = {revision};
  const artDataUrl = {art};
  if (window[disabledKey]) return {{ ok: false, reason: "disabled", revision }};
  const host = window[hostKey];
  if (host && typeof host.applyArt === "function") {{
    return host.applyArt(artDataUrl, revision);
  }}
  const state = window[stateKey];
  if (!state) return {{ ok: false, reason: "no-state", revision }};
  if (state.revision != null && revision != null && state.revision !== revision) {{
    return {{ ok: false, reason: "revision-mismatch", stateRevision: state.revision, revision }};
  }}
  if (typeof state.applyArt === "function") {{
    return state.applyArt(artDataUrl, revision);
  }}
  return {{ ok: false, reason: "no-applyArt", revision }};
}})()"#,
        state_key = serde_json::to_string(state_key).unwrap(),
        disabled_key = serde_json::to_string(disabled_key).unwrap(),
        revision = serde_json::to_string(revision).unwrap(),
        art = serde_json::to_string(art_data_url).unwrap(),
    )
}

pub fn assemble_delta_shell_payload(
    css: &str,
    markers: &Value,
    theme: &Value,
    plugin: &Value,
    revision: &str,
) -> String {
    format!(
        r#"(() => {{
  const hostKey = "__CHATGPT_TOOLS_SKIN_HOST__";
  const host = window[hostKey];
  const delta = {{
    css: {css},
    markers: {markers},
    theme: {theme},
    plugin: {plugin},
    revision: {revision},
  }};
  if (!host || typeof host.applySkin !== "function") {{
    return {{ ok: false, reason: "no-host", needsFullShell: true, revision: delta.revision }};
  }}
  try {{
    return host.applySkin(delta);
  }} catch (error) {{
    return {{
      ok: false,
      reason: "delta-throw",
      message: String(error && error.message ? error.message : error),
      needsFullShell: true,
      revision: delta.revision,
    }};
  }}
}})()"#,
        css = serde_json::to_string(css).unwrap(),
        markers = serde_json::to_string(markers).unwrap(),
        theme = serde_json::to_string(theme).unwrap(),
        plugin = serde_json::to_string(plugin).unwrap(),
        revision = serde_json::to_string(revision).unwrap(),
    )
}

pub fn core_template_path(project_root: &Path) -> PathBuf {
    project_root
        .join("engine")
        .join("runtime")
        .join("renderer-core.js")
}

pub fn load_skin_bundle(skin_dir: &Path, project_root: &Path) -> Result<SkinBundle, PayloadError> {
    let manifest_path = skin_dir.join("skin.json");
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| PayloadError::msg(format!("read skin.json: {e}")))?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .map_err(|e| PayloadError::msg(format!("parse skin.json: {e}")))?;
    let assets = manifest
        .get("assets")
        .ok_or_else(|| PayloadError::msg("skin.json requires assets.css and assets.art"))?;
    let css_rel = assets
        .get("css")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PayloadError::msg("skin.json requires assets.css and assets.art"))?;
    let art_rel = assets
        .get("art")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PayloadError::msg("skin.json requires assets.css and assets.art"))?;
    if assets.get("plugin").and_then(|v| v.as_str()).is_none() {
        return Err(PayloadError::msg(
            "skin.json requires assets.plugin (shared renderer-core)",
        ));
    }

    let markers = normalize_markers(&manifest)?;
    let mut theme = normalize_theme_config(&manifest)?;
    let css_path = skin_dir.join(css_rel);
    let art_path = skin_dir.join(art_rel);
    let core_path = core_template_path(project_root);

    let css = std::fs::read_to_string(&css_path)
        .map_err(|e| PayloadError::msg(format!("read css: {e}")))?;
    let art_bytes =
        std::fs::read(&art_path).map_err(|e| PayloadError::msg(format!("read art: {e}")))?;
    let core_template = std::fs::read_to_string(&core_path).map_err(|e| {
        PayloadError::msg(format!(
            "read renderer-core.js at {}: {e}",
            core_path.display()
        ))
    })?;
    let plugin = load_plugin(skin_dir, &manifest)?;

    assert_art_bytes(
        art_bytes.len() as u64,
        &format!(
            "Art for {}",
            manifest
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("skin")
        ),
    )
    .map_err(|e| PayloadError::msg(e.to_string()))?;

    let extension = art_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let art_metadata = read_image_metadata(&art_bytes, &extension).ok_or_else(|| {
        PayloadError::msg(format!(
            "Art metadata is invalid or exceeds the 16384px / 50MP safety limit ({})",
            manifest
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or(art_path.to_string_lossy().as_ref())
        ))
    })?;

    let mime = detect_mime_from_bytes(&art_bytes, &extension);
    let art_key = sha_hex_prefix(&art_bytes, 20);
    let core_revision = sha_hex_prefix(core_template.as_bytes(), 16);
    let plugin_json = serde_json::to_string(&plugin).unwrap_or_else(|_| "{}".into());
    let revision = sha_hex_multi(
        &[
            manifest_text.as_bytes(),
            css.as_bytes(),
            &art_bytes,
            plugin_json.as_bytes(),
            core_revision.as_bytes(),
        ],
        24,
    );

    if let Some(obj) = theme.as_object_mut() {
        obj.insert("artKey".into(), json!(art_key));
        obj.insert(
            "artMetadata".into(),
            serde_json::to_value(&art_metadata).unwrap_or(Value::Null),
        );
        obj.insert(
            "version".into(),
            plugin
                .get("version")
                .cloned()
                .unwrap_or_else(|| json!("2.0.0")),
        );
        obj.insert("coreRevision".into(), json!(core_revision));
        if plugin
            .get("skipAnalysis")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            obj.insert("skipAnalysis".into(), json!(true));
        }
    }

    let recommended = (art_bytes.len() as u64) <= RECOMMENDED_ART_BYTES;
    Ok(SkinBundle {
        manifest,
        markers,
        theme,
        plugin,
        css,
        core_template,
        art_bytes,
        mime,
        art_metadata,
        revision: revision.clone(),
        core_revision,
        fingerprint: revision,
        recommended,
    })
}

pub fn build_staged_payload(
    skin_dir: &Path,
    project_root: &Path,
) -> Result<StagedPayload, PayloadError> {
    let bundle = load_skin_bundle(skin_dir, project_root)?;
    let cache_key = format!("staged:{}", bundle.fingerprint);
    {
        let guard = cache().lock();
        if let Some(hit) = guard.get(&cache_key) {
            let mut c = hit.clone();
            c.cache_hit = true;
            return Ok(c);
        }
    }

    let mut reps = HashMap::new();
    reps.insert(
        "__SKIN_CSS_JSON__",
        serde_json::to_string(&bundle.css).unwrap(),
    );
    reps.insert("__SKIN_ART_JSON__", serde_json::to_string("").unwrap());
    reps.insert(
        "__SKIN_THEME_JSON__",
        serde_json::to_string(&bundle.theme).unwrap(),
    );
    reps.insert(
        "__SKIN_MARKERS_JSON__",
        serde_json::to_string(&bundle.markers).unwrap(),
    );
    reps.insert(
        "__SKIN_PLUGIN_JSON__",
        serde_json::to_string(&bundle.plugin).unwrap(),
    );
    reps.insert(
        "__SKIN_REVISION_JSON__",
        serde_json::to_string(&bundle.revision).unwrap(),
    );

    let shell_payload = assemble_payload(&bundle.core_template, &reps)?;
    assert_payload_syntax(&shell_payload)?;

    let art_data_url = art_data_url(&bundle.mime, &bundle.art_bytes);
    let art_payload = assemble_art_payload(&bundle.markers, &art_data_url, &bundle.revision);
    assert_payload_syntax(&art_payload)?;

    let delta_shell_payload = assemble_delta_shell_payload(
        &bundle.css,
        &bundle.markers,
        &bundle.theme,
        &bundle.plugin,
        &bundle.revision,
    );
    assert_payload_syntax(&delta_shell_payload)?;

    let result = StagedPayload {
        shell_bytes: shell_payload.len(),
        delta_shell_bytes: delta_shell_payload.len(),
        art_payload_bytes: art_payload.len(),
        art_bytes: bundle.art_bytes.len(),
        total_bytes: shell_payload.len() + art_payload.len(),
        shell_payload,
        delta_shell_payload,
        art_payload,
        art_data_url,
        fingerprint: bundle.fingerprint,
        revision: bundle.revision,
        core_revision: bundle.core_revision,
        markers: bundle.markers,
        theme: bundle.theme,
        manifest: bundle.manifest,
        art_metadata: bundle.art_metadata,
        recommended: bundle.recommended,
        deferred_art: true,
        supports_delta: true,
        cache_hit: false,
    };

    {
        let mut guard = cache().lock();
        if guard.len() >= 8 {
            if let Some(k) = guard.keys().next().cloned() {
                guard.remove(&k);
            }
        }
        guard.insert(cache_key, result.clone());
    }
    Ok(result)
}

pub fn art_evaluate_timeout_ms(staged: &StagedPayload) -> u64 {
    const BASE: u64 = 25_000;
    const BYTES_PER_MS: u64 = 4_000;
    let bytes = staged.art_payload_bytes.max(staged.art_bytes) as u64;
    if bytes == 0 {
        return BASE;
    }
    (BASE.max(bytes / BYTES_PER_MS + 8_000)).min(120_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("parent")
            .to_path_buf()
    }

    #[test]
    fn assemble_replaces_all_placeholders() {
        let core = "/* __SKIN_CSS_JSON__ */ ((__SKIN_CSS_JSON__, __SKIN_ART_JSON__, __SKIN_THEME_JSON__, __SKIN_MARKERS_JSON__, __SKIN_PLUGIN_JSON__, __SKIN_REVISION_JSON__) => {})()";
        let mut reps = HashMap::new();
        reps.insert("__SKIN_CSS_JSON__", "\"css\"".into());
        reps.insert("__SKIN_ART_JSON__", "\"\"".into());
        reps.insert("__SKIN_THEME_JSON__", "{}".into());
        reps.insert("__SKIN_MARKERS_JSON__", "{}".into());
        reps.insert("__SKIN_PLUGIN_JSON__", "{}".into());
        reps.insert("__SKIN_REVISION_JSON__", "\"r1\"".into());
        let out = assemble_payload(core, &reps).expect("assemble");
        assert!(!out.contains("__SKIN_"));
        assert_payload_syntax(&out).expect("syntax");
    }

    #[test]
    fn build_staged_from_bundled_skin_if_present() {
        let root = repo_root();
        let skins = root.join("skins");
        if !skins.is_dir() {
            return;
        }
        let mut found = None;
        if let Ok(entries) = std::fs::read_dir(&skins) {
            for ent in entries.flatten() {
                let p = ent.path();
                if p.join("skin.json").is_file() {
                    found = Some(p);
                    break;
                }
            }
        }
        let Some(skin_dir) = found else {
            return;
        };
        let staged = build_staged_payload(&skin_dir, &root).expect("staged");
        assert!(!staged.shell_payload.is_empty());
        assert!(!staged.delta_shell_payload.is_empty());
        assert!(!staged.art_payload.is_empty());
        assert!(staged.shell_bytes > 1000);
        assert!(staged.supports_delta);
        assert_payload_syntax(&staged.shell_payload).expect("shell js");
        assert_payload_syntax(&staged.delta_shell_payload).expect("delta js");
    }
}
