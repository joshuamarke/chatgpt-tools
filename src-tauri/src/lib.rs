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
mod tray;

use tauri::{Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Resolve project root early (dev: repo root; prod: resource dir)
    let root = resolve_app_root();
    engine::init_project_root(root.clone());
    std::env::set_var("CODEX_SKIN_ROOT", &root);
    // 状态目录名：%LOCALAPPDATA%\ChatGPTTools
    std::env::set_var("CODEX_SKIN_STATE_NAME", "ChatGPTTools");
    // Align cloud version filters with product version (GUI APP_VERSION).
    if std::env::var("CODEX_SKIN_APP_VERSION").is_err() {
        std::env::set_var("CODEX_SKIN_APP_VERSION", "1.1.12");
    }

    let mut builder = tauri::Builder::default();

    // Second launch focuses the existing instance (required for tray-resident apps).
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }));
    }

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
            // Prefer resource_dir in production bundles
            if let Ok(resource_dir) = app.path().resource_dir() {
                let candidates = [
                    resource_dir.clone(),
                    resource_dir.join("resources"),
                    resource_dir.join(".."),
                ];
                for c in candidates {
                    if c.join("engine").join("cli.mjs").is_file()
                        || c.join("engine").join("manager.js").is_file()
                    {
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

            // Warm UI settings cache (CloseRequested uses the atomic fast path).
            let _ = app_settings::get_settings();

            // System tray: hide-to-tray keeps local routing alive.
            if let Err(e) = tray::setup_tray(app.handle()) {
                eprintln!("[tray] setup failed: {e}");
            }

            // Re-assert local routing if last session left takeover enabled.
            proxy::restore_on_startup();

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

fn resolve_app_root() -> std::path::PathBuf {
    if let Ok(env_root) = std::env::var("CODEX_SKIN_ROOT") {
        let p = std::path::PathBuf::from(env_root);
        if p.join("engine").join("cli.mjs").is_file() {
            return p;
        }
    }
    // CARGO_MANIFEST_DIR = src-tauri → parent is project root
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest);
    if repo.join("engine").join("cli.mjs").is_file() {
        return repo;
    }
    std::env::current_dir().unwrap_or(repo)
}
