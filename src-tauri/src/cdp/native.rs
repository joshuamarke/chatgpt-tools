//! High-level native engine ops: apply (hot + cold), status, restore.
//! Import/export/design-wallpaper still fall back to Node CLI.

use super::host::{
    self, append_diag, host_lifecycle_to_json, host_status_json, invalidate_host_probe_cache,
    note_host_ready, probe_host_lifecycle, probe_host_lifecycle_force,
};
use super::http::read_browser_identity;
use super::inject::{inject_once, remove_once};
use super::keep::{keep_armed, start_keep, stop_keep};
use super::launch::{ensure_debug_port, inject_budget, stop_host};
use super::payload::build_staged_payload;
use super::theme::{self, apply_desktop_theme, restore_desktop_theme};
use crate::engine::{self, EngineError};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const ENGINE_NAME: &str = "chatgpt-tools-engine";
pub const ENGINE_VERSION: &str = "2.3.2";
pub const ENGINE_PROTOCOL: u32 = 2;
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
    fs::create_dir_all(root.join("skins"))
        .map_err(|e| EngineError::msg(format!("create state dir: {e}")))?;
    fs::create_dir_all(root.join("runtime-skins"))
        .map_err(|e| EngineError::msg(format!("create runtime-skins: {e}")))?;
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

pub(crate) fn safe_skin_id(id: &str) -> String {
    let s: String = id
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = s.trim_matches('-');
    trimmed.chars().take(64).collect()
}

/// Public alias for package module.
pub fn safe_skin_id_pub(id: &str) -> String {
    safe_skin_id(id)
}

fn bundled_skins_dir() -> PathBuf {
    engine::project_root().join("skins")
}

pub(crate) fn user_skins_dir() -> PathBuf {
    state_root().join("skins")
}

pub fn user_skins_dir_pub() -> PathBuf {
    user_skins_dir()
}

fn read_skin_from_dir(dir: &Path, source: &str) -> Option<Value> {
    let manifest_path = dir.join("skin.json");
    if !manifest_path.is_file() {
        return None;
    }
    let text = fs::read_to_string(manifest_path).ok()?;
    let mut manifest: Value = serde_json::from_str(&text).ok()?;
    if manifest.get("id").and_then(|v| v.as_str()).is_none() {
        if let Some(obj) = manifest.as_object_mut() {
            obj.insert(
                "id".into(),
                json!(dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()),
            );
        }
    }
    if let Some(obj) = manifest.as_object_mut() {
        obj.insert("dir".into(), json!(dir.to_string_lossy()));
        obj.insert("source".into(), json!(source));
        obj.insert("builtin".into(), json!(source == "bundled"));
    }
    Some(manifest)
}

pub fn list_skins() -> Vec<Value> {
    let _ = ensure_state_dir();
    let mut map: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    for (root, source) in [
        (bundled_skins_dir(), "bundled"),
        (user_skins_dir(), "user"),
    ] {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for ent in entries.flatten() {
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(skin) = read_skin_from_dir(&path, source) {
                if let Some(id) = skin.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()) {
                    map.insert(id, skin);
                }
            }
        }
    }
    let mut skins: Vec<_> = map.into_values().collect();
    skins.sort_by(|a, b| {
        let na = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let nb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        na.cmp(nb)
    });
    skins
}

pub fn get_skin(skin_id: &str) -> Result<Value, EngineError> {
    list_skins()
        .into_iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(skin_id))
        .ok_or_else(|| EngineError::msg(format!("Skin not found: {skin_id}")))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), EngineError> {
    fs::create_dir_all(dst).map_err(|e| EngineError::msg(format!("mkdir {}: {e}", dst.display())))?;
    for ent in fs::read_dir(src).map_err(|e| EngineError::msg(format!("read_dir: {e}")))? {
        let ent = ent.map_err(|e| EngineError::msg(e.to_string()))?;
        let from = ent.path();
        let to = dst.join(ent.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .map_err(|e| EngineError::msg(format!("copy {}: {e}", from.display())))?;
        }
    }
    Ok(())
}

fn skin_material_stamp(skin_dir: &Path) -> String {
    let manifest_path = skin_dir.join("skin.json");
    let mut parts = vec![
        skin_dir
            .canonicalize()
            .unwrap_or_else(|_| skin_dir.to_path_buf())
            .to_string_lossy()
            .to_string(),
        "v2".into(),
    ];
    let manifest: Value = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({}));
    let rels = [
        Some("skin.json".to_string()),
        manifest
            .pointer("/assets/css")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        manifest
            .pointer("/assets/art")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        manifest
            .pointer("/assets/plugin")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    ];
    for rel in rels.into_iter().flatten() {
        let abs = skin_dir.join(&rel);
        if let Ok(st) = fs::metadata(&abs) {
            let mtime = st
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis())
                .unwrap_or(0);
            parts.push(format!("{rel}:{}:{mtime}", st.len()));
        } else {
            parts.push(format!("{rel}:missing"));
        }
    }
    parts.join("|")
}

