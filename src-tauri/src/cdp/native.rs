//! High-level native engine ops: apply (hot + cold), status, restore, packages.
//! Single path — no Node CLI fallback.

use super::host::{
    self, append_diag, host_lifecycle_to_json, host_status_json, invalidate_host_lifecycle_sticky,
    invalidate_host_probe_cache, note_host_ready, probe_host_lifecycle, probe_host_lifecycle_force,
    resolve_timing_budget, wait_until_renderer_ready,
};
use super::http::read_browser_identity;
use super::inject::{
    begin_apply_operation, dismiss_apply_operation, finish_apply_operation, inject_art_followup,
    inject_once_with_staged, next_op_token, op_token_current, remove_once, restamp_apply_operation,
    soft_shell_present, wait_until_injectable, InjectOnceOpts, OperationKind,
};
use super::keep::{
    art_generation, art_job_still_valid, keep_armed, start_keep, stop_keep,
};
use super::launch::{
    ensure_debug_port, inject_budget_from, read_last_store_package, restart_host_fire_and_forget,
    store_package_status_json,
};
use super::payload::build_staged_payload;
use super::theme::{self, apply_desktop_theme, restore_desktop_theme};
use crate::engine::{self, EngineError};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const ENGINE_NAME: &str = "chatgpt-tools-engine";
pub const ENGINE_VERSION: &str = "2.4.1";
pub const ENGINE_PROTOCOL: u32 = 2;
/// state.json schema: 2 = native keep; 3 = + Store package identity fields
pub const STATE_SCHEMA: u32 = 3;
pub const SHARED_PORT: u16 = 9335;

/// Serialize apply/restore so concurrent GUI clicks cannot interleave CDP ops.
static ENGINE_LOCK: Mutex<()> = Mutex::new(());

/// Non-blocking lock for keep-alive re-inject (skip if apply/restore in progress).
pub(crate) fn engine_try_lock() -> Option<parking_lot::MutexGuard<'static, ()>> {
    ENGINE_LOCK.try_lock()
}

fn shared_port() -> u16 {
    std::env::var("CODEX_SKIN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|p: &u16| (1024..=65535).contains(p))
        .unwrap_or(SHARED_PORT)
}

pub fn state_root() -> PathBuf {
    host::state_root()
}

pub fn ensure_state_dir() -> Result<(), EngineError> {
    let root = state_root();
    fs::create_dir_all(&root)
        .map_err(|e| EngineError::msg(format!("create state dir: {e}")))?;
    // Single install root + migration/seed (see library.rs).
    super::library::ensure_library()?;
    Ok(())
}

pub fn state_path() -> PathBuf {
    state_root().join("state.json")
}

pub fn read_state() -> Option<Value> {
    let p = state_path();
    let text = fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

/// Same-directory temp + replace (Dream Skin habit: never leave a half-written
/// `state.json` that later reads as success with empty/corrupt content).
pub fn write_state(state: &Value) -> Result<(), EngineError> {
    ensure_state_dir()?;
    let text = serde_json::to_string_pretty(state)
        .map_err(|e| EngineError::msg(format!("serialize state: {e}")))?;
    let path = state_path();
    let parent = path
        .parent()
        .ok_or_else(|| EngineError::msg("state.json has no parent"))?;
    let tmp = parent.join(format!(
        ".state.json.chatgpt-tools.{}.tmp",
        std::process::id()
    ));
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp)
            .map_err(|e| EngineError::msg(format!("create state temp: {e}")))?;
        f.write_all(format!("{text}\n").as_bytes())
            .map_err(|e| EngineError::msg(format!("write state temp: {e}")))?;
        f.sync_all()
            .map_err(|e| EngineError::msg(format!("sync state temp: {e}")))?;
    }
    if path.is_file() {
        let bak = parent.join(format!(
            ".state.json.chatgpt-tools.{}.bak",
            std::process::id()
        ));
        if let Err(e) = fs::rename(&path, &bak) {
            let _ = fs::remove_file(&tmp);
            return Err(EngineError::msg(format!("stage state backup: {e}")));
        }
        if let Err(e) = fs::rename(&tmp, &path) {
            let _ = fs::rename(&bak, &path);
            let _ = fs::remove_file(&tmp);
            return Err(EngineError::msg(format!("replace state.json: {e}")));
        }
        // Post-commit cleanup must never mask success (Dream #71).
        let _ = fs::remove_file(&bak);
    } else if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(EngineError::msg(format!("write state.json: {e}")));
    }
    Ok(())
}

/// Patch art flags after background / keep art work finishes.
///
/// Contract for GUI:
/// - `artPending` is true only while work is still in flight
/// - when art settles (ok or failed), `artPending` must become false so the
///   active-skin pill does not stick on 「立绘加载中」
pub(crate) fn patch_state_art_flags(skin_id: &str, art_ok: bool) {
    let Some(mut st) = read_state() else {
        return;
    };
    let still = st
        .get("skinId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        == skin_id;
    if !still {
        return;
    }
    if let Some(obj) = st.as_object_mut() {
        obj.insert("artOk".into(), json!(art_ok));
        // Terminal: never leave pending=true after the art job ends.
        obj.insert("artPending".into(), json!(false));
        if art_ok {
            obj.insert("phase".into(), json!("active"));
        }
        obj.insert("artSettledAt".into(), json!(iso_now()));
    }
    if let Err(e) = write_state(&st) {
        append_diag(&format!("patch_state_art_flags write failed: {e}"));
    } else {
        append_diag(&format!(
            "state art settled skin={skin_id} artOk={art_ok} artPending=false"
        ));
    }
}

/// Archive (not silently truncate) a state file that must leave the active path.
fn archive_state_file() -> Option<PathBuf> {
    let path = state_path();
    if !path.is_file() {
        return None;
    }
    let parent = path.parent()?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let archive = parent.join(format!(
        "state.stale-{}-{}.json",
        stamp,
        std::process::id()
    ));
    match fs::rename(&path, &archive) {
        Ok(()) => Some(archive),
        Err(_) => {
            let _ = fs::remove_file(&path);
            None
        }
    }
}

pub fn is_paused() -> bool {
    state_root().join("paused.flag").is_file()
}

pub fn set_paused(paused: bool) {
    let _ = ensure_state_dir();
    let flag = state_root().join("paused.flag");
    if paused {
        // Atomic-ish: write temp then rename when possible.
        let parent = state_root();
        let tmp = parent.join(format!(".paused.flag.{}.tmp", std::process::id()));
        if fs::write(&tmp, b"1\n").is_ok() {
            if fs::rename(&tmp, &flag).is_err() {
                let _ = fs::write(&flag, b"1\n");
                let _ = fs::remove_file(&tmp);
            }
        } else {
            let _ = fs::write(&flag, b"1\n");
        }
    } else {
        let _ = fs::remove_file(&flag);
    }
}

/// Public alias for package / design modules.
pub fn safe_skin_id_pub(id: &str) -> String {
    super::library::safe_skin_id(id)
}

fn bundled_skins_dir() -> PathBuf {
    super::library::bundled_skins_dir()
}

pub fn list_skins() -> Vec<Value> {
    super::library::list_skins()
}

pub fn get_skin(skin_id: &str) -> Result<Value, EngineError> {
    super::library::get_skin(skin_id)
}

/// Resolve skin dir for apply — in-place (no runtime-skins mirror).
pub fn materialize_skin(skin: &Value) -> Result<Value, EngineError> {
    super::library::materialize_skin(skin)
}

fn iso_now() -> String {
    // Simple RFC3339-ish without chrono dependency
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn settings_path() -> PathBuf {
    state_root().join("settings.json")
}

pub fn read_settings() -> Value {
    let _ = ensure_state_dir();
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}))
}

