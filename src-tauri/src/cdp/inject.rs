//! One-shot CDP inject / soft-verify (replaces `node injector.mjs --once`).

use super::http::{list_app_targets, wait_for_app_targets, CdpHttpError};
use super::keep::{art_job_still_valid, art_generation};
use super::payload::{
    art_evaluate_timeout_ms, assemble_art_chunk_pipeline, build_art_payload_only,
    build_staged_payload, StagedPayload, ART_SINGLE_EVAL_MAX_CHARS,
};
use super::session::{CdpSession, CdpSessionError};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// One-shot soft presence on the primary app:// page (no wait loop).
/// Used by cold-path conditional repair — avoid a second inject when shell still holds.
pub fn soft_shell_present(port: u16, markers: &Value) -> bool {
    let Ok(targets) = list_app_targets(port) else {
        return false;
    };
    let Some(target) = targets.first() else {
        return false;
    };
    let Ok(session) = CdpSession::open(target, port, 2500) else {
        return false;
    };
    let expr = soft_verify_expression(markers);
    let pass = session
        .evaluate(&expr, 3000)
        .ok()
        .and_then(|v| v.get("pass").and_then(|p| p.as_bool()))
        .unwrap_or(false);
    session.close();
    pass
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

/// CDP evaluate budget for Operation UI (best-effort; never gate inject success).
const OP_UI_EVAL_MS: u64 = 500;
const OP_UI_HOST_ID: &str = "chatgpt-tools-skin-operation";
const OP_UI_TOKEN_KEY: &str = "__CHATGPT_TOOLS_OP_TOKEN__";

/// Monotonic apply-op token. Each user apply gets one token; stale finish is ignored.
static OP_UI_TOKEN: AtomicU64 = AtomicU64::new(1);

/// Options for one-shot inject. **Never** drives page Operation UI —
/// apply orchestration owns a single begin/finish per user action.
#[derive(Debug, Clone, Copy)]
pub struct InjectOnceOpts {
    pub soft: bool,
    pub timeout_ms: u64,
    /// When false: shell + soft verify only (fast path; artPending if art exists).
    pub attach_art: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Apply,
    Switch,
}

impl OperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::Switch => "switch",
        }
    }

    fn loading_label(self) -> &'static str {
        match self {
            Self::Switch => "正在切换皮肤…",
            Self::Apply => "正在应用皮肤…",
        }
    }
}

impl Default for InjectOnceOpts {
    fn default() -> Self {
        Self {
            soft: true,
            timeout_ms: 8_000,
            attach_art: true,
        }
    }
}

/// Allocate a new apply-op token (invalidates any in-flight page toast).
pub fn next_op_token() -> u64 {
    OP_UI_TOKEN
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1)
}

/// True when `token` is still the latest apply-op (not superseded by a newer apply).
pub fn op_token_current(token: u64) -> bool {
    token != 0 && OP_UI_TOKEN.load(Ordering::SeqCst) == token
}

fn begin_op_expr(kind: OperationKind, token: u64) -> String {
    // Single lightweight bootstrap (no Shadow DOM). Host may later upgrade the same id.
    // Token is stamped on the node + window so finish can ignore superseded ops.
    format!(
        r#"(() => {{
  try {{
    const token = {token};
    const label = {label};
    const kind = {kind};
    window[{token_key}] = token;
    const id = {host_id};
    let el = document.getElementById(id);
    // Drop any prior toast (other token / leftover shadow host).
    if (el) {{ try {{ el.remove(); }} catch (_) {{}} el = null; }}
    const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
    if (host && typeof host.showOperation === "function") {{
      const r = host.showOperation(kind, label, token);
      // Ensure token is on the live node even if host ignored the 3rd arg.
      el = document.getElementById(id);
      if (el) {{
        el.setAttribute("data-op-token", String(token));
        el.setAttribute("data-state", "loading");
      }}
      return r || {{ ok: true, token, via: "host" }};
    }}
    el = document.createElement("div");
    el.id = id;
    el.setAttribute("data-bootstrap", "1");
    el.setAttribute("data-op-token", String(token));
    el.setAttribute("data-state", "loading");
    el.style.cssText = "all:initial;position:fixed;right:12px;bottom:12px;z-index:2147483646;pointer-events:none;font-family:system-ui,sans-serif;max-width:min(280px,calc(100vw - 20px))";
    el.innerHTML = '<div data-msg="1" style="padding:10px 12px;border-radius:10px;background:rgba(32,33,38,.94);color:#f3f3f6;font-size:12.5px;box-shadow:0 8px 22px rgba(0,0,0,.28)">'+label+'</div>';
    (document.documentElement||document.body).appendChild(el);
    return {{ ok: true, token, bootstrap: true }};
  }} catch (e) {{
    return {{ ok: false, message: String(e && e.message || e) }};
  }}
}})()"#,
        token = token,
        label = serde_json::to_string(kind.loading_label()).unwrap_or_else(|_| "\"…\"".into()),
        kind = serde_json::to_string(kind.as_str()).unwrap_or_else(|_| "\"apply\"".into()),
        token_key = serde_json::to_string(OP_UI_TOKEN_KEY).unwrap(),
        host_id = serde_json::to_string(OP_UI_HOST_ID).unwrap(),
    )
}

