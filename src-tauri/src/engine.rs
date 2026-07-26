//! Skin engine bridge: prefer in-process Rust CDP for apply/status/restore/detect;
//! fall back to Node `engine/cli.mjs` only for import / export / design-wallpaper
//! (and when `CODEX_SKIN_FORCE_NODE=1`).
//!
//! Cold launch + restart are handled natively (ensure_debug_port) for speed and
//! so end-users do not need system Node on the main skin path.

use crate::cdp;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::RwLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("{0}")]
    Message(String),
    #[error("engine process failed to start: {0}")]
    Spawn(String),
    #[error("engine returned invalid JSON: {0}")]
    Json(String),
}

impl EngineError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

static PROJECT_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Force Node path even when native is available (`CODEX_SKIN_FORCE_NODE=1`).
fn force_node() -> bool {
    matches!(
        std::env::var("CODEX_SKIN_FORCE_NODE")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Disable native path (`CODEX_SKIN_NATIVE=0`).
fn native_enabled() -> bool {
    if force_node() {
        return false;
    }
    match std::env::var("CODEX_SKIN_NATIVE")
        .unwrap_or_else(|_| "1".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "0" | "false" | "no" | "off" => false,
        _ => true,
    }
}

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
        if normalized.join("engine").join("cli.mjs").is_file()
            || normalized.join("engine").join("manager.js").is_file()
            || normalized
                .join("engine")
                .join("runtime")
                .join("renderer-core.js")
                .is_file()
        {
            return Some(normalized);
        }
    }
    None
}

fn engine_cli_path() -> Result<PathBuf, EngineError> {
    let root = project_root();
    let p = root.join("engine").join("cli.mjs");
    if p.is_file() {
        return Ok(p);
    }
    Err(EngineError::msg(format!(
        "engine CLI not found at {} (root={})",
        p.display(),
        root.display()
    )))
}

fn find_node() -> Result<PathBuf, EngineError> {
    if let Ok(custom) = std::env::var("CODEX_SKIN_NODE") {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Ok(p);
        }
    }
    let which = if cfg!(windows) { "where" } else { "which" };
    let output = Command::new(which)
        .arg("node")
        .output()
        .map_err(|e| EngineError::Spawn(e.to_string()))?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let p = PathBuf::from(line);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    if cfg!(windows) {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
        for cand in [
            PathBuf::from(&local).join(r"Programs\node\node.exe"),
            PathBuf::from(&pf).join(r"nodejs\node.exe"),
            PathBuf::from(r"C:\Program Files\nodejs\node.exe"),
            PathBuf::from(r"C:\Program Files (x86)\nodejs\node.exe"),
        ] {
            if cand.is_file() {
                return Ok(cand);
            }
        }
    }
    Err(EngineError::msg(
        "未找到 Node.js。主路径换肤已不需要 Node；仅导入/导出/自定义皮肤或强制 Node 回退时需要。请安装 Node 18+ 或设置 CODEX_SKIN_NODE。",
    ))
}

/// Run `node engine/cli.mjs <args...>` and parse JSON stdout.
pub fn run_cli(args: &[&str]) -> Result<Value, EngineError> {
    let node = find_node()?;
    let cli = engine_cli_path()?;
    let root = project_root();

    let mut cmd = Command::new(&node);
    cmd.arg(&cli);
    cmd.args(args);
    cmd.current_dir(&root);
    cmd.env("CODEX_SKIN_ROOT", &root);
    cmd.env("CODEX_SKIN_STATE_NAME", "ChatGPTTools");
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    // Prefer app root node_modules, then engine-local (self-contained engine deps)
    let node_path = [
        root.join("node_modules").to_string_lossy().to_string(),
        root.join("engine")
            .join("node_modules")
            .to_string_lossy()
            .to_string(),
    ]
    .join(if cfg!(windows) { ";" } else { ":" });
    cmd.env("NODE_PATH", node_path);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd
        .output()
        .map_err(|e| EngineError::Spawn(format!("{} ({})", e, node.display())))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        if let Ok(v) = serde_json::from_str::<Value>(&stderr) {
            if let Some(msg) = v.get("error").and_then(|x| x.as_str()) {
                return Err(EngineError::msg(msg.to_string()));
            }
        }
        let msg = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("engine exit code {:?}", output.status.code())
        };
        return Err(EngineError::msg(msg));
    }

    if stdout.is_empty() {
        return Ok(serde_json::json!({ "ok": true }));
    }
    serde_json::from_str(&stdout).map_err(|e| {
        EngineError::Json(format!(
            "{e}; stdout={}",
            stdout.chars().take(400).collect::<String>()
        ))
    })
}

