//! One-shot CDP inject / soft-verify (replaces `node injector.mjs --once`).

use super::http::{list_app_targets, wait_for_app_targets, CdpHttpError};
use super::payload::{art_evaluate_timeout_ms, build_staged_payload, StagedPayload};
use super::session::{CdpSession, CdpSessionError};
use serde_json::{json, Value};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("{0}")]
    Message(String),
}

impl InjectError {
    fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

impl From<CdpHttpError> for InjectError {
    fn from(e: CdpHttpError) -> Self {
        Self::msg(e.to_string())
    }
}

impl From<CdpSessionError> for InjectError {
    fn from(e: CdpSessionError) -> Self {
        Self::msg(e.to_string())
    }
}

impl From<super::payload::PayloadError> for InjectError {
    fn from(e: super::payload::PayloadError) -> Self {
        Self::msg(e.to_string())
    }
}

fn probe_host_resident(session: &CdpSession) -> Value {
    session
        .evaluate(
            r#"(() => {
      const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
      if (!host || typeof host.applySkin !== "function") {
        return { resident: false };
      }
      const active = typeof host.getActive === "function" ? host.getActive() : null;
      return {
        resident: true,
        coreRevision: host.coreRevision || null,
        revision: active?.revision || null,
        skinId: active?.skinId || null,
        artReady: Boolean(active?.artReady),
      };
    })()"#,
            5000,
        )
        .unwrap_or(json!({ "resident": false }))
}

fn apply_staged(
    session: &CdpSession,
    staged: &StagedPayload,
    art: bool,
    prefer_delta: bool,
) -> Result<Value, InjectError> {
    let mut shell_mode = "full";
    let mut shell_result: Option<Value> = None;

    if prefer_delta && !staged.delta_shell_payload.is_empty() {
        let host = probe_host_resident(session);
        let resident = host
            .get("resident")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let host_core = host
            .get("coreRevision")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let core_ok = staged.core_revision.is_empty()
            || host_core.is_empty()
            || host_core == staged.core_revision;
        if resident && core_ok {
            match session.evaluate(&staged.delta_shell_payload, 12_000) {
                Ok(v) if v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false) => {
                    shell_mode = "delta";
                    shell_result = Some(v);
                }
                Ok(v) if v
                    .get("needsFullShell")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false) =>
                {
                    shell_result = None;
                }
                _ => shell_result = None,
            }
        }
    }

    if shell_result
        .as_ref()
        .and_then(|v| v.get("ok").and_then(|x| x.as_bool()))
        != Some(true)
    {
        let v = session.evaluate(&staged.shell_payload, 15_000)?;
        shell_mode = if v.get("mode").and_then(|m| m.as_str()) == Some("delta") {
            "delta"
        } else {
            "full"
        };
        shell_result = Some(v);
    }

    let mut art_result = Value::Null;
    let mut art_ok = !art || staged.art_payload.is_empty();
    if art && !staged.art_payload.is_empty() {
        let timeout = art_evaluate_timeout_ms(staged);
        match session.evaluate(&staged.art_payload, timeout) {
            Ok(v) => {
                art_ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false)
                    || v.get("already").and_then(|x| x.as_bool()).unwrap_or(false);
                art_result = v;
            }
            Err(e) => {
                art_result = json!({
                    "ok": false,
                    "reason": "art-evaluate-failed",
                    "message": e.to_string()
                });
                art_ok = false;
            }
        }
    }

    Ok(json!({
        "shell": shell_result,
        "art": art_result,
        "shellMode": shell_mode,
        "deferredArt": staged.deferred_art,
        "artOk": art_ok,
        "artPending": !staged.art_payload.is_empty() && !art_ok,
    }))
}

fn soft_verify_expression(markers: &Value) -> String {
    let root = markers
        .get("rootClass")
        .and_then(|v| v.as_str())
        .unwrap_or("codex-skin");
    let style_id = markers
        .get("styleId")
        .and_then(|v| v.as_str())
        .unwrap_or("codex-skin-style");
    let state_key = markers
        .get("stateKey")
        .and_then(|v| v.as_str())
        .unwrap_or("__CODEX_SKIN_STATE__");
    format!(
        r#"(() => {{
  const root = document.documentElement;
  const style = document.getElementById({style_id});
  const state = window[{state_key}];
  const installed = root.classList.contains({root});
  const stylePresent = Boolean(style);
  return {{
    pass: installed && stylePresent,
    installed,
    stylePresent,
    revision: state?.revision ?? null,
    artReady: Boolean(state?.artReady),
    artOk: Boolean(state?.artReady && state?.artUrl),
    artPending: Boolean(state && !state.artReady),
  }};
}})()"#,
        style_id = serde_json::to_string(style_id).unwrap(),
        state_key = serde_json::to_string(state_key).unwrap(),
        root = serde_json::to_string(root).unwrap(),
    )
}