fn finish_op_expr(token: u64, ok: bool, message: &str) -> String {
    let state = if ok { "success" } else { "error" };
    let hide_ms = if ok { 1100 } else { 2000 };
    format!(
        r#"(() => {{
  try {{
    const token = {token};
    const want = {state};
    const msg = {msg};
    const hideMs = {hide_ms};
    const id = {host_id};
    const tokenKey = {token_key};
    // Stale apply — do nothing (newer op owns the page).
    const liveTok = Number(window[tokenKey] || 0);
    const el0 = document.getElementById(id);
    const elTok = el0 ? Number(el0.getAttribute("data-op-token") || 0) : 0;
    if (liveTok && liveTok !== token) {{
      return {{ ok: false, reason: "stale", token, live: liveTok }};
    }}
    if (elTok && elTok !== token) {{
      return {{ ok: false, reason: "stale-el", token, elTok }};
    }}

    const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
    if (host && typeof host.finishOperation === "function") {{
      try {{ host.finishOperation(want, msg, token); }} catch (_) {{}}
    }}

    let el = document.getElementById(id);
    // Bootstrap (no shadow) or host failed to update — force final state.
    if (!el) {{
      // Success with no toast is fine (skin already visible). Error still show briefly.
      if (want === "success") return {{ ok: true, cleared: true }};
      el = document.createElement("div");
      el.id = id;
      el.setAttribute("data-bootstrap", "1");
      el.setAttribute("data-op-token", String(token));
      el.style.cssText = "all:initial;position:fixed;right:12px;bottom:12px;z-index:2147483646;pointer-events:none;font-family:system-ui,sans-serif;max-width:min(280px,calc(100vw - 20px))";
      el.innerHTML = '<div data-msg="1" style="padding:10px 12px;border-radius:10px;background:rgba(32,33,38,.94);color:#f3f3f6;font-size:12.5px;box-shadow:0 8px 22px rgba(0,0,0,.28)"></div>';
      try {{ (document.documentElement||document.body).appendChild(el); }} catch (_) {{ return {{ ok: false }}; }}
    }}

    el.setAttribute("data-op-token", String(token));
    el.setAttribute("data-state", want);
    el.removeAttribute("data-bootstrap");
    if (el.shadowRoot) {{
      try {{
        el.setAttribute("data-visible", "true");
        const text = el.shadowRoot.querySelector(".msg");
        if (text) text.textContent = msg;
      }} catch (_) {{}}
    }} else {{
      const msgEl = el.querySelector("[data-msg]") || el.querySelector("div");
      if (msgEl) msgEl.textContent = msg;
    }}

    // Token-checked dismiss — never leave loading forever; ignore if superseded.
    setTimeout(() => {{
      try {{
        const cur = document.getElementById(id);
        if (!cur) return;
        if (Number(cur.getAttribute("data-op-token") || 0) !== token) return;
        if (Number(window[tokenKey] || 0) !== token) return;
        if (cur.shadowRoot) {{
          try {{ cur.setAttribute("data-visible", "false"); }} catch (_) {{}}
          setTimeout(() => {{
            try {{
              const n = document.getElementById(id);
              if (n && Number(n.getAttribute("data-op-token") || 0) === token) n.remove();
            }} catch (_) {{}}
          }}, 160);
        }} else {{
          cur.remove();
        }}
        if (Number(window[tokenKey] || 0) === token) {{
          try {{ delete window[tokenKey]; }} catch (_) {{ window[tokenKey] = 0; }}
        }}
      }} catch (_) {{}}
    }}, hideMs);
    return {{ ok: true, state: want, token }};
  }} catch (e) {{
    try {{
      const el = document.getElementById({host_id});
      if (el && Number(el.getAttribute("data-op-token") || 0) === {token}) el.remove();
    }} catch (_) {{}}
    return {{ ok: false, message: String(e && e.message || e) }};
  }}
}})()"#,
        token = token,
        state = serde_json::to_string(state).unwrap_or_else(|_| "\"success\"".into()),
        msg = serde_json::to_string(message).unwrap_or_else(|_| "\"\"".into()),
        hide_ms = hide_ms,
        host_id = serde_json::to_string(OP_UI_HOST_ID).unwrap(),
        token_key = serde_json::to_string(OP_UI_TOKEN_KEY).unwrap(),
    )
}

