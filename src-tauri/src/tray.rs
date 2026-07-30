//! Native system tray — keep the process (and local routing proxy) alive while
//! the main window is hidden, with quick provider switching for Codex / Grok.
//!
//! Design goals (performance + stability):
//! - Menu is built only on demand (startup + provider mutations), never on a timer
//! - Rebuilds are coalesced (50ms) so bursty GUI saves don't thrash the tray
//! - Hide keeps the WebView in memory (instant restore); true quit restores live configs
//! - No skin switching (CDP apply is heavier and not needed for proxy survival)

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::providers::models::AppKind;
use crate::providers::store;

pub const TRAY_ID: &str = "chatgpt-tools";

const ID_SHOW_MAIN: &str = "show_main";
const ID_QUIT: &str = "quit";
const PREFIX_CODEX: &str = "prov_codex_";
const PREFIX_GROK: &str = "prov_grok_";

static REFRESH_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// Create the tray icon once during app setup.
pub fn setup_tray(app: &AppHandle) -> Result<(), String> {
    let menu = build_menu(app).map_err(|e| format!("创建托盘菜单失败: {e}"))?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("ChatGPT Tools")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            // Double-left-click restores the main window (common Windows habit).
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder
        .build(app)
        .map_err(|e| format!("创建系统托盘失败: {e}"))?;

    // Tooltip with current providers (best-effort, non-fatal).
    refresh_tooltip(app);
    Ok(())
}

/// Coalesced full menu rebuild after provider list / current changes.
pub fn schedule_tray_refresh(app: &AppHandle) {
    if REFRESH_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        REFRESH_SCHEDULED.store(false, Ordering::Release);
        refresh_tray_menu(&app);
    });
}

pub fn refresh_tray_menu(app: &AppHandle) {
    let Ok(menu) = build_menu(app) else {
        return;
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_menu(Some(menu));
    }
    refresh_tooltip(app);
}

/// Call after any provider archive mutation (switch / add / delete / …).
pub fn notify_providers_changed(app: &AppHandle) {
    schedule_tray_refresh(app);
}

pub fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_skip_taskbar(false);
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    #[cfg(target_os = "macos")]
    {
        apply_tray_policy(app, true);
    }
}

pub fn hide_main_to_tray(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.hide();
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_skip_taskbar(true);
    }
    #[cfg(target_os = "macos")]
    {
        apply_tray_policy(app, false);
    }
}

use std::sync::atomic::{AtomicBool, Ordering};

static HAS_CLEANED: AtomicBool = AtomicBool::new(false);

/// Remove the tray icon before process exit (avoids Windows ghost icons).
pub fn remove_tray_icon_before_exit(app: &AppHandle) {
    if HAS_CLEANED.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(false);
    }
}

pub fn quit_app(app: &AppHandle) {
    // First close the main window (triggers hide-to-tray if configured).
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.close();
    }
    remove_tray_icon_before_exit(app);
    app.exit(0);
}

#[cfg(target_os = "macos")]
fn apply_tray_policy(app: &AppHandle, dock_visible: bool) {
    use tauri::ActivationPolicy;
    let policy = if dock_visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };
    let _ = app.set_dock_visibility(dock_visible);
    let _ = app.set_activation_policy(policy);
}

fn handle_menu_event(app: &AppHandle, event_id: &str) {
    match event_id {
        ID_SHOW_MAIN => show_main_window(app),
        ID_QUIT => quit_app(app),
        id if id.starts_with(PREFIX_CODEX) => {
            let provider_id = id[PREFIX_CODEX.len()..].to_string();
            switch_from_tray(app, AppKind::Codex, provider_id);
        }
        id if id.starts_with(PREFIX_GROK) => {
            let provider_id = id[PREFIX_GROK.len()..].to_string();
            switch_from_tray(app, AppKind::Grok, provider_id);
        }
        _ => {}
    }
}

