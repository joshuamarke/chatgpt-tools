//! In-process skin keep-alive: re-inject when ChatGPT navigates / refreshes.
//!
//! Design (steady-state first):
//! - Healthy host: exponential backoff (almost no CDP).
//! - Soft probe only when a tick is due; reinject shell only on miss.
//! - After full document wipe, optionally restore art once (cached path).
//! - Prefer this over a long-lived Node `injector.mjs --watch`.
//!
//! Runs on a single background thread inside the Tauri process.

use super::host::append_diag;
use super::http::{is_renderer_ready, list_app_targets};
use super::inject::{inject_art_followup, inject_once_with_opts, InjectOnceOpts};
use super::native::{engine_try_lock, is_paused, patch_state_art_flags, read_state};
use crate::engine;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Fast probe while recovering from a miss / just after arm.
const POLL_FAST_MS: u64 = 2_000;
/// After a few consecutive passes.
const POLL_MED_MS: u64 = 6_000;
/// Steady state: skin present, no recent miss.
const POLL_STEADY_MS: u64 = 15_000;
/// Ceiling while host stays healthy for a long time.
const POLL_MAX_MS: u64 = 30_000;

const INJECT_COOLDOWN_MS: u64 = 2_500;
const ART_RESTORE_COOLDOWN_MS: u64 = 8_000;
const SOFT_TIMEOUT_MS: u64 = 6_000;
/// Consecutive soft-pass ticks before entering steady backoff.
const STEADY_PASS_THRESHOLD: u32 = 3;

/// Generation token for background art jobs. Bumped on stop_host / restart so
/// in-flight art evaluate is ignored and new art is not started against a dying host.
static ART_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Bump art generation (call before kill / forced restart). Returns new value.
pub fn bump_art_generation() -> u64 {
    let g = ART_GENERATION.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    append_diag(&format!("art_generation bump → {g}"));
    g
}

pub fn art_generation() -> u64 {
    ART_GENERATION.load(Ordering::SeqCst)
}

/// True when a background art job started with `started_gen` is still valid.
pub fn art_job_still_valid(started_gen: u64) -> bool {
    ART_GENERATION.load(Ordering::SeqCst) == started_gen
}

#[derive(Clone)]
struct KeepConfig {
    port: u16,
    skin_dir: PathBuf,
    skin_id: String,
    project_root: PathBuf,
    /// Soft markers for presence probe (optional JSON).
    markers: serde_json::Value,
    /// Skin declares wallpaper — keep may restore art after full wipe.
    has_art: bool,
}

/// Runtime cadence (snapshotted per tick so CDP work never holds the mutex).
#[derive(Clone)]
struct KeepCadence {
    consecutive_pass: u32,
    poll_ms: u64,
    last_reinject: Option<Instant>,
    last_art_restore: Option<Instant>,
}

impl KeepCadence {
    fn fresh() -> Self {
        Self {
            consecutive_pass: 0,
            poll_ms: POLL_FAST_MS,
            last_reinject: None,
            last_art_restore: None,
        }
    }

    fn on_pass(&mut self) {
        self.consecutive_pass = self.consecutive_pass.saturating_add(1);
        self.poll_ms = if self.consecutive_pass >= STEADY_PASS_THRESHOLD + 4 {
            POLL_MAX_MS
        } else if self.consecutive_pass >= STEADY_PASS_THRESHOLD {
            POLL_STEADY_MS
        } else if self.consecutive_pass >= 1 {
            POLL_MED_MS
        } else {
            POLL_FAST_MS
        };
    }

    fn on_miss(&mut self) {
        self.consecutive_pass = 0;
        self.poll_ms = POLL_FAST_MS;
    }

    fn on_arm(&mut self) {
        self.consecutive_pass = 0;
        self.poll_ms = POLL_FAST_MS;
        self.last_reinject = None;
        // Allow art restore after a fresh arm (new apply already owns first art).
        self.last_art_restore = Some(Instant::now());
    }
}

