//! Grok Build local history sessions (`~/.grok/sessions`).
//!
//! Layout (per session directory):
//! - `summary.json` — metadata (id, cwd, title, timestamps, model)
//! - `chat_history.jsonl` — conversation lines (`type`: user/assistant/system/tool)
//!
//! Grok Build session scan / delete / Markdown export under `~/.grok/sessions`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::models::{DeleteResult, DeleteStatus, ExportResult, ExportStatus};
use super::storage::LocalSession;
use super::util::normalize_display_path;

const TITLE_MAX_CHARS: usize = 80;

#[derive(Debug, Deserialize)]
struct GrokSessionInfo {
    id: String,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GrokSessionSummary {
    info: GrokSessionInfo,
    #[serde(default)]
    session_summary: Option<String>,
    #[serde(default)]
    generated_title: Option<String>,
    #[serde(default)]
    created_at: Option<Value>,
    #[serde(default)]
    updated_at: Option<Value>,
    #[serde(default)]
    last_active_at: Option<Value>,
    #[serde(default)]
    current_model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokSessionsPayload {
    pub sessions: Vec<LocalSession>,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
    pub grok_home: String,
    pub session_roots: Vec<String>,
}

/// Grok Build home (`GROK_HOME` override or `~/.grok`).
pub fn default_grok_home_dir() -> PathBuf {
    if let Ok(p) = std::env::var("GROK_HOME") {
        let t = p.trim();
        if !t.is_empty() {
            let path = PathBuf::from(t);
            if path.is_dir() {
                return path;
            }
        }
    }
    user_home_dir().join(".grok")
}

fn user_home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        return PathBuf::from(home);
    }
    PathBuf::from(".")
}

pub fn session_roots() -> Vec<PathBuf> {
    let home = default_grok_home_dir();
    vec![
        home.join("sessions"),
        home.join("archived_sessions"),
    ]
}

/// Scan all Grok sessions and return a paged slice (newest first).
pub fn list_sessions_paged(offset: usize, limit: usize) -> GrokSessionsPayload {
    let home = default_grok_home_dir();
    let roots = session_roots();
    let mut sessions = scan_all_sessions(&roots);
    sessions.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| b.id.cmp(&a.id))
    });
    let mut seen = std::collections::HashSet::new();
    sessions.retain(|s| seen.insert(s.id.clone()));
    let has_more = sessions.len() > offset.saturating_add(limit);
    let page: Vec<_> = sessions.into_iter().skip(offset).take(limit).collect();
    GrokSessionsPayload {
        sessions: page,
        offset,
        limit,
        has_more,
        grok_home: home.to_string_lossy().to_string(),
        session_roots: roots
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    }
}

fn scan_all_sessions(roots: &[PathBuf]) -> Vec<LocalSession> {
    let mut files = Vec::new();
    for root in roots {
        collect_summary_files(root, &mut files);
    }
    files
        .into_iter()
        .filter_map(|path| parse_summary_as_local(&path))
        .collect()
}

fn collect_summary_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_summary_files(&path, files);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("summary.json") {
            files.push(path);
        }
    }
}

fn read_summary(path: &Path) -> Result<GrokSessionSummary, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 Grok 会话摘要失败：{e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("解析 Grok 会话摘要失败：{e}"))
}

fn parse_summary_as_local(path: &Path) -> Option<LocalSession> {
    let summary = read_summary(path).ok()?;
    let session_id = summary.info.id;
    let title = summary
        .generated_title
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            summary
                .session_summary
                .as_deref()
                .filter(|v| !v.trim().is_empty())
        })
        .map(|v| truncate_summary(v, TITLE_MAX_CHARS))
        .unwrap_or_else(|| "未命名会话".into());
    let updated_at_ms = summary
        .last_active_at
        .as_ref()
        .or(summary.updated_at.as_ref())
        .or(summary.created_at.as_ref())
        .and_then(parse_timestamp_to_ms);
    let archived = path
        .components()
        .any(|c| c.as_os_str() == "archived_sessions");
    let model = summary
        .current_model_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "grok".into());
    Some(LocalSession {
        id: session_id,
        title,
        cwd: normalize_display_path(&summary.info.cwd.unwrap_or_default()),
        model_provider: model,
        archived,
        updated_at_ms,
        // Reuse rollout_path as the summary.json source path for delete/export.
        rollout_path: path.to_string_lossy().to_string(),
        db_path: String::new(),
    })
}

