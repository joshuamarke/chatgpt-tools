//! Host (ChatGPT / Codex desktop) lifecycle probe — parity with engine/host-probe.js.
//!
//! Three independent signals so UI/apply never rely on a single false negative:
//!   processRunning  — OS process named ChatGPT/Codex (best-effort, L2)
//!   debugPortOpen   — loopback CDP HTTP answers
//!   rendererReady   — at least one app:// page target
//!
//! Public `probe_host_lifecycle` applies TTL cache + hysteresis so GUI pills
//! do not flicker on transient CDP / process-list misses.

use super::http::{is_debug_port_open, is_renderer_ready};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
#[cfg(not(windows))]
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Sticky ready window: brief CDP blips keep reporting ready.
const STICKY_READY_MS: u64 = 5_000;
/// Raw offline must persist this long (or N consecutive hits) before stable offline.
const OFFLINE_HOLD_MS: u64 = 3_000;
const OFFLINE_CONFIRM: u32 = 2;
/// L1 CDP result cache TTL for polling.
const CDP_CACHE_MS: u64 = 1_000;
/// Process list cache TTL (expensive on Windows).
const PROCESS_CACHE_MS: u64 = 3_000;
/// Full-status style calls may reuse a slightly longer CDP cache when not forced.
const STATUS_CDP_CACHE_MS: u64 = 1_200;

/// State directory (same layout as native / Node manager).
pub fn state_root() -> PathBuf {
    if let Ok(p) = std::env::var("CODEX_SKIN_MANAGER_STATE") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let name = std::env::var("CODEX_SKIN_STATE_NAME").unwrap_or_else(|_| "ChatGPTTools".into());
    if cfg!(windows) {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
            std::env::var("USERPROFILE")
                .map(|u| format!(r"{u}\AppData\Local"))
                .unwrap_or_else(|_| ".".into())
        });
        PathBuf::from(local).join(name)
    } else if cfg!(target_os = "macos") {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("Library")
            .join("Application Support")
            .join(name)
    } else {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".local")
            .join("share")
            .join(name)
    }
}

/// Append a line to `%STATE%/diag.log` (best-effort, rotated lightly by size in callers).
pub fn append_diag(line: &str) {
    let root = state_root();
    let _ = fs::create_dir_all(&root);
    let log_path = root.join("diag.log");
    if let Ok(meta) = fs::metadata(&log_path) {
        if meta.len() > 2_000_000 {
            let bak = root.join("diag.log.1");
            let _ = fs::remove_file(&bak);
            let _ = fs::rename(&log_path, &bak);
        }
    }
    // Millisecond timestamps so hot-path gaps are measurable (was whole seconds).
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "[{ts_ms}] {line}")
        });
}

#[derive(Debug, Clone)]
pub struct HostLifecycle {
    #[allow(dead_code)]
    pub port: u16,
    pub pids: Vec<u32>,
    pub process_running: bool,
    pub debug_port_open: bool,
    pub renderer_ready: bool,
    /// Stable lifecycle after hysteresis (offline | starting | ready).
    pub lifecycle: &'static str,
    /// Raw three-signal classification before hysteresis.
    pub lifecycle_raw: &'static str,
    /// high | probing | stale
    pub confidence: &'static str,
    pub can_hot_apply: bool,
    pub needs_restart_for_inject: bool,
    /// Age of the underlying probe sample in ms (0 if just taken).
    pub probe_age_ms: u64,
}

impl HostLifecycle {
    pub fn codex_running(&self) -> bool {
        self.process_running || self.debug_port_open || self.renderer_ready
            || self.lifecycle == "ready"
            || self.lifecycle == "starting"
    }