fn dismiss_op_expr(token: u64) -> String {
    format!(
        r#"(() => {{
  try {{
    const token = {token};
    const id = {host_id};
    const tokenKey = {token_key};
    const el = document.getElementById(id);
    if (el) {{
      const elTok = Number(el.getAttribute("data-op-token") || 0);
      if (!elTok || elTok === token) el.remove();
    }}
    if (Number(window[tokenKey] || 0) === token) {{
      try {{ delete window[tokenKey]; }} catch (_) {{ window[tokenKey] = 0; }}
    }}
    return {{ ok: true }};
  }} catch (e) {{
    return {{ ok: false }};
  }}
}})()"#,
        token = token,
        host_id = serde_json::to_string(OP_UI_HOST_ID).unwrap(),
        token_key = serde_json::to_string(OP_UI_TOKEN_KEY).unwrap(),
    )
}

fn eval_op_on_targets(port: u16, expr: &str, max_targets: usize) {
    let Ok(targets) = list_app_targets(port) else {
        return;
    };
    for target in targets.iter().take(max_targets.max(1)) {
        let Ok(session) = CdpSession::open(target, port, 2200) else {
            continue;
        };
        let _ = session.evaluate(expr, OP_UI_EVAL_MS);
        session.close();
    }
}

/// Begin page Operation UI **once** for this apply (Scheme B).
/// Returns a token that must be passed to [`finish_apply_operation`].
/// Best-effort; never fails the caller. No-op when no app:// target yet (cold start).
pub fn begin_apply_operation(port: u16, kind: OperationKind) -> u64 {
    let token = next_op_token();
    let expr = begin_op_expr(kind, token);
    // Hot path: primary target only (perf). Cold may have 0 targets — token still reserved.
    eval_op_on_targets(port, &expr, 1);
    token
}

/// Finish page Operation UI **once** for `token` using **final** shell outcome.
/// Intermediate soft-miss retries must NOT call this with failure.
/// Stale tokens are ignored (superseded apply).
pub fn finish_apply_operation(port: u16, token: u64, ok: bool, message: &str) {
    if token == 0 {
        return;
    }
    // Only the latest token may finish; older applies are already invalidated.
    if !op_token_current(token) {
        return;
    }
    let expr = finish_op_expr(token, ok, message);
    // Cover multi-target hosts (max 2 for perf).
    eval_op_on_targets(port, &expr, 2);
}

/// Hard-dismiss toast for `token` (cancel / superseded). Best-effort.
pub fn dismiss_apply_operation(port: u16, token: u64) {
    if token == 0 {
        return;
    }
    let expr = dismiss_op_expr(token);
    eval_op_on_targets(port, &expr, 2);
}

/// Invalidate any in-flight page op without opening CDP (new apply supersedes).
#[allow(dead_code)]
pub fn bump_op_token() -> u64 {
    next_op_token()
}

/// Re-paint loading toast for an **already reserved** token (cold path after boot).
/// Does not allocate; no-op if `token` is stale. Best-effort.
pub fn restamp_apply_operation(port: u16, kind: OperationKind, token: u64) {
    if token == 0 || !op_token_current(token) {
        return;
    }
    let expr = begin_op_expr(kind, token);
    eval_op_on_targets(port, &expr, 1);
}

