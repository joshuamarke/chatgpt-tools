mod cdp;
mod commands;
mod engine;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Resolve project root early (dev: repo root; prod: resource dir)
    let root = resolve_app_root();
    engine::init_project_root(root.clone());
    std::env::set_var("CODEX_SKIN_ROOT", &root);
    // 状态目录名：%LOCALAPPDATA%\ChatGPTTools
    std::env::set_var("CODEX_SKIN_STATE_NAME", "ChatGPTTools");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running ChatGPT Tools");
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