fn wait_soft_verify(
    session: &CdpSession,
    markers: &Value,
    timeout_ms: u64,
) -> Result<Value, InjectError> {
    let expr = soft_verify_expression(markers);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut last = json!({ "pass": false });
    while Instant::now() < deadline {
        match session.evaluate(&expr, 4000) {
            Ok(v) => {
                last = v;
                if last.get("pass").and_then(|x| x.as_bool()).unwrap_or(false) {
                    return Ok(last);
                }
            }
            Err(_) => {}
        }
        thread::sleep(Duration::from_millis(120));
    }
    Ok(last)
}

/// Inject shell (prefer delta) + soft verify + progressive art on all app:// pages.
/// Returns a JSON shape compatible with Node soft-once / soft-verify parsing.
pub fn inject_once(
    skin_dir: &Path,
    project_root: &Path,
    port: u16,
    soft: bool,
    timeout_ms: u64,
) -> Result<Value, InjectError> {
    let staged = build_staged_payload(skin_dir, project_root)?;
    let wait_budget = timeout_ms.min(if soft { 4000 } else { timeout_ms });
    let targets = wait_for_app_targets(port, wait_budget)?;
    if targets.is_empty() {
        return Err(InjectError::msg("No app:// page targets"));
    }

    let mut results = Vec::new();
    let mut any_pass = false;
    let mut shell_ok = false;
    let mut art_ok = false;
    let mut art_pending = true;
    let mut shell_mode = "full".to_string();

    for target in &targets {
        let session = match CdpSession::open(target, port, 8000) {
            Ok(s) => s,
            Err(e) => {
                results.push(json!({
                    "targetId": target.id,
                    "error": e.to_string()
                }));
                continue;
            }
        };

        // Soft once: shell first, art after soft verify.
        let applied = apply_staged(&session, &staged, false, true)?;
        shell_mode = applied
            .get("shellMode")
            .and_then(|v| v.as_str())
            .unwrap_or("full")
            .to_string();

        thread::sleep(Duration::from_millis(if soft { 60 } else { 400 }));

        let verify_budget = if soft {
            timeout_ms
                .min(timeout_ms.saturating_mul(6) / 10)
                .max(2500)
                .min(timeout_ms)
        } else {
            timeout_ms
        };
        let mut verified = wait_soft_verify(&session, &staged.markers, verify_budget)?;

        if verified.get("pass").and_then(|v| v.as_bool()).unwrap_or(false)
            && !staged.art_payload.is_empty()
        {
            let art_timeout = art_evaluate_timeout_ms(&staged);
            match session.evaluate(&staged.art_payload, art_timeout) {
                Ok(art_value) => {
                    let ok = art_value
                        .get("ok")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false)
                        || art_value
                            .get("already")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false);
                    if let Some(obj) = verified.as_object_mut() {
                        obj.insert("artOk".into(), json!(ok));
                        obj.insert("artPending".into(), json!(!ok));
                        obj.insert("artAttached".into(), json!(ok));
                    }
                }
                Err(e) => {
                    if let Some(obj) = verified.as_object_mut() {
                        obj.insert("artOk".into(), json!(false));
                        obj.insert("artPending".into(), json!(true));
                        obj.insert("artError".into(), json!(e.to_string()));
                    }
                }
            }
        }

        let pass = verified
            .get("pass")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if pass {
            any_pass = true;
            shell_ok = true;
            art_ok = verified
                .get("artOk")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            art_pending = verified
                .get("artPending")
                .and_then(|v| v.as_bool())
                .unwrap_or(!art_ok);
        }

        results.push(json!({
            "targetId": target.id,
            "result": verified,
            "shellMode": shell_mode,
        }));

        session.close();
        if soft && pass {
            break;
        }
    }

    Ok(json!({
        "ok": any_pass || shell_ok,
        "mode": if soft { "once" } else { "verify" },
        "soft": soft,
        "shellOk": shell_ok,
        "artOk": art_ok,
        "artPending": art_pending,
        "shellMode": shell_mode,
        "engine": "native-rust",
        "fingerprint": staged.fingerprint,
        "revision": staged.revision,
        "results": results,
    }))
}