    pub fn host_engaged(&self) -> bool {
        self.lifecycle != "offline"
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TimingBudget {
    pub scale: f64,
    pub wait_debug_port_ms: u64,
    pub wait_renderer_ms: u64,
    pub soft_once_timeout_ms: u64,
    /// Deprecated: event-driven wait replaced fixed sleeps (kept for API compat).
    #[allow(dead_code)]
    pub launch_settle_ms: u64,
    #[allow(dead_code)]
    pub stop_settle_ms: u64,
    pub poll_ms: u64,
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn slow_scale_env() -> f64 {
    std::env::var("CODEX_SKIN_SLOW_SCALE")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(0.0)
}

/// Adaptive apply/launch budgets (aligned with host-probe.resolveTimingBudget).
///
/// Production note: slow first paint must not fail the apply path. When the host
/// is still starting (or just relaunched), scale waits so soft verify / renderer
/// readiness can absorb Store cold starts without killing a healthy session.
pub fn resolve_timing_budget(seed: Option<&HostLifecycle>) -> TimingBudget {
    let env_scale = slow_scale_env();
    let starting = seed.map(|s| s.lifecycle == "starting").unwrap_or(false);
    let offline = seed.map(|s| s.lifecycle == "offline").unwrap_or(false);
    let scale = if env_scale > 0.0 {
        env_scale.clamp(1.0, 3.0)
    } else if starting || offline {
        // Cold / first paint: slightly more patient than a hot ready host.
        1.6
    } else {
        1.0
    };
    TimingBudget {
        scale,
        wait_debug_port_ms: (28_000.0 * scale).round() as u64,
        // Up to ~72s at scale 1.6 — pairs with ensure_debug_port verify window (≥90s).
        wait_renderer_ms: (45_000.0 * scale).round() as u64,
        // Soft inject verify on cold path needs room for SPA hydrate (not just 8s).
        soft_once_timeout_ms: if starting || offline {
            (14_000.0 * scale).round() as u64
        } else {
            (8_000.0 * scale).round() as u64
        },
        // Legacy fields kept for TimingBudget shape; settle is event-driven now.
        launch_settle_ms: 0,
        stop_settle_ms: 0,
        poll_ms: if scale > 1.3 { 280 } else { 180 },
    }
}

#[cfg(not(windows))]
fn parse_pid_lines(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Ok(id) = t.parse::<u32>() {
            if id > 0 {
                out.push(id);
            }
        }
    }
    out
}

/// Best-effort main process PIDs.
/// Windows: Toolhelp32 + path filter (no PowerShell / tasklist).
/// macOS: pgrep. Other: empty.
pub fn find_host_main_pids() -> Vec<u32> {
    #[cfg(windows)]
    {
        return super::win_native::find_host_pids_toolhelp();
    }

    #[cfg(not(windows))]
    {
        let mut pids: Vec<u32> = Vec::new();
        if cfg!(target_os = "macos") {
            if let Ok(output) = Command::new("pgrep")
                .args([
                    "-f",
                    r"/Applications/ChatGPT\.app/Contents/MacOS/ChatGPT|/Applications/Codex\.app/Contents/MacOS/(ChatGPT|Codex)|/ChatGPT\.app/Contents/MacOS/ChatGPT",
                ])
                .output()
            {
                if output.status.success() {
                    for id in parse_pid_lines(&String::from_utf8_lossy(&output.stdout)) {
                        if !pids.contains(&id) {
                            pids.push(id);
                        }
                    }
                }
            }
        }
        pids
    }
}

fn classify_raw(process_running: bool, debug_port_open: bool, renderer_ready: bool) -> &'static str {
    if renderer_ready {
        "ready"
    } else if process_running || debug_port_open {
        "starting"
    } else {
        "offline"
    }
}

// ── Probe cache + hysteresis ───────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ProcessCache {
    at: Instant,
    pids: Vec<u32>,
}

#[derive(Debug, Clone)]
struct CdpCache {
    at: Instant,
    port: u16,
    debug_port_open: bool,
    renderer_ready: bool,
}

#[derive(Debug, Clone)]
struct HysteresisState {
    stable_lifecycle: &'static str,
    last_ready_at: Option<Instant>,
    offline_since: Option<Instant>,
    offline_hits: u32,
    last_flip_log: Option<&'static str>,
}

impl Default for HysteresisState {
    fn default() -> Self {
        Self {
            stable_lifecycle: "offline",
            last_ready_at: None,
            offline_since: None,
            offline_hits: 0,
            last_flip_log: None,
        }
    }
}

struct ProbeService {
    process: Option<ProcessCache>,
    cdp: Option<CdpCache>,
    hyst: HysteresisState,
    /// Last published snapshot (for L0).
    last: Option<(Instant, HostLifecycle)>,
}

impl Default for ProbeService {
    fn default() -> Self {
        Self {
            process: None,
            cdp: None,
            hyst: HysteresisState::default(),
            last: None,
        }
    }
}

static PROBE: Mutex<ProbeService> = Mutex::new(ProbeService {
    process: None,
    cdp: None,
    hyst: HysteresisState {
        stable_lifecycle: "offline",
        last_ready_at: None,
        offline_since: None,
        offline_hits: 0,
        last_flip_log: None,
    },
    last: None,
});