fn switch_from_tray(app: &AppHandle, kind: AppKind, provider_id: String) {
    let app_handle = app.clone();
    // Provider switch does file I/O / optional proxy work — keep the UI thread free.
    tauri::async_runtime::spawn_blocking(move || {
        let app_str = kind.as_str().to_string();
        match crate::providers::switch_provider(app_handle.clone(), app_str, provider_id.clone()) {
            Ok(result) => {
                // Emit only for tray-originated switches so the GUI can refresh
                // without double-toasting its own switch actions.
                let _ = app_handle.emit(
                    "provider-switched",
                    serde_json::json!({
                        "app": kind.as_str(),
                        "providerId": provider_id,
                        "message": result.message,
                        "ok": result.ok,
                        "source": "tray",
                    }),
                );
            }
            Err(err) => {
                let _ = app_handle.emit(
                    "provider-switch-failed",
                    serde_json::json!({
                        "app": kind.as_str(),
                        "providerId": provider_id,
                        "error": err,
                        "source": "tray",
                    }),
                );
            }
        }
    });
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let show = MenuItem::with_id(app, ID_SHOW_MAIN, "打开主界面", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "退出", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;

    let codex_sub = build_provider_submenu(app, AppKind::Codex)?;
    let grok_sub = build_provider_submenu(app, AppKind::Grok)?;

    MenuBuilder::new(app)
        .item(&show)
        .item(&sep)
        .item(&codex_sub)
        .item(&grok_sub)
        .item(&sep)
        .item(&quit)
        .build()
}

fn build_provider_submenu(
    app: &AppHandle,
    kind: AppKind,
) -> tauri::Result<tauri::menu::Submenu<tauri::Wry>> {
    let file = store::load().unwrap_or_default();
    let store = file.for_kind(kind);
    let current = store.current.as_str();
    let takeover = store.takeover_enabled;

    let current_name = store
        .providers
        .iter()
        .find(|p| p.id == current)
        .map(|p| p.name.as_str())
        .unwrap_or(if current.is_empty() { "未选择" } else { current });

    let header = if takeover {
        format!("{} · {} ⚡", kind.display_name(), current_name)
    } else {
        format!("{} · {}", kind.display_name(), current_name)
    };

    let mut sub = SubmenuBuilder::new(app, header);

    if store.providers.is_empty() {
        let empty = MenuItem::with_id(
            app,
            format!("{}_empty", kind.as_str()),
            "(无供应商)",
            false,
            None::<&str>,
        )?;
        sub = sub.item(&empty);
        return sub.build();
    }

    // Stable order: sort_index then name (matches typical GUI list expectations).
    let mut providers = store.providers.clone();
    providers.sort_by(|a, b| {
        a.sort_index
            .unwrap_or(usize::MAX)
            .cmp(&b.sort_index.unwrap_or(usize::MAX))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let prefix = match kind {
        AppKind::Codex => PREFIX_CODEX,
        AppKind::Grok => PREFIX_GROK,
    };

    for p in providers {
        let id = format!("{prefix}{}", p.id);
        let checked = p.id == current;
        // Official is always switchable; third-party needs key/url (cheap check).
        let enabled = p.is_official()
            || match kind {
                AppKind::Codex => crate::providers::codex::validate_for_switch(&p).is_ok(),
                AppKind::Grok => crate::providers::grok::validate_for_switch(&p).is_ok(),
            };
        let item = CheckMenuItem::with_id(app, id, &p.name, enabled, checked, None::<&str>)?;
        sub = sub.item(&item);
    }

    sub.build()
}

fn refresh_tooltip(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let file = store::load().unwrap_or_default();
    let codex = file.codex.providers.iter().find(|p| p.id == file.codex.current);
    let grok = file.grok.providers.iter().find(|p| p.id == file.grok.current);
    let codex_name = codex.map(|p| p.name.as_str()).unwrap_or("—");
    let grok_name = grok.map(|p| p.name.as_str()).unwrap_or("—");
    let tip = format!("ChatGPT Tools\nCodex: {codex_name}\nGrok: {grok_name}");
    // Windows shell tooltip is short; keep under ~120 chars.
    let tip = if tip.len() > 120 {
        format!("ChatGPT Tools · C:{codex_name} · G:{grok_name}")
            .chars()
            .take(120)
            .collect::<String>()
    } else {
        tip
    };
    let _ = tray.set_tooltip(Some(tip));
}