/// Copy skin to writable runtime-skins (same as Node materializeSkin).
pub fn materialize_skin(skin: &Value) -> Result<Value, EngineError> {
    let id = skin
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EngineError::msg("skin missing id"))?;
    let dir = skin
        .get("dir")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EngineError::msg("skin missing dir"))?;
    let src = PathBuf::from(dir);
    ensure_state_dir()?;
    let dest_root = state_root()
        .join("runtime-skins")
        .join(safe_skin_id(id));
    let stamp_path = dest_root.join(".src");
    let stamp = skin_material_stamp(&src);
    let mut need_copy = true;
    if dest_root.join("skin.json").is_file() && stamp_path.is_file() {
        if let Ok(prev) = fs::read_to_string(&stamp_path) {
            if prev.trim() == stamp {
                need_copy = false;
            }
        }
    }
    if need_copy {
        let _ = fs::remove_dir_all(&dest_root);
        copy_dir_recursive(&src, &dest_root)?;
        let _ = fs::write(&stamp_path, &stamp);
    }
    // Validate assets
    let manifest: Value = fs::read_to_string(dest_root.join("skin.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({}));
    for key in ["css", "art", "plugin"] {
        let rel = manifest
            .pointer(&format!("/assets/{key}"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if rel.is_empty() || !dest_root.join(rel).is_file() {
            // force recopy
            let _ = fs::remove_dir_all(&dest_root);
            copy_dir_recursive(&src, &dest_root)?;
            let _ = fs::write(&stamp_path, &stamp);
            break;
        }
    }
    let mut out = skin.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("dir".into(), json!(dest_root.to_string_lossy()));
    }
    Ok(out)
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

fn rm_dir_recursive(path: &Path) -> Result<(), EngineError> {
    if path.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| EngineError::msg(format!("remove {}: {e}", path.display())))?;
    }
    Ok(())
}

/// Delete user skin (or user override of bundled). No Node.
pub fn delete_skin_native(skin_id: &str) -> Result<Value, EngineError> {
    let skin = get_skin(skin_id)?;
    let source = skin
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let builtin = skin
        .get("builtin")
        .and_then(|v| v.as_bool())
        .unwrap_or(source == "bundled");
    let user_dir = user_skins_dir().join(skin_id);

    if builtin || source == "bundled" {
        if user_dir.is_dir() {
            rm_dir_recursive(&user_dir)?;
            return Ok(json!({
                "ok": true,
                "skinId": skin_id,
                "removed": "user-override",
                "engine": "native-rust",
            }));
        }
        return Err(EngineError::msg("内置皮肤不能删除，只能导出"));
    }
    if !user_dir.is_dir() {
        return Err(EngineError::msg("未找到可删除的用户皮肤"));
    }
    rm_dir_recursive(&user_dir)?;
    Ok(json!({
        "ok": true,
        "skinId": skin_id,
        "removed": "user",
        "engine": "native-rust",
    }))
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

/// `restart`: force stop+relaunch host (GUI auto-restart / desktopTheme refresh).
/// Full path: materialize → write desktopTheme → ensure debug port → staged CDP inject.
/// Theme is written **before** host restart so relaunch reads the new config.toml.
pub fn apply_skin_native_opts(skin_id: &str, restart: bool) -> Result<Value, EngineError> {
    let _guard = ENGINE_LOCK.lock();
    let port = shared_port();
    invalidate_host_probe_cache();
    let before = probe_host_lifecycle_force(port);
    let was_ready = before.can_hot_apply && !restart;

    append_diag(&format!(
        "apply_skin_native id={skin_id} restart={restart} lifecycle={} canHot={}",
        before.lifecycle, before.can_hot_apply
    ));

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

    set_paused(false);
    let root = engine::project_root();

    // Desktop chrome theme MUST be written before ensure_debug_port(restart):
    // host only reloads ~/.codex/config.toml [desktop] on process start.
    // Non-fatal for CSS skins, but surface result so UI/diag can show skip reason.
    let theme_result = if let Some(dt) = skin.get("desktopTheme") {
        match apply_desktop_theme(dt, &state_root()) {
            Ok(v) => {
                append_diag(&format!("apply_desktop_theme ok: {v}"));
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

    // Cold start / restart: bring host to injectable renderer without Node.
    ensure_debug_port(port, restart)?;

    let probe = probe_host_lifecycle_force(port);
    if !probe.renderer_ready && !probe.can_hot_apply {
        return Err(EngineError::msg(format!(
            "调试端口已处理但渲染页未就绪（lifecycle={}）",
            probe.lifecycle
        )));
    }
    note_host_ready(port);

    // Preflight payload assembly (catches bad skins before CDP).
    let staged = build_staged_payload(&skin_dir, &root)
        .map_err(|e| EngineError::msg(e.to_string()))?;
    let markers = staged.markers.clone();

    let browser_id = read_browser_identity(port)
        .ok()
        .map(|b| b.browser_id);

    let prev = read_state();
    let apply_mode = if restart || !was_ready {
        "native-cold"
    } else if prev
        .as_ref()
        .and_then(|p| p.get("skinId").and_then(|v| v.as_str()))
        == Some(id.as_str())
    {
        "native-hot-reapply"
    } else {
        "native-hot-switch"
    };

    let budget = inject_budget(port);
    let soft_timeout = budget.soft_once_timeout_ms;

    let mut last_err: Option<String> = None;
    let mut verified = false;
    let mut shell_ok = false;
    let mut art_ok = false;
    let mut art_pending = true;
    let mut shell_mode = "full".to_string();

    for i in 0..5 {
        match inject_once(&skin_dir, &root, port, true, soft_timeout) {
            Ok(parsed) => {
                if parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
                    || parsed
                        .get("shellOk")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                {
                    verified = true;
                    shell_ok = true;
                    art_ok = parsed
                        .get("artOk")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    art_pending = parsed
                        .get("artPending")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(!art_ok);
                    shell_mode = parsed
                        .get("shellMode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("full")
                        .to_string();
                    break;
                }
                last_err = Some("soft once did not pass".into());
            }
            Err(e) => last_err = Some(e.to_string()),
        }
        std::thread::sleep(std::time::Duration::from_millis(250 + i * 100));
    }

    if !verified {
        for _ in 0..4 {
            std::thread::sleep(std::time::Duration::from_millis(300));
            match inject_once(&skin_dir, &root, port, true, soft_timeout.max(12_000)) {
                Ok(parsed)
                    if parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
                        || parsed
                            .get("shellOk")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false) =>
                {
                    verified = true;
                    shell_ok = true;
                    art_ok = parsed
                        .get("artOk")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    art_pending = !art_ok;
                    break;
                }
                Ok(_) => {}
                Err(e) => last_err = Some(e.to_string()),
            }
        }
    }

    if !verified {
        return Err(EngineError::msg(format!(
            "换肤未完成（native CDP）: {}",
            last_err.unwrap_or_else(|| "unknown".into())
        )));
    }

    let started_at = prev
        .as_ref()
        .and_then(|p| p.get("startedAt").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(iso_now);

    let state = json!({
        "schema": 2,
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
        "verifiedAt": iso_now(),
        "engineVersion": ENGINE_VERSION,
        "nativeEngine": true,
        // No Node injector process on pure native path
        "injectorPid": null,
        "injectorScript": null,
        "nodePath": null,
    });
    write_state(&state)?;
    // Keep skin across ChatGPT refresh / SPA navigation without Node watch injector.
    start_keep(port, &id, skin_dir.clone(), markers);
    append_diag(&format!(
        "apply_skin_native ok id={id} mode={apply_mode} shellOk={shell_ok} artOk={art_ok} keep=1"
    ));

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
        "browserId": browser_id,
        "skinDir": skin_dir.to_string_lossy(),
        "lifecycle": "ready",
        "engineVersion": ENGINE_VERSION,
        "engine": "native-rust",
        "native": true,
        "restarted": restart || !was_ready,
        "keepAlive": true,
        "theme": theme_result,
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
        stop_host();
        std::thread::sleep(std::time::Duration::from_millis(700));
        match ensure_debug_port(port, false) {
            Ok(()) => relaunched = true,
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
        "paused": paused,
        "protocol": ENGINE_PROTOCOL,
        "engineVersion": ENGINE_VERSION,
        "engineName": ENGINE_NAME,
        "ok": true,
        "shellOk": state.as_ref().and_then(|s| s.get("shellOk").and_then(|v| v.as_bool())).unwrap_or(false),
        "artOk": state.as_ref().and_then(|s| s.get("artOk").and_then(|v| v.as_bool())).unwrap_or(false),
        "artPending": state.as_ref().and_then(|s| s.get("artPending").and_then(|v| v.as_bool())).unwrap_or(false),
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
    let mut body = host_lifecycle_to_json(&life, keep_armed());
    if let Some(obj) = body.as_object_mut() {
        obj.insert("platform".into(), json!(std::env::consts::OS));
        obj.insert("exe".into(), json!(exe));
        obj.insert("aumid".into(), Value::Null);
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
        "userSkins": user_skins_dir().to_string_lossy(),
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