/// Delete one Grok session directory (permanent; no undo token).
pub fn delete_session(session_id: &str, source_path: Option<&str>) -> Result<DeleteResult, String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err("会话 ID 不能为空。".into());
    }

    let path = resolve_summary_path(session_id, source_path)?;
    let roots = session_roots();
    let validated_source = canonicalize_existing(&path, "会话源路径")?;

    let mut saw_root = false;
    for root in &roots {
        if !root.exists() {
            continue;
        }
        saw_root = true;
        let validated_root = match canonicalize_existing(root, "会话根目录") {
            Ok(r) => r,
            Err(_) => continue,
        };
        if validated_source.starts_with(&validated_root) {
            delete_session_dir(&validated_root, &validated_source, session_id)?;
            return Ok(DeleteResult {
                status: DeleteStatus::LocalDeleted,
                session_id: session_id.to_string(),
                message: format!("已删除 Grok 会话 {session_id}"),
                undo_token: None,
                backup_path: None,
            });
        }
    }

    if !saw_root {
        return Err(format!(
            "未找到 Grok 会话目录（请确认已安装 Grok Build 并存在 ~/.grok/sessions）。"
        ));
    }
    Err(format!(
        "会话路径不在 Grok 会话根目录内：{}",
        path.display()
    ))
}

fn resolve_summary_path(session_id: &str, source_path: Option<&str>) -> Result<PathBuf, String> {
    if let Some(raw) = source_path.map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(raw);
        if p.is_file() {
            return Ok(p);
        }
    }
    // Fallback: rescan and match by id.
    for root in session_roots() {
        let mut files = Vec::new();
        collect_summary_files(&root, &mut files);
        for path in files {
            if let Ok(summary) = read_summary(&path) {
                if summary.info.id == session_id {
                    return Ok(path);
                }
            }
        }
    }
    Err(format!("未找到 Grok 会话：{session_id}"))
}

fn delete_session_dir(root: &Path, path: &Path, session_id: &str) -> Result<(), String> {
    if !path.starts_with(root) {
        return Err(format!(
            "Grok 会话源路径在根目录之外：{}",
            path.display()
        ));
    }
    if path.file_name().and_then(|n| n.to_str()) != Some("summary.json") {
        return Err(format!("意外的 Grok 会话源：{}", path.display()));
    }
    let summary = read_summary(path)?;
    if summary.info.id != session_id {
        return Err(format!(
            "会话 ID 不匹配：期望 {session_id}，实际 {}",
            summary.info.id
        ));
    }
    let session_dir = path
        .parent()
        .ok_or_else(|| format!("无效的 Grok 会话路径：{}", path.display()))?;
    if session_dir == root || !session_dir.starts_with(root) {
        return Err(format!(
            "拒绝删除根目录外的会话目录：{}",
            session_dir.display()
        ));
    }
    if session_dir.file_name().and_then(|n| n.to_str()) != Some(session_id) {
        return Err(format!(
            "会话目录名与 ID 不一致：{}",
            session_dir.display()
        ));
    }
    std::fs::remove_dir_all(session_dir).map_err(|e| {
        format!(
            "删除 Grok 会话目录失败 {}：{e}",
            session_dir.display()
        )
    })?;
    Ok(())
}

/// Export Grok chat history as Markdown.
pub fn export_markdown(session_id: &str, title: &str, source_path: Option<&str>) -> ExportResult {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return ExportResult {
            status: ExportStatus::Failed,
            session_id: String::new(),
            message: "会话 ID 不能为空。".into(),
            filename: None,
            markdown: None,
        };
    }
    match export_markdown_inner(session_id, title, source_path) {
        Ok(r) => r,
        Err(message) => ExportResult {
            status: ExportStatus::Failed,
            session_id: session_id.to_string(),
            message,
            filename: None,
            markdown: None,
        },
    }
}

