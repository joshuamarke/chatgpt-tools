//! Codex desktop model whitelist unlock (CDP inject).
//!
//! Codex desktop gates the model picker with an in-app whitelist (Statsig
//! `available_models` + list-models-for-host). Non-whitelisted slugs
//! (DeepSeek / Claude / Gemini / Grok / …) hide or collapse to「自定义」.
//!
//! We inject a data-layer patch into the app:// renderer:
//! Response.json / Statsig / sendRequest / React fiber state.
//! (No DOM text rewriting — that mis-hits unrelated controls.)
//!
//! # When we inject
//! - **Third-party** live routing only (never OpenAI Official / official proxy).
//! - **Event-driven**: provider switch / host ready / catalog change wake the
//!   watcher. After a **healthy** inject the keep loop **parks** (no 8s hammer).
//! - Slow watchdog (~2 min) only to recover SPA remounts that dropped hooks.
//! - Unchanged lists + healthy page → zero CDP work.
//!
//! Requires Codex launched with remote debugging (ChatGPT Tools host port,
//! default 9335).

use serde_json::{json, Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::Duration;

use super::catalog;
use super::codex;
use super::store;

const UNLOCK_SCRIPT: &str = include_str!("resources/model_unlock.js");

/// Active retry while waiting for first healthy inject / port.
const KEEP_ACTIVE_SECS: u64 = 4;
/// After healthy: rare SPA-remount check only (not continuous inject).
const KEEP_STABLE_WATCHDOG_SECS: u64 = 120;
/// Port closed while desired: back off between looks.
const KEEP_PORT_WAIT_SECS: u64 = 8;

static KEEP_STARTED: AtomicBool = AtomicBool::new(false);
/// Third-party wants unlock available (catalog non-empty, not official).
/// Does **not** mean “poll every few seconds forever”.
static UNLOCK_DESIRED: AtomicBool = AtomicBool::new(false);
/// Page was verified healthy for current fingerprint — park the keep loop.
static UNLOCK_STABLE: AtomicBool = AtomicBool::new(false);
/// Fingerprint of last successful full inject (models + meta).
static LAST_INJECT_FP: AtomicU64 = AtomicU64::new(0);
/// Serialized last model list for diagnostics / JS compare.
static LAST_MODELS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Wake the keep thread early (provider switch / host ready).
static KEEP_WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

#[derive(Debug, Clone)]
pub struct UnlockResult {
    pub attempted: bool,
    pub ok: bool,
    pub models: Vec<String>,
    pub message: String,
    /// True when CDP was skipped because models were unchanged / not needed.
    pub skipped_unchanged: bool,
}

impl UnlockResult {
    pub fn skipped(msg: impl Into<String>) -> Self {
        Self {
            attempted: false,
            ok: false,
            models: Vec::new(),
            message: msg.into(),
            skipped_unchanged: false,
        }
    }

    pub fn skipped_unchanged(models: Vec<String>, msg: impl Into<String>) -> Self {
        Self {
            attempted: false,
            ok: true,
            models,
            message: msg.into(),
            skipped_unchanged: true,
        }
    }
}

fn shared_debug_port() -> u16 {
    std::env::var("CODEX_SKIN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|p: &u16| (1024..=65535).contains(p))
        .unwrap_or(crate::cdp::native::SHARED_PORT)
}

/// Candidate debug ports: configured host port first, then common fallbacks.
fn candidate_debug_ports() -> Vec<u16> {
    let primary = shared_debug_port();
    let mut ports = vec![primary];
    for p in [9335u16, 9222, 9229, 9333, 9334] {
        if !ports.contains(&p) {
            ports.push(p);
        }
    }
    ports
}

fn first_open_debug_port() -> Option<u16> {
    candidate_debug_ports()
        .into_iter()
        .find(|p| crate::cdp::http::is_debug_port_open(*p, 250))
}

/// Stable fingerprint of unlock payload (order-insensitive for models).
fn fingerprint(models: &[String], meta: &Map<String, Value>) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut sorted = models.to_vec();
    sorted.sort();
    sorted.dedup();
    for m in &sorted {
        m.hash(&mut hasher);
        if let Some(v) = meta.get(m) {
            if let Some(dn) = v.get("displayName").and_then(|x| x.as_str()) {
                dn.hash(&mut hasher);
            }
            if let Some(d) = v.get("description").and_then(|x| x.as_str()) {
                d.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

/// True when the **archive / UI** current Codex provider is OpenAI Official.
pub fn current_provider_is_official() -> bool {
    let Ok(file) = store::load() else {
        return false;
    };
    file.codex
        .providers
        .iter()
        .find(|p| p.id == file.codex.current)
        .map(|p| p.is_official())
        .unwrap_or(false)
}

/// Live routing that should **not** receive third-party model unlock.
///
/// Includes:
/// - empty / comment-only / built-in openai config ([`codex::is_official_live_config`])
/// - local-routing official shell `model_provider = chatgpt-tools-official`
/// - current archive provider marked `category = official`
pub fn should_skip_unlock_for_official() -> bool {
    if current_provider_is_official() {
        return true;
    }
    let cfg = codex::read_config_text().unwrap_or_default();
    if codex::is_official_live_config(&cfg) {
        return true;
    }
    // Proxy-mode official: OAuth passthrough table — still official models only.
    if live_model_provider(&cfg)
        .map(|id| id == crate::proxy::CODEX_OFFICIAL_PROXY_PROVIDER_ID)
        .unwrap_or(false)
    {
        return true;
    }
    false
}

fn live_model_provider(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<toml::Value>().ok()?;
    doc.get("model_provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Resolve model slugs to unlock: live catalog file first, then settings.
pub fn resolve_unlock_models(settings: Option<&Value>) -> Vec<String> {
    let home = codex::codex_home_dir();
    let mut models = catalog::model_slugs_from_catalog_file(&home);
    if models.is_empty() {
        if let Some(s) = settings {
            models = catalog::model_slugs_from_settings(s);
        }
    }
    if let Ok(cfg) = codex::read_config_text() {
        if let Some(m) = codex::extract_model(&cfg) {
            if !models.iter().any(|x| x == &m) {
                models.insert(0, m);
            }
        }
    }
    models
}

/// displayName map from live catalog + settings for inject meta.
pub fn resolve_unlock_meta(settings: Option<&Value>) -> Map<String, Value> {
    let mut meta = Map::new();
    let home = codex::codex_home_dir();
    let path = catalog::catalog_path(&home);
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(models) = v.get("models").and_then(|m| m.as_array()) {
                for m in models {
                    let slug = m
                        .get("slug")
                        .or_else(|| m.get("model"))
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let Some(slug) = slug else { continue };
                    let display = m
                        .get("display_name")
                        .or_else(|| m.get("displayName"))
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(slug);
                    let description = m
                        .get("description")
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(display);
                    meta.insert(
                        slug.to_string(),
                        json!({
                            "displayName": display,
                            "description": description,
                        }),
                    );
                }
            }
        }
    }
    if let Some(s) = settings {
        for spec in catalog::specs_from_settings(s) {
            meta.entry(spec.model.clone()).or_insert_with(|| {
                json!({
                    "displayName": spec.display_name,
                    "description": spec.display_name,
                })
            });
        }
    }
    meta
}

/// Build a self-contained evaluate expression with models + display meta.
pub fn build_evaluate_script(models: &[String], meta: &Map<String, Value>) -> String {
    let models_json = serde_json::to_string(models).unwrap_or_else(|_| "[]".into());
    let meta_json = serde_json::to_string(meta).unwrap_or_else(|_| "{}".into());
    format!(
        r#"(function(){{
  try {{
    window.__CGT_MODEL_UNLOCK_MODELS__ = {models_json};
    window.__CGT_MODEL_UNLOCK_META__ = {meta_json};
  }} catch (e) {{}}
  return ({body});
}})()"#,
        models_json = models_json,
        meta_json = meta_json,
        body = UNLOCK_SCRIPT.trim().trim_end_matches(';')
    )
}

/// Lightweight page probe: unlock installed + model list fingerprint match.
fn build_probe_script(fp: u64, models: &[String]) -> String {
    let models_json = serde_json::to_string(models).unwrap_or_else(|_| "[]".into());
    format!(
        r#"(function(){{
  var installed = window.__chatgptToolsModelUnlock === "1";
  var cleared = !!window.__cgtModelUnlockCleared;
  var ver = window.__cgtModelUnlockVersion || 0;
  var cur = Array.isArray(window.__cgtModelNames) ? window.__cgtModelNames.slice() : [];
  var want = {models_json};
  var hooksOk = typeof window.__cgtPatchJsonPayload === "function"
    && typeof window.__cgtModelBurstRefresh === "function"
    && !!window.__cgtModelPoll;
  function same(a, b) {{
    if (a.length !== b.length) return false;
    var as = a.slice().sort(), bs = b.slice().sort();
    for (var i = 0; i < as.length; i++) if (as[i] !== bs[i]) return false;
    return true;
  }}
  var modelsMatch = same(cur, want);
  return {{
    installed: installed && !cleared && ver > 0,
    version: ver,
    modelsMatch: modelsMatch,
    hooksOk: hooksOk,
    healthy: installed && !cleared && ver > 0 && modelsMatch && hooksOk && want.length > 0,
    fp: {fp},
    count: cur.length
  }};
}})()"#,
        models_json = models_json,
        fp = fp
    )
}

