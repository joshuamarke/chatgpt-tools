//! In-process skin keep-alive: re-inject when ChatGPT navigates / refreshes.
//!
//! Prefer polling soft-verify over a long-lived Node `injector.mjs --watch`.
//! Runs on a single background thread inside the Tauri process.

use super::host::append_diag;
use super::http::{is_renderer_ready, list_app_targets};
use super::inject::inject_once;
use super::native::{engine_try_lock, is_paused, read_state};
use crate::engine;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const POLL_MS: u64 = 1800;
const INJECT_COOLDOWN_MS: u64 = 2500;
const SOFT_TIMEOUT_MS: u64 = 6000;

#[derive(Clone)]
struct KeepConfig {
    port: u16,
    skin_dir: PathBuf,
    skin_id: String,
    project_root: PathBuf,
    /// Soft markers for presence probe (optional JSON).
    markers: serde_json::Value,
}

static KEEP_STOP: AtomicBool = AtomicBool::new(true);
static KEEP_CFG: Mutex<Option<KeepConfig>> = Mutex::new(None);
static KEEP_STARTED: AtomicBool = AtomicBool::new(false);
static LAST_REINJECT: Mutex<Option<Instant>> = Mutex::new(None);

/// Soft presence: root class + style id on documentElement.
fn soft_present_expression(markers: &serde_json::Value) -> String {
    let root = markers
        .get("rootClass")
        .and_then(|v| v.as_str())
        .unwrap_or("codex-skin");
    let style_id = markers
        .get("styleId")
        .and_then(|v| v.as_str())
        .unwrap_or("codex-skin-style");
    format!(
        r#"(() => {{
  try {{
    const root = document.documentElement;
    const style = document.getElementById({style});
    const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
    const installed = root && root.classList && root.classList.contains({root});
    const stylePresent = Boolean(style);
    const hostOk = host && typeof host.applySkin === "function";
    return {{ pass: Boolean(installed && stylePresent), installed, stylePresent, hostOk }};
  }} catch (e) {{
    return {{ pass: false, error: String(e && e.message || e) }};
  }}
}})()"#,
        style = serde_json::to_string(style_id).unwrap_or_else(|_| "\"codex-skin-style\"".into()),
        root = serde_json::to_string(root).unwrap_or_else(|_| "\"codex-skin\"".into()),
    )
}

fn any_target_missing_skin(cfg: &KeepConfig) -> bool {
    let Ok(targets) = list_app_targets(cfg.port) else {
        return false;
    };
    if targets.is_empty() {
        return false;
    }
    let expr = soft_present_expression(&cfg.markers);
    for target in &targets {
        let session = match super::session::CdpSession::open(target, cfg.port, 4000) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let present = session
            .evaluate(&expr, 3500)
            .ok()
            .and_then(|v| v.get("pass").and_then(|p| p.as_bool()))
            .unwrap_or(false);
        session.close();
        if !present {
            return true;
        }
    }
    false
}

fn try_reinject(cfg: &KeepConfig) {
    if is_paused() {
        return;
    }
    if !is_renderer_ready(cfg.port) {
        return;
    }
    {
        let last = LAST_REINJECT.lock();
        if let Some(t) = *last {
            if t.elapsed() < Duration::from_millis(INJECT_COOLDOWN_MS) {
                return;
            }
        }
    }

    // Avoid fighting user-driven apply/restore.
    let Some(_guard) = engine_try_lock() else {
        return;
    };

    if !any_target_missing_skin(cfg) {
        return;
    }

    *LAST_REINJECT.lock() = Some(Instant::now());
    append_diag(&format!(
        "keep: re-inject skin={} dir={}",
        cfg.skin_id,
        cfg.skin_dir.display()
    ));
    match inject_once(
        &cfg.skin_dir,
        &cfg.project_root,
        cfg.port,
        true,
        SOFT_TIMEOUT_MS,
    ) {
        Ok(v) => {
            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false)
                || v.get("shellOk").and_then(|x| x.as_bool()).unwrap_or(false);
            append_diag(&format!(
                "keep: re-inject done ok={ok} mode={}",
                v.get("shellMode").and_then(|m| m.as_str()).unwrap_or("?")
            ));
        }
        Err(e) => append_diag(&format!("keep: re-inject failed: {e}")),
    }
}

fn keep_loop() {
    append_diag("keep: background re-inject thread started");
    while !KEEP_STOP.load(Ordering::SeqCst) {
        let cfg = KEEP_CFG.lock().clone();
        if let Some(cfg) = cfg {
            // Drop if state cleared (restore) or skin id mismatch
            let still_active = read_state()
                .and_then(|s| {
                    let id = s.get("skinId").and_then(|v| v.as_str())?.to_string();
                    let phase = s
                        .get("phase")
                        .and_then(|v| v.as_str())
                        .unwrap_or("active");
                    Some(id == cfg.skin_id && phase == "active")
                })
                .unwrap_or(false);
            if still_active && !is_paused() {
                try_reinject(&cfg);
            }
        }
        // Sleep in slices so stop is responsive
        for _ in 0..(POLL_MS / 100) {
            if KEEP_STOP.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    append_diag("keep: background re-inject thread stopped");
    KEEP_STARTED.store(false, Ordering::SeqCst);
}

fn ensure_thread() {
    if KEEP_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    KEEP_STOP.store(false, Ordering::SeqCst);
    let _ = thread::Builder::new()
        .name("skin-keep".into())
        .spawn(keep_loop);
}

/// Start or update keep-alive for the active skin (call after successful apply).
pub fn start_keep(port: u16, skin_id: &str, skin_dir: PathBuf, markers: serde_json::Value) {
    let project_root = engine::project_root();
    *KEEP_CFG.lock() = Some(KeepConfig {
        port,
        skin_dir,
        skin_id: skin_id.to_string(),
        project_root,
        markers,
    });
    KEEP_STOP.store(false, Ordering::SeqCst);
    ensure_thread();
    append_diag(&format!("keep: armed skin={skin_id} port={port}"));
}

/// Stop keep-alive (restore / pause / app teardown).
pub fn stop_keep() {
    *KEEP_CFG.lock() = None;
    KEEP_STOP.store(true, Ordering::SeqCst);
    append_diag("keep: disarmed");
}

/// Whether keep thread has a config (for status).
pub fn keep_armed() -> bool {
    KEEP_CFG.lock().is_some() && !KEEP_STOP.load(Ordering::SeqCst)
}


