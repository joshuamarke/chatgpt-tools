//! Local environment probes for the Overview page.
//!
//! Detects ChatGPT / Codex **desktop** installs (skin-capable), Codex **CLI**
//! (skins are not supported), and Grok Build CLI / home layout.
//! Lightweight install / version detection for the Overview page.

use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Soft cache so Overview refresh does not re-spawn CLIs every click.
static ENV_CACHE: parking_lot::Mutex<Option<(Instant, Value)>> = parking_lot::Mutex::new(None);
const ENV_CACHE_TTL: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInstallInfo {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    /// desktop | cli | app
    pub kind: String,
    /// Whether ChatGPT Tools skins can target this install.
    pub skin_supported: bool,
    pub note: Option<String>,
    pub source: Option<String>,
    pub error: Option<String>,
}

fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn platform_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

fn extract_version_token(raw: &str) -> String {
    let t = raw.trim();
    // Prefer first semver-like token (e.g. "codex-cli 0.1.2" / "grok 1.2.3")
    for part in t.split_whitespace() {
        let cleaned = part.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-');
        if cleaned.chars().any(|c| c.is_ascii_digit()) && cleaned.contains('.') {
            return cleaned.trim_start_matches('v').to_string();
        }
    }
    // Fallback: first non-empty line, shortened
    t.lines()
        .next()
        .unwrap_or(t)
        .trim()
        .chars()
        .take(80)
        .collect()
}

