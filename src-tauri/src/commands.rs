//! Tauri commands — stable API surface for the frontend `skinAPI`.

use crate::engine::{self, EngineError};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::{DialogExt, FilePath, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

/// Secondary window label for the skin / host DevTools UI.
pub const DEVTOOLS_WINDOW_LABEL: &str = "devtools";

/// Serializes open/focus so rapid clicks cannot race-create multiple windows.
static OPEN_DEVTOOLS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn focus_devtools_window(app: &AppHandle) -> bool {
    let Some(win) = app.get_webview_window(DEVTOOLS_WINDOW_LABEL) else {
        return false;
    };
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
    true
}

fn map_err(e: EngineError) -> String {
    e.to_string()
}

fn bool_str(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

async fn run_engine_async(args: Vec<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        // Single path: in-process Rust engine only (no Node spawn).
        engine::run_engine(&refs)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(map_err)
}

/// Guess mime from file extension for data-URL previews.
fn mime_from_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/png"
    }
}

/// Enrich status skins with data-URL previews.
/// Prefer `assets/screenshot.png` (thumbnail) over full art so WebView does not
/// choke on multi-megabyte illustrations. Failures are non-fatal.
fn enrich_previews(mut status: Value) -> Result<Value, EngineError> {
    let Some(skins) = status.get_mut("skins").and_then(|s| s.as_array_mut()) else {
        // 不要因字段缺失导致整个 status 失败 → 前端白屏/无卡片
        return Ok(status);
    };

    // Keep status IPC light so first paint is not multi-second base64 work.
    // Full wallpapers are for inject only — GUI cards use screenshot/preview thumbs.
    const MAX_TOTAL_PREVIEW: usize = 2_500_000;
    const MAX_SINGLE_PREVIEW: usize = 350_000;
    let mut total = 0usize;

    for skin in skins.iter_mut() {
        let id = skin
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        if total >= MAX_TOTAL_PREVIEW {
            break;
        }

        // Never pull multi-MB `art` into status previews (was a major boot stall).
        // resolve-asset("preview") already prefers screenshot over art when present.
        let kinds = ["preview", "screenshot"];
        for kind in kinds {
            match engine::run_engine(&["resolve-asset", "--skin-id", &id, "--kind", kind]) {
                Ok(resolved) => {
                    if let Some(path) = resolved.get("path").and_then(|p| p.as_str()) {
                        if let Ok(bytes) = engine::read_file_bytes(std::path::Path::new(path)) {
                            if bytes.len() > MAX_SINGLE_PREVIEW {
                                continue;
                            }
                            if total + bytes.len() > MAX_TOTAL_PREVIEW {
                                continue;
                            }
                            total += bytes.len();
                            let mime = mime_from_path(path);
                            let url = format!("data:{mime};base64,{}", B64.encode(bytes));
                            if let Some(obj) = skin.as_object_mut() {
                                obj.insert("previewUrl".into(), Value::String(url));
                                obj.insert(
                                    "previewKind".into(),
                                    Value::String(kind.to_string()),
                                );
                            }
                            break;
                        }
                    }
                }
                Err(_) => continue,
            }
        }
    }
    Ok(status)
}

#[tauri::command]
pub async fn status() -> Result<Value, String> {
    let raw = run_engine_async(vec!["status".into()]).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut status = enrich_previews(raw)?;
        // Merge disk catalog (network refresh is explicit via cloud_refresh).
        let cfg = crate::cloud::load_cloud_config();
        if cfg.enabled {
            let cat = crate::cloud::load_catalog_disk();
            crate::cloud::merge_remote_into_status(&mut status, cat.as_ref(), &cfg);
            // Catalog preview meta on every card (remote + local missing assets).
            crate::cloud::attach_remote_preview_meta(&mut status, cat.as_ref());
            // Instant fill from disk preview cache — no network on status path.
            crate::cloud::attach_disk_previews(&mut status);
        }
        Ok(status)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(map_err)
}

/// Lightweight ChatGPT/Codex lifecycle for GUI polling (no skin list / previews).
#[tauri::command]
pub async fn host_status(force: Option<bool>) -> Result<Value, String> {
    let force = force.unwrap_or(false);
    run_engine_async(vec![
        "host-status".into(),
        "--force".into(),
        bool_str(force).into(),
    ])
    .await
}