/// Clear third-party unlock hooks in the renderer (official switch / disarm).
///
/// Important: fully drop the install flag so the next third-party inject does a
/// **full** rebind (early-return path would skip reinstalling hooks).
fn build_clear_script() -> String {
    r#"(function(){
  try {
    window.__CGT_MODEL_UNLOCK_MODELS__ = [];
    window.__CGT_MODEL_UNLOCK_META__ = {};
    window.__cgtModelNames = [];
    window.__cgtModelMeta = {};
    if (window.__cgtModelMo && typeof window.__cgtModelMo.disconnect === "function") {
      try { window.__cgtModelMo.disconnect(); } catch (e) {}
    }
    window.__cgtModelMo = null;
    window.__cgtModelMoVersion = 0;
    if (window.__cgtModelPoll) {
      try { clearInterval(window.__cgtModelPoll); } catch (e) {}
    }
    window.__cgtModelPoll = 0;
    window.__cgtModelPollVersion = 0;
    if (window.__cgtModelBurstTimer) {
      try { clearTimeout(window.__cgtModelBurstTimer); } catch (e) {}
    }
    window.__cgtModelBurstTimer = 0;
    window.__cgtModelBurstUntil = 0;
    // Drop install marker — next inject must full-install hooks again.
    try { delete window.__chatgptToolsModelUnlock; } catch (e) {
      window.__chatgptToolsModelUnlock = null;
    }
    window.__cgtModelUnlockVersion = 0;
    window.__cgtModelUnlockCleared = true;
    // Empty live patch entry points (Response.json wrapper may remain as no-op).
    window.__cgtPatchJsonPayload = function (p) { return p; };
    window.__cgtPatchModelContainer = function () { return false; };
    window.__cgtPatchModelArray = function () { return false; };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
  return { ok: true, cleared: true, models: 0 };
})()"#
    .to_string()
}

