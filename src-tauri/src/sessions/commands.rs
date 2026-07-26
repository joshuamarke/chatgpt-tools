//! Tauri IPC for local Codex session management (list / delete / undo / export / repair).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use super::backup::BackupStore;
use super::discovery::{codex_session_db_paths_from_home, default_codex_home_dir};
use super::grok;
use super::markdown::export_markdown_from_paths;
use super::models::{DeleteStatus, ExportStatus, SessionRef};
use super::paths::session_backups_dir;
use super::provider_sync::{
    apply_session_index_cleanup as apply_index_cleanup,
    load_provider_sync_targets as load_sync_targets, preview_session_index_cleanup as preview_index,
    run_provider_sync_with_target,
};
use super::storage::{delete_local_from_paths, LocalSession, SQLiteStorageAdapter};

const DEFAULT_PAGE: usize = 50;
const MAX_PAGE: usize = 100;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListLocalSessionsRequest {
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_page")]
    pub limit: usize,
}

fn default_page() -> usize {
    DEFAULT_PAGE
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSessionsPayload {
    pub db_path: String,
    pub db_paths: Vec<String>,
    pub sessions: Vec<LocalSession>,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
    pub codex_home: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLocalSessionRequest {
    pub session_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub db_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoLocalSessionRequest {
    pub undo_token: String,
    #[serde(default)]
    pub db_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLocalSessionRequest {
    pub session_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub db_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProvidersRequest {
    #[serde(default)]
    pub target_provider: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplySessionIndexCleanupRequest {
    pub snapshot_sha256: String,
    #[serde(default)]
    pub thread_ids: Vec<String>,
}

fn candidate_db_paths(preferred: Option<&str>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = preferred {
        let path = PathBuf::from(path);
        if path.is_file() {
            paths.push(path);
        }
    }
    let home = default_codex_home_dir();
    for path in codex_session_db_paths_from_home(&home) {
        if !paths.iter().any(|c| c == &path) {
            paths.push(path);
        }
    }
    paths
}

fn local_session_adapter(db_path: &PathBuf) -> SQLiteStorageAdapter {
    let backup = BackupStore::new(session_backups_dir());
    let mut adapter = SQLiteStorageAdapter::new(db_path.clone(), backup);
    let home = default_codex_home_dir();
    let all = codex_session_db_paths_from_home(&home);
    adapter = adapter.with_allowed_db_paths(all);
    adapter
}

fn file_path_to_string(path: tauri_plugin_dialog::FilePath) -> Result<String, String> {
    match path.into_path() {
        Ok(p) => Ok(p.to_string_lossy().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Blocking scan of local Codex SQLite DBs (runs on worker thread).
fn list_local_sessions_blocking(
    request: Option<ListLocalSessionsRequest>,
) -> Result<Value, String> {
    let request = request.unwrap_or(ListLocalSessionsRequest {
        offset: 0,
        limit: DEFAULT_PAGE,
    });
    let offset = request.offset;
    let limit = request.limit.clamp(1, MAX_PAGE);
    let fetch_limit = offset.saturating_add(limit).saturating_add(1);
    let home = default_codex_home_dir();
    let db_paths = codex_session_db_paths_from_home(&home);
    let mut sessions = Vec::new();
    let mut errors = Vec::new();

    for db_path in &db_paths {
        let adapter = local_session_adapter(db_path);
        match adapter.list_local_sessions_limited(fetch_limit) {
            Ok(mut items) => sessions.append(&mut items),
            Err(error) if db_path.exists() => {
                errors.push(format!("{}: {error}", db_path.to_string_lossy()));
            }
            Err(_) => {}
        }
    }

    sessions.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    let mut seen = std::collections::HashSet::new();
    sessions.retain(|s| seen.insert(s.id.clone()));
    let has_more = sessions.len() > offset.saturating_add(limit);
    let sessions: Vec<_> = sessions.into_iter().skip(offset).take(limit).collect();

    let payload = LocalSessionsPayload {
        db_path: db_paths
            .first()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        db_paths: db_paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        sessions,
        offset,
        limit,
        has_more,
        codex_home: home.to_string_lossy().to_string(),
    };

    let mut out = serde_json::to_value(payload).map_err(|e| e.to_string())?;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("ok".into(), json!(errors.is_empty()));
        if !errors.is_empty() {
            obj.insert("warnings".into(), json!(errors));
        }
    }
    Ok(out)
}

/// List local Codex sessions (paged, multi-DB merge).
///
/// Async + `spawn_blocking` so SQLite I/O does not freeze the Tauri main / UI thread
/// while the sessions view shell is already painted.
#[tauri::command]
pub async fn list_local_sessions(
    request: Option<ListLocalSessionsRequest>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || list_local_sessions_blocking(request))
        .await
        .map_err(|e| e.to_string())?
}

/// Delete one local session (DB rows + rollout file); writes undo backup under app state.
#[tauri::command]
pub fn delete_local_session(request: DeleteLocalSessionRequest) -> Result<Value, String> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return Err("会话 ID 不能为空。".into());
    }
    let session = SessionRef {
        session_id: session_id.to_string(),
        title: request.title,
    };
    let candidate_paths = candidate_db_paths(request.db_path.as_deref());
    let result = delete_local_from_paths(
        candidate_paths,
        BackupStore::new(session_backups_dir()),
        &session,
    );

    let ok = matches!(result.status, DeleteStatus::LocalDeleted);
    let mut value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("ok".into(), json!(ok));
    }
    if !ok {
        return Err(result.message);
    }
    Ok(value)
}

/// Restore a previously deleted session from an undo backup token.
#[tauri::command]
pub fn undo_local_session(request: UndoLocalSessionRequest) -> Result<Value, String> {
    let token = request.undo_token.trim();
    if token.is_empty() {
        return Err("撤销令牌不能为空。".into());
    }
    let paths = candidate_db_paths(request.db_path.as_deref());
    let Some(primary) = paths.first().cloned() else {
        return Err("未找到可用的本地会话数据库。".into());
    };
    let adapter = local_session_adapter(&primary).with_allowed_db_paths(paths);
    let result = adapter.undo(token);
    let ok = matches!(result.status, DeleteStatus::Undone);
    let mut value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("ok".into(), json!(ok));
    }
    if !ok {
        return Err(result.message);
    }
    Ok(value)
}

/// Export one session as Markdown and open a save dialog.
#[tauri::command]
pub fn export_local_session_markdown(
    app: AppHandle,
    request: ExportLocalSessionRequest,
) -> Result<Value, String> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return Err("会话 ID 不能为空。".into());
    }
    let session = SessionRef {
        session_id: session_id.to_string(),
        title: request.title.clone(),
    };
    let paths = candidate_db_paths(request.db_path.as_deref());
    let result = export_markdown_from_paths(paths, &session);
    if !matches!(result.status, ExportStatus::Exported) {
        return Err(result.message);
    }
    let markdown = result
        .markdown
        .clone()
        .ok_or_else(|| "导出内容为空。".to_string())?;
    let default_name = result
        .filename
        .clone()
        .unwrap_or_else(|| format!("{session_id}.md"));

    let file = app
        .dialog()
        .file()
        .set_title("导出会话 Markdown")
        .set_file_name(&default_name)
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .blocking_save_file();

    let Some(path) = file else {
        return Ok(json!({
            "ok": false,
            "canceled": true,
            "sessionId": result.session_id,
            "message": "已取消保存",
        }));
    };
    let out = file_path_to_string(path)?;
    std::fs::write(&out, markdown.as_bytes()).map_err(|e| format!("写入失败：{e}"))?;

    Ok(json!({
        "ok": true,
        "status": "exported",
        "sessionId": result.session_id,
        "message": format!("已导出到 {out}"),
        "filename": default_name,
        "path": out,
    }))
}

/// List provider ids that can be used as historical repair targets.
/// Offloaded to a worker thread (config + history scan can be slow).
#[tauri::command]
pub async fn load_provider_sync_targets() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let home = default_codex_home_dir();
        let list = load_sync_targets(Some(&home));
        let mut value = serde_json::to_value(list).map_err(|e| e.to_string())?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("ok".into(), json!(true));
            obj.insert("codexHome".into(), json!(home.to_string_lossy()));
        }
        Ok(value)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Rewrite historical session_meta / SQLite provider markers to a target provider.