static KEEP_STOP: AtomicBool = AtomicBool::new(true);
static KEEP_CFG: Mutex<Option<KeepConfig>> = Mutex::new(None);
static KEEP_STARTED: AtomicBool = AtomicBool::new(false);
/// Shared cadence so arm/stop can reset without racing the loop body incorrectly.
static KEEP_CADENCE: Mutex<KeepCadence> = Mutex::new(KeepCadence {
    consecutive_pass: 0,
    poll_ms: POLL_FAST_MS,
    last_reinject: None,
    last_art_restore: None,
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SoftPresence {
    pass: bool,
    installed: bool,
    style_present: bool,
    host_ok: bool,
    art_ready: bool,
}

/// Soft presence: root class + style id; optionally artReady for restore decisions.
fn soft_present_expression(markers: &serde_json::Value, expect_art: bool) -> String {
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
  try {{
    const root = document.documentElement;
    const style = document.getElementById({style});
    const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
    const state = window[{state_key}];
    const installed = root && root.classList && root.classList.contains({root});
    const stylePresent = Boolean(style);
    const hostOk = host && typeof host.applySkin === "function";
    const artReady = Boolean(state && state.artReady && state.artUrl);
    const shellPass = Boolean(installed && stylePresent);
    // expectArt is compile-time for this probe; art is advisory unless shell missing.
    return {{
      pass: shellPass,
      installed,
      stylePresent,
      hostOk,
      artReady,
      expectArt: {expect_art}
    }};
  }} catch (e) {{
    return {{ pass: false, installed: false, stylePresent: false, hostOk: false, artReady: false, error: String(e && e.message || e) }};
  }}
}})()"#,
        style = serde_json::to_string(style_id).unwrap_or_else(|_| "\"codex-skin-style\"".into()),
        root = serde_json::to_string(root).unwrap_or_else(|_| "\"codex-skin\"".into()),
        state_key = serde_json::to_string(state_key).unwrap_or_else(|_| "\"__CODEX_SKIN_STATE__\"".into()),
        expect_art = if expect_art { "true" } else { "false" },
    )
}

fn parse_presence(v: &serde_json::Value) -> SoftPresence {
    SoftPresence {
        pass: v.get("pass").and_then(|p| p.as_bool()).unwrap_or(false),
        installed: v
            .get("installed")
            .and_then(|p| p.as_bool())
            .unwrap_or(false),
        style_present: v
            .get("stylePresent")
            .and_then(|p| p.as_bool())
            .unwrap_or(false),
        host_ok: v.get("hostOk").and_then(|p| p.as_bool()).unwrap_or(false),
        art_ready: v.get("artReady").and_then(|p| p.as_bool()).unwrap_or(false),
    }
}

/// Probe the primary app:// page only (skip aux windows that never hold shell).
/// Returns None when no target / CDP unavailable (not a miss).
fn probe_primary_presence(cfg: &KeepConfig) -> Option<SoftPresence> {
    let targets = list_app_targets(cfg.port).ok()?;
    if targets.is_empty() {
        return None;
    }
    // Prefer first target — matches inject soft-once "first pass wins" model.
    let target = &targets[0];
    let session = super::session::CdpSession::open(target, cfg.port, 3500).ok()?;
    let expr = soft_present_expression(&cfg.markers, cfg.has_art);
    let presence = session
        .evaluate(&expr, 3000)
        .ok()
        .map(|v| parse_presence(&v));
    session.close();
    presence
}

fn reinject_allowed(cadence: &KeepCadence) -> bool {
    match cadence.last_reinject {
        Some(t) => t.elapsed() >= Duration::from_millis(INJECT_COOLDOWN_MS),
        None => true,
    }
}

fn art_restore_allowed(cadence: &KeepCadence) -> bool {
    match cadence.last_art_restore {
        Some(t) => t.elapsed() >= Duration::from_millis(ART_RESTORE_COOLDOWN_MS),
        None => true,
    }
}

