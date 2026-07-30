//! Local request-log store for the routing proxy.
//!
//! Stores metadata only (no API keys / bodies). Controlled by
//! `GlobalProxyConfig.enable_logging` on the write path.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use once_cell::sync::Lazy;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEFAULT_RETENTION_DAYS: u32 = 7;
const MIN_RETENTION_DAYS: u32 = 1;
const MAX_RETENTION_DAYS: u32 = 365;
const MAX_ROWS_HARD_CAP: i64 = 5000;
const META_RETENTION_KEY: &str = "retention_days";

static DB_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn db_path() -> PathBuf {
    crate::sessions::paths::app_state_dir().join("proxy-request-logs.db")
}

fn open_db() -> Result<Connection, String> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建日志目录失败: {e}"))?;
    }
    let conn = Connection::open(&path).map_err(|e| format!("打开请求日志库失败: {e}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS request_logs (
            id TEXT PRIMARY KEY NOT NULL,
            created_at INTEGER NOT NULL,
            app TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            provider_name TEXT NOT NULL DEFAULT '',
            model TEXT NOT NULL DEFAULT '',
            method TEXT NOT NULL DEFAULT '',
            path TEXT NOT NULL DEFAULT '',
            status_code INTEGER NOT NULL DEFAULT 0,
            latency_ms INTEGER NOT NULL DEFAULT 0,
            is_streaming INTEGER NOT NULL DEFAULT 0,
            attempt INTEGER NOT NULL DEFAULT 1,
            error_message TEXT,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            first_token_ms INTEGER
         );
         CREATE INDEX IF NOT EXISTS idx_req_logs_created ON request_logs(created_at DESC);
         CREATE INDEX IF NOT EXISTS idx_req_logs_app ON request_logs(app, created_at DESC);
         CREATE INDEX IF NOT EXISTS idx_req_logs_provider ON request_logs(provider_id);
         CREATE INDEX IF NOT EXISTS idx_req_logs_status ON request_logs(status_code);
         CREATE TABLE IF NOT EXISTS log_meta (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
         );",
    )
    .map_err(|e| format!("初始化请求日志库失败: {e}"))?;
    // Migrate older DBs that predate first_token_ms / token columns.
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN first_token_ms INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN input_tokens INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN output_tokens INTEGER NOT NULL DEFAULT 0",
        [],
    );
    Ok(conn)
}

fn with_db<T>(f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
    let _guard = DB_LOCK
        .lock()
        .map_err(|_| "请求日志库锁损坏".to_string())?;
    let conn = open_db()?;
    f(&conn)
}

fn read_retention_days(conn: &Connection) -> u32 {
    conn.query_row(
        "SELECT value FROM log_meta WHERE key = ?1",
        params![META_RETENTION_KEY],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| s.parse::<u32>().ok())
    .map(clamp_retention_days)
    .unwrap_or(DEFAULT_RETENTION_DAYS)
}

pub fn clamp_retention_days(days: u32) -> u32 {
    days.clamp(MIN_RETENTION_DAYS, MAX_RETENTION_DAYS)
}

