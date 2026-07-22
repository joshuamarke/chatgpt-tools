//! Launch / stop ChatGPT·Codex with remote debugging — parity with manager ensureDebugPort.

use super::host::{
    append_diag, find_host_main_pids, invalidate_host_probe_cache, note_host_ready,
    probe_host_lifecycle_force, resolve_timing_budget, run_powershell, state_root,
    wait_for_host_lifecycle, wait_until_renderer_ready, TimingBudget,
};
use crate::engine::EngineError;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

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
        for c in [
            "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
            "/Applications/Codex.app/Contents/MacOS/Codex",
            "/Applications/Codex.app/Contents/MacOS/ChatGPT",
        ] {
            if Path::new(c).is_file() {
                return Some(c.into());
            }
        }
    }
    None
}

/// Stop ChatGPT / Codex host processes.
///
/// Prefer PID list from `find_host_main_pids` (name-scoped). Image-wide
/// `taskkill /IM` is only a last resort after CIM path match, so we never
/// treat "command ran" as success without re-probing emptiness.
pub fn stop_host() {
    append_diag("stop_host: begin");
    if cfg!(target_os = "macos") {
        let _ = Command::new("osascript")
            .args(["-e", r#"tell application "ChatGPT" to quit"#])
            .output();
        let _ = Command::new("osascript")
            .args(["-e", r#"tell application "Codex" to quit"#])
            .output();
        sleep_ms(600);
        for pid in find_host_main_pids() {
            let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).output();
        }
        sleep_ms(350);
        for pid in find_host_main_pids() {
            let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).output();
        }
        return;
    }

    if cfg!(windows) {
        // 1) Graceful-ish: only PIDs we already identified as ChatGPT/Codex mains.
        let pids = find_host_main_pids();
        for pid in &pids {
            let mut cmd = Command::new("taskkill");
            cmd.args(["/PID", &pid.to_string(), "/T"]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let _ = cmd.output();
        }
        sleep_ms(400);
        // 2) Force only remaining known PIDs (re-probe — never kill a recycled PID blindly).
        for pid in find_host_main_pids() {
            let mut cmd = Command::new("taskkill");
            cmd.args(["/F", "/PID", &pid.to_string(), "/T"]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let _ = cmd.output();
        }
        // 3) CIM path-scoped force (OpenAI package paths only — not every ChatGPT-named binary).
        if !find_host_main_pids().is_empty() {
            let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
Get-CimInstance Win32_Process | Where-Object {
  $_.Name -match '^(ChatGPT|Codex)\.exe$' -and (
    -not $_.ExecutablePath -or
    $_.ExecutablePath -match 'OpenAI\.(Codex|ChatGPT)|\\Programs\\(ChatGPT|Codex)\\|\\WindowsApps\\'
  )
} | ForEach-Object {
  Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
}
"#;
            let _ = run_powershell(script, 15_000);
        }
        for _ in 0..40 {
            if find_host_main_pids().is_empty() {
                break;
            }
            sleep_ms(150);
        }
        sleep_ms(500);
        if !find_host_main_pids().is_empty() {
            append_diag("stop_host: some host PIDs still alive after stop attempts");
        }
    }
    append_diag("stop_host: done");
}

fn first_non_empty_line(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|s| s.to_string())
}

fn resolve_windows_store_aumid() -> Option<String> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$pkgs = @()
foreach ($n in @('OpenAI.Codex','OpenAI.ChatGPT','OpenAI.ChatGPT-Desktop')) {
  $pkgs += Get-AppxPackage -Name $n
}
$pkgs += Get-AppxPackage | Where-Object {
  $_.Name -match 'ChatGPT|Codex' -or $_.PackageFamilyName -match 'OpenAI'
}
$p = $pkgs | Sort-Object Version -Descending | Select-Object -First 1
if (-not $p) { return }
$manifest = Join-Path $p.InstallLocation 'AppxManifest.xml'
if (-not (Test-Path -LiteralPath $manifest)) {
  Write-Output ($p.PackageFamilyName + '!App')
  return
}
try {
  [xml]$x = Get-Content -LiteralPath $manifest
  $app = @($x.Package.Applications.Application) | Select-Object -First 1
  if ($app -and $app.Id) {
    Write-Output ($p.PackageFamilyName + '!' + $app.Id)
    return
  }
} catch {}
Write-Output ($p.PackageFamilyName + '!App')
"#;
    let stdout = run_powershell(script, 20_000)?;
    first_non_empty_line(&stdout)
}

fn launch_windows_store_app(port: u16, aumid_pref: Option<&str>) -> Result<u32, EngineError> {
    let aumid = aumid_pref
        .map(|s| s.to_string())
        .or_else(resolve_windows_store_aumid)
        .ok_or_else(|| EngineError::msg("未找到 Microsoft Store 版 ChatGPT/Codex AUMID"))?;
    let arg = format!("--remote-debugging-port={port}");
    // Escape for PowerShell single-quoted strings
    let aumid_esc = aumid.replace('\'', "''");
    let arg_esc = arg.replace('\'', "''");
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
if (-not ('ChatGPTToolsAppLauncher' -as [type])) {{
  $code = @'
using System;
using System.Runtime.InteropServices;
public class ChatGPTToolsAppLauncher {{
  [ComImport, Guid("2e941141-7f97-4756-ba1d-9decde894a3d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IApplicationActivationManager {{
    IntPtr ActivateApplication([In] String appUserModelId, [In] String arguments, [In] UInt32 options, [Out] out UInt32 processId);
  }}
  [ComImport, Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C")]
  class ApplicationActivationManager {{}}
  public static uint Launch(string aumid, string args) {{
    var mgr = new ApplicationActivationManager();
    var iam = (IApplicationActivationManager)mgr;
    uint pid;
    iam.ActivateApplication(aumid, args, 0, out pid);
    return pid;
  }}
}}
'@
  Add-Type -TypeDefinition $code
}}
$launchPid = [ChatGPTToolsAppLauncher]::Launch('{aumid_esc}', '{arg_esc}')
Write-Output $launchPid
"#
    );
    let stdout = run_powershell(&script, 30_000)
        .ok_or_else(|| EngineError::msg("Store 应用激活失败（PowerShell）"))?;
    let line = first_non_empty_line(&stdout).unwrap_or_default();
    let pid: u32 = line
        .parse()
        .map_err(|_| EngineError::msg(format!("Store 激活返回无效 PID: {line}")))?;
    if pid == 0 {
        return Err(EngineError::msg("Store 激活 PID 为 0"));
    }
    append_diag(&format!("launch_windows_store_app aumid={aumid} pid={pid}"));
    Ok(pid)
}

fn spawn_with_debug_port(exe: &str, port: u16) -> Result<u32, EngineError> {
    let arg = format!("--remote-debugging-port={port}");
    append_diag(&format!("spawn_with_debug_port exe={exe}"));
    let mut cmd = Command::new(exe);
    cmd.arg(&arg);
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
                shell.args(["/C", "start", "", exe, &arg]);
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

/// Launch ChatGPT/Codex with remote debugging port.
pub fn launch_host(port: u16) -> Result<u32, EngineError> {
    // Configured non-Store path first
    if let Some(configured) = get_configured_app_path() {
        if let Some(fixed) = expand_configured_path(&configured) {
            if !is_windows_store_path(&fixed) && Path::new(&fixed).is_file() {
                return spawn_with_debug_port(&fixed, port);
            }
        }
    }

    if cfg!(windows) {
        if let Some(aumid) = resolve_windows_store_aumid() {
            match launch_windows_store_app(port, Some(&aumid)) {
                Ok(pid) => return Ok(pid),
                Err(e) => append_diag(&format!("store launch soft-fail: {e}")),
            }
        }
    }

    let exe = resolve_exe_quick().ok_or_else(|| {
        EngineError::msg(
            "未找到 Codex / ChatGPT 桌面版。可在界面点「指定客户端」选择 ChatGPT.exe，或设置 CODEX_APP_PATH。",
        )
    })?;

    if cfg!(windows) && is_windows_store_path(&exe) {
        let pid = launch_windows_store_app(port, None)?;
        return Ok(pid);
    }

    spawn_with_debug_port(&exe, port)
}

/// Ensure CDP is up with remote debugging (parity with manager.ensureDebugPort).
///
/// - `restart=true`: always stop + relaunch (GUI auto-restart / desktopTheme)
/// - already ready + restart=false: return immediately
/// - port open, no app://: wait (slow cold start) before relaunch
/// - process without debug port: error unless we can relaunch (restart or auto)
pub fn ensure_debug_port(port: u16, restart: bool) -> Result<(), EngineError> {
    invalidate_host_probe_cache();
    let mut probe = probe_host_lifecycle_force(port);
    let budget = resolve_timing_budget(Some(&probe));
    append_diag(&format!(
        "ensure_debug_port begin port={port} lifecycle={} process={} portOpen={} renderer={} scale={} restart={restart}",
        probe.lifecycle,
        probe.process_running,
        probe.debug_port_open,
        probe.renderer_ready,
        budget.scale
    ));

    if restart {
        append_diag(&format!(
            "ensure_debug_port: forced restart wasReady={}",
            probe.renderer_ready
        ));
        stop_host();
        invalidate_host_probe_cache();
        sleep_ms(budget.stop_settle_ms);
        let _ = launch_host(port)?;
        sleep_ms(budget.launch_settle_ms);
        let after = wait_for_host_lifecycle(
            port,
            &["starting", "ready"],
            budget.wait_debug_port_ms,
            budget.poll_ms,
        );
        if after.renderer_ready || after.lifecycle == "ready" {
            note_host_ready(port);
            return Ok(());
        }
        if after.debug_port_open
            && wait_until_renderer_ready(port, budget.wait_renderer_ms, budget.poll_ms)
        {
            note_host_ready(port);
            return Ok(());
        }
        // fall through to hard relaunch
    } else {
        if probe.renderer_ready || (probe.lifecycle == "ready" && probe.can_hot_apply) {
            note_host_ready(port);
            return Ok(());
        }

        if probe.debug_port_open && !probe.renderer_ready {
            append_diag("ensure_debug_port: port open, waiting for app:// renderer");
            if wait_until_renderer_ready(port, budget.wait_renderer_ms, budget.poll_ms) {
                note_host_ready(port);
                return Ok(());
            }
            probe = probe_host_lifecycle_force(port);
        }

        let running = probe.codex_running() || !find_host_main_pids().is_empty();
        if running && !probe.debug_port_open {
            // Host up without debug port — must relaunch with our flag.
            append_diag(
                "ensure_debug_port: host running without debug port → stop+relaunch",
            );
            stop_host();
            invalidate_host_probe_cache();
            sleep_ms(budget.stop_settle_ms);
        } else if running && probe.debug_port_open && !probe.renderer_ready {
            append_diag("ensure_debug_port: running but not ready after wait; relaunch");
            stop_host();
            invalidate_host_probe_cache();
            sleep_ms(budget.stop_settle_ms);
        } else if !running {
            // cold launch
        } else if probe.renderer_ready {
            note_host_ready(port);
            return Ok(());
        }

        if !probe_host_lifecycle_force(port).renderer_ready {
            let _ = launch_host(port)?;
            sleep_ms(budget.launch_settle_ms);
        }
    }

    // Shared tail
    let after_launch = wait_for_host_lifecycle(
        port,
        &["starting", "ready"],
        budget.wait_debug_port_ms,
        budget.poll_ms,
    );
    if after_launch.renderer_ready || after_launch.lifecycle == "ready" {
        note_host_ready(port);
        return Ok(());
    }
    if after_launch.debug_port_open {
        append_diag("ensure_debug_port: launched, waiting for renderer");
        if wait_until_renderer_ready(port, budget.wait_renderer_ms, budget.poll_ms) {
            note_host_ready(port);
            return Ok(());
        }
    }

    append_diag("ensure_debug_port: retry hard relaunch");
    stop_host();
    invalidate_host_probe_cache();
    sleep_ms(budget.stop_settle_ms + 300);
    let _ = launch_host(port)?;
    sleep_ms(budget.launch_settle_ms);

    let final_snap = wait_for_host_lifecycle(
        port,
        &["ready"],
        budget.wait_renderer_ms + budget.wait_debug_port_ms,
        budget.poll_ms,
    );
    if final_snap.renderer_ready || final_snap.lifecycle == "ready" {
        note_host_ready(port);
        return Ok(());
    }

    let last = probe_host_lifecycle_force(port);
    append_diag(&format!(
        "ensure_debug_port: failed lifecycle={} process={} portOpen={}",
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
pub fn inject_budget(port: u16) -> TimingBudget {
    let probe = probe_host_lifecycle_force(port);
    resolve_timing_budget(Some(&probe))
}

#[allow(dead_code)]
pub fn exe_looks_valid(p: &PathBuf) -> bool {
    p.is_file() || is_windows_store_path(&p.to_string_lossy())
}