fn try_recover(cfg: &KeepConfig, cadence: &mut KeepCadence) {
    if is_paused() {
        return;
    }
    if !is_renderer_ready(cfg.port) {
        return;
    }

    let Some(presence) = probe_primary_presence(cfg) else {
        // No target / probe failed — do not treat as miss; stay on current cadence.
        return;
    };

    if presence.pass {
        cadence.on_pass();
        // Shell OK but art wiped (full reload): restore wallpaper once, without shell re-eval.
        if cfg.has_art && !presence.art_ready && art_restore_allowed(cadence) {
            if let Some(_guard) = engine_try_lock() {
                append_diag(&format!(
                    "keep: art restore skin={} (shell present, art missing)",
                    cfg.skin_id
                ));
                cadence.last_art_restore = Some(Instant::now());
                match inject_art_followup(&cfg.skin_dir, &cfg.project_root, cfg.port) {
                    Ok(v) => {
                        let ok = v.get("artOk").and_then(|x| x.as_bool()).unwrap_or(false)
                            || v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
                        append_diag(&format!("keep: art restore done ok={ok}"));
                        patch_state_art_flags(&cfg.skin_id, ok);
                    }
                    Err(e) => {
                        append_diag(&format!("keep: art restore failed: {e}"));
                        // Failed restore is terminal for this attempt — clear pending so GUI
                        // does not stick on 「立绘加载中」until the next apply.
                        patch_state_art_flags(&cfg.skin_id, false);
                    }
                }
            }
        }
        return;
    }

    // Shell missing.
    cadence.on_miss();
    if !reinject_allowed(cadence) {
        return;
    }

    let Some(_guard) = engine_try_lock() else {
        return;
    };

    // Re-check under lock — apply may have just finished.
    let Some(again) = probe_primary_presence(cfg) else {
        return;
    };
    if again.pass {
        cadence.on_pass();
        return;
    }

    cadence.last_reinject = Some(Instant::now());
    append_diag(&format!(
        "keep: re-inject shell skin={} dir={}",
        cfg.skin_id,
        cfg.skin_dir.display()
    ));
    // Silent keep: shell only first (no heavy art in the same CDP burst).
    match inject_once_with_opts(
        &cfg.skin_dir,
        &cfg.project_root,
        cfg.port,
        InjectOnceOpts {
            soft: true,
            timeout_ms: SOFT_TIMEOUT_MS,
            attach_art: false,
        },
    ) {
        Ok(v) => {
            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false)
                || v.get("shellOk").and_then(|x| x.as_bool()).unwrap_or(false);
            append_diag(&format!(
                "keep: re-inject done ok={ok} mode={}",
                v.get("shellMode").and_then(|m| m.as_str()).unwrap_or("?")
            ));
            if ok && cfg.has_art && art_restore_allowed(cadence) {
                cadence.last_art_restore = Some(Instant::now());
                match inject_art_followup(&cfg.skin_dir, &cfg.project_root, cfg.port) {
                    Ok(art) => {
                        let art_ok = art.get("artOk").and_then(|x| x.as_bool()).unwrap_or(false);
                        append_diag(&format!("keep: post-shell art restore ok={art_ok}"));
                        patch_state_art_flags(&cfg.skin_id, art_ok);
                    }
                    Err(e) => {
                        append_diag(&format!("keep: post-shell art restore failed: {e}"));
                        patch_state_art_flags(&cfg.skin_id, false);
                    }
                }
            }
        }
        Err(e) => append_diag(&format!("keep: re-inject failed: {e}")),
    }
}

fn sleep_interruptible(total_ms: u64) {
    for _ in 0..(total_ms / 100).max(1) {
        if KEEP_STOP.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn keep_loop() {
    append_diag("keep: background re-inject thread started");
    while !KEEP_STOP.load(Ordering::SeqCst) {
        let cfg = KEEP_CFG.lock().clone();
        let poll_ms = if let Some(cfg) = cfg {
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
                // Snapshot cadence, run recovery without holding the mutex across CDP,
                // then write back (arm/stop may reset cadence in parallel — last write wins).
                let mut cadence = KEEP_CADENCE.lock().clone();
                try_recover(&cfg, &mut cadence);
                let ms = cadence.poll_ms;
                *KEEP_CADENCE.lock() = cadence;
                ms
            } else {
                POLL_STEADY_MS
            }
        } else {
            POLL_STEADY_MS
        };
        sleep_interruptible(poll_ms.clamp(POLL_FAST_MS, POLL_MAX_MS));
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
pub fn start_keep(
    port: u16,
    skin_id: &str,
    skin_dir: PathBuf,
    markers: serde_json::Value,
    has_art: bool,
) {
    let project_root = engine::project_root();
    *KEEP_CFG.lock() = Some(KeepConfig {
        port,
        skin_dir,
        skin_id: skin_id.to_string(),
        project_root,
        markers,
        has_art,
    });
    {
        let mut cadence = KEEP_CADENCE.lock();
        cadence.on_arm();
    }
    KEEP_STOP.store(false, Ordering::SeqCst);
    ensure_thread();
    append_diag(&format!(
        "keep: armed skin={skin_id} port={port} hasArt={has_art} cadence=fast→steady"
    ));
}

/// Stop keep-alive (restore / pause / app teardown / host restart).
/// Also bumps art generation so background evaluate is cancelled.
pub fn stop_keep() {
    *KEEP_CFG.lock() = None;
    KEEP_STOP.store(true, Ordering::SeqCst);
    *KEEP_CADENCE.lock() = KeepCadence::fresh();
    let _ = bump_art_generation();
    append_diag("keep: disarmed");
}

/// Whether keep thread has a config (for status).
pub fn keep_armed() -> bool {
    KEEP_CFG.lock().is_some() && !KEEP_STOP.load(Ordering::SeqCst)
}