pub fn default_retention_days() -> u32 {
    DEFAULT_RETENTION_DAYS
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogEntry {
    pub id: String,
    pub created_at: i64,
    pub app: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub latency_ms: u64,
    pub is_streaming: bool,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// Time to first response byte / first SSE event (ms), when measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_token_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogFilters {
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    /// `all` | `2xx` | `4xx` | `5xx` | `error`
    #[serde(default)]
    pub status_class: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
}

fn default_page_size() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogPage {
    pub data: Vec<RequestLogEntry>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    pub retention_days: u32,
    pub logging_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct NewRequestLog {
    pub app: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub latency_ms: u64,
    pub is_streaming: bool,
    pub attempt: u32,
    pub error_message: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub first_token_ms: Option<u64>,
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<RequestLogEntry> {
    Ok(RequestLogEntry {
        id: row.get(0)?,
        created_at: row.get(1)?,
        app: row.get(2)?,
        provider_id: row.get(3)?,
        provider_name: row.get(4)?,
        model: row.get(5)?,
        method: row.get(6)?,
        path: row.get(7)?,
        status_code: row.get::<_, i64>(8)? as u16,
        latency_ms: row.get::<_, i64>(9)? as u64,
        is_streaming: row.get::<_, i64>(10)? != 0,
        attempt: row.get::<_, i64>(11)? as u32,
        error_message: row.get(12)?,
        input_tokens: row.get::<_, i64>(13).unwrap_or(0) as u64,
        output_tokens: row.get::<_, i64>(14).unwrap_or(0) as u64,
        first_token_ms: row
            .get::<_, Option<i64>>(15)
            .ok()
            .flatten()
            .map(|v| v as u64),
    })
}

/// Best-effort insert used from the proxy hot path. Never panics.
pub fn try_insert(entry: NewRequestLog) {
    if let Err(e) = insert(entry) {
        eprintln!("[proxy-log] insert failed: {e}");
    }
}

pub fn insert(entry: NewRequestLog) -> Result<(), String> {
    with_db(|conn| {
        let id = Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().timestamp();
        let path = truncate(&entry.path, 400);
        let err = entry
            .error_message
            .as_ref()
            .map(|s| truncate(s, 600));
        let model = truncate(&entry.model, 120);
        let provider_name = truncate(&entry.provider_name, 120);
        conn.execute(
            "INSERT INTO request_logs (
                id, created_at, app, provider_id, provider_name, model,
                method, path, status_code, latency_ms, is_streaming, attempt,
                error_message, input_tokens, output_tokens, first_token_ms
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                id,
                created_at,
                entry.app,
                entry.provider_id,
                provider_name,
                model,
                entry.method,
                path,
                entry.status_code as i64,
                entry.latency_ms as i64,
                if entry.is_streaming { 1 } else { 0 },
                entry.attempt as i64,
                err,
                entry.input_tokens as i64,
                entry.output_tokens as i64,
                entry.first_token_ms.map(|v| v as i64),
            ],
        )
        .map_err(|e| format!("写入请求日志失败: {e}"))?;
        prune_with_conn(conn, read_retention_days(conn))?;
        Ok(())
    })
}

fn prune_with_conn(conn: &Connection, retention_days: u32) -> Result<u64, String> {
    let days = clamp_retention_days(retention_days);
    let cutoff = chrono::Utc::now().timestamp() - (i64::from(days) * 86_400);
    let mut deleted = conn
        .execute(
            "DELETE FROM request_logs WHERE created_at < ?1",
            params![cutoff],
        )
        .map_err(|e| format!("按天清理日志失败: {e}"))? as u64;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM request_logs", [], |r| r.get(0))
        .unwrap_or(0);
    if count > MAX_ROWS_HARD_CAP {
        let excess = count - MAX_ROWS_HARD_CAP;
        deleted += conn
            .execute(
                "DELETE FROM request_logs WHERE id IN (
                    SELECT id FROM request_logs ORDER BY created_at ASC LIMIT ?1
                 )",
                params![excess],
            )
            .map_err(|e| format!("按容量清理日志失败: {e}"))? as u64;
    }
    Ok(deleted)
}

pub fn get_retention_days() -> Result<u32, String> {
    with_db(|conn| Ok(read_retention_days(conn)))
}

pub fn set_retention_days(days: u32) -> Result<u32, String> {
    let days = clamp_retention_days(days);
    with_db(|conn| {
        conn.execute(
            "INSERT INTO log_meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![META_RETENTION_KEY, days.to_string()],
        )
        .map_err(|e| format!("保存保留天数失败: {e}"))?;
        let _ = prune_with_conn(conn, days)?;
        Ok(days)
    })
}

pub fn list(filters: RequestLogFilters) -> Result<RequestLogPage, String> {
    with_db(|conn| {
        let page_size = filters.page_size.clamp(1, 100);
        let page = filters.page;
        let offset = i64::from(page) * i64::from(page_size);

        let mut where_parts: Vec<String> = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(app) = filters
            .app
            .as_ref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty() && s != "all")
        {
            where_parts.push(format!("app = ?{}", values.len() + 1));
            values.push(Box::new(app));
        }
        if let Some(pid) = filters
            .provider_id
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            where_parts.push(format!("provider_id = ?{}", values.len() + 1));
            values.push(Box::new(pid));
        }
        match filters
            .status_class
            .as_ref()
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("2xx") => where_parts.push("status_code >= 200 AND status_code < 300".into()),
            Some("4xx") => where_parts.push("status_code >= 400 AND status_code < 500".into()),
            Some("5xx") => where_parts.push("status_code >= 500 AND status_code < 600".into()),
            Some("error") => {
                where_parts.push("(status_code = 0 OR status_code >= 400 OR error_message IS NOT NULL)"
                    .into())
            }
            _ => {}
        }
        if let Some(q) = filters
            .q
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            let like = format!("%{q}%");
            let n = values.len() + 1;
            where_parts.push(format!(
                "(provider_name LIKE ?{n} OR model LIKE ?{n} OR path LIKE ?{n} OR provider_id LIKE ?{n})"
            ));
            values.push(Box::new(like));
        }

        let where_sql = if where_parts.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_parts.join(" AND "))
        };

        let count_sql = format!("SELECT COUNT(*) FROM request_logs {where_sql}");
        let total: i64 = {
            let mut stmt = conn
                .prepare(&count_sql)
                .map_err(|e| format!("统计日志失败: {e}"))?;
            let params_ref: Vec<&dyn rusqlite::ToSql> =
                values.iter().map(|v| v.as_ref()).collect();
            stmt.query_row(params_ref.as_slice(), |r| r.get(0))
                .map_err(|e| format!("统计日志失败: {e}"))?
        };

        let list_sql = format!(
            "SELECT id, created_at, app, provider_id, provider_name, model,
                    method, path, status_code, latency_ms, is_streaming, attempt,
                    error_message, input_tokens, output_tokens, first_token_ms
             FROM request_logs {where_sql}
             ORDER BY created_at DESC
             LIMIT ?{} OFFSET ?{}",
            values.len() + 1,
            values.len() + 2
        );
        values.push(Box::new(i64::from(page_size)));
        values.push(Box::new(offset));

        let mut stmt = conn
            .prepare(&list_sql)
            .map_err(|e| format!("查询日志失败: {e}"))?;
        let params_ref: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), row_to_entry)
            .map_err(|e| format!("查询日志失败: {e}"))?;
        let mut data = Vec::new();
        for row in rows {
            data.push(row.map_err(|e| format!("读取日志行失败: {e}"))?);
        }

        let logging_enabled = crate::providers::store::load()
            .map(|f| f.proxy.enable_logging)
            .unwrap_or(true);

        Ok(RequestLogPage {
            data,
            total: total as u64,
            page,
            page_size,
            retention_days: read_retention_days(conn),
            logging_enabled,
        })
    })
}