/// Expression: page is past boot splash and has a real DOM root.
/// `app://` alone is not enough — Electron often exposes targets while React still hydrates.
/// Restart cold-inject used to fire here and shell/art would be wiped by later remounts.
///
/// Use `r##"..."##` so JS attribute selectors containing `"#...` do not end the raw string early.
const PAGE_STABLE_EXPR: &str = r##"(() => {
  try {
    const rs = document.readyState;
    const body = document.body;
    if (rs !== "complete" && rs !== "interactive") {
      return { ok: false, reason: "readyState", readyState: rs };
    }
    if (!body) return { ok: false, reason: "no-body", readyState: rs };

    const href = String(location.href || "");
    const appUrl =
      href.startsWith("app://") ||
      href.startsWith("file://") ||
      /chatgpt|codex|openai/i.test(href);
    if (!appUrl) {
      return { ok: false, reason: "href", readyState: rs, href: href.slice(0, 120) };
    }

    // Boot / about:blank-style shells still report app:// but have almost no tree.
    const root =
      document.getElementById("root") ||
      document.getElementById("app") ||
      document.querySelector("#__next") ||
      document.querySelector('[data-testid="root"]') ||
      document.querySelector("main") ||
      body;
    const kids = root && typeof root.childElementCount === "number" ? root.childElementCount : 0;
    const bodyKids = body.childElementCount || 0;
    const allEls = (() => {
      try { return document.getElementsByTagName("*").length || 0; } catch (_) { return 0; }
    })();
    // Real desktop UI mounts a sizable tree; splash is usually < ~30 elements.
    const hasTree = allEls >= 40 || (kids >= 2 && bodyKids >= 1 && allEls >= 20);

    // Prefer a chrome signal so we do not inject into pure loading splash.
    const chrome =
      document.querySelector("nav") ||
      document.querySelector("aside") ||
      document.querySelector("textarea") ||
      document.querySelector('[contenteditable="true"]') ||
      document.querySelector("button") ||
      document.querySelector('[role="navigation"]') ||
      document.querySelector('[role="main"]') ||
      document.querySelector("[data-testid]");
    const hasChrome = Boolean(chrome);

    let visible = true;
    try { visible = document.visibilityState !== "prerender"; } catch (_) {}

    // complete is preferred; interactive only if DOM already looks like the real app.
    const readyOk = rs === "complete" || (rs === "interactive" && hasTree && hasChrome && allEls >= 60);
    const ok = Boolean(readyOk && hasTree && (hasChrome || allEls >= 80) && visible);

    return {
      ok,
      readyState: rs,
      kids,
      bodyKids,
      allEls,
      hasChrome,
      href: href.slice(0, 120),
    };
  } catch (e) {
    return { ok: false, reason: "exception", message: String(e && e.message || e) };
  }
})()"##;

/// One-shot: is the first app:// page document stable enough for shell inject?
pub fn probe_page_stable(port: u16) -> bool {
    let Ok(targets) = list_app_targets(port) else {
        return false;
    };
    let Some(target) = targets.first() else {
        return false;
    };
    let Ok(session) = CdpSession::open(target, port, 2500) else {
        return false;
    };
    let ok = session
        .evaluate(PAGE_STABLE_EXPR, 3500)
        .ok()
        .and_then(|v| v.get("ok").and_then(|x| x.as_bool()))
        .unwrap_or(false);
    session.close();
    ok
}

/// Wait until app:// exists **and** document is stable for consecutive polls.
/// Prevents fire-and-forget cold inject during host boot / SPA hydrate.
///
/// - `stable_hits`: consecutive successful page-stable probes required (default use 3)
/// - `hold_ms`: after first success, require continuous stability for this long
pub fn wait_until_injectable(
    port: u16,
    timeout_ms: u64,
    poll_ms: u64,
    stable_hits: u32,
    hold_ms: u64,
) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let poll = poll_ms.max(120);
    let need_hits = stable_hits.max(2);
    let mut hits = 0u32;
    let mut stable_since: Option<Instant> = None;

    while Instant::now() < deadline {
        // Fast path: need app:// targets first
        let has_target = list_app_targets(port)
            .map(|t| !t.is_empty())
            .unwrap_or(false);
        if !has_target {
            hits = 0;
            stable_since = None;
            thread::sleep(Duration::from_millis(poll));
            continue;
        }
        if probe_page_stable(port) {
            hits = hits.saturating_add(1);
            if stable_since.is_none() {
                stable_since = Some(Instant::now());
            }
            let held = stable_since
                .map(|t| t.elapsed() >= Duration::from_millis(hold_ms))
                .unwrap_or(false);
            if hits >= need_hits && held {
                return true;
            }
        } else {
            hits = 0;
            stable_since = None;
        }
        thread::sleep(Duration::from_millis(poll));
    }
    // Last chance single probe
    probe_page_stable(port)
}