/// Inject unlock into all app:// Codex targets on the shared debug port.
///
/// Skips when official; skips full CDP when model list fingerprint is unchanged
/// **and** the page still has a matching install (keep path). Forced paths from
/// provider switch still inject when fingerprint changes.
///
/// **Call sites that touch the GUI command thread** (provider switch / write_live)
/// should use [`schedule_desktop_unlock`] instead — CDP open/evaluate can take
/// several seconds and would freeze the UI if run inline.
pub fn try_inject_desktop_unlock(settings: Option<&Value>) -> UnlockResult {
    try_inject_desktop_unlock_inner(settings, InjectMode::Normal)
}

/// Fire-and-forget desktop unlock after a provider write (non-blocking for GUI).
///
/// Cheap work (desired flag + keep-thread arm) runs immediately on the caller
/// thread; full CDP probe/inject runs on a background worker.
pub fn schedule_desktop_unlock(settings: Option<Value>) {
    if should_skip_unlock_for_official() {
        schedule_official_activated();
        return;
    }
    // Arm keep loop immediately so host-ready path still works if Codex is down.
    let models = resolve_unlock_models(settings.as_ref());
    if models.is_empty() {
        clear_unlock_desired();
        // Still re-gate toolbox enhancements (force-chinese / plugin unlock).
        schedule_toolbox_provider_changed();
        return;
    }
    mark_unlock_desired_active();
    schedule_toolbox_provider_changed();
    let _ = thread::Builder::new()
        .name("cgt-model-unlock".into())
        .spawn(move || {
            let _ = try_inject_desktop_unlock_inner(settings.as_ref(), InjectMode::Normal);
        });
}

