//! Launch / stop ChatGPT·Codex with remote debugging — parity with manager ensureDebugPort.

use super::host::{
    append_diag, find_host_main_pids, invalidate_host_lifecycle_sticky, invalidate_host_probe_cache,
    note_host_ready, probe_host_lifecycle, probe_host_lifecycle_force, resolve_timing_budget,
    state_root, wait_until_renderer_ready, HostLifecycle, TimingBudget,
};
use super::http::is_debug_port_open;
use crate::engine::EngineError;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn is_windows_store_path(p: &str) -> bool {
    p.replace('\\', "/")
        .to_ascii_lowercase()
        .contains("/windowsapps/")
}

fn expand_configured_path(configured: &str) -> Option<String> {
    let c = configured.trim();
    if c.is_empty() {
        return None;
    }
    let candidates = [
        c.to_string(),
        format!(r"{c}\ChatGPT.exe"),
        format!(r"{c}\Codex.exe"),
        format!(r"{c}\app\ChatGPT.exe"),
        format!(r"{c}\app\Codex.exe"),
    ];
    for cand in candidates {
        let path = Path::new(&cand);
        if path.is_file() {
            return Some(cand);
        }
        if is_windows_store_path(&cand) && cand.to_ascii_lowercase().ends_with(".exe") {
            return Some(cand);
        }
    }
    Some(c.to_string())
}

fn get_configured_app_path() -> Option<String> {
    if let Ok(from_env) = std::env::var("CODEX_APP_PATH") {
        let t = from_env.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let settings_path = state_root().join("settings.json");
    let text = fs::read_to_string(settings_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("appPath")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn path_looks_like_exe(p: &str) -> bool {
    let path = Path::new(p);
    if path.is_file() {
        return true;
    }
    is_windows_store_path(p) && p.to_ascii_lowercase().ends_with(".exe")
}

fn windows_exe_candidates() -> Vec<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let user = std::env::var("USERPROFILE").unwrap_or_default();
    [
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
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

fn resolve_exe_quick() -> Option<String> {
    if let Some(configured) = get_configured_app_path() {
        let candidates = [
            configured.clone(),
            format!(r"{configured}\ChatGPT.exe"),
            format!(r"{configured}\Codex.exe"),
            format!(r"{configured}\app\ChatGPT.exe"),
            format!(r"{configured}\app\Codex.exe"),
        ];
        for c in candidates {
            if path_looks_like_exe(&c) {
                return Some(c);
            }
        }
        if !configured.is_empty() {
            return Some(configured);
        }
    }
    if cfg!(windows) {
        for c in windows_exe_candidates() {
            if c.is_file() {
                return Some(c.to_string_lossy().to_string());
            }
        }
    } else if cfg!(target_os = "macos") {
        for c in macos_exe_candidates() {
            if Path::new(&c).is_file() {
                return Some(c);
            }
        }
    }
    None
}

/// Official + common install locations on macOS (ChatGPT / Codex desktop).
fn macos_exe_candidates() -> Vec<String> {
    let mut out = vec![
        "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT".into(),
        "/Applications/Codex.app/Contents/MacOS/Codex".into(),
        "/Applications/Codex.app/Contents/MacOS/ChatGPT".into(),
    ];
    if let Ok(home) = std::env::var("HOME") {
        out.push(format!(
            "{home}/Applications/ChatGPT.app/Contents/MacOS/ChatGPT"
        ));
        out.push(format!(
            "{home}/Applications/Codex.app/Contents/MacOS/Codex"
        ));
        out.push(format!(
            "{home}/Applications/Codex.app/Contents/MacOS/ChatGPT"
        ));
    }
    out
}

/// Bundle root for a Mac executable path
/// (`…/ChatGPT.app/Contents/MacOS/ChatGPT` → `…/ChatGPT.app`).
fn macos_bundle_root(exe: &str) -> Option<PathBuf> {
    let p = Path::new(exe);
    // …/App.app/Contents/MacOS/Binary
    p.parent()? // MacOS
        .parent()? // Contents
        .parent() // App.app
        .map(|b| b.to_path_buf())
}

/// Prefer launching via `open -n -a <App.app> --args --remote-debugging-port=…`
/// so LaunchServices does not drop Chromium flags as easily as bare exec.
fn launch_macos_app(exe: &str, port: u16) -> Result<u32, EngineError> {
    let arg = format!("--remote-debugging-port={port}");
    let addr = "--remote-debugging-address=127.0.0.1";
    if let Some(bundle) = macos_bundle_root(exe) {
        if bundle.is_dir() {
            append_diag(&format!(
                "launch_macos_app open -n -a {}",
                bundle.display()
            ));
            let mut cmd = Command::new("open");
            cmd.args([
                "-n",
                "-a",
                &bundle.to_string_lossy(),
                "--args",
                &arg,
                addr,
            ]);
            cmd.stdin(std::process::Stdio::null());
            cmd.stdout(std::process::Stdio::null());
            cmd.stderr(std::process::Stdio::null());
            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    std::mem::forget(child);
                    // `open` returns quickly; host PID may differ — lifecycle probe owns readiness.
                    return Ok(if pid == 0 { 1 } else { pid });
                }
                Err(e) => {
                    append_diag(&format!("launch_macos_app open failed: {e}; fallback spawn"));
                }
            }
        }
    }
    spawn_with_debug_port(exe, port)
}

/// Event-driven wait: host PIDs gone OR timeout (no fixed 700ms sleep).
fn wait_host_gone(timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if find_host_main_pids().is_empty() {
            return true;
        }
        sleep_ms(40);
    }
    find_host_main_pids().is_empty()
}

/// Event-driven wait: debug port answers OR timeout.
fn wait_port_open(port: u16, timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if is_debug_port_open(port, 400) {
            return true;
        }
        sleep_ms(60);
    }
    is_debug_port_open(port, 400)
}