fn run_version_cmd(bin: &Path) -> Result<String, String> {
    #[cfg(windows)]
    {
        let lower = bin
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let output = if lower == "cmd" || lower == "bat" {
            let path = bin.to_string_lossy();
            let quoted = format!("\"{}\"", path.replace('"', ""));
            let command = format!("call {quoted} --version");
            Command::new("cmd")
                .args(["/D", "/S", "/C"])
                .raw_arg(&command)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        } else {
            Command::new(bin)
                .arg("--version")
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        };
        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let raw = if stdout.trim().is_empty() {
                    stderr.trim()
                } else {
                    stdout.trim()
                };
                if raw.is_empty() {
                    Err("empty --version output".into())
                } else {
                    Ok(extract_version_token(raw))
                }
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                let out_s = String::from_utf8_lossy(&out.stdout);
                let detail = if err.trim().is_empty() {
                    out_s.trim()
                } else {
                    err.trim()
                };
                Err(if detail.is_empty() {
                    format!("exit {}", out.status)
                } else {
                    detail.chars().take(200).collect()
                })
            }
            Err(e) => Err(e.to_string()),
        }
    }

    #[cfg(not(windows))]
    {
        match Command::new(bin).arg("--version").output() {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let raw = if stdout.trim().is_empty() {
                    stderr.trim()
                } else {
                    stdout.trim()
                };
                if raw.is_empty() {
                    Err("empty --version output".into())
                } else {
                    Ok(extract_version_token(raw))
                }
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                let out_s = String::from_utf8_lossy(&out.stdout);
                let detail = if err.trim().is_empty() {
                    out_s.trim()
                } else {
                    err.trim()
                };
                Err(if detail.is_empty() {
                    format!("exit {}", out.status)
                } else {
                    detail.chars().take(200).collect()
                })
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Candidate dirs for CLI tools (codex / grok), PATH first then common installs.
fn cli_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push = |p: PathBuf| {
        if p.as_os_str().is_empty() {
            return;
        }
        if !dirs.iter().any(|d| d == &p) {
            dirs.push(p);
        }
    };

    if let Some(path) = std::env::var_os("PATH") {
        for part in std::env::split_paths(&path) {
            push(part);
        }
    }

    let home = user_home();
    push(home.join(".local").join("bin"));
    push(home.join(".npm-global").join("bin"));
    push(home.join(".volta").join("bin"));
    push(home.join("n").join("bin"));
    push(home.join(".grok").join("bin"));
    if let Ok(grok_bin) = std::env::var("GROK_BIN_DIR") {
        let t = grok_bin.trim();
        if !t.is_empty() {
            push(PathBuf::from(t));
        }
    }

    #[cfg(target_os = "macos")]
    {
        push(PathBuf::from("/opt/homebrew/bin"));
        push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(target_os = "linux")]
    {
        push(PathBuf::from("/usr/local/bin"));
        push(PathBuf::from("/usr/bin"));
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            push(PathBuf::from(appdata).join("npm"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            push(PathBuf::from(&local).join("Programs"));
            push(PathBuf::from(&local).join("pnpm"));
            push(PathBuf::from(local).join("Volta").join("bin"));
        }
    }

    dirs
}

fn cli_bin_candidates(tool: &str, dir: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            dir.join(format!("{tool}.cmd")),
            dir.join(format!("{tool}.exe")),
            dir.join(tool),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![dir.join(tool)]
    }
}

/// Locate a CLI binary and optionally run `--version`.
fn probe_cli(tool: &str) -> (bool, Option<String>, Option<String>, Option<String>, Option<String>) {
    // which / where first for path default
    if let Some(resolved) = resolve_on_path(tool) {
        match run_version_cmd(&resolved) {
            Ok(ver) => {
                return (
                    true,
                    Some(ver),
                    Some(resolved.to_string_lossy().to_string()),
                    Some("path".into()),
                    None,
                );
            }
            Err(e) => {
                return (
                    true,
                    None,
                    Some(resolved.to_string_lossy().to_string()),
                    Some("path".into()),
                    Some(e),
                );
            }
        }
    }

    for dir in cli_search_dirs() {
        for cand in cli_bin_candidates(tool, &dir) {
            if !cand.is_file() {
                continue;
            }
            match run_version_cmd(&cand) {
                Ok(ver) => {
                    return (
                        true,
                        Some(ver),
                        Some(cand.to_string_lossy().to_string()),
                        Some(infer_cli_source(&cand)),
                        None,
                    );
                }
                Err(e) => {
                    return (
                        true,
                        None,
                        Some(cand.to_string_lossy().to_string()),
                        Some(infer_cli_source(&cand)),
                        Some(e),
                    );
                }
            }
        }
    }
    (false, None, None, None, None)
}

fn infer_cli_source(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    if s.contains("/.grok/") {
        "grok-native".into()
    } else if s.contains("/homebrew/") || s.contains("/cellar/") || s.contains("/opt/homebrew/") {
        "homebrew".into()
    } else if s.contains("/.npm") || s.contains("/npm/") || s.ends_with("/npm") {
        "npm".into()
    } else if s.contains("/.volta/") || s.contains("/volta/") {
        "volta".into()
    } else if s.contains("/pnpm/") {
        "pnpm".into()
    } else {
        "system".into()
    }
}

fn resolve_on_path(tool: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let out = Command::new("cmd")
            .args(["/C", &format!("where {tool}")])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let raw = String::from_utf8_lossy(&out.stdout);
        let first = raw.lines().next()?.trim();
        if first.is_empty() {
            return None;
        }
        let p = PathBuf::from(first);
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".into());
        let out = Command::new(shell)
            .arg("-lc")
            .arg(format!("command -v {tool}"))
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let raw = String::from_utf8_lossy(&out.stdout);
        let line = raw
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with('/'))?;
        let p = PathBuf::from(line);
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    }
}

// ── Desktop ChatGPT / Codex ─────────────────────────────────────────────

fn desktop_exe_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
        let pf86 =
            std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
        let user = std::env::var("USERPROFILE").unwrap_or_default();
        for s in [
            format!(r"{local}\Programs\ChatGPT\ChatGPT.exe"),
            format!(r"{local}\Programs\Codex\Codex.exe"),
            format!(r"{local}\Programs\chatgpt\ChatGPT.exe"),
            format!(r"{local}\Programs\OpenAI\ChatGPT\ChatGPT.exe"),
            format!(r"{local}\Programs\OpenAI\Codex\Codex.exe"),
            format!(r"{local}\Microsoft\WindowsApps\ChatGPT.exe"),
            format!(r"{local}\Microsoft\WindowsApps\Codex.exe"),
            format!(r"{pf}\ChatGPT\ChatGPT.exe"),
            format!(r"{pf}\Codex\Codex.exe"),
            format!(r"{pf}\OpenAI\ChatGPT\ChatGPT.exe"),
            format!(r"{pf}\OpenAI\Codex\Codex.exe"),
            format!(r"{pf86}\ChatGPT\ChatGPT.exe"),
            format!(r"{pf86}\Codex\Codex.exe"),
            format!(r"{user}\AppData\Local\Programs\ChatGPT\ChatGPT.exe"),
        ] {
            out.push(PathBuf::from(s));
        }
    }
    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from(
            "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
        ));
        out.push(PathBuf::from("/Applications/Codex.app/Contents/MacOS/Codex"));
        out.push(PathBuf::from(
            "/Applications/Codex.app/Contents/MacOS/ChatGPT",
        ));
        let home = user_home();
        out.push(home.join("Applications/ChatGPT.app/Contents/MacOS/ChatGPT"));
        out.push(home.join("Applications/Codex.app/Contents/MacOS/Codex"));
        out.push(home.join("Applications/Codex.app/Contents/MacOS/ChatGPT"));
    }
    let _ = &out;
    out
}