#[tauri::command]
pub async fn detect() -> Result<Value, String> {
    run_engine_async(vec!["detect".into()]).await
}

#[tauri::command]
pub async fn apply(skin_id: String, restart: Option<bool>) -> Result<Value, String> {
    // Default false: prefer hot-switch; GUI checkbox opts into client restart.
    let restart = restart.unwrap_or(false);
    run_engine_async(vec![
        "apply".into(),
        "--skin-id".into(),
        skin_id,
        "--restart".into(),
        bool_str(restart).into(),
    ])
    .await
}

#[tauri::command]
pub async fn restore(restore_theme: Option<bool>) -> Result<Value, String> {
    let restore_theme = restore_theme.unwrap_or(true);
    run_engine_async(vec![
        "restore".into(),
        "--restore-theme".into(),
        bool_str(restore_theme).into(),
    ])
    .await
}

#[tauri::command]
pub async fn pause() -> Result<Value, String> {
    run_engine_async(vec!["pause".into()]).await
}

#[tauri::command]
pub async fn resume(restart: Option<bool>) -> Result<Value, String> {
    let restart = restart.unwrap_or(false);
    run_engine_async(vec![
        "resume".into(),
        "--restart".into(),
        bool_str(restart).into(),
    ])
    .await
}

/// Launch ChatGPT/Codex; re-apply last session skin when available.
#[tauri::command]
pub async fn start_host() -> Result<Value, String> {
    run_engine_async(vec!["start-host".into()]).await
}

/// Hard restart ChatGPT/Codex; re-apply last session skin when available.
#[tauri::command]
pub async fn restart_host() -> Result<Value, String> {
    run_engine_async(vec!["restart-host".into()]).await
}

#[tauri::command]
pub async fn export_skin(app: AppHandle, skin_id: String) -> Result<Value, String> {
    let default_name = format!("{skin_id}.skin");
    let file = app
        .dialog()
        .file()
        .set_title("导出皮肤")
        .set_file_name(&default_name)
        .add_filter("ChatGPT 皮肤包", &["skin", "zip"])
        .blocking_save_file();

    let Some(path) = file else {
        return Ok(json!({ "ok": false, "canceled": true }));
    };
    let out = file_path_to_string(path)?;
    run_engine_async(vec![
        "export-skin".into(),
        "--skin-id".into(),
        skin_id,
        "--output".into(),
        out,
    ])
    .await
}

#[tauri::command]
pub async fn import_skin(app: AppHandle) -> Result<Value, String> {
    let cont = app
        .dialog()
        .message(
            "皮肤包可包含脚本（renderer-inject.js），换肤时会在 ChatGPT 界面中执行。\n\n\
可能带来的风险：\n\
• 读取或改动页面内容\n\
• 伪装界面 / 诱导操作\n\
• 在允许时发起网络请求\n\n\
请只导入你自己导出的，或完全信任的来源。",
        )
        .title("导入皮肤安全提示")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "继续选择文件".into(),
            "取消".into(),
        ))
        .blocking_show();
    if !cont {
        return Ok(json!({ "ok": false, "canceled": true }));
    }

    let file = app
        .dialog()
        .file()
        .set_title("导入皮肤")
        .add_filter("ChatGPT 皮肤包", &["skin", "zip", "cgskin"])
        .blocking_pick_file();
    let Some(path) = file else {
        return Ok(json!({ "ok": false, "canceled": true }));
    };
    let package_path = file_path_to_string(path)?;

    let info = run_engine_async(vec![
        "inspect-skin".into(),
        "--path".into(),
        package_path.clone(),
    ])
    .await?;

    let name = info
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("皮肤")
        .to_string();
    let skin_id = info
        .get("skinId")
        .and_then(|v| v.as_str())
        .unwrap_or("-")
        .to_string();
    let inject_path = info
        .get("injectPath")
        .and_then(|v| v.as_str())
        .unwrap_or("-")
        .to_string();
    let inject_bytes = info
        .get("injectBytes")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "?".into());
    let hash = info
        .get("injectSha256")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let hash_short = if hash.len() > 24 {
        format!("{}…{}", &hash[..16], &hash[hash.len() - 8..])
    } else {
        hash.clone()
    };
    let risks = info
        .get("risks")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| format!("• {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let warning = info
        .get("warning")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let file_name = info
        .get("fileName")
        .and_then(|v| v.as_str())
        .unwrap_or(package_path.as_str())
        .to_string();

    let detail = format!(
        "文件：{file_name}\nID：{skin_id}\n脚本：{inject_path}（{inject_bytes} 字节）\n\
脚本 SHA-256：{hash_short}\n\n安全检查：\n{risks}\n\n{warning}\n\n\
哈希用于核对脚本是否被改动；当前版本不会自动联网验签。"
    );

    let confirmed = app
        .dialog()
        .message(&detail)
        .title(&format!("确认导入「{name}」？"))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "确认导入".into(),
            "取消".into(),
        ))
        .blocking_show();
    if !confirmed {
        return Ok(json!({ "ok": false, "canceled": true }));
    }

    let mut result = run_engine_async(vec![
        "import-skin".into(),
        "--path".into(),
        package_path,
        "--overwrite".into(),
        "true".into(),
    ])
    .await?;
    if let Some(obj) = result.as_object_mut() {
        obj.insert("inspect".into(), info);
    }
    Ok(result)
}