/// Monotonic counter for diagnostics (optional).
static PROBE_GEN: AtomicU64 = AtomicU64::new(0);

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Probe options: force skips TTL; full_process forces L2 process scan.
#[derive(Debug, Clone, Copy)]
pub struct ProbeOpts {
    pub force: bool,
    pub full_process: bool,
    /// When false, may skip process scan if CDP already proves host up (L1).
    pub need_process: bool,
}

impl Default for ProbeOpts {
    fn default() -> Self {
        Self {
            force: false,
            full_process: false,
            need_process: true,
        }
    }
}

impl ProbeOpts {
    pub fn force() -> Self {
        Self {
            force: true,
            full_process: true,
            need_process: true,
        }
    }
}

fn probe_cdp_parallel(port: u16) -> (bool, bool) {
    let (tx_port, rx_port) = std::sync::mpsc::channel();
    let (tx_rend, rx_rend) = std::sync::mpsc::channel();
    // Short timeouts: GUI host pill / boot must not wait multi-seconds when
    // nothing listens on the debug port (common cold start).
    std::thread::spawn(move || {
        let _ = tx_port.send(is_debug_port_open(port, 450));
    });
    std::thread::spawn(move || {
        let _ = tx_rend.send(is_renderer_ready(port));
    });
    let debug_port_open = rx_port.recv_timeout(Duration::from_millis(800)).unwrap_or(false);
    let renderer_ready = rx_rend.recv_timeout(Duration::from_millis(800)).unwrap_or(false);
    // If renderer is ready, port must be open (list succeeded).
    let debug_port_open = debug_port_open || renderer_ready;
    (debug_port_open, renderer_ready)
}

fn get_process_pids(svc: &mut ProbeService, force: bool, full: bool) -> Vec<u32> {
    if !force {
        if let Some(ref c) = svc.process {
            if c.at.elapsed() < Duration::from_millis(PROCESS_CACHE_MS) {
                return c.pids.clone();
            }
        }
    }
    // L2 only when full or cache miss with force path already decided by caller
    let _ = full;
    let pids = find_host_main_pids();
    svc.process = Some(ProcessCache {
        at: Instant::now(),
        pids: pids.clone(),
    });
    pids
}

fn apply_hysteresis(
    hyst: &mut HysteresisState,
    raw: &'static str,
    debug_port_open: bool,
) -> (&'static str, &'static str) {
    let now = Instant::now();
    match raw {
        "ready" => {
            hyst.last_ready_at = Some(now);
            hyst.offline_since = None;
            hyst.offline_hits = 0;
            if hyst.stable_lifecycle != "ready" {
                if hyst.last_flip_log != Some("ready") {
                    append_diag(&format!(
                        "host lifecycle flip raw=ready stable={} → ready",
                        hyst.stable_lifecycle
                    ));
                    hyst.last_flip_log = Some("ready");
                }
            }
            hyst.stable_lifecycle = "ready";
            ("ready", "high")
        }
        "starting" => {
            hyst.offline_since = None;
            hyst.offline_hits = 0;
            // Sticky ready: brief loss of app:// while port still up (or just was ready)
            if hyst.stable_lifecycle == "ready" {
                if let Some(t) = hyst.last_ready_at {
                    if t.elapsed() < Duration::from_millis(STICKY_READY_MS)
                        && (debug_port_open || t.elapsed() < Duration::from_millis(2_000))
                    {
                        return ("ready", "probing");
                    }
                }
            }
            if hyst.stable_lifecycle != "starting" && hyst.stable_lifecycle != "ready" {
                if hyst.last_flip_log != Some("starting") {
                    append_diag("host lifecycle flip → starting");
                    hyst.last_flip_log = Some("starting");
                }
            }
            // Leaving sticky ready for real starting
            if hyst.stable_lifecycle == "ready" {
                append_diag("host lifecycle sticky ready expired → starting");
                hyst.last_flip_log = Some("starting");
            }
            hyst.stable_lifecycle = "starting";
            ("starting", "high")
        }
        _ => {
            // offline raw
            hyst.offline_hits = hyst.offline_hits.saturating_add(1);
            if hyst.offline_since.is_none() {
                hyst.offline_since = Some(now);
            }
            let held = hyst
                .offline_since
                .map(|t| t.elapsed() >= Duration::from_millis(OFFLINE_HOLD_MS))
                .unwrap_or(false);
            let confirmed = hyst.offline_hits >= OFFLINE_CONFIRM && held
                || hyst.offline_hits >= OFFLINE_CONFIRM + 1;

            if hyst.stable_lifecycle == "ready" {
                if let Some(t) = hyst.last_ready_at {
                    if t.elapsed() < Duration::from_millis(STICKY_READY_MS) && !confirmed {
                        return ("ready", "probing");
                    }
                }
            }

            if confirmed || hyst.stable_lifecycle == "offline" {
                if hyst.stable_lifecycle != "offline" {
                    append_diag(&format!(
                        "host lifecycle flip → offline (hits={} held={})",
                        hyst.offline_hits, held
                    ));
                    hyst.last_flip_log = Some("offline");
                }
                hyst.stable_lifecycle = "offline";
                ("offline", if confirmed { "high" } else { "probing" })
            } else {
                // Hold previous non-offline state while confirming
                let keep = if hyst.stable_lifecycle == "offline" {
                    "offline"
                } else {
                    hyst.stable_lifecycle
                };
                (keep, "probing")
            }
        }
    }
}