/// High-level dispatch: native Rust first for main path; Node for packages / force.
pub fn run_engine(args: &[&str]) -> Result<Value, EngineError> {
    if args.is_empty() {
        return Err(EngineError::msg("missing engine command"));
    }
    let cmd = args[0];

    if native_enabled() {
        match cmd {
            "version" => return Ok(cdp::engine_version_native()),
            "paths" => return Ok(cdp::engine_paths_native()),
            "detect" => {
                if let Ok(v) = cdp::detect_native() {
                    return Ok(v);
                }
            }
            "status" => {
                if let Ok(v) = cdp::get_status_native() {
                    return Ok(v);
                }
            }
            "host-status" | "host_status" => {
                let force = flag_bool(args, "force", false);
                if let Ok(v) = cdp::get_host_status_native(force) {
                    return Ok(v);
                }
            }
            "apply" => {
                if let Some(skin_id) = flag_value(args, "skin-id").or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                }) {
                    let restart = flag_bool(args, "restart", false);
                    match cdp::apply_skin_native_opts(&skin_id, restart) {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            eprintln!("[chatgpt-tools] native apply fallback to Node: {e}");
                        }
                    }
                }
            }
            "restore" => {
                let restore_theme = flag_bool(args, "restore-theme", true);
                match cdp::restore_skin_native(restore_theme) {
                    Ok(v) => return Ok(v),
                    Err(e) => {
                        eprintln!("[chatgpt-tools] native restore fallback to Node: {e}");
                    }
                }
            }
            "pause" => match cdp::pause_skin_native() {
                Ok(v) => return Ok(v),
                Err(e) => {
                    // Dream habit: do not silently claim pause success via empty Node fallback
                    // when native already wrote the pause flag and failed live remove.
                    eprintln!("[chatgpt-tools] native pause: {e}");
                    return Err(e);
                }
            },
            "resume" => {
                let restart = flag_bool(args, "restart", false);
                match cdp::resume_skin_native(restart) {
                    Ok(v) => return Ok(v),
                    Err(e) => {
                        eprintln!("[chatgpt-tools] native resume fallback to Node: {e}");
                    }
                }
            }
            "start-host" | "start_host" => match cdp::start_host_native() {
                Ok(v) => return Ok(v),
                Err(e) => {
                    eprintln!("[chatgpt-tools] native start-host: {e}");
                    return Err(e);
                }
            },
            "resolve-asset" => {
                if let (Some(skin_id), Some(kind)) =
                    (flag_value(args, "skin-id"), flag_value(args, "kind"))
                {
                    if let Ok(v) = cdp::resolve_asset_native(&skin_id, &kind) {
                        return Ok(v);
                    }
                }
            }
            "delete-skin" => {
                if let Some(skin_id) = flag_value(args, "skin-id").or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                }) {
                    match cdp::delete_skin_native(&skin_id) {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            eprintln!("[chatgpt-tools] native delete-skin: {e}");
                            return Err(e);
                        }
                    }
                }
            }
            "set-app-path" => {
                if let Some(path) = flag_value(args, "path").or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                }) {
                    return cdp::set_app_path_native(Some(&path));
                }
            }
            "clear-app-path" => {
                return cdp::set_app_path_native(None);
            }
            "export-skin" => {
                if let (Some(skin_id), Some(output)) = (
                    flag_value(args, "skin-id").or_else(|| {
                        args.get(1)
                            .filter(|a| !a.starts_with("--"))
                            .map(|s| s.to_string())
                    }),
                    flag_value(args, "output"),
                ) {
                    match cdp::export_skin_native(&skin_id, &output) {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            eprintln!("[chatgpt-tools] native export-skin fallback to Node: {e}");
                        }
                    }
                }
            }
            "import-skin" => {
                if let Some(path) = flag_value(args, "path").or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                }) {
                    let overwrite = flag_bool(args, "overwrite", true);
                    match cdp::import_skin_native(&path, overwrite) {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            eprintln!("[chatgpt-tools] native import-skin fallback to Node: {e}");
                        }
                    }
                }
            }
            "inspect-skin" => {
                if let Some(path) = flag_value(args, "path").or_else(|| {
                    args.get(1)
                        .filter(|a| !a.starts_with("--"))
                        .map(|s| s.to_string())
                }) {
                    match cdp::inspect_skin_native(&path) {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            eprintln!("[chatgpt-tools] native inspect-skin fallback to Node: {e}");
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Node fallback: design-wallpaper / native failure recovery
    run_cli(args)
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