/// Convenience: soft inject (silent — no page OpUI). Legacy / external callers.
#[allow(dead_code)]
pub fn inject_once(
    skin_dir: &Path,
    project_root: &Path,
    port: u16,
    soft: bool,
    timeout_ms: u64,
) -> Result<Value, InjectError> {
    inject_once_with_opts(
        skin_dir,
        project_root,
        port,
        InjectOnceOpts {
            soft,
            timeout_ms,
            attach_art: true,
        },
    )
}

/// Inject shell (prefer delta) + soft verify; art optional and never blocks OpUI finish.
/// Builds a shell-only staged payload internally (no multi-MB art base64).
pub fn inject_once_with_opts(
    skin_dir: &Path,
    project_root: &Path,
    port: u16,
    opts: InjectOnceOpts,
) -> Result<Value, InjectError> {
    let staged = build_staged_payload(skin_dir, project_root)?;
    inject_once_with_staged(skin_dir, project_root, port, opts, &staged)
}

/// Same as [`inject_once_with_opts`] but reuses a pre-built staged shell (avoids rebuild).
pub fn inject_once_with_staged(
    skin_dir: &Path,
    project_root: &Path,
    port: u16,
    opts: InjectOnceOpts,
    staged: &StagedPayload,
) -> Result<Value, InjectError> {
    let soft = opts.soft;
    let timeout_ms = opts.timeout_ms;
    // has_art: skin declares wallpaper (may still defer base64 until follow-up).
    let has_art = staged.has_art || !staged.art_payload.is_empty();
    let wait_budget = timeout_ms.min(if soft { 4000 } else { timeout_ms });
    let targets = wait_for_app_targets(port, wait_budget)?;
    if targets.is_empty() {
        return Err(InjectError::msg("No app:// page targets"));
    }

    let mut results = Vec::new();
    let mut any_pass = false;
    let mut shell_ok = false;
    let mut art_ok = !has_art;
    let mut art_pending = has_art;
    let mut shell_mode = "full".to_string();
    let mut delta_hits: u32 = 0;
    let mut delta_attempts: u32 = 0;

    // Lazily load full art only if this inject attaches it (keep/shell paths skip).
    let mut art_payload_cache: Option<String> = None;

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

        // Phase 1: shell only (delta preferred). Art is never in this evaluate.
        // OpUI is owned by apply orchestration (single begin/finish) — silent here.
        let applied = match apply_staged(&session, staged, false, true) {
            Ok(v) => v,
            Err(e) => {
                results.push(json!({
                    "targetId": target.id,
                    "error": e.to_string()
                }));
                session.close();
                continue;
            }
        };
        shell_mode = applied
            .get("shellMode")
            .and_then(|v| v.as_str())
            .unwrap_or("full")
            .to_string();
        delta_attempts += 1;
        if shell_mode == "delta" {
            delta_hits += 1;
        }

        thread::sleep(Duration::from_millis(if soft { 30 } else { 200 }));

        let verify_budget = if soft {
            timeout_ms
                .min(timeout_ms.saturating_mul(6) / 10)
                .max(1_500)
                .min(timeout_ms)
        } else {
            timeout_ms
        };
        let mut verified = wait_soft_verify(&session, &staged.markers, verify_budget)?;

        let pass = verified
            .get("pass")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Phase 2 (optional): art on same session (caller owns page OpUI).
        if pass && opts.attach_art && has_art {
            let art_js = if !staged.art_payload.is_empty() {
                staged.art_payload.as_str()
            } else {
                if art_payload_cache.is_none() {
                    match build_art_payload_only(skin_dir, project_root) {
                        Ok(full) => art_payload_cache = Some(full.art_payload),
                        Err(e) => {
                            if let Some(obj) = verified.as_object_mut() {
                                obj.insert("artOk".into(), json!(false));
                                obj.insert("artPending".into(), json!(true));
                                obj.insert("artError".into(), json!(e.to_string()));
                            }
                            art_payload_cache = Some(String::new());
                        }
                    }
                }
                art_payload_cache.as_deref().unwrap_or("")
            };
            if !art_js.is_empty() {
                let art_timeout = art_evaluate_timeout_ms(staged).min(90_000);
                match session.evaluate(art_js, art_timeout) {
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
        } else if pass && has_art && !opts.attach_art {
            if let Some(obj) = verified.as_object_mut() {
                obj.insert("artOk".into(), json!(false));
                obj.insert("artPending".into(), json!(true));
                obj.insert("artDeferred".into(), json!(true));
            }
        } else if pass && !has_art {
            if let Some(obj) = verified.as_object_mut() {
                obj.insert("artOk".into(), json!(true));
                obj.insert("artPending".into(), json!(false));
            }
        }

        if pass {
            any_pass = true;
            shell_ok = true;
            art_ok = verified
                .get("artOk")
                .and_then(|v| v.as_bool())
                .unwrap_or(!has_art);
            art_pending = verified
                .get("artPending")
                .and_then(|v| v.as_bool())
                .unwrap_or(has_art && !art_ok);
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
        "deltaHits": delta_hits,
        "deltaAttempts": delta_attempts,
        "deltaPreferred": true,
        "engine": "native-rust",
        "nodeRequired": false,
        "fingerprint": staged.fingerprint,
        "revision": staged.revision,
        "coreRevision": staged.core_revision,
        "results": results,
    }))
}