fn build_lifecycle(
    port: u16,
    pids: Vec<u32>,
    debug_port_open: bool,
    renderer_ready: bool,
    lifecycle: &'static str,
    lifecycle_raw: &'static str,
    confidence: &'static str,
    probe_age_ms: u64,
) -> HostLifecycle {
    let process_running = !pids.is_empty();
    // canHotApply: true renderer, or sticky ready with open port
    let can_hot_apply =
        renderer_ready || (lifecycle == "ready" && debug_port_open);
    let needs_restart_for_inject = process_running && !debug_port_open;
    HostLifecycle {
        port,
        pids,
        process_running,
        debug_port_open,
        renderer_ready,
        lifecycle,
        lifecycle_raw,
        confidence,
        can_hot_apply,
        needs_restart_for_inject,
        probe_age_ms,
    }
}

/// Cached + hysteresis probe (default for status / GUI).
pub fn probe_host_lifecycle(port: u16) -> HostLifecycle {
    probe_host_lifecycle_opts(port, ProbeOpts::default())
}

/// Force fresh signals (apply / ensure_debug_port / host_status force).
pub fn probe_host_lifecycle_force(port: u16) -> HostLifecycle {
    probe_host_lifecycle_opts(port, ProbeOpts::force())
}

pub fn probe_host_lifecycle_opts(port: u16, opts: ProbeOpts) -> HostLifecycle {
    let mut svc = PROBE.lock();
    let gen = PROBE_GEN.fetch_add(1, Ordering::Relaxed);

    // L0: return last snapshot if very fresh and not forced
    if !opts.force {
        if let Some((at, ref snap)) = svc.last {
            if snap.port == port && at.elapsed() < Duration::from_millis(CDP_CACHE_MS) {
                let mut out = snap.clone();
                out.probe_age_ms = at.elapsed().as_millis() as u64;
                if out.confidence == "high" {
                    out.confidence = "stale";
                }
                return out;
            }
        }
    }

    // L1 CDP (cache unless force)
    let (debug_port_open, renderer_ready) = if !opts.force {
        if let Some(ref c) = svc.cdp {
            if c.port == port && c.at.elapsed() < Duration::from_millis(STATUS_CDP_CACHE_MS) {
                (c.debug_port_open, c.renderer_ready)
            } else {
                let pair = probe_cdp_parallel(port);
                svc.cdp = Some(CdpCache {
                    at: Instant::now(),
                    port,
                    debug_port_open: pair.0,
                    renderer_ready: pair.1,
                });
                pair
            }
        } else {
            let pair = probe_cdp_parallel(port);
            svc.cdp = Some(CdpCache {
                at: Instant::now(),
                port,
                debug_port_open: pair.0,
                renderer_ready: pair.1,
            });
            pair
        }
    } else {
        let pair = probe_cdp_parallel(port);
        svc.cdp = Some(CdpCache {
            at: Instant::now(),
            port,
            debug_port_open: pair.0,
            renderer_ready: pair.1,
        });
        pair
    };

    // Process: L2 when forced/full, when CDP says offline, or when need_process for restart hint
    let host_up_cdp = debug_port_open || renderer_ready;
    let should_scan_process = opts.force
        || opts.full_process
        || opts.need_process
        || !host_up_cdp
        || svc
            .process
            .as_ref()
            .map(|p| p.at.elapsed() >= Duration::from_millis(PROCESS_CACHE_MS))
            .unwrap_or(true);

    let pids = if should_scan_process {
        // When CDP already ready and not force, reuse process cache if present
        if host_up_cdp && !opts.force && !opts.full_process {
            if let Some(ref c) = svc.process {
                if c.at.elapsed() < Duration::from_millis(PROCESS_CACHE_MS) {
                    c.pids.clone()
                } else {
                    // Optional light refresh: skip expensive scan when renderer ready
                    c.pids.clone()
                }
            } else {
                // No process info yet — one scan for needsRestart accuracy
                get_process_pids(&mut svc, true, true)
            }
        } else if !host_up_cdp || opts.force || opts.full_process {
            get_process_pids(&mut svc, opts.force, true)
        } else {
            svc.process
                .as_ref()
                .map(|c| c.pids.clone())
                .unwrap_or_default()
        }
    } else {
        svc.process
            .as_ref()
            .map(|c| c.pids.clone())
            .unwrap_or_default()
    };

    let process_running = !pids.is_empty();
    let lifecycle_raw = classify_raw(process_running, debug_port_open, renderer_ready);
    let (lifecycle, confidence) =
        apply_hysteresis(&mut svc.hyst, lifecycle_raw, debug_port_open);

    let life = build_lifecycle(
        port,
        pids,
        debug_port_open,
        renderer_ready,
        lifecycle,
        lifecycle_raw,
        confidence,
        0,
    );

    if gen % 20 == 0 || lifecycle != lifecycle_raw {
        append_diag(&format!(
            "host probe port={port} raw={lifecycle_raw} stable={lifecycle} conf={confidence} process={} portOpen={} renderer={}",
            life.process_running, debug_port_open, renderer_ready
        ));
    }

    svc.last = Some((Instant::now(), life.clone()));
    life
}