#[cfg(target_os = "macos")]
fn macos_bundle_version(exe: &Path) -> Option<String> {
    // …/App.app/Contents/MacOS/Binary → …/App.app/Contents/Info.plist
    let plist = exe
        .parent()? // MacOS
        .parent()? // Contents
        .join("Info.plist");
    let text = std::fs::read_to_string(plist).ok()?;
    // Prefer CFBundleShortVersionString, then CFBundleVersion
    for key in ["CFBundleShortVersionString", "CFBundleVersion"] {
        if let Some(v) = plist_string_value(&text, key) {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn plist_string_value(plist: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let idx = plist.find(&needle)?;
    let rest = &plist[idx + needle.len()..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")? + start;
    Some(rest[start..end].trim().to_string())
}

fn version_from_windows_path(path: &str) -> Option<String> {
    // Store: ...\OpenAI.Codex_1.2.3.0_x64__pub\...
    let norm = path.replace('/', "\\");
    for part in norm.split('\\') {
        if part.starts_with("OpenAI.") {
            // Name_Version_Arch_...
            if let Some(ver) = part.split('_').nth(1) {
                if ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    return Some(ver.to_string());
                }
            }
        }
    }
    None
}

fn configured_app_path() -> Option<String> {
    if let Ok(from_env) = std::env::var("CODEX_APP_PATH") {
        let t = from_env.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let settings = crate::cdp::native_state_root().join("settings.json");
    let text = std::fs::read_to_string(settings).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("appPath")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn probe_desktop_host() -> ToolInstallInfo {
    // 1) Microsoft Store package (Windows) — richest version metadata
    #[cfg(windows)]
    {
        let store = crate::cdp::store_package_status_json();
        if store.get("available").and_then(|v| v.as_bool()) == Some(true) {
            let version = store
                .get("version")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let path = store
                .get("executable")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    store
                        .get("installLocation")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                })
                .map(|s| s.to_string());
            let family = store
                .get("packageFamilyName")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let name = if family.contains("Codex") {
                "ChatGPT / Codex 桌面端"
            } else {
                "ChatGPT 桌面端"
            };
            return ToolInstallInfo {
                id: "chatgpt-desktop".into(),
                name: name.into(),
                installed: true,
                version,
                path,
                kind: "desktop".into(),
                skin_supported: true,
                note: Some("可通过本工具切换皮肤".into()),
                source: Some("microsoft-store".into()),
                error: None,
            };
        }
    }

    // 2) Configured path (user chose client)
    if let Some(configured) = configured_app_path() {
        let candidates = [
            configured.clone(),
            format!(r"{configured}\ChatGPT.exe"),
            format!(r"{configured}\Codex.exe"),
            format!(r"{configured}\app\ChatGPT.exe"),
            format!(r"{configured}\app\Codex.exe"),
            format!("{configured}/Contents/MacOS/ChatGPT"),
            format!("{configured}/Contents/MacOS/Codex"),
        ];
        for c in candidates {
            let p = PathBuf::from(&c);
            if p.is_file() {
                let version = read_desktop_version(&p);
                return ToolInstallInfo {
                    id: "chatgpt-desktop".into(),
                    name: "ChatGPT / Codex 桌面端".into(),
                    installed: true,
                    version,
                    path: Some(p.to_string_lossy().to_string()),
                    kind: "desktop".into(),
                    skin_supported: true,
                    note: Some("使用「选择客户端」配置的路径".into()),
                    source: Some("configured".into()),
                    error: None,
                };
            }
        }
        // Configured but missing — still report
        return ToolInstallInfo {
            id: "chatgpt-desktop".into(),
            name: "ChatGPT / Codex 桌面端".into(),
            installed: false,
            version: None,
            path: Some(configured),
            kind: "desktop".into(),
            skin_supported: true,
            note: Some("已配置路径，但未找到可执行文件".into()),
            source: Some("configured".into()),
            error: Some("path not found".into()),
        };
    }

    // 3) Well-known install locations
    for cand in desktop_exe_candidates() {
        if cand.is_file() {
            let version = read_desktop_version(&cand);
            let source = if cand.to_string_lossy().contains("WindowsApps") {
                "microsoft-store"
            } else if cand.to_string_lossy().contains("Applications") {
                "applications"
            } else {
                "local"
            };
            return ToolInstallInfo {
                id: "chatgpt-desktop".into(),
                name: "ChatGPT / Codex 桌面端".into(),
                installed: true,
                version,
                path: Some(cand.to_string_lossy().to_string()),
                kind: "desktop".into(),
                skin_supported: true,
                note: Some("可通过本工具切换皮肤".into()),
                source: Some(source.into()),
                error: None,
            };
        }
    }

    ToolInstallInfo {
        id: "chatgpt-desktop".into(),
        name: "ChatGPT / Codex 桌面端".into(),
        installed: false,
        version: None,
        path: None,
        kind: "desktop".into(),
        skin_supported: true,
        note: Some(
            "未检测到桌面客户端。可前往官网下载，或在侧栏「选择客户端」手动指定".into(),
        ),
        source: None,
        error: None,
    }
}

fn read_desktop_version(exe: &Path) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(v) = macos_bundle_version(exe) {
            return Some(v);
        }
    }
    let s = exe.to_string_lossy();
    version_from_windows_path(&s)
}

/// Official install helpers exposed to the Overview UI.
pub const INSTALL_CMD_CODEX_CLI: &str = "npm i -g @openai/codex@latest";
pub const INSTALL_CMD_GROK_BUILD: &str = "npm i -g @xai-official/grok@latest";
pub const INSTALL_URL_CODEX_DESKTOP: &str = "https://openai.com/zh-Hans-CN/codex/";

fn install_meta_for(id: &str, installed: bool) -> Value {
    if installed {
        return json!(null);
    }
    match id {
        "chatgpt-desktop" => json!({
            "type": "url",
            "url": INSTALL_URL_CODEX_DESKTOP,
            "label": "前往下载安装",
            "hint": "打开 OpenAI Codex 桌面端下载页，自行安装后点「刷新检测」",
        }),
        "codex-cli" => json!({
            "type": "npm",
            "command": INSTALL_CMD_CODEX_CLI,
            "label": "安装 Codex CLI",
            "hint": "将在系统终端中执行 npm 全局安装",
        }),
        "grok-build" => json!({
            "type": "npm",
            "command": INSTALL_CMD_GROK_BUILD,
            "label": "安装 Grok Build",
            "hint": "将在系统终端中执行 npm 全局安装",
        }),
        _ => json!(null),
    }
}

fn probe_codex_cli() -> ToolInstallInfo {
    let (installed, version, path, source, error) = probe_cli("codex");
    ToolInstallInfo {
        id: "codex-cli".into(),
        name: "Codex CLI".into(),
        installed,
        version,
        path,
        kind: "cli".into(),
        skin_supported: false,
        note: Some(
            if installed {
                "命令行 Codex 不支持皮肤注入（仅桌面端可换肤）"
            } else {
                "未检测到 codex。可一键在系统终端执行 npm 全局安装"
            }
            .into(),
        ),
        source,
        error,
    }
}

fn probe_grok_build() -> ToolInstallInfo {
    let (installed, version, path, source, error) = probe_cli("grok");
    let home = crate::sessions::default_grok_home_dir();
    let home_exists = home.is_dir();
    let sessions_exist = home.join("sessions").is_dir();

    if installed {
        return ToolInstallInfo {
            id: "grok-build".into(),
            name: "Grok Build".into(),
            installed: true,
            version,
            path: path.or_else(|| Some(home.to_string_lossy().to_string())),
            kind: "cli".into(),
            skin_supported: false,
            note: Some(format!(
                "数据目录 {}{}",
                home.display(),
                if sessions_exist {
                    "（含 sessions）"
                } else {
                    ""
                }
            )),
            source,
            error,
        };
    }

    // No `grok` binary — still treat as installed if ~/.grok layout exists
    if home_exists {
        return ToolInstallInfo {
            id: "grok-build".into(),
            name: "Grok Build".into(),
            installed: true,
            version: None,
            path: Some(home.to_string_lossy().to_string()),
            kind: "app".into(),
            skin_supported: false,
            note: Some(
                if sessions_exist {
                    "检测到 ~/.grok 数据目录（CLI 未在 PATH）"
                } else {
                    "检测到 ~/.grok，但尚未发现 sessions"
                }
                .into(),
            ),
            source: Some("grok-home".into()),
            error: None,
        };
    }

    ToolInstallInfo {
        id: "grok-build".into(),
        name: "Grok Build".into(),
        installed: false,
        version: None,
        path: None,
        kind: "cli".into(),
        skin_supported: false,
        note: Some("未检测到 Grok Build。可一键在系统终端执行 npm 全局安装".into()),
        source: None,
        error: None,
    }
}

fn probe_runtime(tool: &str, name: &str, note_ok: &str, note_miss: &str) -> ToolInstallInfo {
    let (installed, version, path, source, error) = probe_cli(tool);
    ToolInstallInfo {
        id: tool.into(),
        name: name.into(),
        installed,
        version,
        path,
        kind: "runtime".into(),
        skin_supported: false,
        note: Some(if installed { note_ok } else { note_miss }.into()),
        source,
        error,
    }
}

fn probe_node_runtime() -> ToolInstallInfo {
    probe_runtime(
        "node",
        "Node.js",
        "CLI 工具运行时依赖",
        "未检测到 Node.js。安装 Codex CLI / Grok Build 前请先安装 Node.js（含 npm）",
    )
}

fn probe_npm_runtime() -> ToolInstallInfo {
    probe_runtime(
        "npm",
        "npm",
        "可用于全局安装 Codex CLI 与 Grok Build",
        "未检测到 npm。请先安装 Node.js 或确保 npm 已加入 PATH",
    )
}

/// Allow-list for Overview “install in terminal” actions.
fn is_allowed_install_command(cmd: &str) -> bool {
    matches!(
        cmd.trim(),
        INSTALL_CMD_CODEX_CLI
            | INSTALL_CMD_GROK_BUILD
            | "npm install -g @openai/codex@latest"
            | "npm install -g @xai-official/grok@latest"
    )
}

#[cfg(target_os = "macos")]
fn escape_for_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Open the platform default terminal and run `command` (visible, interactive).
fn spawn_system_terminal(command: &str) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("命令为空".into());
    }

    #[cfg(windows)]
    {
        // `start "" cmd /K …` opens a visible console; CREATE_NO_WINDOW only hides the launcher.
        let child = Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K", command])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("无法打开系统终端：{e}"))?;
        // Detach — do not wait for the user to close the install window.
        drop(child);
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
            escape_for_applescript(command)
        );
        Command::new("osascript")
            .args(["-e", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("无法打开 Terminal：{e}"))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let candidates: [(&str, Vec<String>); 5] = [
            (
                "gnome-terminal",
                vec!["--".into(), "bash".into(), "-lc".into(), format!("{command}; exec bash")],
            ),
            (
                "konsole",
                vec!["-e".into(), "bash".into(), "-lc".into(), format!("{command}; exec bash")],
            ),
            (
                "xfce4-terminal",
                vec![
                    "-e".into(),
                    format!("bash -lc '{}'; exec bash", command.replace('\'', "'\\''")),
                ],
            ),
            (
                "x-terminal-emulator",
                vec!["-e".into(), "bash".into(), "-lc".into(), format!("{command}; exec bash")],
            ),
            (
                "xterm",
                vec!["-hold".into(), "-e".into(), "bash".into(), "-lc".into(), command.into()],
            ),
        ];
        for (bin, args) in candidates {
            if resolve_on_path(bin).is_none() && !PathBuf::from(format!("/usr/bin/{bin}")).is_file()
            {
                continue;
            }
            if Command::new(bin)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok()
            {
                return Ok(());
            }
        }
        return Err("未找到可用的系统终端（gnome-terminal / konsole / xterm 等）".into());
    }

    #[cfg(not(any(windows, target_os = "macos", all(unix, not(target_os = "macos")))))]
    {
        let _ = command;
        Err("当前平台暂不支持拉起系统终端安装".into())
    }
}