pub fn get(id: &str) -> Result<Option<RequestLogEntry>, String> {
    with_db(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, app, provider_id, provider_name, model,
                        method, path, status_code, latency_ms, is_streaming, attempt,
                        error_message, input_tokens, output_tokens, first_token_ms
                 FROM request_logs WHERE id = ?1",
            )
            .map_err(|e| format!("查询日志详情失败: {e}"))?;
        let row = stmt
            .query_row(params![id], row_to_entry)
            .optional()
            .map_err(|e| format!("查询日志详情失败: {e}"))?;
        Ok(row)
    })
}

pub fn clear_all() -> Result<u64, String> {
    with_db(|conn| {
        let n = conn
            .execute("DELETE FROM request_logs", [])
            .map_err(|e| format!("清空日志失败: {e}"))?;
        Ok(n as u64)
    })
}

/// Extract OpenAI-compatible `model` from a JSON request body (best effort).
pub fn extract_model_from_body(body: &[u8]) -> String {
    if body.is_empty() {
        return String::new();
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return String::new();
    };
    v.get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

pub fn looks_streaming(path: &str, headers: &http::HeaderMap) -> bool {
    let p = path.to_ascii_lowercase();
    if p.contains("stream=true") || p.contains("/responses") || p.contains("chat/completions") {
        if let Some(accept) = headers
            .get(http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
        {
            if accept.contains("text/event-stream") {
                return true;
            }
        }
        // Chat completions / responses are often streamed by clients.
        if p.contains("stream=true") {
            return true;
        }
    }
    // Body stream flag is checked by caller when available; header-only heuristic:
    headers
        .get(http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|a| a.contains("text/event-stream"))
        .unwrap_or(false)
}

pub fn body_requests_stream(body: &[u8]) -> bool {
    if body.is_empty() {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("stream").and_then(|s| s.as_bool()))
        .unwrap_or(false)
}
