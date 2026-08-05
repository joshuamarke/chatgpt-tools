//! Toolbox enhancements: force Chinese locale, fast startup, Computer Use Guard.
//!
//! Domain layout follows `docs/architecture/features.md`:
//! backend here + `src/features/toolbox/` GUI.

pub mod computer_use_guard;
pub mod enhance_inject;
pub mod gate;
pub mod plugin_marketplace;
pub mod settings;

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use computer_use_guard::{
    ensure_computer_use_config_with_artifacts, resolve_computer_use_guard_artifacts, GuardArtifacts,
};
use gate::ToolboxRuntimeStatus;

const POST_LAUNCH_GUARD_SECONDS: &[u64] = &[0, 5, 15, 30, 60, 120, 180, 240, 300];
const POST_LAUNCH_STABLE_ATTEMPTS: usize = 3;

static GUARD_WATCHDOG_STOP: AtomicBool = AtomicBool::new(false);
static GUARD_WATCHDOG_STARTED: AtomicBool = AtomicBool::new(false);
static GUARD_ARTIFACTS: OnceLock<Mutex<Option<GuardArtifacts>>> = OnceLock::new();

fn guard_artifacts_slot() -> &'static Mutex<Option<GuardArtifacts>> {
    GUARD_ARTIFACTS.get_or_init(|| Mutex::new(None))
}

fn codex_home() -> PathBuf {
    crate::sessions::default_codex_home_dir()
}

/// Ensure Computer Use config when the setting is enabled (best-effort).
pub fn ensure_computer_use_guard_if_enabled() -> Result<Value, String> {
    if !settings::computer_use_guard_enabled() {
        return Ok(json!({
            "ok": true,
            "skipped": true,
            "reason": "disabled",
        }));
    }
    ensure_computer_use_guard_now()
}

/// Run guard once (config + marketplace + runtime exports).
pub fn ensure_computer_use_guard_now() -> Result<Value, String> {
    #[cfg(not(windows))]
    {
        return Ok(json!({
            "ok": true,
            "skipped": true,
            "reason": "windows-only",
            "message": "Windows Computer Use Guard 仅在 Windows 上生效",
        }));
    }
    #[cfg(windows)]
    {
        let home = codex_home();
        let artifacts = resolve_computer_use_guard_artifacts(&home).map_err(|e| e.to_string())?;
        let result =
            ensure_computer_use_config_with_artifacts(&home, &artifacts).map_err(|e| e.to_string())?;
        if let Ok(mut slot) = guard_artifacts_slot().lock() {
            *slot = Some(artifacts.clone());
        }
        Ok(json!({
            "ok": true,
            "changed": result.changed,
            "notifyExe": result.notify_exe.map(|p| p.to_string_lossy().to_string()),
            "marketplace": artifacts.marketplace_path.map(|p| p.to_string_lossy().to_string()),
            "runtimeExportsNeeded": artifacts.runtime_exports_needed,
        }))
    }
}

fn post_launch_artifacts_ready(artifacts: &GuardArtifacts) -> bool {
    artifacts.notify_exe.is_some()
        && artifacts.marketplace_path.is_some()
        && (!artifacts.runtime_exports_needed || artifacts.sky_package_json.is_some())
}

/// Start background retries after host launch (Windows only).
pub fn start_computer_use_guard_watchdog_if_enabled() {
    if !settings::computer_use_guard_enabled() {
        return;
    }
    #[cfg(not(windows))]
    {
        return;
    }
    #[cfg(windows)]
    {
        GUARD_WATCHDOG_STOP.store(false, Ordering::SeqCst);
        if GUARD_WATCHDOG_STARTED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            // Already running — kick another ensure in background.
            let _ = thread::Builder::new()
                .name("cgt-cu-guard-once".into())
                .spawn(|| {
                    let _ = ensure_computer_use_guard_now();
                });
            return;
        }
        let _ = thread::Builder::new()
            .name("cgt-cu-guard-watch".into())
            .spawn(|| post_launch_guard_loop());
    }
}

#[cfg(windows)]
fn post_launch_guard_loop() {
    let home = codex_home();
    let mut previous_delay = 0_u64;
    let mut stable_unchanged = 0_usize;
    let mut artifacts = guard_artifacts_slot()
        .lock()
        .ok()
        .and_then(|g| g.clone());

    for (index, delay) in POST_LAUNCH_GUARD_SECONDS.iter().copied().enumerate() {
        if GUARD_WATCHDOG_STOP.load(Ordering::SeqCst) {
            break;
        }
        let wait = delay.saturating_sub(previous_delay);
        previous_delay = delay;
        if wait > 0 {
            thread::sleep(Duration::from_secs(wait));
        }
        if GUARD_WATCHDOG_STOP.load(Ordering::SeqCst) {
            break;
        }
        if !settings::computer_use_guard_enabled() {
            break;
        }

        let resolved = match artifacts.take() {
            Some(a) => a,
            None => match resolve_computer_use_guard_artifacts(&home) {
                Ok(a) => a,
                Err(_) => {
                    stable_unchanged = 0;
                    continue;
                }
            },
        };
        let ready = post_launch_artifacts_ready(&resolved);
        artifacts = ready.then_some(resolved.clone());
        match ensure_computer_use_config_with_artifacts(&home, &resolved) {
            Ok(result) => {
                if !result.changed && ready {
                    stable_unchanged += 1;
                } else {
                    stable_unchanged = 0;
                }
                if stable_unchanged >= POST_LAUNCH_STABLE_ATTEMPTS && ready {
                    break;
                }
            }
            Err(_) => {
                stable_unchanged = 0;
                artifacts = None;
            }
        }
        let _ = index;
    }
    GUARD_WATCHDOG_STARTED.store(false, Ordering::SeqCst);
}