/// Evaluate chunked art pipeline on a live session. Aborts early if art gen invalid.
fn evaluate_art_on_session(
    session: &CdpSession,
    staged: &StagedPayload,
    art_gen: u64,
) -> Result<Value, InjectError> {
    if !art_job_still_valid(art_gen) {
        return Ok(json!({ "ok": false, "reason": "art-gen-cancelled" }));
    }

    // Large images: multi-evaluate chunk pipeline (no multi-MB single frame).
    if staged.art_payload.len() > ART_SINGLE_EVAL_MAX_CHARS
        || staged.art_bytes > ART_SINGLE_EVAL_MAX_CHARS / 2
    {
        // Rebuild from bytes via build_art — staged already has art_data_url when Full.
        // Prefer re-loading bytes for chunk path when art_payload is huge.
        return evaluate_art_chunked(session, staged, art_gen);
    }

    if staged.art_payload.is_empty() {
        return Ok(json!({ "ok": false, "reason": "empty-art" }));
    }
    let art_timeout = art_evaluate_timeout_ms(staged).min(30_000);
    if !art_job_still_valid(art_gen) {
        return Ok(json!({ "ok": false, "reason": "art-gen-cancelled" }));
    }
    let art_value = session.evaluate(&staged.art_payload, art_timeout)?;
    Ok(art_value)
}

fn evaluate_art_chunked(
    session: &CdpSession,
    staged: &StagedPayload,
    art_gen: u64,
) -> Result<Value, InjectError> {
    // Need raw bytes: re-read from art_data_url decode is expensive; use Full staged rebuild.
    // staged.art_data_url is "data:mime;base64,..." — decode for chunk pipeline.
    let url = &staged.art_data_url;
    let (mime, raw) = if let Some(rest) = url.strip_prefix("data:") {
        if let Some((meta, b64)) = rest.split_once(',') {
            let mime = meta.split(';').next().unwrap_or("image/png");
            use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
            let bytes = B64
                .decode(b64.as_bytes())
                .map_err(|e| InjectError::msg(format!("art b64 decode: {e}")))?;
            (mime.to_string(), bytes)
        } else {
            return Err(InjectError::msg("bad art data url"));
        }
    } else if !staged.art_payload.is_empty() && staged.art_payload.len() <= ART_SINGLE_EVAL_MAX_CHARS
    {
        // Fall back to single evaluate
        let v = session.evaluate(&staged.art_payload, 20_000)?;
        return Ok(v);
    } else {
        return Err(InjectError::msg("no art bytes for chunked transfer"));
    };

    if !art_job_still_valid(art_gen) {
        return Ok(json!({ "ok": false, "reason": "art-gen-cancelled" }));
    }

    let (begin, appends, finish) =
        assemble_art_chunk_pipeline(&staged.markers, &mime, &raw, &staged.revision);

    let _ = session.evaluate(&begin, 4_000)?;
    for (i, expr) in appends.iter().enumerate() {
        if !art_job_still_valid(art_gen) {
            // Best-effort clear xfer state
            let _ = session.evaluate(
                r#"(() => { try { delete window.__CHATGPT_TOOLS_ART_XFER__; } catch(_){} return {ok:false,reason:"cancelled"}; })()"#,
                2_000,
            );
            return Ok(json!({ "ok": false, "reason": "art-gen-cancelled", "chunk": i }));
        }
        let _ = session.evaluate(expr, 6_000)?;
    }
    if !art_job_still_valid(art_gen) {
        return Ok(json!({ "ok": false, "reason": "art-gen-cancelled" }));
    }
    let art_value = session.evaluate(&finish, 20_000)?;
    Ok(art_value)
}

