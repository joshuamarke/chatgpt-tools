mod app_settings;
mod cdp;
mod cloud;
mod commands;
mod engine;
mod env;
mod live_config;
mod providers;
mod proxy;
mod sessions;
mod toolbox;
mod tray;
/// GUI WebView inspect policy (debug allow / release block). See module docs.
pub(crate) mod webview_guard;

use tauri::{Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Resolve project root early (dev: repo root; prod: resource dir)
    let root = resolve_app_root();
    engine::init_project_root(root.clone());
    std::env::set_var("CODEX_SKIN_ROOT", &root);
    // 状态目录名：%LOCALAPPDATA%\ChatGPTTools
    std::env::set_var("CODEX_SKIN_STATE_NAME", "ChatGPTTools");
    // Windows 免安装包：记下当前 exe 目录，供 NSIS /UPDATE 原地覆盖（不要落到默认 LOCALAPPDATA）。
    #[cfg(all(windows, not(debug_assertions)))]
    persist_windows_update_install_dir();
    // Align cloud version filters with the package version (Cargo / tauri.conf).
    // Single source of truth — do not hardcode a second GUI version string.
    if std::env::var("CODEX_SKIN_APP_VERSION").is_err() {
        std::env::set_var("CODEX_SKIN_APP_VERSION", env!("CARGO_PKG_VERSION"));
    }

    // Rebind (not `mut`) so debug builds — which skip the release-only plugins
    // below — do not warn about unused mutability.
    let builder = tauri::Builder::default();

    // Second launch focuses the existing instance (tray-resident apps).
    // Debug only: skip single-instance so `tauri dev` is never swallowed by an
    // already-running *release* install (same bundle id) — that made F12/右键
    // look "broken in dev" while the focused window was actually packaged.
    #[cfg(all(
        not(debug_assertions),
        any(target_os = "macos", windows, target_os = "linux")
    ))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        tray::show_main_window(app);
    }));

    // Packaged/release only: strip WebView right-click menu + F12 / DevTools hotkeys.
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(webview_guard::plugin());

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Only the main window participates in minimize-to-tray.
                if window.label() != "main" {
                    return;
                }
                if app_settings::minimize_to_tray_on_close() {
                    api.prevent_close();
                    tray::hide_main_to_tray(window.app_handle());
                }
                // else: allow close → last window exit → RunEvent::Exit → proxy shutdown
            }
        })
        .setup(move |app| {
            // Production only: rebind to the packaged resource dir (engine + staged skins).
            //
            // Do **not** do this in `tauri dev`: resource_dir still exists and often
            // contains `bundle-resources/skins` (default install set, e.g. only
            // qingkong). Overwriting the repo root made the GUI list a single
            // "内置" skin and hide the full `skins/` workspace.
            #[cfg(not(debug_assertions))]
            if let Ok(resource_dir) = app.path().resource_dir() {
                let candidates = [
                    resource_dir.clone(),
                    resource_dir.join("resources"),
                    resource_dir.join(".."),
                ];
                for c in candidates {
                    if engine::is_app_root(&c) {
                        let normalized = c.canonicalize().unwrap_or(c);
                        engine::init_project_root(normalized.clone());
                        std::env::set_var("CODEX_SKIN_ROOT", &normalized);
                        break;
                    }
                }
            }

            // 去掉系统/默认原生菜单栏（Windows 菜单条、macOS 默认 App 菜单）
            let _ = app.remove_menu();
            for (_, window) in app.webview_windows() {
                let _ = window.remove_menu();
            }

            // Debug: force WebView2 context menu + DevTools on (F12 / 右键审查).
            // Release: native off + JS guard plugin already registered above.
            #[cfg(debug_assertions)]
            webview_guard::enable_dev_inspect_all(app.handle());
            #[cfg(not(debug_assertions))]
            webview_guard::harden_all(app.handle());

            // Warm UI settings cache (CloseRequested uses the atomic fast path).
            let _ = app_settings::get_settings();
            // Toolbox enhancements (force Chinese / fast startup / Computer Use Guard).
            toolbox::warm_settings();
            // Cleanup accumulated provider live backups on boot
            providers::backup_utils::prune_all_provider_backups();

            // System tray: hide-to-tray keeps local routing alive.
            if let Err(e) = tray::setup_tray(app.handle()) {
                eprintln!("[tray] setup failed: {e}");
            }

            // Re-assert local routing off the setup critical path so first paint
            // is not blocked by proxy bind / live config rewrite.
            tauri::async_runtime::spawn(async {
                let _ = tauri::async_runtime::spawn_blocking(|| {
                    proxy::restore_on_startup();
                })
                .await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::host_status,
            commands::detect,
            commands::apply,
            commands::restore,
            commands::pause,
            commands::resume,
            commands::start_host,
            commands::restart_host,
            commands::export_skin,
            commands::import_skin,
            commands::delete_skin,
            commands::design_wallpaper,
            commands::choose_app,
            commands::clear_app_path,
            commands::choose_wallpaper,
            commands::open_path,
            commands::open_external,
            commands::reveal_export,
            commands::engine_paths,
            commands::engine_version,
            env::env_check,
            env::env_check_tool,
            env::open_install_terminal,
            commands::open_devtools,
            commands::inspect_connect,
            commands::inspect_disconnect,
            commands::inspect_status,
            commands::inspect_set_picking,
            commands::inspect_poll,
            commands::inspect_get_document,
            commands::inspect_get_children,
            commands::inspect_select_node,
            commands::inspect_highlight,
            commands::cloud_status,
            commands::cloud_refresh,
            commands::cloud_announcements,
            commands::cloud_mark_announcement_read,
            commands::cloud_download_skin,
            commands::cloud_ensure_previews,
            commands::cloud_check_update,
            commands::cloud_about,
            commands::cloud_clear_skin_cache,
            sessions::commands::list_local_sessions,
            sessions::commands::delete_local_session,
            sessions::commands::undo_local_session,
            sessions::commands::export_local_session_markdown,
            sessions::commands::load_provider_sync_targets,
            sessions::commands::sync_providers_now,
            sessions::commands::preview_session_index_cleanup,
            sessions::commands::apply_session_index_cleanup_cmd,
            sessions::commands::session_paths_info,
            sessions::commands::list_grok_sessions,
            sessions::commands::delete_grok_session,
            sessions::commands::export_grok_session_markdown,
            providers::list_providers,
            providers::get_provider,
            providers::add_provider,
            providers::update_provider,
            providers::delete_provider,
            providers::switch_provider,
            providers::import_live_as_provider,
            providers::provider_paths_info,
            providers::list_provider_presets,
            providers::reapply_current_provider,
            providers::get_preserve_codex_official_auth,
            providers::set_preserve_codex_official_auth,
            providers::test_provider_connectivity,
            providers::fetch_provider_models,
            providers::refresh_codex_model_unlock,
            proxy::get_proxy_status,
            proxy::get_proxy_config,
            proxy::update_proxy_config,
            proxy::get_proxy_takeover_status,
            proxy::set_proxy_takeover,
            proxy::get_app_proxy_settings,
            proxy::update_app_proxy_settings,
            proxy::set_auto_failover,
            proxy::get_failover_queue,
            proxy::add_to_failover_queue,
            proxy::remove_from_failover_queue,
            proxy::reorder_failover_queue,
            proxy::reset_provider_circuit,
            proxy::stop_proxy_with_restore,
            proxy::repair_proxy_takeover,
            proxy::check_proxy_listen_port,
            proxy::list_proxy_request_logs,
            proxy::get_proxy_request_log,
            proxy::clear_proxy_request_logs,
            proxy::get_proxy_log_retention_days,
            proxy::set_proxy_log_retention_days,
            app_settings::get_app_ui_settings,
            app_settings::set_minimize_to_tray_on_close_cmd,
            toolbox::get_toolbox_settings,
            toolbox::update_toolbox_settings,
            toolbox::apply_computer_use_guard_now,
            toolbox::plugin_marketplace_status,
            toolbox::repair_plugin_marketplace,
        ])
        .build(tauri::generate_context!())
        .expect("error while building ChatGPT Tools")
        .run(|_app_handle, event| {
            if let RunEvent::Exit = event {
                // Only real process exit restores direct live configs.
                // Hide-to-tray must NOT reach here — proxy stays up for Codex/Grok.
                // tray removal already happened in quit_app; guard prevents double call.
                proxy::shutdown_on_exit();
            }
        });
}

/// Remember the directory of the running exe so the next in-app NSIS update
/// (`/UPDATE`) can overwrite this copy instead of `%LOCALAPPDATA%\ChatGPT Tools`.
#[cfg(all(windows, not(debug_assertions)))]
fn persist_windows_update_install_dir() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else {
        return;
    };
    let binding = dir.to_string_lossy();
    let Some(dir) = normalize_windows_install_dir(&binding) else {
        return;
    };
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("reg")
        .args([
            "add",
            r"HKCU\Software\ChatGPTTools",
            "/v",
            "UpdateInstallDir",
            "/t",
            "REG_SZ",
            "/d",
            dir,
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

#[cfg(all(windows, not(debug_assertions)))]
fn normalize_windows_install_dir(dir: &str) -> Option<&str> {
    let dir = dir.strip_prefix(r"\\?\").unwrap_or(dir).trim();
    let dir = dir.trim_end_matches(['\\', '/']);
    if dir.is_empty() {
        None
    } else {
        Some(dir)
    }
}

fn resolve_app_root() -> std::path::PathBuf {
    if let Ok(env_root) = std::env::var("CODEX_SKIN_ROOT") {
        let p = std::path::PathBuf::from(env_root);
        if engine::is_app_root(&p) {
            return p;
        }
    }
    // CARGO_MANIFEST_DIR = src-tauri → parent is project root
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest);
    if engine::is_app_root(&repo) {
        return repo;
    }
    std::env::current_dir().unwrap_or(repo)
}