fn export_markdown_inner(
    session_id: &str,
    title: &str,
    source_path: Option<&str>,
) -> Result<ExportResult, String> {
    let path = resolve_summary_path(session_id, source_path)?;
    let summary = read_summary(&path)?;
    if summary.info.id != session_id {
        return Err(format!(
            "会话 ID 不匹配：期望 {session_id}，实际 {}",
            summary.info.id
        ));
    }
    let session_dir = path
        .parent()
        .ok_or_else(|| format!("无效路径：{}", path.display()))?;
    let chat_path = session_dir.join("chat_history.jsonl");
    let messages = load_messages_from_chat(&chat_path)?;

    let display_title = if !title.trim().is_empty() {
        title.trim().to_string()
    } else {
        summary
            .generated_title
            .as_deref()
            .or(summary.session_summary.as_deref())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("未命名会话")
            .to_string()
    };

    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", display_title));
    md.push_str(&format!("- **Session ID**: `{session_id}`\n"));
    if let Some(cwd) = summary.info.cwd.as_deref().filter(|s| !s.is_empty()) {
        md.push_str(&format!("- **CWD**: `{cwd}`\n"));
    }
    if let Some(model) = summary.current_model_id.as_deref().filter(|s| !s.is_empty()) {
        md.push_str(&format!("- **Model**: `{model}`\n"));
    }
    md.push_str(&format!(
        "- **Source**: `{}`\n\n",
        path.to_string_lossy()
    ));
    md.push_str("---\n\n");

    if messages.is_empty() {
        md.push_str("_（无对话消息）_\n");
    } else {
        for (role, content) in messages {
            let heading = match role.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                "system" => "System",
                "tool" => "Tool",
                other => other,
            };
            md.push_str(&format!("### {heading}\n\n"));
            md.push_str(content.trim());
            md.push_str("\n\n");
        }
    }

    let safe_title: String = display_title
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let safe_title = safe_title.trim().chars().take(48).collect::<String>();
    let filename = if safe_title.is_empty() {
        format!("{session_id}.md")
    } else {
        format!("{safe_title}-{session_id}.md")
    };

    Ok(ExportResult {
        status: ExportStatus::Exported,
        session_id: session_id.to_string(),
        message: "已生成 Markdown".into(),
        filename: Some(filename),
        markdown: Some(md),
    })
}

fn load_messages_from_chat(chat_path: &Path) -> Result<Vec<(String, String)>, String> {
    if !chat_path.is_file() {
        return Ok(Vec::new());
    }
    let file = File::open(chat_path).map_err(|e| format!("打开 chat_history 失败：{e}"))?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let role = match kind {
            "system" | "user" | "assistant" | "tool" => kind,
            // Skip reasoning / internal records.
            _ => continue,
        };
        let content = value
            .get("content")
            .map(extract_text)
            .unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }
        messages.push((role.to_string(), content));
    }
    Ok(messages)
}

fn canonicalize_existing(path: &Path, label: &str) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|e| format!("{label} 无效（{}）：{e}", path.display()))
}

fn parse_timestamp_to_ms(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return Some(if n > 1_000_000_000_000 { n } else { n * 1000 });
    }
    if let Some(n) = value.as_f64() {
        let n = n as i64;
        return Some(if n > 1_000_000_000_000 { n } else { n * 1000 });
    }
    let raw = value.as_str()?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn extract_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(extract_text_from_item)
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => map
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn extract_text_from_item(item: &Value) -> Option<String> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
    if item_type == "tool_use" {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Some(format!("[Tool: {name}]"));
    }
    if item_type == "tool_result" {
        if let Some(content) = item.get("content") {
            let text = extract_text(content);
            if !text.is_empty() {
                return Some(text);
            }
        }
        return None;
    }
    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }
    if let Some(text) = item.get("input_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }
    if let Some(text) = item.get("output_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }
    if let Some(content) = item.get("content") {
        let text = extract_text(content);
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

fn truncate_summary(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut result = trimmed.chars().take(max_chars).collect::<String>();
    result.push_str("...");
    result
}