/// Invalidate caches after apply/restore/launch so the next probe is fresh.
pub fn invalidate_host_probe_cache() {
    let mut svc = PROBE.lock();
    svc.cdp = None;
    svc.process = None;
    svc.last = None;
}

/// Clear lifecycle sticky/hysteresis (call on forced restart so offline is not
/// reported as ready while the host is dying).
pub fn invalidate_host_lifecycle_sticky() {
    let mut svc = PROBE.lock();
    svc.cdp = None;
    svc.process = None;
    svc.last = None;
    svc.hyst = HysteresisState::default();
    // Also clear boot appearance snap so next engage re-reads config.
    {
        let mut boot = BOOT_APPEARANCE.lock();
        boot.theme = None;
        boot.pids.clear();
        boot.engaged = false;
    }
}

/// Publish a known-good ready snapshot (after successful ensure_debug_port).
pub fn note_host_ready(port: u16) {
    let mut svc = PROBE.lock();
    svc.hyst.stable_lifecycle = "ready";
    svc.hyst.last_ready_at = Some(Instant::now());
    svc.hyst.offline_since = None;
    svc.hyst.offline_hits = 0;
    let life = build_lifecycle(port, vec![], true, true, "ready", "ready", "high", 0);
    svc.cdp = Some(CdpCache {
        at: Instant::now(),
        port,
        debug_port_open: true,
        renderer_ready: true,
    });
    svc.last = Some((Instant::now(), life));
}

/// Wait until lifecycle is one of `want` (e.g. "ready", "starting").
pub fn wait_for_host_lifecycle(
    port: u16,
    want: &[&str],
    timeout_ms: u64,
    poll_ms: u64,
) -> HostLifecycle {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut last = probe_host_lifecycle_force(port);
    while Instant::now() < deadline {
        last = probe_host_lifecycle_force(port);
        if want.iter().any(|w| *w == last.lifecycle) {
            return last;
        }
        sleep_ms(poll_ms);
    }
    last
}

pub fn wait_until_renderer_ready(port: u16, timeout_ms: u64, poll_ms: u64) -> bool {
    let snap = wait_for_host_lifecycle(port, &["ready"], timeout_ms, poll_ms);
    snap.renderer_ready || snap.lifecycle == "ready"
}