fn app_self_info() -> Value {
    let version = std::env::var("CODEX_SKIN_APP_VERSION").unwrap_or_else(|_| "2.2.0".into());
    let root = crate::engine::project_root();
    let state = crate::cdp::native_state_root();
    json!({
        "name": "ChatGPT Tools",
        "version": version,
        "root": root.to_string_lossy(),
        "stateRoot": state.to_string_lossy(),
        "platform": platform_label(),
        "arch": std::env::consts::ARCH,
    })
}

fn tool_to_json(tool: &ToolInstallInfo) -> Value {
    let mut v = serde_json::to_value(tool).unwrap_or_else(|_| json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "install".into(),
            install_meta_for(&tool.id, tool.installed),
        );
    }
    v
}

/// Full environment snapshot for the Overview UI.
pub fn collect_environment(force: bool) -> Value {
    if !force {
        if let Some((at, cached)) = ENV_CACHE.lock().as_ref() {
            if at.elapsed() < ENV_CACHE_TTL {
                return cached.clone();
            }
        }
    }

    let desktop = probe_desktop_host();
    let codex_cli = probe_codex_cli();
    let grok = probe_grok_build();
    let node = probe_node_runtime();
    let npm = probe_npm_runtime();

    let tools = vec![desktop, codex_cli, grok];
    let node_installed = node.installed;
    let npm_installed = npm.installed;
    let runtimes = vec![node, npm];
    let installed_count = tools.iter().filter(|t| t.installed).count();
    let runtime_ready = node_installed && npm_installed;

    let body = json!({
        "ok": true,
        "checkedAt": chrono_now_iso(),
        "platform": platform_label(),
        "app": app_self_info(),
        "tools": tools.iter().map(tool_to_json).collect::<Vec<_>>(),
        "runtimes": runtimes.iter().map(|t| {
            let mut v = serde_json::to_value(t).unwrap_or_else(|_| json!({}));
            if let Some(obj) = v.as_object_mut() {
                obj.insert("install".into(), json!(null));
            }
            v
        }).collect::<Vec<_>>(),
        "summary": {
            "installedCount": installed_count,
            "toolCount": tools.len(),
            "skinCapable": tools.iter().any(|t| t.installed && t.skin_supported),
            "runtimeReady": runtime_ready,
            "npmInstalled": npm_installed,
            "nodeInstalled": node_installed,
        },
        "installCommands": {
            "codexCli": INSTALL_CMD_CODEX_CLI,
            "grokBuild": INSTALL_CMD_GROK_BUILD,
            "codexDesktopUrl": INSTALL_URL_CODEX_DESKTOP,
        },
    });

    *ENV_CACHE.lock() = Some((Instant::now(), body.clone()));
    body
}