#[tauri::command]
pub fn sync_providers_now(request: Option<SyncProvidersRequest>) -> Result<Value, String> {
    let home = default_codex_home_dir();
    let target = request.and_then(|r| r.target_provider).filter(|s| !s.trim().is_empty());
    let result = run_provider_sync_with_target(Some(&home), target.as_deref());
    let mut value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
    let ok = matches!(
        result.status,
        super::provider_sync::ProviderSyncStatus::Synced
    );
    if let Some(obj) = value.as_object_mut() {
        obj.insert("ok".into(), json!(ok));
        // PathBuf serializes as string already; normalize for camelCase consumers
        if let Some(dir) = result.backup_dir.as_ref() {
            obj.insert("backupDir".into(), json!(dir.to_string_lossy()));
        }
        let skipped: Vec<String> = result
            .skipped_locked_rollout_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        obj.insert("skippedLockedRolloutFiles".into(), json!(skipped));
    }
    if !ok && result.status != super::provider_sync::ProviderSyncStatus::Synced {
        // Skipped is not always fatal for UI — still return payload with ok:false
        // so the frontend can show message without throw when desired.
    }
    Ok(value)
}

/// Preview orphan entries only present in session_index.jsonl.
#[tauri::command]
pub fn preview_session_index_cleanup() -> Result<Value, String> {
    let home = default_codex_home_dir();
    let preview = preview_index(Some(&home)).map_err(|e| e.to_string())?;
    let mut value = serde_json::to_value(&preview).map_err(|e| e.to_string())?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("ok".into(), json!(true));
        obj.insert("codexHome".into(), json!(home.to_string_lossy()));
    }
    Ok(value)
}