/// Host launch / ready hook: enhancements inject + optional Computer Use Guard.
pub fn on_host_ready() {
    // Third-party: best-effort ensure remote plugin cache + local marketplace config
    // so list-plugins merge has catalogs even under API Key auth.
    if gate::plugin_marketplace_unlock_effective() {
        let home = codex_home();
        let _ = plugin_marketplace::ensure_plugin_marketplaces_for_third_party(&home);
    }
    enhance_inject::on_host_ready();
    if settings::computer_use_guard_enabled() {
        let _ = ensure_computer_use_guard_now();
        start_computer_use_guard_watchdog_if_enabled();
    }
}

/// Provider switch / live routing change: re-evaluate third-party gate and re-inject.
///
/// Marketplace config touch + enhance inject are relatively cheap when local-only,
/// but enhance still opens CDP — callers on the GUI command path should prefer
/// scheduling this off-thread (see `model_unlock::schedule_*`).
pub fn on_provider_changed() {
    if gate::plugin_marketplace_unlock_effective() {
        let home = codex_home();
        let _ = plugin_marketplace::ensure_plugin_marketplaces_for_third_party(&home);
    }
    enhance_inject::on_settings_changed();
}

/// Warm settings cache at app startup.
pub fn warm_settings() {
    let _ = settings::get_settings();
    // Arm enhance keep so first host open applies effective force-chinese / fast-startup.
    if settings::force_chinese_locale()
        || settings::plugin_marketplace_unlock()
        || settings::fast_startup()
    {
        enhance_inject::on_settings_changed();
    }
}

#[tauri::command]
pub fn get_toolbox_settings() -> ToolboxRuntimeStatus {
    gate::runtime_status()
}

#[tauri::command]
pub fn update_toolbox_settings(
    force_chinese_locale: Option<bool>,
    plugin_marketplace_unlock: Option<bool>,
    fast_startup: Option<bool>,
    computer_use_guard_enabled: Option<bool>,
) -> Result<ToolboxRuntimeStatus, String> {
    let prev = settings::get_settings();
    let next = settings::update_settings(
        force_chinese_locale,
        plugin_marketplace_unlock,
        fast_startup,
        computer_use_guard_enabled,
    )?;

    // Re-inject when third-party-gated prefs or fast-startup change.
    if next.force_chinese_locale != prev.force_chinese_locale
        || next.plugin_marketplace_unlock != prev.plugin_marketplace_unlock
        || next.fast_startup != prev.fast_startup
    {
        if next.plugin_marketplace_unlock && gate::third_party_codex_active() {
            let home = codex_home();
            let _ = plugin_marketplace::ensure_plugin_marketplaces_for_third_party(&home);
        }
        enhance_inject::on_settings_changed();
    }

    // Computer Use Guard: apply immediately when turned on.
    if next.computer_use_guard_enabled && !prev.computer_use_guard_enabled {
        let _ = ensure_computer_use_guard_now();
        start_computer_use_guard_watchdog_if_enabled();
    }
    if !next.computer_use_guard_enabled && prev.computer_use_guard_enabled {
        GUARD_WATCHDOG_STOP.store(true, Ordering::SeqCst);
    }

    Ok(gate::runtime_status())
}

#[tauri::command]
pub fn apply_computer_use_guard_now() -> Result<Value, String> {
    ensure_computer_use_guard_now()
}

#[tauri::command]
pub fn plugin_marketplace_status() -> Result<Value, String> {
    let home = codex_home();
    let local = plugin_marketplace::openai_curated_marketplace_status(&home);
    Ok(json!({
        "codexHome": home.to_string_lossy(),
        "local": {
            "marketplaceRoot": local.marketplace_root.as_ref().map(|p| p.to_string_lossy().to_string()),
            "configRegistered": local.config_registered,
            "needsRepair": local.needs_repair(),
        },
        "thirdPartyActive": gate::third_party_codex_active(),
        "pluginUnlockEffective": gate::plugin_marketplace_unlock_effective(),
        "localCatalogCount": plugin_marketplace::local_plugin_marketplaces_json(&home)
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0),
        "networkHint": plugin_marketplace::PLUGIN_MARKETPLACE_NETWORK_HINT,
        // Unlock on + third-party but local catalog still missing.
        "needsRepairForFullUnlock": local.needs_repair()
            && gate::plugin_marketplace_unlock_effective(),
    }))
}

/// Download openai/plugins from GitHub and register local curated marketplaces.
/// Requires network access to codeload.github.com — no built-in zip.
#[tauri::command]
pub async fn repair_plugin_marketplace() -> Result<Value, String> {
    let home = codex_home();
    match plugin_marketplace::initialize_openai_curated_marketplace_and_configure(&home).await {
        Ok(result) => {
            let status = plugin_marketplace::openai_curated_marketplace_status(&home);
            if gate::plugin_marketplace_unlock_effective() {
                enhance_inject::on_settings_changed();
            }
            Ok(json!({
                "ok": true,
                "initialized": result.initialized,
                "configured": result.configured,
                "needsRepair": status.needs_repair(),
                "marketplaceRoot": status.marketplace_root.map(|p| p.to_string_lossy().to_string()),
                "networkHint": plugin_marketplace::PLUGIN_MARKETPLACE_NETWORK_HINT,
                "message": if result.initialized {
                    "已从 GitHub openai/plugins 下载并注册本地插件市场。"
                } else if result.configured {
                    "本地插件市场已存在，已重新写入配置。"
                } else {
                    "本地插件市场已可用，无需修复。"
                },
            }))
        }
        Err(e) => Err(format!(
            "插件市场修复失败：{e}\n{}",
            plugin_marketplace::PLUGIN_MARKETPLACE_NETWORK_HINT
        )),
    }
}