/// Background: clear third-party unlock hooks + re-gate toolbox after Official.
pub fn schedule_official_activated() {
    clear_unlock_desired();
    let _ = thread::Builder::new()
        .name("cgt-official-clear".into())
        .spawn(|| {
            let _ = on_official_activated();
        });
}

fn schedule_toolbox_provider_changed() {
    let _ = thread::Builder::new()
        .name("cgt-toolbox-gate".into())
        .spawn(|| {
            crate::toolbox::on_provider_changed();
        });
}

/// Same as [`try_inject_desktop_unlock`] but used by the keep loop (probe-first).
fn try_inject_desktop_unlock_inner(settings: Option<&Value>, mode: InjectMode) -> UnlockResult {
    if should_skip_unlock_for_official() {
        clear_unlock_desired();
        // Best-effort: clear any previous third-party hooks so official is not mis-hit.
        if matches!(mode, InjectMode::Normal) {
            let _ = clear_on_open_ports();
        }
        return UnlockResult::skipped(String::new());
    }

    let models = resolve_unlock_models(settings);
    if models.is_empty() {
        clear_unlock_desired();
        return UnlockResult::skipped(
            "无可用模型列表（请在供应商「模型映射」中添加 DeepSeek / Claude / Grok 等，并重新启用）",
        );
    }
    let meta = resolve_unlock_meta(settings);
    let fp = fingerprint(&models, &meta);

    // Desire unlock (third-party + catalog). Normal/Force wake active watching;
    // Keep mode only ensures the thread exists without clearing STABLE.
    match mode {
        InjectMode::Normal | InjectMode::Force => {
            mark_unlock_desired_active();
        }
        InjectMode::Keep => {
            ensure_desired_flag_only();
        }
    }

    let Some(port) = first_open_debug_port() else {
        let prefer = shared_debug_port();
        // Not stable until a page is actually healthy.
        UNLOCK_STABLE.store(false, Ordering::SeqCst);
        return UnlockResult {
            attempted: false,
            ok: false,
            models: models.clone(),
            skipped_unchanged: false,
            message: format!(
                "【桌面白名单待注入】未检测到 Codex 调试端口（优先 :{prefer}）。\
已排队：用本工具启动 Codex 后会自动注入（{} 个模型：{}）。CLI 不受此限制。",
                models.len(),
                preview_models(&models, 6)
            ),
        };
    };

    // Skip full CDP only when page is healthy with the same model list.
    if matches!(mode, InjectMode::Normal | InjectMode::Keep | InjectMode::Force) {
        match probe_page_state(port, fp, &models) {
            Ok(true) => {
                enter_stable(fp, &models);
                return UnlockResult::skipped_unchanged(models.clone(), String::new());
            }
            Ok(false) | Err(_) => {
                UNLOCK_STABLE.store(false, Ordering::SeqCst);
            }
        }
    }

    match inject_on_port(port, &models, &meta) {
        Ok(detail) => {
            enter_stable(fp, &models);
            // Silent success — GUI stays undisturbed; detail kept for logs/IPC only if needed.
            let _ = detail;
            UnlockResult {
                attempted: true,
                ok: true,
                models: models.clone(),
                skipped_unchanged: false,
                message: String::new(),
            }
        }
        Err(e) => {
            // Stay desired + active so keep retries until healthy.
            UNLOCK_STABLE.store(false, Ordering::SeqCst);
            ensure_desired_flag_only();
            wake_keep_thread();
            UnlockResult {
                attempted: true,
                ok: false,
                models: models.clone(),
                skipped_unchanged: false,
                message: format!(
                    "桌面白名单注入失败（{}），将在 Codex 就绪后自动重试。详情: {e}",
                    preview_models(&models, 4)
                ),
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum InjectMode {
    /// Provider switch / reapply / manual refresh — wake active watching.
    Normal,
    /// Background keep: probe-first; parks after healthy.
    Keep,
    /// Host just became ready — same as Normal (force active until healthy).
    Force,
}

fn enter_stable(fp: u64, models: &[String]) {
    LAST_INJECT_FP.store(fp, Ordering::SeqCst);
    if let Ok(mut g) = LAST_MODELS.lock() {
        *g = models.to_vec();
    }
    UNLOCK_DESIRED.store(true, Ordering::SeqCst);
    UNLOCK_STABLE.store(true, Ordering::SeqCst);
    // Thread may be sleeping in long watchdog — no need to wake.
}

fn preview_models(models: &[String], n: usize) -> String {
    let head: Vec<&str> = models.iter().take(n).map(|s| s.as_str()).collect();
    if models.len() > n {
        format!("{}…", head.join(", "))
    } else {
        head.join(", ")
    }
}

/// Returns Ok(true) if every app target has a **healthy** unlock (installed,
/// not cleared, hooks alive, model list matches).
fn probe_page_state(port: u16, fp: u64, models: &[String]) -> Result<bool, String> {
    let targets = crate::cdp::http::list_app_targets(port).map_err(|e| e.to_string())?;
    if targets.is_empty() {
        return Ok(false);
    }
    let script = build_probe_script(fp, models);
    let mut all_ok = true;
    let mut any = false;
    for target in &targets {
        let session = crate::cdp::session::CdpSession::open(target, port, 4000)
            .map_err(|e| e.to_string())?;
        match session.evaluate(&script, 8_000) {
            Ok(v) => {
                any = true;
                let healthy = v
                    .get("healthy")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                if !healthy {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
        session.close();
    }
    Ok(any && all_ok)
}

fn inject_on_port(
    port: u16,
    models: &[String],
    meta: &Map<String, Value>,
) -> Result<String, String> {
    let targets = crate::cdp::http::list_app_targets(port).map_err(|e| e.to_string())?;
    if targets.is_empty() {
        return Err("无 app:// 页面目标（Codex 可能仍在加载）".into());
    }

    let script = build_evaluate_script(models, meta);
    let mut ok_count = 0u32;
    let mut last_err = String::new();
    let mut last_result = Value::Null;

    for target in &targets {
        match crate::cdp::session::CdpSession::open(target, port, 6000) {
            Ok(session) => {
                match session.evaluate(&script, 15_000) {
                    Ok(v) => {
                        ok_count += 1;
                        last_result = v;
                    }
                    Err(e) => last_err = e.to_string(),
                }
                session.close();
            }
            Err(e) => last_err = e.to_string(),
        }
    }

    if ok_count == 0 {
        return Err(if last_err.is_empty() {
            "evaluate 未成功".into()
        } else {
            last_err
        });
    }

    let already = last_result
        .get("already")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let skipped_same = last_result
        .get("skippedSameModels")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let version = last_result
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(format!(
        " · 目标 {ok_count}/{}{}{} v{version}",
        targets.len(),
        if already { "（已安装）" } else { "" },
        if skipped_same { "（列表未变）" } else { "" },
    ))
}

fn clear_on_open_ports() -> Result<(), String> {
    let Some(port) = first_open_debug_port() else {
        return Ok(());
    };
    let targets = crate::cdp::http::list_app_targets(port).map_err(|e| e.to_string())?;
    let script = build_clear_script();
    for target in &targets {
        if let Ok(session) = crate::cdp::session::CdpSession::open(target, port, 4000) {
            let _ = session.evaluate(&script, 8_000);
            session.close();
        }
    }
    LAST_INJECT_FP.store(0, Ordering::SeqCst);
    if let Ok(mut g) = LAST_MODELS.lock() {
        g.clear();
    }
    Ok(())
}

/// Inject into an already-open CDP session (used after skin shell inject).
pub fn try_inject_into_session(
    session: &crate::cdp::session::CdpSession,
    models: &[String],
) -> Result<Value, String> {
    if should_skip_unlock_for_official() {
        clear_unlock_desired();
        let _ = session.evaluate(&build_clear_script(), 8_000);
        return Ok(json!({ "ok": false, "reason": "official", "skipped": true }));
    }
    if models.is_empty() {
        return Ok(json!({ "ok": false, "reason": "no_models" }));
    }
    // Session inject is a one-shot on an open page — park after success.
    let meta = resolve_unlock_meta(None);
    let fp = fingerprint(models, &meta);
    let script = build_evaluate_script(models, &meta);
    let v = session
        .evaluate(&script, 15_000)
        .map_err(|e| e.to_string())?;
    enter_stable(fp, models);
    ensure_keep_thread();
    Ok(v)
}

pub fn try_inject_from_live_catalog() -> UnlockResult {
    try_inject_desktop_unlock(None)
}

/// Call when enabling OpenAI Official so keep-loop and page hooks stop.
///
/// Prefer [`schedule_official_activated`] from GUI command paths — this does
/// synchronous CDP clear + toolbox re-inject.
pub fn on_official_activated() -> UnlockResult {
    // Toolbox third-party gate: force-chinese / plugin unlock turn off on official.
    crate::toolbox::on_provider_changed();
    clear_unlock_desired();
    let _ = clear_on_open_ports();
    UnlockResult::skipped(String::new())
}

/// Mark third-party unlock as desired and **actively** watch until healthy.
/// Safe when Codex is not up yet — inject runs once the port opens, then parks.
pub fn arm_keep_loop() {
    if should_skip_unlock_for_official() {
        clear_unlock_desired();
        return;
    }
    mark_unlock_desired_active();
}

pub fn disarm_keep_loop() {
    clear_unlock_desired();
}

/// Provider / catalog changed: leave stable park and re-evaluate (single inject).
///
/// Non-blocking for the GUI: arms keep + schedules CDP on a worker thread.
pub fn notify_provider_or_catalog_changed() -> UnlockResult {
    if should_skip_unlock_for_official() {
        schedule_official_activated();
        return UnlockResult::skipped(String::new());
    }
    let models = resolve_unlock_models(None);
    if models.is_empty() {
        clear_unlock_desired();
        schedule_toolbox_provider_changed();
        return UnlockResult::skipped(String::new());
    }
    mark_unlock_desired_active();
    schedule_toolbox_provider_changed();
    let models_for_ui = models.clone();
    let _ = thread::Builder::new()
        .name("cgt-model-unlock-force".into())
        .spawn(|| {
            let _ = try_inject_desktop_unlock_inner(None, InjectMode::Force);
        });
    UnlockResult {
        attempted: false,
        ok: true,
        models: models_for_ui,
        skipped_unchanged: false,
        message: String::new(),
    }
}

/// Desire + leave stable + wake keep (active phase until healthy).
fn mark_unlock_desired_active() {
    UNLOCK_DESIRED.store(true, Ordering::SeqCst);
    UNLOCK_STABLE.store(false, Ordering::SeqCst);
    ensure_keep_thread();
    wake_keep_thread();
}

/// Only ensure DESIRED flag (keep mode) — do not clear STABLE.
fn ensure_desired_flag_only() {
    UNLOCK_DESIRED.store(true, Ordering::SeqCst);
    ensure_keep_thread();
}

fn clear_unlock_desired() {
    UNLOCK_DESIRED.store(false, Ordering::SeqCst);
    UNLOCK_STABLE.store(false, Ordering::SeqCst);
    LAST_INJECT_FP.store(0, Ordering::SeqCst);
    if let Ok(mut g) = LAST_MODELS.lock() {
        g.clear();
    }
    wake_keep_thread();
}

fn wake_keep_thread() {
    if let Ok(mut guard) = KEEP_WAKE.0.lock() {
        *guard = true;
        KEEP_WAKE.1.notify_one();
    }
}

/// Sleep `secs`, but return early if nudged (provider switch / host ready).
fn wait_or_nudge(secs: u64) {
    let (lock, cvar) = &KEEP_WAKE;
    let Ok(guard) = lock.lock() else {
        thread::sleep(Duration::from_secs(secs));
        return;
    };
    match cvar.wait_timeout_while(guard, Duration::from_secs(secs), |pending| !*pending) {
        Ok((mut g, _timed_out)) => {
            // Clear nudge so the next wait blocks again.
            *g = false;
        }
        Err(_) => {
            thread::sleep(Duration::from_secs(secs));
        }
    }
}

/// After host/Codex launch: one-shot inject then park if healthy.
pub fn on_host_ready() {
    if should_skip_unlock_for_official() {
        clear_unlock_desired();
        return;
    }
    let models = resolve_unlock_models(None);
    if models.is_empty() {
        return;
    }
    let _ = try_inject_desktop_unlock_inner(None, InjectMode::Force);
}

fn ensure_keep_thread() {
    if KEEP_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::Builder::new()
        .name("cgt-model-unlock-keep".into())
        .spawn(|| {
            loop {
                if !UNLOCK_DESIRED.load(Ordering::SeqCst) {
                    // Fully idle — wait for nudge (official off / no work).
                    wait_or_nudge(30);
                    continue;
                }
                if should_skip_unlock_for_official() {
                    clear_unlock_desired();
                    let _ = clear_on_open_ports();
                    wait_or_nudge(30);
                    continue;
                }

                let stable = UNLOCK_STABLE.load(Ordering::SeqCst);
                let Some(_port) = first_open_debug_port() else {
                    // Waiting for Codex — not stable; light wait, wakeable.
                    UNLOCK_STABLE.store(false, Ordering::SeqCst);
                    wait_or_nudge(KEEP_PORT_WAIT_SECS);
                    continue;
                };

                if stable {
                    // Parked: rare watchdog only (SPA remount recovery).
                    // No CDP inject unless probe says unhealthy.
                    let models = resolve_unlock_models(None);
                    if models.is_empty() {
                        clear_unlock_desired();
                        wait_or_nudge(30);
                        continue;
                    }
                    let meta = resolve_unlock_meta(None);
                    let fp = fingerprint(&models, &meta);
                    match probe_page_state(_port, fp, &models) {
                        Ok(true) => {
                            // Still healthy — stay parked.
                            wait_or_nudge(KEEP_STABLE_WATCHDOG_SECS);
                        }
                        Ok(false) | Err(_) => {
                            // Lost hooks — one repair inject, then park again.
                            UNLOCK_STABLE.store(false, Ordering::SeqCst);
                            let _ = try_inject_desktop_unlock_inner(None, InjectMode::Keep);
                            wait_or_nudge(KEEP_ACTIVE_SECS);
                        }
                    }
                    continue;
                }

                // Active phase: retry until healthy, then enter_stable parks us.
                let _ = try_inject_desktop_unlock_inner(None, InjectMode::Keep);
                if UNLOCK_STABLE.load(Ordering::SeqCst) {
                    // Just became stable — long sleep until event or watchdog.
                    wait_or_nudge(KEEP_STABLE_WATCHDOG_SECS);
                } else {
                    wait_or_nudge(KEEP_ACTIVE_SECS);
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_stable_for_same_set() {
        let mut meta = Map::new();
        meta.insert(
            "a".into(),
            json!({"displayName": "A", "description": "A"}),
        );
        meta.insert(
            "b".into(),
            json!({"displayName": "B", "description": "B"}),
        );
        let f1 = fingerprint(&["b".into(), "a".into()], &meta);
        let f2 = fingerprint(&["a".into(), "b".into()], &meta);
        assert_eq!(f1, f2);
        let f3 = fingerprint(&["a".into(), "b".into(), "c".into()], &meta);
        assert_ne!(f1, f3);
    }
}