/// Apply selected session_index orphan cleanup (requires matching snapshot hash).
#[tauri::command]
pub fn apply_session_index_cleanup_cmd(
    request: ApplySessionIndexCleanupRequest,
) -> Result<Value, String> {
    let home = default_codex_home_dir();
    let sha = request.snapshot_sha256.trim();
    if sha.is_empty() {
        return Err("缺少 snapshotSha256，请先预览。".into());
    }
    // Pass None so process-stop check matches Codex++ Manager safety gate.
    let result = apply_index_cleanup(None, sha, &request.thread_ids).map_err(|e| {
        let mut msg = e.message;
        if let Some(dir) = e.backup_dir {
            msg = format!("{msg}（备份：{}）", dir.to_string_lossy());
        }
        msg
    })?;
    Ok(json!({
        "ok": true,
        "prunedEntries": result.pruned_entries,
        "backupDir": result.backup_dir.map(|p| p.to_string_lossy().to_string()),
        "codexHome": home.to_string_lossy(),
    }))
}

/// Resolve Codex home + candidate DB paths (for UI diagnostics).
#[tauri::command]
pub fn session_paths_info() -> Result<Value, String> {
    let home = default_codex_home_dir();
    let db_paths = codex_session_db_paths_from_home(&home);
    let grok_home = grok::default_grok_home_dir();
    Ok(json!({
        "ok": true,
        "codexHome": home.to_string_lossy(),
        "homeExists": home.is_dir(),
        "dbPaths": db_paths.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "backupsDir": session_backups_dir().to_string_lossy(),
        "grokHome": grok_home.to_string_lossy(),
        "grokHomeExists": grok_home.is_dir(),
        "grokSessionRoots": grok::session_roots()
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>(),
    }))
}

// ── Grok Build local history ──────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListGrokSessionsRequest {
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_page")]
    pub limit: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteGrokSessionRequest {
    pub session_id: String,
    #[serde(default)]
    pub title: String,
    /// Path to `summary.json` (from list `rolloutPath`).
    #[serde(default)]
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportGrokSessionRequest {
    pub session_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub source_path: Option<String>,
}

/// List Grok Build sessions under `~/.grok/sessions` (paged, newest first).
/// Directory walk + summary parse runs on a worker thread.
#[tauri::command]
pub async fn list_grok_sessions(
    request: Option<ListGrokSessionsRequest>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let request = request.unwrap_or(ListGrokSessionsRequest {
            offset: 0,
            limit: DEFAULT_PAGE,
        });
        let offset = request.offset;
        let limit = request.limit.clamp(1, MAX_PAGE);
        let payload = grok::list_sessions_paged(offset, limit);
        let mut out = serde_json::to_value(payload).map_err(|e| e.to_string())?;
        if let Some(obj) = out.as_object_mut() {
            obj.insert("ok".into(), json!(true));
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Permanently delete one Grok Build session directory (no undo).
#[tauri::command]
pub fn delete_grok_session(request: DeleteGrokSessionRequest) -> Result<Value, String> {
    let _ = request.title;
    let result = grok::delete_session(
        &request.session_id,
        request.source_path.as_deref(),
    )?;
    let ok = matches!(result.status, DeleteStatus::LocalDeleted);
    let mut value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("ok".into(), json!(ok));
    }
    if !ok {
        return Err(result.message);
    }
    Ok(value)
}

/// Export one Grok Build session as Markdown via save dialog.
#[tauri::command]
pub fn export_grok_session_markdown(
    app: AppHandle,
    request: ExportGrokSessionRequest,
) -> Result<Value, String> {
    let result = grok::export_markdown(
        &request.session_id,
        &request.title,
        request.source_path.as_deref(),
    );
    if !matches!(result.status, ExportStatus::Exported) {
        return Err(result.message);
    }
    let markdown = result
        .markdown
        .clone()
        .ok_or_else(|| "导出内容为空。".to_string())?;
    let default_name = result
        .filename
        .clone()
        .unwrap_or_else(|| format!("{}.md", result.session_id));

    let file = app
        .dialog()
        .file()
        .set_title("导出 Grok 会话 Markdown")
        .set_file_name(&default_name)
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .blocking_save_file();

    let Some(path) = file else {
        return Ok(json!({
            "ok": false,
            "canceled": true,
            "sessionId": result.session_id,
            "message": "已取消保存",
        }));
    };
    let out = file_path_to_string(path)?;
    std::fs::write(&out, markdown.as_bytes()).map_err(|e| format!("写入失败：{e}"))?;

    Ok(json!({
        "ok": true,
        "status": "exported",
        "sessionId": result.session_id,
        "message": format!("已导出到 {out}"),
        "filename": default_name,
        "path": out,
    }))
}