#[tauri::command]
pub async fn delete_skin(skin_id: String) -> Result<Value, String> {
    run_engine_async(vec!["delete-skin".into(), "--skin-id".into(), skin_id]).await
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperPayload {
    pub base_skin_id: Option<String>,
    pub image_path: String,
    pub name: Option<String>,
    pub position: Option<String>,
    pub fit: Option<String>,
    pub accent: Option<String>,
    pub background: Option<String>,
    pub text: Option<String>,
    pub panel: Option<String>,
    pub font: Option<String>,
    pub radius: Option<serde_json::Value>,
    pub overlay: Option<serde_json::Value>,
    pub opacity: Option<serde_json::Value>,
    pub appearance: Option<String>,
    pub focus_x: Option<serde_json::Value>,
    pub focus_y: Option<serde_json::Value>,
    pub safe_area: Option<String>,
    pub task_mode: Option<String>,
}

#[tauri::command]
pub async fn design_wallpaper(payload: WallpaperPayload) -> Result<Value, String> {
    let value = serde_json::to_value(&payload).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::cdp::design_wallpaper_native(&value).map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn choose_app(app: AppHandle) -> Result<Value, String> {
    let mut dialog = app.dialog().file().set_title("选择 ChatGPT / Codex 客户端");
    if cfg!(windows) {
        dialog = dialog.add_filter("可执行文件", &["exe"]);
    } else if cfg!(target_os = "macos") {
        dialog = dialog.add_filter("应用", &["app"]);
    }
    let file = dialog.blocking_pick_file();
    let Some(path) = file else {
        return Ok(json!({ "canceled": true }));
    };
    let p = file_path_to_string(path)?;
    run_engine_async(vec!["set-app-path".into(), "--path".into(), p]).await
}

#[tauri::command]
pub async fn clear_app_path() -> Result<Value, String> {
    run_engine_async(vec!["clear-app-path".into()]).await
}

#[tauri::command]
pub async fn choose_wallpaper(app: AppHandle) -> Result<Value, String> {
    /// Match engine MAX_ART_BYTES — wallpaper selection hard cap.
    const MAX_WALLPAPER_BYTES: u64 = 16 * 1024 * 1024;
    let file = app
        .dialog()
        .file()
        .set_title("选择自定义皮肤壁纸")
        .add_filter("图片", &["png", "jpg", "jpeg", "webp"])
        .blocking_pick_file();
    let Some(path) = file else {
        return Ok(json!({ "canceled": true }));
    };
    let p = file_path_to_string(path)?;
    let meta = std::fs::metadata(&p).map_err(|e| format!("无法读取壁纸文件：{e}"))?;
    if !meta.is_file() || meta.len() < 1 {
        return Ok(json!({
            "canceled": false,
            "error": "请选择有效的图片文件"
        }));
    }
    if meta.len() > MAX_WALLPAPER_BYTES {
        return Ok(json!({
            "canceled": false,
            "error": format!(
                "壁纸必须不超过 {} MB（当前 {:.1} MB）",
                MAX_WALLPAPER_BYTES / 1024 / 1024,
                meta.len() as f64 / 1024.0 / 1024.0
            ),
            "size": meta.len(),
        }));
    }
    let name = std::path::Path::new(&p)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(json!({ "path": p, "name": name, "size": meta.len() }))
}

#[tauri::command]
pub async fn open_path(app: AppHandle, target: String) -> Result<(), String> {
    app.opener()
        .open_path(target, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reveal_export(app: AppHandle, file_path: String) -> Result<(), String> {
    let path = std::path::Path::new(&file_path);
    let target = if path.is_file() {
        path.parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(file_path.clone())
    } else {
        file_path.clone()
    };
    app.opener()
        .open_path(target, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn engine_paths() -> Result<Value, String> {
    run_engine_async(vec!["paths".into()]).await
}

#[tauri::command]
pub async fn engine_version() -> Result<Value, String> {
    run_engine_async(vec!["version".into()]).await
}

/// Open (or focus) the independent Skin DevTools window.
/// Reuses a single window labeled `devtools` so the button stays idempotent.
/// A process-wide lock prevents concurrent creates from rapid repeated clicks.
/// Closing the window tears down the dedicated inspect CDP session (Overlay/DOM/CSS).
#[tauri::command]
pub async fn open_devtools(app: AppHandle) -> Result<Value, String> {
    let _guard = OPEN_DEVTOOLS_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock();

    if focus_devtools_window(&app) {
        return Ok(json!({ "ok": true, "reused": true, "label": DEVTOOLS_WINDOW_LABEL }));
    }

    // Match main window chrome: undecorated + custom titlebar (see devtools.html).
    let win = match WebviewWindowBuilder::new(
        &app,
        DEVTOOLS_WINDOW_LABEL,
        WebviewUrl::App("devtools.html".into()),
    )
    .title("Skin DevTools")
    .inner_size(1280.0, 860.0)
    .min_inner_size(960.0, 640.0)
    .resizable(true)
    .center()
    .decorations(false)
    .build()
    {
        Ok(w) => w,
        Err(e) => {
            // Label already registered (race / half-closed): focus existing if present.
            if focus_devtools_window(&app) {
                return Ok(json!({
                    "ok": true,
                    "reused": true,
                    "label": DEVTOOLS_WINDOW_LABEL
                }));
            }
            return Err(e.to_string());
        }
    };

    // Title-bar close / Alt+F4 / OS destroy: always release inspect CDP resources.
    win.on_window_event(|event| {
        if matches!(
            event,
            WindowEvent::Destroyed | WindowEvent::CloseRequested { .. }
        ) {
            let _ = crate::cdp::inspect::disconnect();
        }
    });

    // Debug: allow inspect on Skin DevTools window too; release: lock chrome.
    #[cfg(debug_assertions)]
    crate::webview_guard::enable_dev_inspect_window(&win);
    #[cfg(not(debug_assertions))]
    crate::webview_guard::harden_window(&win);

    let _ = win.show();
    let _ = win.set_focus();

    Ok(json!({
        "ok": true,
        "reused": false,
        "label": DEVTOOLS_WINDOW_LABEL
    }))
}

// ── Host element inspect (Scheme A: real-window Overlay pick) ──────────────

fn map_inspect(e: crate::cdp::inspect::InspectError) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn inspect_connect() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| crate::cdp::inspect::connect().map_err(map_inspect))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn inspect_disconnect() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| crate::cdp::inspect::disconnect().map_err(map_inspect))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn inspect_status() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| crate::cdp::inspect::status().map_err(map_inspect))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn inspect_set_picking(enabled: bool) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::cdp::inspect::set_picking(enabled).map_err(map_inspect)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn inspect_poll(wait_ms: Option<u64>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::cdp::inspect::poll(wait_ms).map_err(map_inspect)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn inspect_get_document(depth: Option<i64>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::cdp::inspect::get_document_tree(depth).map_err(map_inspect)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn inspect_get_children(node_id: i64) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::cdp::inspect::get_children(node_id).map_err(map_inspect)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn inspect_select_node(node_id: i64) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::cdp::inspect::select_node(node_id).map_err(map_inspect)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn inspect_highlight(node_id: i64) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::cdp::inspect::highlight(node_id).map_err(map_inspect)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Cloud CDN (catalog / announcements / secure download) ───────────────────

#[tauri::command]
pub async fn cloud_status(force: Option<bool>) -> Result<Value, String> {
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || crate::cloud::cloud_status_snapshot(force))
        .await
        .map_err(|e| e.to_string())
}

/// Soft-refresh catalog + announcements (TTL + disk cache).
/// Pass `force: true` to bypass soft TTL (e.g. user clicked 刷新).
#[tauri::command]
pub async fn cloud_refresh(force: Option<bool>) -> Result<Value, String> {
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = crate::cloud::load_cloud_config();
        if !cfg.enabled {
            return Ok::<Value, String>(json!({
                "ok": false,
                "enabled": false,
                "error": "云端已关闭",
            }));
        }
        let sync = crate::cloud::soft_network_sync(&cfg, force);
        let snap = crate::cloud::cloud_status_snapshot(false);
        let ok = sync.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
            || snap.get("catalog").is_some();
        Ok(json!({
            "ok": ok,
            "snapshot": snap,
            "sync": sync,
            "catalogError": sync.get("catalogError").cloned().unwrap_or(Value::Null),
            "announcementsError": sync.get("announcementsError").cloned().unwrap_or(Value::Null),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_announcements(refresh: Option<bool>) -> Result<Value, String> {
    let refresh = refresh.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = crate::cloud::load_cloud_config();
        // Prefer disk; only hit network when explicitly refresh=true and TTL expired.
        if refresh {
            let _ = crate::cloud::soft_network_sync(&cfg, false);
        }
        crate::cloud::get_announcements(&cfg, false).map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_mark_announcement_read(id: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::cloud::mark_announcement_read(&id).map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Download skin **by catalog id only** — never accepts arbitrary URL from frontend.
#[tauri::command]
pub async fn cloud_download_skin(skin_id: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = crate::cloud::load_cloud_config();
        crate::cloud::download_skin(&cfg, &skin_id).map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ensure catalog preview thumbnails are cached and return data-URLs for GUI cards.
/// `skin_ids`: optional subset; omit/empty = all catalog skins with `preview.url`.
/// Network only on cache miss; safe to call after list paint (progressive fill).
#[tauri::command]
pub async fn cloud_ensure_previews(skin_ids: Option<Vec<String>>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = crate::cloud::load_cloud_config();
        let ids = skin_ids.and_then(|v| if v.is_empty() { None } else { Some(v) });
        Ok::<Value, String>(crate::cloud::ensure_missing_previews(&cfg, ids))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_check_update() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let cfg = crate::cloud::load_cloud_config();
        // Manual user action: allow soft network (still uses TTL unless cache empty).
        if cfg.enabled {
            let _ = crate::cloud::soft_network_sync(&cfg, false);
        }
        let cat = crate::cloud::load_catalog_disk();
        Ok::<Value, String>(crate::cloud::check_app_version_opts(
            &cfg,
            cat.as_ref(),
            true,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// About / contact from CDN (`/v1/about.json`) — not mixed with app version.
#[tauri::command]
pub async fn cloud_about(refresh: Option<bool>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = crate::cloud::load_cloud_config();
        let network = refresh.unwrap_or(false);
        Ok::<Value, String>(crate::cloud::get_about(&cfg, network))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_clear_skin_cache(skin_id: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Keep preview thumbnails by default so list still shows art after package purge.
        // Pass through package cache clear only.
        crate::cloud::clear_skin_cache(skin_id.as_deref()).map_err(map_err)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn file_path_to_string(path: FilePath) -> Result<String, String> {
    match path.into_path() {
        Ok(p) => Ok(p.to_string_lossy().to_string()),
        Err(e) => Err(e.to_string()),
    }
}