fn chrono_now_iso() -> String {
    // Avoid hard dependency on chrono formatting features — simple UTC-ish stamp.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[tauri::command]
pub async fn env_check(force: Option<bool>) -> Result<Value, String> {
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || collect_environment(force))
        .await
        .map_err(|e| e.to_string())
}

/// Allowed ids for single-tool refresh (Overview per-card buttons).
fn probe_tool_by_id(id: &str) -> Result<ToolInstallInfo, String> {
    match id {
        "chatgpt-desktop" => Ok(probe_desktop_host()),
        "codex-cli" => Ok(probe_codex_cli()),
        "grok-build" => Ok(probe_grok_build()),
        "node" => Ok(probe_node_runtime()),
        "npm" => Ok(probe_npm_runtime()),
        _ => Err(format!(
            "未知环境 id：{id}。支持：chatgpt-desktop / codex-cli / grok-build / node / npm"
        )),
    }
}

fn is_runtime_id(id: &str) -> bool {
    matches!(id, "node" | "npm")
}

/// Recompute overview summary fields from tools + runtimes arrays in a snapshot.
fn recompute_env_summary(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    let tools = obj
        .get("tools")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let runtimes = obj
        .get("runtimes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let installed_count = tools
        .iter()
        .filter(|t| t.get("installed").and_then(|v| v.as_bool()) == Some(true))
        .count();
    let node_installed = runtimes.iter().any(|t| {
        t.get("id").and_then(|v| v.as_str()) == Some("node")
            && t.get("installed").and_then(|v| v.as_bool()) == Some(true)
    });
    let npm_installed = runtimes.iter().any(|t| {
        t.get("id").and_then(|v| v.as_str()) == Some("npm")
            && t.get("installed").and_then(|v| v.as_bool()) == Some(true)
    });
    let skin_capable = tools.iter().any(|t| {
        t.get("installed").and_then(|v| v.as_bool()) == Some(true)
            && t.get("skinSupported").and_then(|v| v.as_bool()) == Some(true)
    });
    obj.insert(
        "summary".into(),
        json!({
            "installedCount": installed_count,
            "toolCount": tools.len(),
            "skinCapable": skin_capable,
            "runtimeReady": node_installed && npm_installed,
            "npmInstalled": npm_installed,
            "nodeInstalled": node_installed,
        }),
    );
    obj.insert("checkedAt".into(), json!(chrono_now_iso()));
}

/// Merge a single tool/runtime probe into the soft ENV_CACHE (if any).
fn merge_tool_into_cache(tool: &ToolInstallInfo) {
    let mut guard = ENV_CACHE.lock();
    let Some((at, cached)) = guard.as_mut() else {
        return;
    };
    *at = Instant::now();
    let list_key = if is_runtime_id(&tool.id) {
        "runtimes"
    } else {
        "tools"
    };
    let entry = if is_runtime_id(&tool.id) {
        // Runtimes do not carry install CTAs.
        let mut v = serde_json::to_value(tool).unwrap_or_else(|_| json!({}));
        if let Some(o) = v.as_object_mut() {
            o.insert("install".into(), json!(null));
        }
        v
    } else {
        tool_to_json(tool)
    };
    if let Some(arr) = cached
        .as_object_mut()
        .and_then(|obj| obj.get_mut(list_key))
        .and_then(|v| v.as_array_mut())
    {
        if let Some(pos) = arr
            .iter()
            .position(|t| t.get("id").and_then(|x| x.as_str()) == Some(tool.id.as_str()))
        {
            arr[pos] = entry;
        } else {
            arr.push(entry);
        }
    }
    recompute_env_summary(cached);
}

/// Probe one environment entry (card-level refresh). Does not re-scan everything.
pub fn collect_single_tool(id: &str) -> Result<Value, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("缺少环境 id".into());
    }
    let tool = probe_tool_by_id(id)?;
    merge_tool_into_cache(&tool);
    let tool_json = if is_runtime_id(id) {
        let mut v = serde_json::to_value(&tool).unwrap_or_else(|_| json!({}));
        if let Some(o) = v.as_object_mut() {
            o.insert("install".into(), json!(null));
        }
        v
    } else {
        tool_to_json(&tool)
    };
    Ok(json!({
        "ok": true,
        "checkedAt": chrono_now_iso(),
        "id": id,
        "kind": if is_runtime_id(id) { "runtime" } else { "tool" },
        "tool": tool_json,
    }))
}

/// Single-tool env probe for Overview card refresh buttons.
#[tauri::command]
pub async fn env_check_tool(id: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || collect_single_tool(&id))
        .await
        .map_err(|e| e.to_string())?
}

/// Open system terminal and run an allow-listed npm install command.
/// Windows: `cmd /K …` via `start`. macOS: Terminal.app via osascript.
#[tauri::command]
pub async fn open_install_terminal(command: String) -> Result<Value, String> {
    let cmd = command.trim().to_string();
    if !is_allowed_install_command(&cmd) {
        return Err(format!(
            "不支持的安装命令。仅允许：{INSTALL_CMD_CODEX_CLI} 或 {INSTALL_CMD_GROK_BUILD}"
        ));
    }
    let platform = platform_label().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        spawn_system_terminal(&cmd)?;
        Ok(json!({
            "ok": true,
            "command": cmd,
            "platform": platform,
            "message": "已打开系统终端执行安装命令，完成后请回到本页点「刷新检测」",
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}
