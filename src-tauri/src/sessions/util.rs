//! Small helpers shared by session domain modules (atomic write, process probe, path display).

use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Strip Windows extended-length / device prefixes so UI paths look like normal drive paths.
///
/// Codex desktop (Electron) often stores cwd as `\\?\E:\foo` or `//?/E:/foo`.
/// cc-switch and other tools display the human form without the `\\?\` prefix.
pub fn normalize_display_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // `\\?\UNC\server\share\…` → `\\server\share\…`
    if let Some(rest) = strip_prefix_ci(trimmed, r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = strip_prefix_ci(trimmed, r"//?/UNC/") {
        let rest = rest.replace('/', "\\");
        return format!(r"\\{rest}");
    }
    // `\\?\E:\…` or `\\?\C:\…`
    if let Some(rest) = strip_prefix_ci(trimmed, r"\\?\") {
        return rest.to_string();
    }
    // Node / some runtimes use forward-slash device form `//?/E:/…`
    if let Some(rest) = strip_prefix_ci(trimmed, r"//?/") {
        return rest.replace('/', "\\");
    }
    trimmed.to_string()
}

fn strip_prefix_ci<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    if value.len() < prefix.len() {
        return None;
    }
    if value
        .get(..prefix.len())?
        .eq_ignore_ascii_case(prefix)
    {
        Some(&value[prefix.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod path_tests {
    use super::normalize_display_path;

    #[test]
    fn strips_win32_device_prefix() {
        assert_eq!(
            normalize_display_path(r"\\?\E:\临时项目\image-2"),
            r"E:\临时项目\image-2"
        );
        assert_eq!(
            normalize_display_path(r"\\?\C:\Users\demo"),
            r"C:\Users\demo"
        );
    }

    #[test]
    fn strips_unc_device_prefix() {
        assert_eq!(
            normalize_display_path(r"\\?\UNC\server\share\proj"),
            r"\\server\share\proj"
        );
    }

    #[test]
    fn strips_forward_slash_device_form() {
        assert_eq!(
            normalize_display_path("//?/E:/tmp/proj"),
            r"E:\tmp\proj"
        );
    }

    #[test]
    fn leaves_normal_paths() {
        assert_eq!(normalize_display_path(r"E:\work\app"), r"E:\work\app");
        assert_eq!(normalize_display_path("/home/user/p"), "/home/user/p");
        assert_eq!(normalize_display_path(""), "");
    }
}

/// Atomic write: temp file next to target, then replace.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    let temp_path = temp_path_for(path);
    fs::write(&temp_path, bytes)
        .with_context(|| format!("failed to write temp file {}", temp_path.display()))?;
    if let Err(error) = replace_file(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "failed to replace {} with {}",
                path.display(),
                temp_path.display()
            )
        });
    }
    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    path.with_file_name(format!(".{name}.{stamp}.tmp"))
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> anyhow::Result<()> {
    // Prefer rename; if target exists, remove then rename.
    if target.exists() {
        let _ = fs::remove_file(target);
    }
    fs::rename(source, target)?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> anyhow::Result<()> {
    fs::rename(source, target)?;
    Ok(())
}

/// Best-effort: PIDs that should not hold session_index while we rewrite it.
pub fn find_session_index_cleanup_blocking_processes() -> Vec<u32> {
    #[cfg(windows)]
    {
        find_processes_by_exe_names(&["Codex.exe", "ChatGPT.exe"])
    }
    #[cfg(target_os = "macos")]
    {
        find_processes_by_pgrep(&["Codex", "ChatGPT"])
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn find_processes_by_exe_names(names: &[&str]) -> Vec<u32> {
    // tasklist CSV: "Image Name","PID",...
    let output = match std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let wanted: std::collections::HashSet<String> =
        names.iter().map(|n| n.to_ascii_lowercase()).collect();
    let mut ids = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 2 {
            continue;
        }
        let name = cols[0].trim().trim_matches('"').to_ascii_lowercase();
        if !wanted.contains(&name) {
            continue;
        }
        let pid_s = cols[1].trim().trim_matches('"');
        if let Ok(pid) = pid_s.parse::<u32>() {
            ids.push(pid);
        }
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[cfg(target_os = "macos")]
fn find_processes_by_pgrep(names: &[&str]) -> Vec<u32> {
    let mut ids = names
        .iter()
        .flat_map(|name| {
            std::process::Command::new("pgrep")
                .args(["-x", name])
                .output()
                .ok()
                .into_iter()
                .flat_map(|output| {
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .filter_map(|line| line.trim().parse::<u32>().ok())
                        .collect::<Vec<_>>()
                })
        })
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
}
