//! Skin engine bridge — **single native path**.
//!
//! All GUI / Tauri commands run in-process via `src-tauri/src/cdp/*`.
//! Host-side assets still live under `engine/runtime/` (renderer-core, CSS),
//! but the app never spawns system Node or `engine/cli.mjs`.

use crate::cdp;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("{0}")]
    Message(String),
}

impl EngineError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

static PROJECT_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Called from `run` / setup so resource paths resolve in dev & production.
pub fn init_project_root(root: PathBuf) {
    if let Ok(mut guard) = PROJECT_ROOT.write() {
        *guard = Some(root);
    }
}

pub fn project_root() -> PathBuf {
    if let Ok(guard) = PROJECT_ROOT.read() {
        if let Some(p) = guard.as_ref() {
            return p.clone();
        }
    }
    discover_root().unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// True when `path` looks like an app resource root (runtime assets + skins).
pub fn is_app_root(path: &Path) -> bool {
    let runtime = path
        .join("engine")
        .join("runtime")
        .join("renderer-core.js");
    if runtime.is_file() {
        return true;
    }
    // Transition / partial layouts
    path.join("engine")
        .join("runtime")
        .join("immersive-skin.css")
        .is_file()
        || (path.join("skins").is_dir() && path.join("engine").join("runtime").is_dir())
}

fn discover_root() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env_root) = std::env::var("CODEX_SKIN_ROOT") {
        candidates.push(PathBuf::from(env_root));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.clone());
        candidates.push(cwd.join(".."));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.to_path_buf());
            candidates.push(dir.join(".."));
            candidates.push(dir.join("../.."));
            candidates.push(dir.join("resources"));
            candidates.push(dir.join("../resources"));
        }
    }
    for c in candidates {
        let normalized = c.canonicalize().unwrap_or(c);
        if is_app_root(&normalized) {
            return Some(normalized);
        }
    }
    None
}

/// High-level dispatch: in-process Rust CDP only (no Node fallback).
pub fn run_engine(args: &[&str]) -> Result<Value, EngineError> {
    if args.is_empty() {
        return Err(EngineError::msg("missing engine command"));
    }
    let cmd = args[0];

    match cmd {
        "version" => Ok(cdp::engine_version_native()),
        "paths" => Ok(cdp::engine_paths_native()),
        "detect" => cdp::detect_native(),
        "status" => cdp::get_status_native(),
        "host-status" | "host_status" => {
            let force = flag_bool(args, "force", false);
            cdp::get_host_status_native(force)
        }
        "apply" => {
            let skin_id = flag_value(args, "skin-id")
                .or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                })
                .ok_or_else(|| EngineError::msg("apply requires --skin-id"))?;
            let restart = flag_bool(args, "restart", false);
            cdp::apply_skin_native_opts(&skin_id, restart)
        }
        "restore" => {
            let restore_theme = flag_bool(args, "restore-theme", true);
            cdp::restore_skin_native(restore_theme)
        }
        "pause" => cdp::pause_skin_native(),
        "resume" => {
            let restart = flag_bool(args, "restart", false);
            cdp::resume_skin_native(restart)
        }
        "start-host" | "start_host" => cdp::start_host_native(),
        "restart-host" | "restart_host" => cdp::restart_host_native(),
        "resolve-asset" => {
            let skin_id = flag_value(args, "skin-id")
                .ok_or_else(|| EngineError::msg("resolve-asset requires --skin-id"))?;
            let kind = flag_value(args, "kind").unwrap_or_else(|| "art".into());
            cdp::resolve_asset_native(&skin_id, &kind)
        }
        "delete-skin" => {
            let skin_id = flag_value(args, "skin-id")
                .or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                })
                .ok_or_else(|| EngineError::msg("delete-skin requires --skin-id"))?;
            cdp::delete_skin_native(&skin_id)
        }
        "set-app-path" => {
            let path = flag_value(args, "path")
                .or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                })
                .ok_or_else(|| EngineError::msg("set-app-path requires --path"))?;
            cdp::set_app_path_native(Some(&path))
        }
        "clear-app-path" => cdp::set_app_path_native(None),
        "export-skin" => {
            let skin_id = flag_value(args, "skin-id")
                .or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                })
                .ok_or_else(|| EngineError::msg("export-skin requires --skin-id"))?;
            let output = flag_value(args, "output")
                .ok_or_else(|| EngineError::msg("export-skin requires --output"))?;
            cdp::export_skin_native(&skin_id, &output)
        }
        "import-skin" => {
            let path = flag_value(args, "path")
                .or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                })
                .ok_or_else(|| EngineError::msg("import-skin requires --path"))?;
            let overwrite = flag_bool(args, "overwrite", true);
            cdp::import_skin_native(&path, overwrite)
        }
        "inspect-skin" => {
            let path = flag_value(args, "path")
                .or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                })
                .ok_or_else(|| EngineError::msg("inspect-skin requires --path"))?;
            cdp::inspect_skin_native(&path)
        }
        "design-wallpaper" => {
            let raw = flag_value(args, "payload")
                .or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                })
                .ok_or_else(|| EngineError::msg("design-wallpaper requires --payload"))?;
            let payload = parse_design_payload(&raw)?;
            cdp::design_wallpaper_native(&payload)
        }
        other => Err(EngineError::msg(format!(
            "未知引擎命令: {other}（已单一路径化，仅支持进程内 Rust 引擎）"
        ))),
    }
}

fn parse_design_payload(raw: &str) -> Result<Value, EngineError> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed)
            .map_err(|e| EngineError::msg(format!("design-wallpaper payload JSON: {e}")));
    }
    let path = PathBuf::from(trimmed);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        EngineError::msg(format!(
            "design-wallpaper 无法读取 payload 文件 {}: {e}",
            path.display()
        ))
    })?;
    serde_json::from_str(&text)
        .map_err(|e| EngineError::msg(format!("design-wallpaper payload JSON: {e}")))
}

fn flag_value(args: &[&str], name: &str) -> Option<String> {
    let key = format!("--{name}");
    let mut i = 0;
    while i < args.len() {
        if args[i] == key {
            if let Some(v) = args.get(i + 1) {
                if !v.starts_with("--") {
                    return Some((*v).to_string());
                }
            }
            return None;
        }
        if args[i].starts_with(&format!("{key}=")) {
            return Some(args[i][key.len() + 1..].to_string());
        }
        i += 1;
    }
    None
}

fn flag_bool(args: &[&str], name: &str, default: bool) -> bool {
    match flag_value(args, name).as_deref() {
        None => default,
        Some(s) => match s.to_ascii_lowercase().as_str() {
            "0" | "false" | "no" | "off" => false,
            "1" | "true" | "yes" | "on" => true,
            _ => default,
        },
    }
}

pub fn read_file_bytes(path: &Path) -> Result<Vec<u8>, EngineError> {
    std::fs::read(path).map_err(|e| EngineError::msg(format!("read {}: {e}", path.display())))
}