/// Snapshot of config.toml appearanceTheme as observed when the host process
/// first became engaged. Codex only reloads [desktop] on process start, so
/// comparing skin desktopTheme.appearanceTheme against this value tells the
/// GUI whether a restart is required for chrome theme to take effect.
struct BootAppearanceSnap {
    theme: Option<String>,
    pids: Vec<u32>,
    engaged: bool,
}

static BOOT_APPEARANCE: Mutex<BootAppearanceSnap> = Mutex::new(BootAppearanceSnap {
    theme: None,
    pids: Vec::new(),
    engaged: false,
});

fn pids_changed(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return true;
    }
    let mut sa = a.to_vec();
    let mut sb = b.to_vec();
    sa.sort_unstable();
    sb.sort_unstable();
    sa != sb
}

/// Track host boot appearance: capture config when process engages; reset on offline / PID flip.
fn sync_boot_appearance_theme(life: &HostLifecycle) -> Option<String> {
    let mut snap = BOOT_APPEARANCE.lock();
    let engaged = life.host_engaged() || life.process_running;
    if !engaged {
        snap.engaged = false;
        snap.theme = None;
        snap.pids.clear();
        return None;
    }
    let restart_cycle = !snap.engaged || pids_changed(&snap.pids, &life.pids);
    if restart_cycle {
        snap.theme = super::theme::read_appearance_theme();
        snap.pids = life.pids.clone();
        snap.engaged = true;
    }
    snap.theme.clone()
}

/// Compact JSON for GUI polling (`host_status` command).
pub fn host_status_json(port: u16, force: bool, keep_alive: bool) -> Value {
    let life = if force {
        probe_host_lifecycle_force(port)
    } else {
        probe_host_lifecycle(port)
    };
    host_lifecycle_to_json(&life, keep_alive)
}

pub fn host_lifecycle_to_json(life: &HostLifecycle, keep_alive: bool) -> Value {
    let boot_theme = sync_boot_appearance_theme(life);
    let config_theme = super::theme::read_appearance_theme();
    // Lightweight session art/shell flags so GUI host polls can clear
    // 「立绘加载中」 without a full status/catalog refresh.
    let session = read_session_art_flags();
    json!({
        "ok": true,
        "port": life.port,
        "processRunning": life.process_running,
        "debugPortOpen": life.debug_port_open,
        "rendererReady": life.renderer_ready,
        "debugReady": life.renderer_ready || life.lifecycle == "ready",
        "lifecycle": life.lifecycle,
        "lifecycleRaw": life.lifecycle_raw,
        "lifecycleLabel": life.lifecycle,
        "confidence": life.confidence,
        "codexRunning": life.codex_running(),
        "canHotApply": life.can_hot_apply,
        "needsRestartForInject": life.needs_restart_for_inject,
        "hostPids": life.pids,
        "keepAlive": keep_alive,
        "probeAgeMs": life.probe_age_ms,
        "probedAt": now_unix_ms(),
        "hostBootAppearanceTheme": boot_theme,
        "configAppearanceTheme": config_theme,
        "shellOk": session.shell_ok,
        "artOk": session.art_ok,
        "artPending": session.art_pending,
        "skinId": session.skin_id,
        "phase": session.phase,
        "signals": {
            "process": life.process_running,
            "port": life.debug_port_open,
            "renderer": life.renderer_ready,
        },
        "engine": "native-rust",
    })
}

struct SessionArtFlags {
    shell_ok: bool,
    art_ok: bool,
    art_pending: bool,
    skin_id: Value,
    phase: Value,
}

/// Read session art/shell flags from state.json without importing native
/// (avoids host ↔ native module cycle).
fn read_session_art_flags() -> SessionArtFlags {
    let st = fs::read_to_string(state_root().join("state.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let shell_ok = st
        .as_ref()
        .and_then(|s| s.get("shellOk").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    let art_ok = st
        .as_ref()
        .and_then(|s| s.get("artOk").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    // Only treat as pending when explicitly true AND art not already ok.
    // Stale state with artPending=true + artOk=true must not show loading.
    let raw_pending = st
        .as_ref()
        .and_then(|s| s.get("artPending").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    let art_pending = raw_pending && !art_ok;
    SessionArtFlags {
        shell_ok,
        art_ok,
        art_pending,
        skin_id: st
            .as_ref()
            .and_then(|s| s.get("skinId").cloned())
            .unwrap_or(Value::Null),
        phase: st
            .as_ref()
            .and_then(|s| s.get("phase").cloned())
            .unwrap_or(Value::Null),
    }
}