/// Stop ChatGPT / Codex host processes.
///
/// Windows: Toolhelp + TerminateProcess (OpenAI-path filtered). No taskkill / PowerShell.
/// Callers (apply / ensure_debug_port) must `stop_keep()` before this when possible.
/// Also disarms keep here so restore/ensure never kill with CDP still attached.
pub fn stop_host() {
    let t0 = Instant::now();
    append_diag("stop_host: begin");
    // Crash static: never leave keep / art evaluate attached while killing the host.
    super::keep::stop_keep(); // also bumps art generation
    invalidate_host_lifecycle_sticky();

    if cfg!(target_os = "macos") {
        let _ = Command::new("osascript")
            .args(["-e", r#"tell application "ChatGPT" to quit"#])
            .output();
        let _ = Command::new("osascript")
            .args(["-e", r#"tell application "Codex" to quit"#])
            .output();
        let _ = wait_host_gone(800);
        for pid in find_host_main_pids() {
            let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).output();
        }
        let _ = wait_host_gone(400);
        for pid in find_host_main_pids() {
            let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).output();
        }
        let _ = wait_host_gone(600);
        append_diag(&format!(
            "stop_host: done (macos) left={} t={}ms",
            find_host_main_pids().len(),
            t0.elapsed().as_millis()
        ));
        return;
    }

    #[cfg(windows)]
    {
        super::win_native::stop_host_native();
        let gone = wait_host_gone(1_200);
        if !gone {
            append_diag(&format!(
                "stop_host: some host PIDs still alive after Toolhelp terminate left={:?}",
                find_host_main_pids()
            ));
        }
        append_diag(&format!(
            "stop_host: done (win-native) left={} t={}ms",
            find_host_main_pids().len(),
            t0.elapsed().as_millis()
        ));
        return;
    }

    #[cfg(not(windows))]
    {
        append_diag(&format!(
            "stop_host: done (noop) t={}ms",
            t0.elapsed().as_millis()
        ));
    }
}

/// Resolve the best Store package for ChatGPT / Codex (native, no PowerShell).
fn resolve_windows_store_package() -> Option<WindowsStorePackage> {
    resolve_windows_store_package_detail().map(|(pkg, _)| pkg)
}

/// Same as `resolve_windows_store_package`, plus registered package count.
pub fn resolve_windows_store_package_detail() -> Option<(WindowsStorePackage, u32)> {
    #[cfg(windows)]
    {
        let (pkg, count) = super::win_native::resolve_store_package_native()?;
        return Some((
            WindowsStorePackage {
                aumid: pkg.aumid,
                package_full_name: pkg.package_full_name,
                package_family_name: pkg.package_family_name,
                version: pkg.version,
                install_location: pkg.install_location,
                executable: pkg.executable,
            },
            count,
        ));
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// JSON snapshot of Store package for status/detect (Windows only; null-ish elsewhere).
pub fn store_package_status_json() -> Value {
    if !cfg!(windows) {
        return json!({
            "available": false,
            "platform": "non-windows",
        });
    }
    match resolve_windows_store_package_detail() {
        Some((pkg, count)) => {
            let multi = count > 1;
            json!({
                "available": true,
                "aumid": pkg.aumid,
                "packageFullName": pkg.package_full_name,
                "packageFamilyName": pkg.package_family_name,
                "version": pkg.version,
                "installLocation": pkg.install_location,
                "executable": pkg.executable,
                "registeredCount": count,
                "multiPackage": multi,
                "warning": if multi {
                    json!("检测到多个 Store 包版本。若换肤异常，请在任务管理器结束全部 ChatGPT/Codex 后再试。")
                } else {
                    Value::Null
                },
            })
        }
        None => json!({
            "available": false,
            "registeredCount": 0,
            "multiPackage": false,
        }),
    }
}

#[derive(Debug, Clone)]
pub struct WindowsStorePackage {
    pub aumid: String,
    pub package_full_name: String,
    pub package_family_name: String,
    pub version: String,
    pub install_location: String,
    pub executable: String,
}

fn resolve_windows_store_aumid() -> Option<String> {
    resolve_windows_store_package().map(|p| p.aumid)
}

/// Whether a saved Store package identity still matches **any** registered package.
/// During Store auto-update the "current" package may change while the old package
/// still owns a healthy CDP session — accept any registered full/family name.
#[allow(dead_code)]
pub fn store_package_still_registered(full_name: &str, family_name: &str) -> bool {
    if full_name.is_empty() && family_name.is_empty() {
        return false;
    }
    #[cfg(windows)]
    {
        let packages = super::win_native::list_store_packages_native();
        for pkg in &packages {
            if !full_name.is_empty() && pkg.package_full_name == full_name {
                return true;
            }
            if !family_name.is_empty() && pkg.package_family_name == family_name {
                return true;
            }
        }
        // Fallback: best-package resolve (covers scan path when list is empty mid-API).
        if let Some((pkg, _)) = resolve_windows_store_package_detail() {
            if !full_name.is_empty() && pkg.package_full_name == full_name {
                return true;
            }
            if !family_name.is_empty() && pkg.package_family_name == family_name {
                return true;
            }
        }
        return false;
    }
    #[cfg(not(windows))]
    {
        let _ = (full_name, family_name);
        false
    }
}

fn launch_windows_store_app(port: u16, aumid_pref: Option<&str>) -> Result<u32, EngineError> {
    let aumid = aumid_pref
        .map(|s| s.to_string())
        .or_else(resolve_windows_store_aumid)
        .ok_or_else(|| EngineError::msg("未找到 Microsoft Store 版 ChatGPT/Codex AUMID"))?;
    // Pass both port + loopback address so production runtimes that honor flags stay local.
    let args = format!(
        "--remote-debugging-port={port} --remote-debugging-address=127.0.0.1"
    );
    #[cfg(windows)]
    {
        return super::win_native::activate_packaged_app_blocking(&aumid, &args);
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        Err(EngineError::msg("Store activation is Windows-only"))
    }
}

fn spawn_with_debug_port(exe: &str, port: u16) -> Result<u32, EngineError> {
    let arg = format!("--remote-debugging-port={port}");
    // Loopback-only when the host supports the Chromium flag (macOS Codex / ChatGPT).
    let addr = "--remote-debugging-address=127.0.0.1";
    append_diag(&format!("spawn_with_debug_port exe={exe}"));
    let mut cmd = Command::new(exe);
    cmd.arg(&arg);
    cmd.arg(addr);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Detached, show window (Electron app), no console inheritance issues
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            // Intentionally leak/detach: do not wait on ChatGPT.
            std::mem::forget(child);
            Ok(pid)
        }
        Err(e) => {
            if cfg!(windows) {
                // Shell fallback for paths with spaces
                let mut shell = Command::new("cmd");
                shell.args(["/C", "start", "", exe, &arg, addr]);
                shell.stdin(std::process::Stdio::null());
                shell.stdout(std::process::Stdio::null());
                shell.stderr(std::process::Stdio::null());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    shell.creation_flags(CREATE_NO_WINDOW);
                }
                shell
                    .spawn()
                    .map(|c| {
                        let pid = c.id();
                        std::mem::forget(c);
                        pid
                    })
                    .map_err(|e2| {
                        EngineError::msg(format!("无法启动 {exe}: {e}; shell fallback: {e2}"))
                    })
            } else {
                Err(EngineError::msg(format!("无法启动 {exe}: {e}")))
            }
        }
    }
}

/// After Store package activation, detect owl/protocol redirect of CDP flags and
/// try a constrained direct launch of the package's validated `app\ChatGPT.exe`.
/// Does not copy or patch official binaries — only re-spawns the same install tree.
#[cfg(windows)]
fn maybe_recover_store_cdp_redirect(
    port: u16,
    pkg: &WindowsStorePackage,
    activated_pid: u32,
) -> Result<u32, EngineError> {
    use super::win_native::{
        classify_cdp_argument_status, inspect_host_cdp_arg_status, read_process_command_line,
        CdpArgStatus,
    };

    // Brief settle for process start + argument materialization.
    let deadline = Instant::now() + Duration::from_millis(2_500);
    let mut status = CdpArgStatus::Uninspectable;
    while Instant::now() < deadline {
        if is_debug_port_open(port, 250) {
            append_diag("store cdp: port open after package activation (no redirect recovery)");
            return Ok(activated_pid);
        }
        status = inspect_host_cdp_arg_status(port);
        if status == CdpArgStatus::ProtocolRedirected || status == CdpArgStatus::Forwarded {
            break;
        }
        // Also inspect the PID returned by ActivateApplication when available.
        if activated_pid > 0 {
            let cmd = read_process_command_line(activated_pid);
            let s = classify_cdp_argument_status(&cmd, port);
            if s == CdpArgStatus::ProtocolRedirected || s == CdpArgStatus::Forwarded {
                status = s;
                break;
            }
        }
        sleep_ms(120);
    }

    if is_debug_port_open(port, 400) {
        return Ok(activated_pid);
    }

    if status != CdpArgStatus::ProtocolRedirected {
        append_diag(&format!(
            "store cdp: no protocol-redirect detected status={status:?} pid={activated_pid}"
        ));
        return Ok(activated_pid);
    }

    let exe = pkg.executable.trim();
    if exe.is_empty() || !Path::new(exe).is_file() {
        return Err(EngineError::msg(format!(
            "Codex 将调试参数改写成了 codex:// 协议路径，且无法定位已验证的 Store 可执行文件（package={}）。请更新客户端或改用非 Store 安装。",
            pkg.package_full_name
        )));
    }
    // Only allow direct launch under the same registered install tree.
    let exe_l = exe.replace('/', "\\").to_ascii_lowercase();
    let root_l = pkg
        .install_location
        .replace('/', "\\")
        .to_ascii_lowercase();
    if !root_l.is_empty() && !exe_l.starts_with(&root_l) {
        return Err(EngineError::msg(
            "Store 直启回退拒绝：可执行文件不在已注册包安装目录内",
        ));
    }
    if !exe_l.contains("\\windowsapps\\")
        && !exe_l.contains("openai.codex")
        && !exe_l.contains("openai.chatgpt")
    {
        return Err(EngineError::msg(
            "Store 直启回退拒绝：路径未通过 OpenAI Store 宿主校验",
        ));
    }

    append_diag(&format!(
        "store cdp: protocol-redirected → direct spawn exe={} full={}",
        exe, pkg.package_full_name
    ));

    // Close the package-activated session that swallowed CDP, then spawn with raw flags.
    stop_host();
    let _ = wait_host_gone(2_000);
    invalidate_host_probe_cache();

    match spawn_with_debug_port(exe, port) {
        Ok(pid) => {
            // Confirm the port actually opens; Access Denied / still-redirected → error.
            if wait_port_open(port, 8_000) {
                append_diag(&format!(
                    "store cdp: direct spawn recovered port={port} pid={pid}"
                ));
                write_last_store_package(pkg);
                return Ok(pid);
            }
            let after = inspect_host_cdp_arg_status(port);
            Err(EngineError::msg(format!(
                "Codex Store 运行时将 --remote-debugging-port 转成了协议路径；直启已验证可执行文件后调试口仍未开放（status={after:?}）。此环境可能因 ACL 限制无法暴露 CDP，换肤无法继续。"
            )))
        }
        Err(e) => Err(EngineError::msg(format!(
            "Codex 吞掉了 CDP 参数，直启 Store 包内可执行文件失败: {e}"
        ))),
    }
}

#[cfg(not(windows))]
fn maybe_recover_store_cdp_redirect(
    _port: u16,
    _pkg: &WindowsStorePackage,
    activated_pid: u32,
) -> Result<u32, EngineError> {
    Ok(activated_pid)
}

/// Launch ChatGPT/Codex with remote debugging port.
///
/// Platform notes (no system Node required on this path):
/// - Windows Store: AUMID activation + re-resolve package every launch (survives updates)
/// - macOS: `open -n -a App.app --args` with loopback debug flags
/// - Classic install: direct spawn with `--remote-debugging-port`
pub fn launch_host(port: u16) -> Result<u32, EngineError> {
    // Configured non-Store path first
    if let Some(configured) = get_configured_app_path() {
        if let Some(fixed) = expand_configured_path(&configured) {
            if !is_windows_store_path(&fixed) && Path::new(&fixed).is_file() {
                if cfg!(target_os = "macos") {
                    return launch_macos_app(&fixed, port);
                }
                return spawn_with_debug_port(&fixed, port);
            }
        }
    }

    if cfg!(windows) {
        if let Some((pkg, count)) = resolve_windows_store_package_detail() {
            if count > 1 {
                append_diag(&format!(
                    "store multi-package registeredCount={count} using full={} (prefer running package when present)",
                    pkg.package_full_name
                ));
            }
            match launch_windows_store_app(port, Some(&pkg.aumid)) {
                Ok(pid) => {
                    write_last_store_package(&pkg);
                    if count > 1 {
                        append_diag(
                            "store: multiple package versions registered; identity prefers running package",
                        );
                    }
                    // Owl runtime may swallow CDP flags into codex:// — recover when proven.
                    return maybe_recover_store_cdp_redirect(port, &pkg, pid);
                }
                Err(e) => {
                    if count > 1 {
                        return Err(EngineError::msg(format!(
                            "Store 应用激活失败，且检测到 {count} 个已注册包版本。请打开任务管理器结束全部 ChatGPT/Codex 进程后重试。详情: {e}"
                        )));
                    }
                    append_diag(&format!("store launch soft-fail: {e}"));
                }
            }
        }
    }

    let exe = resolve_exe_quick().ok_or_else(|| {
        EngineError::msg(
            "未找到 Codex / ChatGPT 桌面版。可在界面点「指定客户端」选择 ChatGPT.exe，或设置 CODEX_APP_PATH。",
        )
    })?;

    if cfg!(windows) && is_windows_store_path(&exe) {
        let detail = resolve_windows_store_package_detail();
        let aumid = detail.as_ref().map(|(p, _)| p.aumid.as_str());
        if let Some((p, count)) = detail.as_ref() {
            write_last_store_package(p);
            if *count > 1 {
                append_diag(&format!("store multi-package count={count} on classic store path"));
            }
        }
        match launch_windows_store_app(port, aumid) {
            Ok(pid) => {
                if let Some((p, _)) = detail.as_ref() {
                    return maybe_recover_store_cdp_redirect(port, p, pid);
                }
                return Ok(pid);
            }
            Err(e) => {
                if detail.as_ref().map(|(_, c)| *c > 1).unwrap_or(false) {
                    return Err(EngineError::msg(format!(
                        "Store 激活失败（多版本包并存）。请结束全部 ChatGPT/Codex 后再试: {e}"
                    )));
                }
                return Err(e);
            }
        }
    }

    if cfg!(target_os = "macos") {
        return launch_macos_app(&exe, port);
    }

    spawn_with_debug_port(&exe, port)
}

fn write_last_store_package(pkg: &WindowsStorePackage) {
    let path = state_root().join("last-store-package.json");
    let body = serde_json::json!({
        "aumid": pkg.aumid,
        "packageFullName": pkg.package_full_name,
        "packageFamilyName": pkg.package_family_name,
        "version": pkg.version,
        "installLocation": pkg.install_location,
        "executable": pkg.executable,
        "resolvedAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    if let Ok(text) = serde_json::to_string_pretty(&body) {
        let _ = fs::create_dir_all(state_root());
        let _ = fs::write(path, format!("{text}\n"));
    }
}

/// Read last resolved Store package (if any).
pub fn read_last_store_package() -> Option<serde_json::Value> {
    let path = state_root().join("last-store-package.json");
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Fire-and-forget: stop + launch only (no wait for renderer).
/// Callers that need inject must wait in a background job.
/// Hard relaunch is limited to **one** extra stop+launch (never chain kills).
pub fn restart_host_fire_and_forget(port: u16) -> Result<u32, EngineError> {
    append_diag(&format!(
        "restart_host_fire_and_forget port={port}"
    ));
    stop_host();
    invalidate_host_probe_cache();
    // Event: PIDs gone (cap ~1.2s already inside stop); brief port-clear check.
    let _ = wait_port_open(port, 50); // if still open, process may be dying — continue
    let pid = launch_host(port)?;
    append_diag(&format!(
        "restart_host_fire_and_forget launched pid={pid}"
    ));
    Ok(pid)
}

/// Ensure CDP is up with remote debugging.
///
/// - `restart=true`: stop + relaunch (event-driven settle); at most **one** hard relaunch
/// - already ready + restart=false: return immediately
/// - soft-miss callers should NOT call this with relaunch; wait renderer first
/// - first-paint miss uses a wide retry window before declaring failure (slow machines)
pub fn ensure_debug_port(port: u16, restart: bool) -> Result<(), EngineError> {
    // Always disarm keep before any stop path inside.
    if restart {
        super::keep::stop_keep(); // bumps art gen + disarms keep
        invalidate_host_lifecycle_sticky();
    } else {
        invalidate_host_probe_cache();
    }

    let mut probe = probe_host_lifecycle_force(port);
    let budget = resolve_timing_budget(Some(&probe));
    // Production-grade: slow first paint must not fail the whole apply path.
    // Cap is higher than a single wait_renderer so ensure can absorb Store cold starts.
    let verify_window_ms = budget
        .wait_renderer_ms
        .saturating_add(budget.wait_debug_port_ms)
        .max(90_000)
        .min(120_000);
    append_diag(&format!(
        "ensure_debug_port begin port={port} lifecycle={} process={} portOpen={} renderer={} scale={} restart={restart} verifyWindowMs={verify_window_ms}",
        probe.lifecycle,
        probe.process_running,
        probe.debug_port_open,
        probe.renderer_ready,
        budget.scale
    ));

    let mut hard_relaunch_used = false;
    let overall_deadline = Instant::now() + Duration::from_millis(verify_window_ms);

    if restart {
        append_diag(&format!(
            "ensure_debug_port: forced restart wasReady={}",
            probe.renderer_ready
        ));
        let _ = restart_host_fire_and_forget(port)?;
        // Event-driven: port open then renderer (no fixed 900ms sleep).
        if !wait_port_open(port, budget.wait_debug_port_ms) {
            append_diag("ensure_debug_port: port not open after restart launch");
        }
        if wait_until_renderer_ready(port, budget.wait_renderer_ms, budget.poll_ms) {
            note_host_ready(port);
            return Ok(());
        }
        // One hard relaunch only
        hard_relaunch_used = true;
        append_diag("ensure_debug_port: one hard relaunch after forced restart");
        let _ = restart_host_fire_and_forget(port)?;
        let _ = wait_port_open(port, budget.wait_debug_port_ms);
        // Wide window after relaunch — do not fail on first early-boot miss.
        let remain = overall_deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        if wait_until_renderer_ready(port, remain.max(budget.wait_renderer_ms), budget.poll_ms) {
            note_host_ready(port);
            return Ok(());
        }
    } else {
        if probe.renderer_ready || (probe.lifecycle == "ready" && probe.can_hot_apply) {
            note_host_ready(port);
            return Ok(());
        }

        if probe.debug_port_open && !probe.renderer_ready {
            append_diag("ensure_debug_port: port open, waiting for app:// renderer (wide window)");
            let remain = overall_deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64;
            if wait_until_renderer_ready(
                port,
                remain.max(budget.wait_renderer_ms),
                budget.poll_ms,
            ) {
                note_host_ready(port);
                return Ok(());
            }
            probe = probe_host_lifecycle_force(port);
        }

        let running = probe.codex_running() || !find_host_main_pids().is_empty();
        if running && !probe.debug_port_open {
            append_diag(
                "ensure_debug_port: host running without debug port → stop+relaunch",
            );
            let _ = restart_host_fire_and_forget(port)?;
            hard_relaunch_used = true;
        } else if running && probe.debug_port_open && !probe.renderer_ready {
            // Port open but renderer late: prefer waiting out the verify window
            // before killing a healthy debug session (slow first paint).
            let remain = overall_deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64;
            if remain > 3_000 {
                append_diag(&format!(
                    "ensure_debug_port: debug port open, extending wait remainMs={remain} before relaunch"
                ));
                if wait_until_renderer_ready(port, remain, budget.poll_ms) {
                    note_host_ready(port);
                    return Ok(());
                }
            }
            append_diag("ensure_debug_port: running but not ready after wide wait; relaunch");
            let _ = restart_host_fire_and_forget(port)?;
            hard_relaunch_used = true;
        } else if !running {
            let _ = launch_host(port)?;
        } else if probe.renderer_ready {
            note_host_ready(port);
            return Ok(());
        }

        let _ = wait_port_open(port, budget.wait_debug_port_ms);
        let remain = overall_deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        if wait_until_renderer_ready(
            port,
            remain.max(budget.wait_renderer_ms.min(45_000)),
            budget.poll_ms,
        ) {
            note_host_ready(port);
            return Ok(());
        }

        if !hard_relaunch_used {
            append_diag("ensure_debug_port: one hard relaunch (cold path)");
            let _ = restart_host_fire_and_forget(port)?;
            let _ = wait_port_open(port, budget.wait_debug_port_ms);
            let remain = overall_deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64;
            if wait_until_renderer_ready(
                port,
                remain.max(15_000),
                budget.poll_ms,
            ) {
                note_host_ready(port);
                return Ok(());
            }
        }
    }

    let last = probe_host_lifecycle_force(port);
    append_diag(&format!(
        "ensure_debug_port: failed lifecycle={} process={} portOpen={} hardUsed={hard_relaunch_used}",
        last.lifecycle, last.process_running, last.debug_port_open
    ));
    let diag = state_root().join("diag.log");
    Err(EngineError::msg(format!(
        "未能就绪调试端口 {port}（当前状态: {}）。慢速电脑请多等片刻后重试；或勾选自动重启并完全退出 ChatGPT 后再试。日志: {}",
        last.lifecycle,
        diag.display()
    )))
}

/// Soft budget for inject retries after ensure.
/// Prefer a known lifecycle seed to avoid an extra force probe on the hot path.
#[allow(dead_code)]
pub fn inject_budget(port: u16) -> TimingBudget {
    inject_budget_from(port, None)
}

/// When `seed` is Some (e.g. post-ensure probe), skip another force CDP/process scan.
pub fn inject_budget_from(port: u16, seed: Option<&HostLifecycle>) -> TimingBudget {
    if let Some(s) = seed {
        return resolve_timing_budget(Some(s));
    }
    // Cached probe first; only force when we have no useful snapshot.
    let cached = probe_host_lifecycle(port);
    if cached.lifecycle == "ready" || cached.can_hot_apply {
        return resolve_timing_budget(Some(&cached));
    }
    let probe = probe_host_lifecycle_force(port);
    resolve_timing_budget(Some(&probe))
}

#[allow(dead_code)]
pub fn exe_looks_valid(p: &PathBuf) -> bool {
    p.is_file() || is_windows_store_path(&p.to_string_lossy())
}