pub fn write_settings(next: &Value) -> Result<Value, EngineError> {
    ensure_state_dir()?;
    let text = serde_json::to_string_pretty(next)
        .map_err(|e| EngineError::msg(format!("serialize settings: {e}")))?;
    fs::write(settings_path(), format!("{text}\n"))
        .map_err(|e| EngineError::msg(format!("write settings: {e}")))?;
    Ok(next.clone())
}

pub fn get_configured_app_path() -> Option<String> {
    if let Ok(from_env) = std::env::var("CODEX_APP_PATH") {
        let t = from_env.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    read_settings()
        .get("appPath")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn set_app_path_native(app_path: Option<&str>) -> Result<Value, EngineError> {
    let mut settings = read_settings();
    let obj = settings
        .as_object_mut()
        .ok_or_else(|| EngineError::msg("settings.json is not an object"))?;
    match app_path.map(str::trim).filter(|s| !s.is_empty()) {
        Some(p) => {
            obj.insert("appPath".into(), json!(p));
        }
        None => {
            obj.remove("appPath");
        }
    }
    write_settings(&settings)?;
    Ok(json!({
        "ok": true,
        "appPath": get_configured_app_path(),
        "engine": "native-rust",
    }))
}

/// Delete an installed library skin (workspace/seed builtins are protected).
pub fn delete_skin_native(skin_id: &str) -> Result<Value, EngineError> {
    super::library::delete_skin(skin_id)
}

fn path_looks_like_exe(p: &str) -> bool {
    let path = Path::new(p);
    if path.is_file() {
        return true;
    }
    // Store paths may fail existsSync but still be valid for launch
    p.replace('\\', "/")
        .to_ascii_lowercase()
        .contains("/windowsapps/")
        && p.to_ascii_lowercase().ends_with(".exe")
}

fn windows_exe_candidates() -> Vec<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let user = std::env::var("USERPROFILE").unwrap_or_default();
    [
        format!(r"{local}\Programs\ChatGPT\ChatGPT.exe"),
        format!(r"{local}\Programs\Codex\Codex.exe"),
        format!(r"{local}\Programs\chatgpt\ChatGPT.exe"),
        format!(r"{local}\Programs\OpenAI\ChatGPT\ChatGPT.exe"),
        format!(r"{local}\Programs\OpenAI\Codex\Codex.exe"),
        format!(r"{local}\Microsoft\WindowsApps\ChatGPT.exe"),
        format!(r"{local}\Microsoft\WindowsApps\Codex.exe"),
        format!(r"{pf}\ChatGPT\ChatGPT.exe"),
        format!(r"{pf}\Codex\Codex.exe"),
        format!(r"{pf}\OpenAI\ChatGPT\ChatGPT.exe"),
        format!(r"{pf}\OpenAI\Codex\Codex.exe"),
        format!(r"{pf86}\ChatGPT\ChatGPT.exe"),
        format!(r"{pf86}\Codex\Codex.exe"),
        format!(r"{user}\AppData\Local\Programs\ChatGPT\ChatGPT.exe"),
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

fn resolve_exe_quick() -> Option<String> {
    if let Some(configured) = get_configured_app_path() {
        let candidates = [
            configured.clone(),
            format!(r"{configured}\ChatGPT.exe"),
            format!(r"{configured}\Codex.exe"),
            format!(r"{configured}\app\ChatGPT.exe"),
            format!(r"{configured}\app\Codex.exe"),
        ];
        for c in candidates {
            if path_looks_like_exe(&c) {
                return Some(c);
            }
        }
        // Return configured even if exists check fails (Store)
        if !configured.is_empty() {
            return Some(configured);
        }
    }
    if cfg!(windows) {
        for c in windows_exe_candidates() {
            if c.is_file() {
                return Some(c.to_string_lossy().to_string());
            }
        }
    } else if cfg!(target_os = "macos") {
        for c in [
            "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
            "/Applications/Codex.app/Contents/MacOS/Codex",
            "/Applications/Codex.app/Contents/MacOS/ChatGPT",
        ] {
            if Path::new(c).is_file() {
                return Some(c.into());
            }
        }
    }
    None
}

/// Merge cached Store identity from last-store-package.json / previous state.
fn merge_store_identity(prev: Option<&Value>) -> Value {
    let mut store_identity = read_last_store_package().unwrap_or(json!({}));
    if let Some(prev_state) = prev {
        if store_identity
            .get("packageFullName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            if let Some(obj) = store_identity.as_object_mut() {
                for key in [
                    "packageFullName",
                    "packageFamilyName",
                    "aumid",
                    "version",
                    "installLocation",
                    "executable",
                ] {
                    if let Some(v) = prev_state.get(key) {
                        obj.insert(key.into(), v.clone());
                    }
                }
            }
        }
    }
    store_identity
}

/// Spawn background art after shell_ready. Cancelled when art generation bumps.
/// Waits for page stability again so SPA hydrate does not drop large images.
/// `cold_path`: cold/restart needs a longer settle before heavy art evaluate.
fn spawn_background_art(
    skin_dir: PathBuf,
    root: PathBuf,
    port: u16,
    skin_id: String,
    art_gen: u64,
    cold_path: bool,
) {
    let _ = std::thread::Builder::new()
        .name("skin-art".into())
        .spawn(move || {
            // Yield so shell_ready / host toast can flush first.
            // Cold/restart: host may still remount after shell; wait longer before heavy art.
            std::thread::sleep(std::time::Duration::from_millis(if cold_path {
                700
            } else {
                80
            }));
            if !art_job_still_valid(art_gen) {
                append_diag("apply art deferred: generation cancelled before start");
                return;
            }

            // Re-confirm page is injectable (host often re-navigates after first paint).
            let art_wait = if cold_path { 30_000 } else { 10_000 };
            let stable = wait_until_injectable(
                port,
                art_wait,
                280,
                if cold_path { 3 } else { 2 },
                if cold_path { 900 } else { 200 },
            );
            append_diag(&format!(
                "apply art wait_injectable stable={stable} cold={cold_path}"
            ));
            if !stable && cold_path {
                // One more soft wait: target exists but document not complete yet.
                let _ = wait_until_renderer_ready(port, 8_000, 250);
                let _ = wait_until_injectable(port, 12_000, 300, 3, 700);
            }
            if !art_job_still_valid(art_gen) {
                append_diag("apply art deferred: generation cancelled after settle");
                return;
            }

            let mut guard = None;
            for attempt in 0..10 {
                if !art_job_still_valid(art_gen) {
                    return;
                }
                match engine_try_lock() {
                    Some(g) => {
                        guard = Some(g);
                        break;
                    }
                    None => {
                        std::thread::sleep(std::time::Duration::from_millis(100 + attempt * 50));
                    }
                }
            }
            let Some(_g) = guard else {
                append_diag("apply art deferred: engine busy after retries");
                return;
            };
            if !art_job_still_valid(art_gen) {
                append_diag("apply art deferred: generation cancelled after lock");
                return;
            }
            if read_state()
                .and_then(|s| s.get("skinId").and_then(|v| v.as_str()).map(|x| x.to_string()))
                .as_deref()
                != Some(skin_id.as_str())
            {
                append_diag("apply art deferred: skin changed");
                return;
            }

            // Up to 3 attempts: early SPA swaps can invalidate the first evaluate mid-decode.
            let mut ok = false;
            for attempt in 0..3u32 {
                if !art_job_still_valid(art_gen) {
                    return;
                }
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(
                        500u64 + u64::from(attempt) * 400,
                    ));
                    let _ = wait_until_injectable(port, 6_000, 250, 2, 300);
                }
                match inject_art_followup(&skin_dir, &root, port) {
                    Ok(art_res) => {
                        ok = art_res
                            .get("artOk")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        append_diag(&format!(
                            "apply art attempt={attempt} ok={ok} id={skin_id}"
                        ));
                        if ok {
                            break;
                        }
                    }
                    Err(e) => {
                        append_diag(&format!("apply art attempt={attempt} err: {e}"));
                    }
                }
            }

            if art_job_still_valid(art_gen) {
                // Always clear artPending when the job ends (success or failure).
                // Leaving artPending=true on failure stuck the GUI pill on 「立绘加载中」.
                patch_state_art_flags(&skin_id, ok);
            }
            append_diag(&format!("apply art background done ok={ok} id={skin_id}"));
        });
}

