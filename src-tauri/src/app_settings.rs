//! Lightweight UI/host settings (tray, window close behavior).
//!
//! Stored separately from providers.json so host lifecycle prefs stay small and
//! cheap to read on every CloseRequested.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "app-ui-settings.json";

/// Cached close-to-tray flag — CloseRequested must stay allocation-light.
static MINIMIZE_TO_TRAY: AtomicBool = AtomicBool::new(true);
static LOADED: AtomicBool = AtomicBool::new(false);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUiSettings {
    /// When true, the window close button hides to the system tray instead of
    /// exiting — keeps local routing / proxy alive in the background.
    #[serde(default = "default_true")]
    pub minimize_to_tray_on_close: bool,
}

impl Default for AppUiSettings {
    fn default() -> Self {
        Self {
            minimize_to_tray_on_close: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn settings_path() -> PathBuf {
    crate::sessions::paths::app_state_dir().join(FILE_NAME)
}

fn ensure_loaded() {
    if LOADED.load(Ordering::Acquire) {
        return;
    }
    let settings = read_from_disk();
    MINIMIZE_TO_TRAY.store(settings.minimize_to_tray_on_close, Ordering::Release);
    LOADED.store(true, Ordering::Release);
}

fn read_from_disk() -> AppUiSettings {
    let path = settings_path();
    if !path.is_file() {
        return AppUiSettings::default();
    }
    match fs::read_to_string(&path) {
        Ok(text) if !text.trim().is_empty() => {
            serde_json::from_str(&text).unwrap_or_default()
        }
        _ => AppUiSettings::default(),
    }
}

fn write_to_disk(settings: &AppUiSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| format!("序列化失败: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("写入临时文件失败: {e}"))?;
        f.write_all(text.as_bytes())
            .map_err(|e| format!("写入临时文件失败: {e}"))?;
        f.sync_all().map_err(|e| format!("同步文件失败: {e}"))?;
    }
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("替换配置文件失败: {e}")
    })
}

/// Snapshot for IPC / UI.
pub fn get_settings() -> AppUiSettings {
    ensure_loaded();
    AppUiSettings {
        minimize_to_tray_on_close: MINIMIZE_TO_TRAY.load(Ordering::Acquire),
    }
}

/// Fast path for CloseRequested — no JSON parse after first load.
pub fn minimize_to_tray_on_close() -> bool {
    ensure_loaded();
    MINIMIZE_TO_TRAY.load(Ordering::Acquire)
}

pub fn set_minimize_to_tray_on_close(value: bool) -> Result<AppUiSettings, String> {
    ensure_loaded();
    let _guard = WRITE_LOCK.lock();
    let mut settings = read_from_disk();
    settings.minimize_to_tray_on_close = value;
    write_to_disk(&settings)?;
    MINIMIZE_TO_TRAY.store(value, Ordering::Release);
    LOADED.store(true, Ordering::Release);
    Ok(settings)
}

#[tauri::command]
pub fn get_app_ui_settings() -> AppUiSettings {
    get_settings()
}

#[tauri::command]
pub fn set_minimize_to_tray_on_close_cmd(enabled: bool) -> Result<AppUiSettings, String> {
    set_minimize_to_tray_on_close(enabled)
}
