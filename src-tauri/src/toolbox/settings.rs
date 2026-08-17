//! Toolbox enhancement settings (force Chinese / fast startup / Computer Use Guard).
//!
//! Stored under the app state dir as `toolbox-settings.json`, separate from
//! tray UI prefs and providers so host enhancements stay cheap to read on launch.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "toolbox-settings.json";

static FORCE_CHINESE: AtomicBool = AtomicBool::new(false);
static PLUGIN_MARKETPLACE_UNLOCK: AtomicBool = AtomicBool::new(false);
static FAST_STARTUP: AtomicBool = AtomicBool::new(false);
static COMPUTER_USE_GUARD: AtomicBool = AtomicBool::new(false);
static LOADED: AtomicBool = AtomicBool::new(false);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxSettings {
    /// User preference: allow force zh-CN on third-party Codex. Default off.
    /// Effective only when live routing is non-official (see `gate`).
    #[serde(default)]
    pub force_chinese_locale: bool,
    /// User preference: allow plugin marketplace unlock on third-party. Default off.
    /// Effective only when live routing is non-official.
    #[serde(default)]
    pub plugin_marketplace_unlock: bool,
    /// Fast-fail Statsig network so cold start is shorter without VPN. Default off.
    /// Not gated by third-party (applies whenever host is launched with the flag).
    #[serde(default)]
    pub fast_startup: bool,
    /// Windows-only: keep Computer Use plugins / notify / marketplace healthy.
    #[serde(default)]
    pub computer_use_guard_enabled: bool,
}

impl Default for ToolboxSettings {
    fn default() -> Self {
        Self {
            force_chinese_locale: false,
            plugin_marketplace_unlock: false,
            fast_startup: false,
            computer_use_guard_enabled: false,
        }
    }
}

fn settings_path() -> PathBuf {
    crate::sessions::paths::app_state_dir().join(FILE_NAME)
}

fn ensure_loaded() {
    if LOADED.load(Ordering::Acquire) {
        return;
    }
    let settings = read_from_disk();
    apply_cache(&settings);
    LOADED.store(true, Ordering::Release);
}

fn apply_cache(settings: &ToolboxSettings) {
    FORCE_CHINESE.store(settings.force_chinese_locale, Ordering::Release);
    PLUGIN_MARKETPLACE_UNLOCK.store(settings.plugin_marketplace_unlock, Ordering::Release);
    FAST_STARTUP.store(settings.fast_startup, Ordering::Release);
    COMPUTER_USE_GUARD.store(settings.computer_use_guard_enabled, Ordering::Release);
}

fn read_from_disk() -> ToolboxSettings {
    let path = settings_path();
    if !path.is_file() {
        return ToolboxSettings::default();
    }
    match fs::read_to_string(&path) {
        Ok(text) if !text.trim().is_empty() => {
            serde_json::from_str(&text).unwrap_or_default()
        }
        _ => ToolboxSettings::default(),
    }
}

fn write_to_disk(settings: &ToolboxSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let text =
        serde_json::to_string_pretty(settings).map_err(|e| format!("序列化失败: {e}"))?;
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

/// Snapshot for IPC / UI (preferences only; see `gate::runtime_status` for effective).
pub fn get_settings() -> ToolboxSettings {
    ensure_loaded();
    ToolboxSettings {
        force_chinese_locale: FORCE_CHINESE.load(Ordering::Acquire),
        plugin_marketplace_unlock: PLUGIN_MARKETPLACE_UNLOCK.load(Ordering::Acquire),
        fast_startup: FAST_STARTUP.load(Ordering::Acquire),
        computer_use_guard_enabled: COMPUTER_USE_GUARD.load(Ordering::Acquire),
    }
}

/// User preference only (not third-party gated).
pub fn force_chinese_locale() -> bool {
    ensure_loaded();
    FORCE_CHINESE.load(Ordering::Acquire)
}

/// User preference only (not third-party gated).
pub fn plugin_marketplace_unlock() -> bool {
    ensure_loaded();
    PLUGIN_MARKETPLACE_UNLOCK.load(Ordering::Acquire)
}

pub fn fast_startup() -> bool {
    ensure_loaded();
    FAST_STARTUP.load(Ordering::Acquire)
}

pub fn computer_use_guard_enabled() -> bool {
    ensure_loaded();
    COMPUTER_USE_GUARD.load(Ordering::Acquire)
}

#[allow(dead_code)]
pub fn set_settings(next: ToolboxSettings) -> Result<ToolboxSettings, String> {
    ensure_loaded();
    let _guard = WRITE_LOCK.lock();
    write_to_disk(&next)?;
    apply_cache(&next);
    LOADED.store(true, Ordering::Release);
    Ok(next)
}

pub fn update_settings(
    force_chinese_locale: Option<bool>,
    plugin_marketplace_unlock: Option<bool>,
    fast_startup: Option<bool>,
    computer_use_guard_enabled: Option<bool>,
) -> Result<ToolboxSettings, String> {
    ensure_loaded();
    let _guard = WRITE_LOCK.lock();
    let mut settings = read_from_disk();
    if let Some(v) = force_chinese_locale {
        settings.force_chinese_locale = v;
    }
    if let Some(v) = plugin_marketplace_unlock {
        settings.plugin_marketplace_unlock = v;
    }
    if let Some(v) = fast_startup {
        settings.fast_startup = v;
    }
    if let Some(v) = computer_use_guard_enabled {
        settings.computer_use_guard_enabled = v;
    }
    write_to_disk(&settings)?;
    apply_cache(&settings);
    LOADED.store(true, Ordering::Release);
    Ok(settings)
}