/// Soft-inject shell with retries. Soft-miss: wait **page stable** only (no stop/relaunch).
/// Silent: never begins/finishes page OpUI (caller owns one begin/finish per apply).
fn soft_inject_shell(
    skin_dir: &Path,
    root: &Path,
    port: u16,
    staged: &super::payload::StagedPayload,
    was_ready: bool,
) -> Result<(bool, String), EngineError> {
    let budget = inject_budget_from(port, None);
    let soft_timeout = if was_ready {
        budget.soft_once_timeout_ms.min(6_000)
    } else {
        // Cold / just-started host: allow longer soft verify while SPA settles.
        // Floor 14s so slow first paint is not mistaken for inject failure.
        budget.soft_once_timeout_ms.max(14_000)
    };
    let inject_opts = InjectOnceOpts {
        soft: true,
        timeout_ms: soft_timeout,
        attach_art: false,
    };

    // Overall soft-verify window (production): keep retrying without kill/disarm.
    // Hot path stays short; cold path can absorb ~90s of hydrate (matches ensure window).
    let soft_deadline = std::time::Instant::now()
        + std::time::Duration::from_millis(if was_ready { 12_000 } else { 90_000 });

    let mut last_err: Option<String> = None;
    let max_soft = if was_ready { 2 } else { 6 };
    for i in 0..max_soft {
        if std::time::Instant::now() >= soft_deadline {
            break;
        }
        // Between cold attempts, re-check injectability — early SPA route swaps wipe shell.
        if !was_ready && i > 0 {
            let remain = soft_deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis() as u64;
            let _ = wait_until_injectable(port, remain.min(8_000).max(2_000), 250, 2, 400);
        }
        match inject_once_with_staged(skin_dir, root, port, inject_opts, staged) {
            Ok(parsed) => {
                if parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
                    || parsed
                        .get("shellOk")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                {
                    let shell_mode = parsed
                        .get("shellMode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("full")
                        .to_string();
                    return Ok((true, shell_mode));
                }
                last_err = Some("soft once did not pass".into());
            }
            Err(e) => last_err = Some(e.to_string()),
        }
        std::thread::sleep(std::time::Duration::from_millis(180 + i * 80));
    }

    // Soft miss: wait for document stable, re-inject — DO NOT ensure_debug_port/kill.
    // Intermediate failures never finish OpUI as error (Scheme B).
    append_diag(&format!(
        "apply soft miss wasReady={was_ready} → wait injectable + retry (no kill) err={}",
        last_err.as_deref().unwrap_or("?")
    ));
    let wait_budget = resolve_timing_budget(None);
    let remain = soft_deadline
        .saturating_duration_since(std::time::Instant::now())
        .as_millis() as u64;
    let _ = wait_until_renderer_ready(
        port,
        remain
            .min(wait_budget.wait_renderer_ms)
            .max(if was_ready { 4_000 } else { 12_000 }),
        200,
    );
    let remain = soft_deadline
        .saturating_duration_since(std::time::Instant::now())
        .as_millis() as u64;
    let _ = wait_until_injectable(
        port,
        remain.max(if was_ready { 4_000 } else { 12_000 }),
        250,
        if was_ready { 2 } else { 3 },
        if was_ready { 300 } else { 700 },
    );
    let retry_opts = InjectOnceOpts {
        soft: true,
        timeout_ms: soft_timeout.max(12_000),
        attach_art: false,
    };
    // Keep retrying until soft_deadline — never stop_keep / kill on soft miss.
    while std::time::Instant::now() < soft_deadline {
        std::thread::sleep(std::time::Duration::from_millis(280));
        match inject_once_with_staged(skin_dir, root, port, retry_opts, staged) {
            Ok(parsed)
                if parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
                    || parsed
                        .get("shellOk")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false) =>
            {
                let shell_mode = parsed
                    .get("shellMode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("full")
                    .to_string();
                return Ok((true, shell_mode));
            }
            Ok(_) => {}
            Err(e) => last_err = Some(e.to_string()),
        }
        // Re-probe injectability between attempts (SPA may still be swapping).
        if !was_ready {
            let remain = soft_deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis() as u64;
            if remain > 1_500 {
                let _ = wait_until_injectable(port, remain.min(6_000), 280, 2, 400);
            }
        }
    }

    Err(EngineError::msg(format!(
        "换肤未完成（native CDP）: {}",
        last_err.unwrap_or_else(|| "unknown".into())
    )))
}

/// Background worker: wait host **injectable** (not just app://) → shell → keep + art.
/// Scheme B: one OpUI begin after page is injectable, one finish from final shell_ok.
fn background_cold_apply(
    skin_id: String,
    skin_dir: PathBuf,
    root: PathBuf,
    port: u16,
    restart: bool,
    art_gen: u64,
    theme_result: Value,
    store_identity: Value,
    started_at: String,
    op_token: u64,
) {
    let t0 = std::time::Instant::now();
    append_diag(&format!(
        "background_cold_apply begin id={skin_id} restart={restart} gen={art_gen} op={op_token}"
    ));

    let budget = resolve_timing_budget(None);
    // Phase A: wait until CDP has an app:// target (process/port up).
    // Wide window (≥90s) so slow Store first paint is not mistaken for failure.
    let phase_a_ms = (budget.wait_renderer_ms + budget.wait_debug_port_ms)
        .max(90_000)
        .min(120_000);
    let has_target = wait_until_renderer_ready(port, phase_a_ms, budget.poll_ms);
    if !has_target {
        // ensure_debug_port itself uses a wide verify window; do not disarm keep on soft path.
        if let Err(e) = ensure_debug_port(port, false) {
            append_diag(&format!("background_cold_apply ensure failed: {e}"));
            finish_apply_operation(port, op_token, false, "应用未完成");
            if let Some(mut st) = read_state() {
                if let Some(obj) = st.as_object_mut() {
                    obj.insert("phase".into(), json!("error"));
                    obj.insert("lastError".into(), json!(e.to_string()));
                }
                let _ = write_state(&st);
            }
            return;
        }
    }

    // Phase B: wait until document is actually stable (readyState + real DOM + chrome).
    // Cold restart injects too early here used to fail shell + drop wallpaper/art.
    // Restart needs a longer hold: host often remounts once after first paint.
    let inject_timeout = budget
        .wait_renderer_ms
        .max(if restart { 45_000 } else { 35_000 })
        .min(90_000);
    let hold_ms = if restart { 1_400 } else { 1_000 };
    let injectable = wait_until_injectable(
        port,
        inject_timeout,
        budget.poll_ms.max(240),
        3, // consecutive stable probes
        hold_ms,
    );
    append_diag(&format!(
        "background_cold_apply injectable={injectable} restart={restart} t={}ms",
        t0.elapsed().as_millis()
    ));
    if !injectable {
        // Soft fallback: wait more without killing the host or disarming keep.
        let again = wait_until_injectable(port, 30_000, 300, 3, 1_000);
        append_diag(&format!("background_cold_apply injectable retry={again}"));
        if !again {
            // Host update (Codex 26.7+) often exposes app:// early while SPA remounts,
            // or ranks aux pages first. If CDP renderer is up, attempt inject anyway —
            // soft_inject_shell will no-op cleanly when the document is still empty.
            let probe = probe_host_lifecycle_force(port);
            let force_try = probe.renderer_ready || probe.debug_port_open;
            append_diag(&format!(
                "background_cold_apply injectable hard-fail force_try={force_try} lifecycle={} renderer={}",
                probe.lifecycle, probe.renderer_ready
            ));
            if !force_try {
                finish_apply_operation(
                    port,
                    op_token,
                    false,
                    "宿主尚未就绪",
                );
                if let Some(mut st) = read_state() {
                    if let Some(obj) = st.as_object_mut() {
                        obj.insert("phase".into(), json!("error"));
                        obj.insert(
                            "lastError".into(),
                            json!("宿主页面尚未完成启动，皮肤注入超时。请稍后再试或重新换肤。"),
                        );
                    }
                    let _ = write_state(&st);
                }
                return;
            }
            append_diag("background_cold_apply: proceeding with soft inject despite stable-probe miss");
        }
    }

    if !art_job_still_valid(art_gen) {
        append_diag("background_cold_apply cancelled (art gen)");
        dismiss_apply_operation(port, op_token);
        return;
    }
    note_host_ready(port);
    // Single begin: after page is injectable so boot splash does not wipe the toast.
    // Cold reserved `op_token` at fire-and-forget; restamp paints it without allocating.
    if op_token_current(op_token) {
        restamp_apply_operation(port, OperationKind::Apply, op_token);
    }

    let staged = match build_staged_payload(&skin_dir, &root) {
        Ok(s) => s,
        Err(e) => {
            append_diag(&format!("background_cold_apply staged: {e}"));
            finish_apply_operation(port, op_token, false, "应用未完成");
            return;
        }
    };
    let markers = staged.markers.clone();
    let has_art = staged.has_art;

    // Phase C: inject with soft retries (still no kill on soft-miss). Silent inject.
    let inject = soft_inject_shell(&skin_dir, &root, port, &staged, false);
    let (shell_ok, shell_mode) = match inject {
        Ok(v) => v,
        Err(e) => {
            // One late retry after extra hydrate wait (common: first inject during route swap).
            append_diag(&format!(
                "background_cold_apply inject fail → hydrate wait + retry: {e}"
            ));
            let _ = wait_until_injectable(port, 15_000, 280, 3, 1_000);
            if !art_job_still_valid(art_gen) {
                dismiss_apply_operation(port, op_token);
                return;
            }
            match soft_inject_shell(&skin_dir, &root, port, &staged, false) {
                Ok(v) => v,
                Err(e2) => {
                    append_diag(&format!("background_cold_apply inject final fail: {e2}"));
                    finish_apply_operation(port, op_token, false, "应用未完成");
                    if let Some(mut st) = read_state() {
                        if let Some(obj) = st.as_object_mut() {
                            obj.insert("phase".into(), json!("error"));
                            obj.insert("lastError".into(), json!(e2.to_string()));
                            obj.insert("shellOk".into(), json!(false));
                        }
                        let _ = write_state(&st);
                    }
                    return;
                }
            }
        }
    };

    if !art_job_still_valid(art_gen) {
        append_diag("background_cold_apply cancelled after inject");
        dismiss_apply_operation(port, op_token);
        return;
    }

    // Phase D: brief settle, then **conditional** repair only if soft markers vanished.
    // Unconditional second inject doubled host CDP cost on every cold success path.
    std::thread::sleep(std::time::Duration::from_millis(if restart {
        650
    } else {
        400
    }));
    let still_stable = wait_until_injectable(port, 6_000, 220, 2, 400);
    let soft_still = soft_shell_present(port, &markers);
    append_diag(&format!(
        "background_cold_apply post-shell stable={still_stable} softPresent={soft_still}"
    ));
    if still_stable && !soft_still {
        let repair = InjectOnceOpts {
            soft: true,
            timeout_ms: 6_000,
            attach_art: false,
        };
        match inject_once_with_staged(&skin_dir, &root, port, repair, &staged) {
            Ok(parsed) => {
                let ok = parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
                    || parsed
                        .get("shellOk")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                append_diag(&format!(
                    "background_cold_apply post-shell conditional repair ok={ok}"
                ));
            }
            Err(e) => {
                append_diag(&format!(
                    "background_cold_apply post-shell conditional repair err: {e}"
                ));
            }
        }
    } else if soft_still {
        append_diag("background_cold_apply post-shell repair skipped (soft present)");
    }

    if !art_job_still_valid(art_gen) {
        dismiss_apply_operation(port, op_token);
        return;
    }

    let browser_id = read_browser_identity(port)
        .ok()
        .map(|b| b.browser_id);
    let art_ok = !has_art;
    let art_pending = has_art;
    let apply_mode = if restart {
        "native-restart-async"
    } else {
        "native-cold-async"
    };

    let state = json!({
        "schema": STATE_SCHEMA,
        "skinId": skin_id,
        "port": port,
        "browserId": browser_id,
        "startedAt": started_at,
        "platform": std::env::consts::OS,
        "skinDir": skin_dir.to_string_lossy(),
        "phase": "active",
        "shellOk": shell_ok,
        "artOk": art_ok,
        "artPending": art_pending,
        "applyMode": apply_mode,
        "shellMode": shell_mode,
        "verifiedAt": iso_now(),
        "engineVersion": ENGINE_VERSION,
        "nativeEngine": true,
        "packageFullName": store_identity.get("packageFullName").cloned().unwrap_or(json!(null)),
        "packageFamilyName": store_identity.get("packageFamilyName").cloned().unwrap_or(json!(null)),
        "aumid": store_identity.get("aumid").cloned().unwrap_or(json!(null)),
        "packageVersion": store_identity.get("version").cloned().unwrap_or(json!(null)),
        "codexPackageRoot": store_identity.get("installLocation").cloned().unwrap_or(json!(null)),
        "codexExecutable": store_identity.get("executable").cloned().unwrap_or(json!(null)),
        "theme": theme_result,
        "injectorPid": null,
        "nodeRequired": false,
    });
    let _ = write_state(&state);
    // Keep only after shell is verified — avoid re-inject storm during boot.
    start_keep(port, &skin_id, skin_dir.clone(), markers, has_art);
    append_diag(&format!(
        "background_cold_apply shell_ready id={skin_id} mode={apply_mode} t={}ms",
        t0.elapsed().as_millis()
    ));

    // Single finish from final shell_ok (Scheme B) — not intermediate soft-verify.
    let op_done_msg = if shell_ok {
        if has_art {
            "样式已应用 · 壁纸加载中"
        } else {
            "皮肤已应用"
        }
    } else {
        "应用未完成"
    };
    finish_apply_operation(port, op_token, shell_ok, op_done_msg);

    if has_art && art_job_still_valid(art_gen) {
        // Cold art: long settle + multi-retry so large images are not dropped mid-hydrate.
        spawn_background_art(skin_dir, root, port, skin_id, art_gen, true);
    }
}

/// `restart`: force stop+relaunch (fire-and-forget for cold/restart).
/// Theme is written **before** host restart so relaunch reads the new config.toml.
///
/// - Hot path: sync shell inject, return shell_ready; art async
/// - Cold/restart: stop+launch, return `phase=restarting` immediately; inject in background
pub fn apply_skin_native_opts(skin_id: &str, restart: bool) -> Result<Value, EngineError> {
    let apply_t0 = std::time::Instant::now();
    let _guard = ENGINE_LOCK.lock();
    let port = shared_port();

    let cached = probe_host_lifecycle(port);
    let before = if restart
        || !cached.can_hot_apply
        || cached.lifecycle != "ready"
        || !cached.renderer_ready
    {
        if restart {
            invalidate_host_lifecycle_sticky();
        } else {
            invalidate_host_probe_cache();
        }
        probe_host_lifecycle_force(port)
    } else {
        cached
    };
    let was_ready = before.can_hot_apply && !restart && before.renderer_ready;
    let needs_async_restart = restart || !was_ready;

    append_diag(&format!(
        "apply_skin_native id={skin_id} restart={restart} lifecycle={} canHot={} wasReady={} async={} t={}ms",
        before.lifecycle,
        before.can_hot_apply,
        was_ready,
        needs_async_restart,
        apply_t0.elapsed().as_millis()
    ));

    let op_kind = if was_ready {
        OperationKind::Switch
    } else {
        OperationKind::Apply
    };

    // Resolve skin before any page OpUI (avoid loading toast on missing skin).
    let base = get_skin(skin_id)?;
    let skin = materialize_skin(&base)?;
    let skin_dir = PathBuf::from(
        skin.get("dir")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::msg("materialized skin missing dir"))?,
    );
    let id = skin
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(skin_id)
        .to_string();

    // Scheme B: one op token per user apply after skin is valid.
    // Hot: begin now. Cold: reserve token; restamp after page is injectable.
    let op_token = if was_ready {
        begin_apply_operation(port, op_kind)
    } else {
        next_op_token()
    };

    set_paused(false);
    let root = engine::project_root();

    // Desktop chrome theme MUST be written before host restart.
    let theme_result = if let Some(dt) = skin.get("desktopTheme") {
        match apply_desktop_theme(dt, &state_root()) {
            Ok(v) => {
                append_diag(&format!(
                    "apply_desktop_theme ok t={}ms: {v}",
                    apply_t0.elapsed().as_millis()
                ));
                v
            }
            Err(e) => {
                let msg = e.to_string();
                append_diag(&format!("apply_desktop_theme fail: {msg}"));
                json!({ "skipped": true, "reason": msg })
            }
        }
    } else {
        json!({ "skipped": true, "reason": "no desktopTheme" })
    };

    let prev = read_state();
    let started_at = prev
        .as_ref()
        .and_then(|p| p.get("startedAt").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(iso_now);
    let store_identity = merge_store_identity(prev.as_ref());

    // ── Cold / restart: fire-and-forget ───────────────────────────────────
    if needs_async_restart {
        // Kill + launch only; do not wait for renderer on this thread.
        // stop_host (inside restart) disarms keep + bumps art gen — capture gen AFTER kill.
        let launch_res = if restart || before.process_running || before.debug_port_open {
            restart_host_fire_and_forget(port)
        } else {
            stop_keep();
            invalidate_host_lifecycle_sticky();
            super::launch::launch_host(port)
        };
        if let Err(e) = launch_res {
            dismiss_apply_operation(port, op_token);
            return Err(e);
        }
        let art_gen = art_generation();

        let apply_mode = if restart {
            "native-restart-async"
        } else {
            "native-cold-async"
        };
        let state = json!({
            "schema": STATE_SCHEMA,
            "skinId": id,
            "port": port,
            "startedAt": started_at,
            "platform": std::env::consts::OS,
            "skinDir": skin_dir.to_string_lossy(),
            "phase": "restarting",
            "shellOk": false,
            "artOk": false,
            "artPending": true,
            "applyMode": apply_mode,
            "engineVersion": ENGINE_VERSION,
            "nativeEngine": true,
            "packageFullName": store_identity.get("packageFullName").cloned().unwrap_or(json!(null)),
            "packageFamilyName": store_identity.get("packageFamilyName").cloned().unwrap_or(json!(null)),
            "aumid": store_identity.get("aumid").cloned().unwrap_or(json!(null)),
            "theme": theme_result,
            "nodeRequired": false,
        });
        if let Err(e) = write_state(&state) {
            dismiss_apply_operation(port, op_token);
            return Err(e);
        }

        let bg_id = id.clone();
        let bg_dir = skin_dir.clone();
        let bg_root = root.clone();
        let bg_theme = theme_result.clone();
        let bg_store = store_identity.clone();
        let bg_started = started_at.clone();
        let bg_gen = art_gen;
        let bg_op = op_token;
        let _ = std::thread::Builder::new()
            .name("skin-cold-apply".into())
            .spawn(move || {
                background_cold_apply(
                    bg_id,
                    bg_dir,
                    bg_root,
                    port,
                    restart,
                    bg_gen,
                    bg_theme,
                    bg_store,
                    bg_started,
                    bg_op,
                );
            });

        append_diag(&format!(
            "apply_skin_native fire-and-forget id={id} mode={apply_mode} t={}ms",
            apply_t0.elapsed().as_millis()
        ));

        return Ok(json!({
            "ok": true,
            "skinId": id,
            "port": port,
            "verified": false,
            "pending": true,
            "phase": "restarting",
            "message": if restart { "正在重启客户端…" } else { "正在启动客户端…" },
            "applyMode": apply_mode,
            "shellOk": false,
            "artOk": false,
            "artPending": true,
            "lifecycle": "starting",
            "engineVersion": ENGINE_VERSION,
            "engine": "native-rust",
            "native": true,
            "nodeRequired": false,
            "restarted": true,
            "keepAlive": false,
            "theme": theme_result,
            "storePackage": store_identity,
            "feedback": "gui",
        }));
    }

    // ── Hot path (sync shell) ─────────────────────────────────────────────
    note_host_ready(port);
    append_diag(&format!(
        "apply skip ensure_debug_port (hot ready) t={}ms",
        apply_t0.elapsed().as_millis()
    ));

    let staged = match build_staged_payload(&skin_dir, &root) {
        Ok(s) => s,
        Err(e) => {
            finish_apply_operation(port, op_token, false, "应用未完成");
            return Err(EngineError::msg(e.to_string()));
        }
    };
    let markers = staged.markers.clone();
    let has_art = staged.has_art;
    append_diag(&format!(
        "apply staged shellOk cache={} hasArt={} shellBytes={} t={}ms",
        staged.cache_hit,
        has_art,
        staged.shell_bytes,
        apply_t0.elapsed().as_millis()
    ));

    let inject_result = soft_inject_shell(&skin_dir, &root, port, &staged, true);
    let (shell_ok, shell_mode) = match inject_result {
        Ok(v) => v,
        Err(e) => {
            // Final outcome only — intermediate soft-miss never finished as error.
            finish_apply_operation(port, op_token, false, "应用未完成");
            return Err(e);
        }
    };

    let browser_id = read_browser_identity(port)
        .ok()
        .map(|b| b.browser_id);
    let apply_mode = if prev
        .as_ref()
        .and_then(|p| p.get("skinId").and_then(|v| v.as_str()))
        == Some(id.as_str())
    {
        "native-hot-reapply"
    } else {
        "native-hot-switch"
    };
    let art_ok = !has_art;
    let art_pending = has_art;
    let message = if art_pending {
        "样式已应用 · 壁纸加载中"
    } else {
        "皮肤已应用"
    };
    // Single finish from final shell_ok (Scheme B).
    finish_apply_operation(port, op_token, shell_ok, message);

    let state = json!({
        "schema": STATE_SCHEMA,
        "skinId": id,
        "port": port,
        "browserId": browser_id,
        "startedAt": started_at,
        "platform": std::env::consts::OS,
        "skinDir": skin_dir.to_string_lossy(),
        "phase": "active",
        "shellOk": shell_ok,
        "artOk": art_ok,
        "artPending": art_pending,
        "applyMode": apply_mode,
        "shellMode": shell_mode,
        "verifiedAt": iso_now(),
        "engineVersion": ENGINE_VERSION,
        "nativeEngine": true,
        "packageFullName": store_identity.get("packageFullName").cloned().unwrap_or(json!(null)),
        "packageFamilyName": store_identity.get("packageFamilyName").cloned().unwrap_or(json!(null)),
        "aumid": store_identity.get("aumid").cloned().unwrap_or(json!(null)),
        "packageVersion": store_identity.get("version").cloned().unwrap_or(json!(null)),
        "codexPackageRoot": store_identity.get("installLocation").cloned().unwrap_or(json!(null)),
        "codexExecutable": store_identity.get("executable").cloned().unwrap_or(json!(null)),
        "injectorPid": null,
        "injectorScript": null,
        "nodePath": null,
        "nodeRequired": false,
    });
    write_state(&state)?;
    start_keep(port, &id, skin_dir.clone(), markers, has_art);

    let art_gen = art_generation();
    if has_art {
        // Hot path: host already painted — short settle only.
        spawn_background_art(skin_dir.clone(), root, port, id.clone(), art_gen, false);
    }

    append_diag(&format!(
        "apply_skin_native shell_ready id={id} mode={apply_mode} shellOk={shell_ok} artPending={art_pending} shellMode={shell_mode} feedback=host keep=1 t={}ms",
        apply_t0.elapsed().as_millis()
    ));

    // Ensure third-party model unlock is queued/applied after host is up
    // (shell inject may have already tried; this covers launch-only + retries).
    crate::providers::model_unlock::on_host_ready();
    // Toolbox: force Chinese / fast startup inject + Computer Use Guard.
    crate::toolbox::on_host_ready();

    Ok(json!({
        "ok": true,
        "skinId": id,
        "port": port,
        "verified": true,
        "verifyMode": "native-soft",
        "applyMode": apply_mode,
        "shellOk": shell_ok,
        "artOk": art_ok,
        "artPending": art_pending,
        "shellMode": shell_mode,
        "deltaPreferred": shell_mode == "delta",
        "browserId": browser_id,
        "skinDir": skin_dir.to_string_lossy(),
        "lifecycle": "ready",
        "engineVersion": ENGINE_VERSION,
        "engine": "native-rust",
        "native": true,
        "nodeRequired": false,
        "restarted": false,
        "keepAlive": true,
        "theme": theme_result,
        "storePackage": store_identity,
        "feedback": "host",
        "message": message,
        "phase": if art_pending { "art_pending" } else { "shell_ready" },
    }))
}

/// Pause: mark paused + stop keep-alive + live CDP remove (Dream #168 / macOS parity).
/// Never claims full success when the host is injectable but remove fails.
pub fn pause_skin_native() -> Result<Value, EngineError> {
    let _guard = ENGINE_LOCK.lock();
    let port = shared_port();
    let state = read_state();
    let root = engine::project_root();

    // Flag first so keep-alive cannot race a re-paint (Dream live-pause order).
    set_paused(true);
    stop_keep();

    let skin_dir = state
        .as_ref()
        .and_then(|s| s.get("skinDir").and_then(|v| v.as_str()))
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| {
            let id = state.as_ref()?.get("skinId").and_then(|v| v.as_str())?;
            get_skin(id)
                .ok()
                .and_then(|s| s.get("dir").and_then(|d| d.as_str()).map(PathBuf::from))
                .filter(|p| p.is_dir())
        });

    invalidate_host_probe_cache();
    let probe = probe_host_lifecycle_force(port);
    let host_live = probe.renderer_ready || probe.debug_port_open;

    let mut removed = json!({ "ok": false, "skipped": true });
    let mut remove_error: Option<String> = None;

    if host_live {
        if let Some(dir) = skin_dir.as_ref() {
            match remove_once(dir, &root, port) {
                Ok(r) => removed = r,
                Err(e) => {
                    remove_error = Some(e.to_string());
                    append_diag(&format!("pause_skin_native remove: {e}"));
                }
            }
        } else {
            // No skin dir: still try a generic strip via any known skin markers is hard;
            // report honest partial pause (flag is set, reinject stopped).
            removed = json!({
                "ok": false,
                "skipped": true,
                "reason": "no-skin-dir"
            });
            remove_error = Some("没有可卸下的皮肤目录；已写入暂停标记".into());
        }
    }

    let remove_ok = removed
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let removed_targets = removed
        .get("removedTargets")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // Host offline → pause marker alone is success (nothing live to strip).
    // Host live → require remove ok (or at least one target cleaned).
    if host_live && !remove_ok && removed_targets == 0 {
        let msg = remove_error.unwrap_or_else(|| {
            "已写入暂停标记，但即时卸下皮肤失败；可重试暂停或完全恢复".into()
        });
        append_diag(&format!("pause_skin_native partial: {msg}"));
        return Err(EngineError::msg(msg));
    }

    // Persist paused phase in state without clearing session (resume needs skinId).
    if let Some(mut cur) = state {
        if let Some(obj) = cur.as_object_mut() {
            obj.insert("phase".into(), json!("paused"));
            obj.insert("pausedAt".into(), json!(iso_now()));
            let _ = write_state(&cur);
        }
    }

    append_diag(&format!(
        "pause_skin_native ok hostLive={host_live} removeOk={remove_ok} targets={removed_targets}"
    ));
    Ok(json!({
        "ok": true,
        "paused": true,
        "port": port,
        "removed": removed,
        "hostLive": host_live,
        "engine": "native-rust",
        "native": true,
    }))
}