/// Remove skin markers from all app:// pages (best-effort).
pub fn remove_once(skin_dir: &Path, project_root: &Path, port: u16) -> Result<Value, InjectError> {
    let staged = build_staged_payload(skin_dir, project_root)?;
    let markers = &staged.markers;
    let root = markers
        .get("rootClass")
        .and_then(|v| v.as_str())
        .unwrap_or("codex-skin");
    let style_id = markers
        .get("styleId")
        .and_then(|v| v.as_str())
        .unwrap_or("codex-skin-style");
    let chrome_id = markers
        .get("chromeId")
        .and_then(|v| v.as_str())
        .unwrap_or("codex-skin-chrome");
    let state_key = markers
        .get("stateKey")
        .and_then(|v| v.as_str())
        .unwrap_or("__CODEX_SKIN_STATE__");
    let disabled_key = markers
        .get("disabledKey")
        .and_then(|v| v.as_str())
        .unwrap_or("__CODEX_SKIN_DISABLED__");
    let art_var = markers
        .get("artVar")
        .and_then(|v| v.as_str())
        .unwrap_or("--skin-art");

    let home_class = markers
        .get("homeClass")
        .and_then(|v| v.as_str())
        .unwrap_or("skin-home");
    let home_shell = markers
        .get("homeShellClass")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{home_class}-shell"));
    let home_utility = markers
        .get("homeUtilityClass")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{home_class}-utility"));

    // Full DOM strip aligned with purge-all / renderer-core soft cleanup (no reload).
    let expr = format!(
        r#"(() => {{
  const disabledKey = {disabled};
  const stateKey = {state};
  const rootClass = {root};
  const artVar = {art};
  const styleId = {style};
  const chromeId = {chrome};
  const homeClass = {home};
  const homeShell = {shell};
  const homeUtility = {utility};
  try {{ window[disabledKey] = true; }} catch {{}}
  const state = window[stateKey];
  if (state?.cleanup) {{
    try {{ return state.cleanup(); }} catch {{}}
  }}
  const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
  if (host?.cleanup) {{
    try {{ return host.cleanup(); }} catch {{}}
  }}
  const root = document.documentElement;
  const themeClasses = [
    'dream-theme-light','dream-theme-dark','dream-art-wide','dream-art-standard',
    'dream-focus-left','dream-focus-center','dream-focus-right',
    'dream-safe-left','dream-safe-center','dream-safe-right','dream-safe-none',
    'dream-task-ambient','dream-task-banner','dream-task-off'
  ];
  root?.classList.remove(rootClass, ...themeClasses);
  for (const prop of [artVar, '--dream-art', '--dream-art-position', '--dream-focus-x',
    '--dream-focus-y', '--dream-accent', '--dream-accent-ink', '--dream-image-luma'].filter(Boolean)) {{
    root?.style.removeProperty(prop);
  }}
  root?.removeAttribute('data-chatgpt-tools-skin');
  root?.removeAttribute('data-dream-shell');
  document.getElementById(styleId)?.remove();
  document.getElementById(chromeId)?.remove();
  document.querySelectorAll('style[data-skin-revision], style[id*="-skin-style"]').forEach((n) => n.remove());
  document.querySelectorAll('[id*="-skin-chrome"]').forEach((n) => n.remove());
  for (const cls of [homeClass, homeShell, homeUtility].filter(Boolean)) {{
    document.querySelectorAll('.' + cls).forEach((n) => n.classList.remove(cls));
  }}
  try {{ delete window[stateKey]; }} catch {{}}
  return true;
}})()"#,
        disabled = serde_json::to_string(disabled_key).unwrap(),
        state = serde_json::to_string(state_key).unwrap(),
        root = serde_json::to_string(root).unwrap(),
        art = serde_json::to_string(art_var).unwrap(),
        style = serde_json::to_string(style_id).unwrap(),
        chrome = serde_json::to_string(chrome_id).unwrap(),
        home = serde_json::to_string(home_class).unwrap(),
        shell = serde_json::to_string(&home_shell).unwrap(),
        utility = serde_json::to_string(&home_utility).unwrap(),
    );

    let targets = list_app_targets(port)?;
    if targets.is_empty() {
        // Host answered /json/list but no app:// pages — not a successful strip.
        return Ok(json!({
            "ok": false,
            "removedTargets": 0,
            "reason": "no-app-targets",
            "engine": "native-rust"
        }));
    }
    let mut n = 0;
    let mut eval_fail = 0;
    for target in targets {
        match CdpSession::open(&target, port, 5000) {
            Ok(session) => {
                match session.evaluate(&expr, 8000) {
                    Ok(_) => n += 1,
                    Err(_) => eval_fail += 1,
                }
                session.close();
            }
            Err(_) => eval_fail += 1,
        }
    }
    // Never report ok when zero pages were cleaned (Dream: no false success).
    let ok = n > 0;
    Ok(json!({
        "ok": ok,
        "removedTargets": n,
        "evalFailures": eval_fail,
        "engine": "native-rust"
    }))
}