/// Art-only follow-up (no Operation UI). Safe for background after shell_ready.
/// Uses chunked base64 when large; respects art generation cancellation.
pub fn inject_art_followup(
    skin_dir: &Path,
    project_root: &Path,
    port: u16,
) -> Result<Value, InjectError> {
    let art_gen = art_generation();
    if !art_job_still_valid(art_gen) {
        return Ok(json!({
            "ok": false,
            "artOk": false,
            "artPending": true,
            "reason": "art-gen-cancelled",
        }));
    }

    let staged = build_art_payload_only(skin_dir, project_root)?;
    if staged.art_payload.is_empty() && staged.art_data_url.is_empty() {
        return Ok(json!({
            "ok": true,
            "artOk": true,
            "artPending": false,
            "skipped": true,
        }));
    }
    let targets = list_app_targets(port)?;
    if targets.is_empty() {
        return Ok(json!({
            "ok": false,
            "artOk": false,
            "artPending": true,
            "reason": "no-targets",
        }));
    }

    let mut last_ok = false;
    let mut last_err: Option<String> = None;
    for target in targets.iter().take(2) {
        if !art_job_still_valid(art_gen) {
            last_err = Some("art-gen-cancelled".into());
            break;
        }
        let session = match CdpSession::open(target, port, 6000) {
            Ok(s) => s,
            Err(e) => {
                last_err = Some(e.to_string());
                continue;
            }
        };
        match evaluate_art_on_session(&session, &staged, art_gen) {
            Ok(art_value) => {
                last_ok = art_value
                    .get("ok")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false)
                    || art_value
                        .get("already")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false);
                session.close();
                if last_ok {
                    break;
                }
                if art_value.get("reason").and_then(|r| r.as_str()) == Some("art-gen-cancelled") {
                    last_err = Some("art-gen-cancelled".into());
                    break;
                }
            }
            Err(e) => {
                last_err = Some(e.to_string());
                session.close();
            }
        }
    }
    Ok(json!({
        "ok": last_ok,
        "artOk": last_ok,
        "artPending": !last_ok,
        "error": last_err,
        "chunked": staged.art_payload.len() > ART_SINGLE_EVAL_MAX_CHARS,
        "engine": "native-rust",
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
    'skins-theme-light','skins-theme-dark','skins-art-wide','skins-art-standard','skins-art-none',
    'skins-focus-left','skins-focus-center','skins-focus-right',
    'skins-safe-left','skins-safe-center','skins-safe-right','skins-safe-none',
    'skins-task-ambient','skins-task-banner','skins-task-off',
    'dream-theme-light','dream-theme-dark','dream-art-wide','dream-art-standard',
    'dream-focus-left','dream-focus-center','dream-focus-right',
    'dream-safe-left','dream-safe-center','dream-safe-right','dream-safe-none',
    'dream-task-ambient','dream-task-banner','dream-task-off'
  ];
  root?.classList.remove(rootClass, ...themeClasses);
  for (const prop of [artVar, '--skins-art', '--skins-art-position', '--skins-focus-x',
    '--skins-focus-y', '--skins-accent', '--skins-accent-ink', '--skins-image-luma',
    '--dream-art', '--dream-art-position', '--dream-focus-x',
    '--dream-focus-y', '--dream-accent', '--dream-accent-ink', '--dream-image-luma'].filter(Boolean)) {{
    root?.style.removeProperty(prop);
  }}
  root?.removeAttribute('data-chatgpt-tools-skin');
  root?.removeAttribute('data-skins-shell');
  root?.removeAttribute('data-skins-art-mode');
  root?.removeAttribute('data-skins-art-paint');
  root?.removeAttribute('data-skin-contract');
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