/// Resume after pause: clear flag and re-apply the last session skin.
pub fn resume_skin_native(restart: bool) -> Result<Value, EngineError> {
    let state = read_state().ok_or_else(|| {
        EngineError::msg("没有可恢复的皮肤会话，请先应用一套皮肤")
    })?;
    let skin_id = state
        .get("skinId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| EngineError::msg("没有可恢复的皮肤会话，请先应用一套皮肤"))?
        .to_string();
    set_paused(false);
    apply_skin_native_opts(&skin_id, restart)
}

fn last_session_skin_id() -> Option<String> {
    read_state()
        .as_ref()
        .and_then(|s| s.get("skinId").and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// GUI「启动 ChatGPT」: launch host with debug port.
/// If last session has a skinId → apply that skin (cold path).
/// Otherwise only bring the client up so later apply can hot-inject.
pub fn start_host_native() -> Result<Value, EngineError> {
    let port = shared_port();
    if let Some(skin_id) = last_session_skin_id() {
        // Clear pause so apply re-enables keep-alive after inject.
        set_paused(false);
        append_diag(&format!(
            "start_host_native: apply last skin id={skin_id}"
        ));
        let mut result = apply_skin_native_opts(&skin_id, false)?;
        if let Some(obj) = result.as_object_mut() {
            obj.insert("started".into(), json!(true));
            obj.insert("mode".into(), json!("apply-last-skin"));
            obj.insert("skinId".into(), json!(skin_id));
        }
        return Ok(result);
    }

    append_diag("start_host_native: cold launch without skin session");
    ensure_debug_port(port, false)?;
    invalidate_host_probe_cache();
    let life = probe_host_lifecycle_force(port);
    // Third-party model whitelist: inject once host debug port is up (or queue
    // via keep thread if the renderer is still booting).
    crate::providers::model_unlock::on_host_ready();
    // Toolbox enhancements + optional Computer Use Guard on cold launch.
    crate::toolbox::on_host_ready();
    Ok(json!({
        "ok": true,
        "started": true,
        "mode": "launch-only",
        "port": port,
        "lifecycle": life.lifecycle,
        "debugPortOpen": life.debug_port_open,
        "rendererReady": life.renderer_ready,
        "canHotApply": life.can_hot_apply,
        "engine": "native-rust",
        "native": true,
    }))
}

/// GUI「重启 ChatGPT」: hard stop + relaunch with debug port.
/// Re-applies last session skin when present (same as start, but force restart).
pub fn restart_host_native() -> Result<Value, EngineError> {
    let port = shared_port();
    if let Some(skin_id) = last_session_skin_id() {
        set_paused(false);
        append_diag(&format!(
            "restart_host_native: apply last skin id={skin_id} (forced restart)"
        ));
        let mut result = apply_skin_native_opts(&skin_id, true)?;
        if let Some(obj) = result.as_object_mut() {
            obj.insert("started".into(), json!(true));
            obj.insert("restarted".into(), json!(true));
            obj.insert("mode".into(), json!("apply-last-skin"));
            obj.insert("skinId".into(), json!(skin_id));
        }
        return Ok(result);
    }

    append_diag("restart_host_native: hard relaunch without skin session");
    stop_keep();
    ensure_debug_port(port, true)?;
    invalidate_host_probe_cache();
    let life = probe_host_lifecycle_force(port);
    crate::providers::model_unlock::on_host_ready();
    crate::toolbox::on_host_ready();
    Ok(json!({
        "ok": true,
        "started": true,
        "restarted": true,
        "mode": "launch-only",
        "port": port,
        "lifecycle": life.lifecycle,
        "debugPortOpen": life.debug_port_open,
        "rendererReady": life.renderer_ready,
        "canHotApply": life.can_hot_apply,
        "engine": "native-rust",
        "native": true,
    }))
}

/// Full native restore: CDP remove (if ready) + strip theme + clear state.
/// Optional soft relaunch of host when it was running (desktop chrome refresh).
/// Does not claim full success when a live remove failed (Dream restore honesty).
pub fn restore_skin_native(restore_theme: bool) -> Result<Value, EngineError> {
    let _guard = ENGINE_LOCK.lock();
    let port = shared_port();
    let state = read_state();
    let skin_dir = state
        .as_ref()
        .and_then(|s| s.get("skinDir").and_then(|v| v.as_str()))
        .map(PathBuf::from);
    let root = engine::project_root();
    invalidate_host_probe_cache();
    let was_running = probe_host_lifecycle_force(port).codex_running();

    set_paused(false);
    stop_keep();

    let mut removed = json!({ "ok": false, "skipped": true });
    let mut remove_attempted = false;
    let probe = probe_host_lifecycle_force(port);
    if probe.renderer_ready || probe.debug_port_open {
        remove_attempted = true;
        if let Some(dir) = skin_dir.as_ref() {
            if dir.is_dir() {
                match remove_once(dir, &root, port) {
                    Ok(r) => removed = r,
                    Err(e) => {
                        removed = json!({ "ok": false, "error": e.to_string() });
                        append_diag(&format!("restore remove: {e}"));
                    }
                }
            }
        } else if let Some(id) = state
            .as_ref()
            .and_then(|s| s.get("skinId").and_then(|v| v.as_str()))
        {
            if let Ok(skin) = get_skin(id) {
                if let Some(dir) = skin.get("dir").and_then(|v| v.as_str()) {
                    let p = PathBuf::from(dir);
                    if p.is_dir() {
                        match remove_once(&p, &root, port) {
                            Ok(r) => removed = r,
                            Err(e) => {
                                removed = json!({ "ok": false, "error": e.to_string() });
                                append_diag(&format!("restore remove: {e}"));
                            }
                        }
                    }
                }
            }
        }
    }

    let theme = if restore_theme {
        restore_desktop_theme(&state_root())
    } else {
        json!({ "restored": false, "reason": "skipped" })
    };

    // Archive state (not silent truncate) so a failed mid-restore is diagnosable.
    let archived = archive_state_file();

    // Soft relaunch so desktop chrome picks up theme strip (best-effort, non-fatal).
    let mut relaunched = false;
    if was_running && restore_theme {
        append_diag("restore_skin_native: soft relaunch host for chrome theme");
        stop_keep();
        match restart_host_fire_and_forget(port) {
            Ok(_) => {
                let budget = resolve_timing_budget(None);
                if wait_until_renderer_ready(
                    port,
                    budget.wait_renderer_ms.min(30_000),
                    budget.poll_ms,
                ) {
                    note_host_ready(port);
                    relaunched = true;
                } else {
                    match ensure_debug_port(port, false) {
                        Ok(()) => relaunched = true,
                        Err(e) => append_diag(&format!("restore relaunch soft-fail: {e}")),
                    }
                }
            }
            Err(e) => append_diag(&format!("restore relaunch soft-fail: {e}")),
        }
    }

    let remove_ok = removed
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let theme_restored = theme
        .get("restored")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let theme_skipped = theme
        .get("reason")
        .and_then(|v| v.as_str())
        .map(|r| r == "skipped" || r == "config missing" || r == "no desktop section")
        .unwrap_or(false);
    // Live host + failed remove → partial (still cleared session / keep).
    let partial = remove_attempted && !remove_ok;
    let ok = !partial;

    if partial {
        append_diag("restore_skin_native: partial — live remove failed; session cleared anyway");
    }

    Ok(json!({
        "ok": ok,
        "partial": partial,
        "full": ok && (!restore_theme || theme_restored || theme_skipped),
        "theme": theme,
        "removed": removed,
        "refreshed": relaunched,
        "relaunched": relaunched,
        "archivedState": archived.map(|p| p.to_string_lossy().to_string()),
        "engine": "native-rust",
        "native": true,
        "restoreTheme": restore_theme,
        "error": if partial {
            json!("已清除会话，但即时卸下皮肤失败；请确认 ChatGPT 窗口是否仍显示主题")
        } else {
            Value::Null
        },
    }))
}

/// Status: skins + three-signal host lifecycle (no Node).
pub fn get_status_native() -> Result<Value, EngineError> {
    let _ = ensure_state_dir();
    let state = read_state();
    let port = state
        .as_ref()
        .and_then(|s| s.get("port").and_then(|v| v.as_u64()))
        .map(|p| p as u16)
        .unwrap_or_else(shared_port);

    let life = probe_host_lifecycle(port);
    let debug_port_open = life.debug_port_open;
    let renderer_ready = life.renderer_ready;
    let lifecycle = life.lifecycle;

    let active_skin_id = state
        .as_ref()
        .and_then(|s| s.get("skinId").and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    let paused = is_paused();
    // Prefer stable lifecycle so CDP blips do not clear "using" badge.
    let host_engaged = life.host_engaged();

    let skins: Vec<Value> = list_skins()
        .into_iter()
        .map(|mut s| {
            let id = s
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let active = active_skin_id.as_deref() == Some(id.as_str()) && !paused && host_engaged;
            if let Some(obj) = s.as_object_mut() {
                obj.insert("active".into(), json!(active));
            }
            s
        })
        .collect();

    let keep = keep_armed();
    // Reuse host_status fields so GUI gets boot appearance snapshot on full status too.
    let host_extra = super::host::host_lifecycle_to_json(&life, keep);
    let boot_theme = host_extra.get("hostBootAppearanceTheme").cloned();
    let config_theme = host_extra.get("configAppearanceTheme").cloned();
    let shell_ok = state
        .as_ref()
        .and_then(|s| s.get("shellOk").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    let art_ok = state
        .as_ref()
        .and_then(|s| s.get("artOk").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    let art_pending_raw = state
        .as_ref()
        .and_then(|s| s.get("artPending").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    // Normalize: never report pending when art already ok (stale state safety).
    let art_pending = art_pending_raw && !art_ok;
    Ok(json!({
        "platform": std::env::consts::OS,
        "configPath": theme::config_path().to_string_lossy(),
        "stateRoot": state_root().to_string_lossy(),
        "state": state,
        "debugReady": renderer_ready || lifecycle == "ready",
        "debugPortOpen": debug_port_open,
        "processRunning": life.process_running,
        "rendererReady": renderer_ready,
        "lifecycle": lifecycle,
        "lifecycleRaw": life.lifecycle_raw,
        "lifecycleLabel": lifecycle,
        "confidence": life.confidence,
        "codexRunning": life.codex_running(),
        "canHotApply": life.can_hot_apply,
        "needsRestartForInject": life.needs_restart_for_inject,
        "hostPids": life.pids,
        "hostBootAppearanceTheme": boot_theme.unwrap_or(Value::Null),
        "configAppearanceTheme": config_theme.unwrap_or(Value::Null),
        "paused": paused,
        "protocol": ENGINE_PROTOCOL,
        "engineVersion": ENGINE_VERSION,
        "engineName": ENGINE_NAME,
        "ok": true,
        "shellOk": shell_ok,
        "artOk": art_ok,
        "artPending": art_pending,
        "injectorAlive": false,
        "keepAlive": keep,
        "configuredAppPath": get_configured_app_path(),
        "probeAgeMs": life.probe_age_ms,
        "signals": {
            "process": life.process_running,
            "port": debug_port_open,
            "renderer": renderer_ready,
        },
        "nativeEngine": true,
        "engine": "native-rust",
        "nodeRequired": false,
        "shellMode": state
            .as_ref()
            .and_then(|s| s.get("shellMode").cloned())
            .unwrap_or(Value::Null),
        "applyMode": state
            .as_ref()
            .and_then(|s| s.get("applyMode").cloned())
            .unwrap_or(Value::Null),
        // Store package snapshot for GUI (PowerShell); not on apply hot path.
        "storePackage": store_package_status_json(),
        // Cheap stale hint: compare state full name vs last-store-package cache only.
        // Full Get-AppxPackage re-check stays on detect/launch — avoid double PS on every status.
        "storePackageStale": state
            .as_ref()
            .map(|s| {
                let full = s.get("packageFullName").and_then(|v| v.as_str()).unwrap_or("");
                if full.is_empty() {
                    return false;
                }
                let cached = read_last_store_package()
                    .and_then(|v| {
                        v.get("packageFullName")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_default();
                !cached.is_empty() && cached != full
            })
            .unwrap_or(false),
        "skins": skins,
    }))
}

/// Lightweight host lifecycle for GUI polling (no skins / previews).
pub fn get_host_status_native(force: bool) -> Result<Value, EngineError> {
    let port = read_state()
        .as_ref()
        .and_then(|s| s.get("port").and_then(|v| v.as_u64()))
        .map(|p| p as u16)
        .unwrap_or_else(shared_port);
    Ok(host_status_json(port, force, keep_armed()))
}

pub fn detect_native() -> Result<Value, EngineError> {
    let port = shared_port();
    let life = probe_host_lifecycle_force(port);
    let exe = resolve_exe_quick();
    let configured = get_configured_app_path();
    let found = exe.is_some() || configured.is_some();
    let store = store_package_status_json();
    let aumid = store
        .get("aumid")
        .cloned()
        .unwrap_or(Value::Null);
    let mut body = host_lifecycle_to_json(&life, keep_armed());
    if let Some(obj) = body.as_object_mut() {
        obj.insert("platform".into(), json!(std::env::consts::OS));
        obj.insert("exe".into(), json!(exe));
        obj.insert("aumid".into(), aumid);
        obj.insert("storePackage".into(), store);
        obj.insert("configuredAppPath".into(), json!(configured));
        obj.insert("configExists".into(), json!(theme::config_path().is_file()));
        obj.insert(
            "configPath".into(),
            json!(theme::config_path().to_string_lossy()),
        );
        obj.insert(
            "engineDir".into(),
            json!(engine::project_root().join("engine").to_string_lossy()),
        );
        obj.insert("debugPort".into(), json!(port));
        obj.insert("found".into(), json!(found));
        obj.insert("nodeRequired".into(), json!(false));
        obj.insert("nativeEngine".into(), json!(true));
    }
    Ok(body)
}

pub fn engine_version_native() -> Value {
    json!({
        "ok": true,
        "name": ENGINE_NAME,
        "version": ENGINE_VERSION,
        "protocol": ENGINE_PROTOCOL,
        "root": engine::project_root().to_string_lossy(),
        "native": true,
        "engine": "native-rust",
    })
}

pub fn engine_paths_native() -> Value {
    json!({
        "ok": true,
        "root": engine::project_root().to_string_lossy(),
        "stateRoot": state_root().to_string_lossy(),
        "bundledSkins": bundled_skins_dir().to_string_lossy(),
        "librarySkins": super::library::library_dir().to_string_lossy(),
        // Alias for older tooling / docs
        "userSkins": super::library::library_dir().to_string_lossy(),
        "devWorkspace": super::library::is_dev_workspace(),
        "engine": "native-rust",
    })
}

/// Resolve skin asset path without Node.
pub fn resolve_asset_native(skin_id: &str, kind: &str) -> Result<Value, EngineError> {
    let skin = get_skin(skin_id)?;
    let dir = PathBuf::from(
        skin.get("dir")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::msg("skin missing dir"))?,
    );
    let manifest_path = dir.join("skin.json");
    let manifest: Value = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({}));

    let try_path = |rel: &str| -> Option<PathBuf> {
        if rel.is_empty() {
            return None;
        }
        let p = dir.join(rel);
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    };

    let path = match kind {
        "art" => {
            let rel = manifest
                .pointer("/assets/art")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            try_path(rel)
        }
        "screenshot" | "preview" => {
            // Prefer assets/screenshot.* then art
            let candidates = [
                "assets/screenshot.png",
                "assets/screenshot.jpg",
                "assets/screenshot.jpeg",
                "assets/screenshot.webp",
                "screenshot.png",
                "preview.png",
            ];
            let mut found = None;
            for c in candidates {
                if let Some(p) = try_path(c) {
                    found = Some(p);
                    break;
                }
            }
            if found.is_none() && kind == "preview" {
                let rel = manifest
                    .pointer("/assets/art")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                found = try_path(rel);
            }
            found
        }
        _ => None,
    };

    let path = path.ok_or_else(|| EngineError::msg(format!("asset not found: {kind}")))?;
    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "kind": kind,
        "skinId": skin_id,
        "engine": "native-rust",
    }))
}
