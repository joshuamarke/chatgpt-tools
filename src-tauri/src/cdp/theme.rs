//! Optional Codex `config.toml` [desktop] theme patch / restore.
//! Hardened against fake-success and corrupt writes (Dream Skin engineering habits):
//! - Strict UTF-8 (reject NUL / invalid UTF-8)
//! - Preserve original newline style
//! - Refuse ambiguous TOML (`[desktop.*]` subtables, multiline strings, duplicate keys)
//! - Same-directory atomic replace + concurrent-change abort
//! - Only touch appearance* scalar keys inside `[desktop]`

use crate::engine::EngineError;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("CODEX_CONFIG_PATH") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    home.join(".codex").join("config.toml")
}

pub fn backup_path(state_root: &Path) -> PathBuf {
    state_root.join("config.before-skin-manager.toml")
}

fn is_our_theme_line(line: &str) -> bool {
    let t = line.trim_start();
    // Managed pin marker (v1/v2) — strip on restore so we do not leave stale comments.
    if t.starts_with("# chatgpt-tools:appearance-pin")
        || t.starts_with("# codex-dream-skin:appearance-pin")
    {
        return true;
    }
    // Support optional quotes: appearanceTheme / "appearanceTheme"
    let bare = t.trim_start_matches('"').trim_start_matches('\'');
    bare.starts_with("appearanceTheme")
        || bare.starts_with("appearanceLightCodeThemeId")
        || bare.starts_with("appearanceLightChromeTheme")
        || bare.starts_with("appearanceDarkCodeThemeId")
        || bare.starts_with("appearanceDarkChromeTheme")
}

fn theme_keys() -> &'static [&'static str] {
    &[
        "appearanceTheme",
        "appearanceLightCodeThemeId",
        "appearanceLightChromeTheme",
        "appearanceDarkCodeThemeId",
        "appearanceDarkChromeTheme",
    ]
}

fn theme_str_field(theme: &Value, key: &str) -> Option<String> {
    theme.get(key).map(|v| {
        if v.is_string() {
            v.as_str().unwrap_or("").to_string()
        } else {
            v.to_string()
        }
    })
}

/// Normalize skin-declared appearance to `light` | `dark` | `auto`.
/// Fixed light/dark pins Codex `appearanceTheme` so native dropdown/popover tokens
/// match the skin; `auto` restores the pre-install user preference on apply/restore.
fn normalize_appearance_token(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "dark" => "dark",
        "light" => "light",
        "auto" | "system" | "" => "auto",
        other if other.contains("dark") => "dark",
        other if other.contains("light") => "light",
        _ => "auto",
    }
}

/// Resolve pin mode from desktopTheme JSON (appearanceTheme preferred, then appearance).
pub fn resolve_appearance_pin(theme: &Value) -> &'static str {
    let from_desktop = theme
        .get("appearanceTheme")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !from_desktop.is_empty() {
        return normalize_appearance_token(from_desktop);
    }
    let from_skin = theme
        .get("appearance")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    normalize_appearance_token(from_skin)
}

/// Marker written into managed config so restore can distinguish pin generations.
const APPEARANCE_PIN_MARKER_V2: &str = "# chatgpt-tools:appearance-pin=v2";

/// Build `[desktop]` appearance lines for Codex.
/// Dark skins must emit `appearanceDark*` — otherwise the host keeps light
/// dialogs/settings and OS caption chrome even when `appearanceTheme = "dark"`.
///
/// Pin discipline:
/// - `appearanceTheme = light|dark` when the skin declares a fixed shell
/// - `auto` / missing → restore pre-install appearanceTheme when backup exists;
///   otherwise omit forcing a new appearanceTheme (keep user's current line)
fn build_desktop_theme_settings(theme: &Value) -> Vec<String> {
    let pin = resolve_appearance_pin(theme);
    let appearance = match pin {
        "dark" => "dark",
        "light" => "light",
        _ => {
            // auto: prefer explicit theme field if author still set light/dark chrome only
            theme
                .get("appearanceTheme")
                .and_then(|v| v.as_str())
                .map(normalize_appearance_token)
                .filter(|a| *a == "light" || *a == "dark")
                .unwrap_or("auto")
        }
    };

    let light_code = theme
        .get("appearanceLightCodeThemeId")
        .and_then(|v| v.as_str())
        .or_else(|| theme.get("appearanceDarkCodeThemeId").and_then(|v| v.as_str()))
        .unwrap_or("codex");
    let dark_code = theme
        .get("appearanceDarkCodeThemeId")
        .and_then(|v| v.as_str())
        .or_else(|| theme.get("appearanceLightCodeThemeId").and_then(|v| v.as_str()))
        .unwrap_or("codex");

    // Mirror the authored chrome pair so dialogs never fall back to default white.
    let light_chrome = theme_str_field(theme, "appearanceLightChromeTheme")
        .or_else(|| theme_str_field(theme, "appearanceDarkChromeTheme"));
    let dark_chrome = theme_str_field(theme, "appearanceDarkChromeTheme")
        .or_else(|| theme_str_field(theme, "appearanceLightChromeTheme"));

    let mut lines = Vec::new();
    // Pin marker (comment) for restore discipline — harmless to Codex TOML parsers.
    lines.push(APPEARANCE_PIN_MARKER_V2.to_string());

    if appearance == "auto" {
        // Do not force appearanceTheme for auto skins: restore path / user preference wins.
        // If author explicitly left a light/dark chrome pack, still write chrome keys only.
    } else {
        lines.push(format!("appearanceTheme = \"{appearance}\""));
    }

    lines.push(format!("appearanceLightCodeThemeId = \"{light_code}\""));
    lines.push(format!(
        "appearanceLightChromeTheme = {}",
        light_chrome.as_deref().unwrap_or("{}")
    ));

    let has_dark = appearance == "dark"
        || appearance == "auto"
        || theme.get("appearanceDarkCodeThemeId").is_some()
        || theme.get("appearanceDarkChromeTheme").is_some();
    if has_dark {
        lines.push(format!("appearanceDarkCodeThemeId = \"{dark_code}\""));
        lines.push(format!(
            "appearanceDarkChromeTheme = {}",
            dark_chrome.as_deref().unwrap_or("{}")
        ));
    }

    lines
}

/// When applying an auto skin, put the user's original appearanceTheme back if we
/// have a pre-install backup (so fixed-theme pins do not stick after switching).
fn restore_user_appearance_theme_line(state_root: &Path) -> Option<String> {
    let backup = backup_path(state_root);
    if !backup.is_file() {
        return None;
    }
    let bytes = fs::read(&backup).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let text = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        String::from_utf8(bytes).ok()?
    };
    let header = find_desktop_header(&text)?;
    let rest = &text[header.end..];
    let next = find_next_section(rest).unwrap_or(rest.len());
    let section = &rest[..next];
    for line in section.lines() {
        if line_starts_with_key(line, "appearanceTheme") {
            return Some(line.trim().to_string());
        }
    }
    None
}

/// Read config as strict UTF-8 without BOM preference; reject NUL / invalid sequences.
fn read_config_bytes_strict(path: &Path) -> Result<Vec<u8>, EngineError> {
    let bytes = fs::read(path).map_err(|e| EngineError::msg(format!("read config.toml: {e}")))?;
    if bytes.contains(&0) {
        return Err(EngineError::msg(
            "config.toml contains NUL bytes; refusing to modify (possible UTF-16 or corruption)",
        ));
    }
    // Strip UTF-8 BOM for parsing but remember we rewrite without BOM.
    let body = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        &bytes[..]
    };
    if std::str::from_utf8(body).is_err() {
        return Err(EngineError::msg(
            "config.toml is not valid UTF-8; refusing to modify",
        ));
    }
    Ok(body.to_vec())
}

fn detect_newline(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Reject sections we cannot safely line-edit.
fn validate_desktop_section(section: &str) -> Result<(), EngineError> {
    for (idx, line) in section.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        // Nested [desktop.*] tables inside the slice should not appear if find_next works,
        // but refuse dotted table headers that would collide.
        if t.starts_with('[') && t.contains("desktop.") {
            return Err(EngineError::msg(format!(
                "config.toml has [desktop.*] subtable near line {}; refusing appearance patch",
                idx + 1
            )));
        }
        // Multiline string / array starters for target keys.
        // Allow single-line inline tables `{}` / `{ ... }` (Codex chrome theme form).
        for key in theme_keys() {
            if line_starts_with_key(line, key) {
                let after_eq = line.split_once('=').map(|(_, v)| v.trim()).unwrap_or("");
                if after_eq.starts_with("'''") || after_eq.starts_with("\"\"\"") {
                    return Err(EngineError::msg(format!(
                        "config.toml key {key} uses multiline string form; refusing to edit"
                    )));
                }
                if after_eq.starts_with('[') {
                    return Err(EngineError::msg(format!(
                        "config.toml key {key} uses array form; refusing to edit"
                    )));
                }
            }
        }
    }
    // Duplicate target keys
    for key in theme_keys() {
        let count = section
            .lines()
            .filter(|l| line_starts_with_key(l, key))
            .count();
        if count > 1 {
            return Err(EngineError::msg(format!(
                "config.toml has duplicate key {key} in [desktop]; refusing to edit"
            )));
        }
    }
    Ok(())
}

/// Atomic write: same-dir temp + replace; abort if original bytes changed mid-flight.
fn atomic_write_config(path: &Path, original: &[u8], new_text: &str) -> Result<(), EngineError> {
    // Concurrent change guard
    let now = fs::read(path).map_err(|e| EngineError::msg(format!("re-read config.toml: {e}")))?;
    let now_body = if now.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &now[3..]
    } else {
        &now[..]
    };
    if now_body != original {
        return Err(EngineError::msg(
            "config.toml changed during edit; refusing to overwrite concurrent write",
        ));
    }

    let parent = path
        .parent()
        .ok_or_else(|| EngineError::msg("config.toml has no parent directory"))?;
    let tmp = parent.join(format!(
        ".config.toml.chatgpt-tools.{}.tmp",
        std::process::id()
    ));
    {
        let mut f = fs::File::create(&tmp)
            .map_err(|e| EngineError::msg(format!("create config temp: {e}")))?;
        // No BOM
        f.write_all(new_text.as_bytes())
            .map_err(|e| EngineError::msg(format!("write config temp: {e}")))?;
        f.sync_all()
            .map_err(|e| EngineError::msg(format!("sync config temp: {e}")))?;
    }

    // Final concurrent check before replace
    let now2 = fs::read(path).unwrap_or_default();
    let now2_body = if now2.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &now2[3..]
    } else {
        &now2[..]
    };
    if now2_body != original {
        let _ = fs::remove_file(&tmp);
        return Err(EngineError::msg(
            "config.toml changed before replace; aborting",
        ));
    }

    // Prefer atomic rename; on Windows replace existing
    if path.is_file() {
        let bak = parent.join(format!(
            ".config.toml.chatgpt-tools.{}.bak",
            std::process::id()
        ));
        // File::replace style: rename target → bak, tmp → target, drop bak
        if let Err(e) = fs::rename(path, &bak) {
            let _ = fs::remove_file(&tmp);
            return Err(EngineError::msg(format!("stage config backup: {e}")));
        }
        if let Err(e) = fs::rename(&tmp, path) {
            let _ = fs::rename(&bak, path);
            let _ = fs::remove_file(&tmp);
            return Err(EngineError::msg(format!("replace config.toml: {e}")));
        }
        let _ = fs::remove_file(&bak);
    } else if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(EngineError::msg(format!("write config.toml: {e}")));
    }
    Ok(())
}

/// Best-effort patch of ~/.codex/config.toml [desktop] section.
/// On validation failure returns Err — callers should treat as non-fatal for CSS skins
/// but must not claim config was written.
///
/// Pin discipline (production):
/// - Fixed light/dark skins write `appearanceTheme` so native dropdown tokens match
/// - Auto skins restore the pre-install appearanceTheme line from backup when present
/// - Prefer calling while the host is closed/restarting (Codex reloads [desktop] on boot)
pub fn apply_desktop_theme(theme: &Value, state_root: &Path) -> Result<Value, EngineError> {
    let path = config_path();
    let pin = resolve_appearance_pin(theme);
    let mut settings = build_desktop_theme_settings(theme);
    if pin == "auto" {
        if let Some(user_line) = restore_user_appearance_theme_line(state_root) {
            // Insert after pin marker so appearanceTheme is present for auto→user restore.
            if !settings.iter().any(|l| line_starts_with_key(l, "appearanceTheme")) {
                settings.insert(1, user_line);
            }
        }
    }
    if !path.is_file() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let body = format!("[desktop]\n{}\n", settings.join("\n"));
        // Create via same-dir temp + rename so we never leave a half-written config.
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let tmp = parent.join(format!(
            ".config.toml.chatgpt-tools.create.{}.tmp",
            std::process::id()
        ));
        match (|| -> Result<(), EngineError> {
            {
                let mut f = fs::File::create(&tmp)
                    .map_err(|e| EngineError::msg(format!("create config temp: {e}")))?;
                f.write_all(body.as_bytes())
                    .map_err(|e| EngineError::msg(format!("write config temp: {e}")))?;
                f.sync_all()
                    .map_err(|e| EngineError::msg(format!("sync config temp: {e}")))?;
            }
            // Refuse to clobber if another process created config mid-flight.
            if path.is_file() {
                let _ = fs::remove_file(&tmp);
                return Err(EngineError::msg(
                    "config.toml appeared during create; refusing to overwrite",
                ));
            }
            fs::rename(&tmp, &path)
                .map_err(|e| EngineError::msg(format!("create config.toml: {e}")))?;
            Ok(())
        })() {
            Ok(()) => {
                return Ok(serde_json::json!({
                    "created": true,
                    "path": path.to_string_lossy(),
                    "atomic": true
                }))
            }
            Err(_) => {
                let _ = fs::remove_file(&tmp);
                return Ok(serde_json::json!({
                    "skipped": true,
                    "reason": "config missing and create failed"
                }));
            }
        }
    }

    let original = read_config_bytes_strict(&path)?;
    let content = String::from_utf8(original.clone())
        .map_err(|e| EngineError::msg(format!("config utf-8: {e}")))?;
    let nl = detect_newline(&content);

    // Note: Codex commonly has sibling tables like `[desktop.open-in-target-preferences]`.
    // We only line-edit the exact `[desktop]` section (until the next `[...]` header);
    // subtables are preserved as-is in the "after" slice. Do NOT refuse the whole file.

    let backup = backup_path(state_root);
    if !backup.is_file() {
        // Durable pre-install backup: same-dir temp + rename (never half-written).
        let parent = backup.parent().unwrap_or(state_root);
        let _ = fs::create_dir_all(parent);
        let tmp = parent.join(format!(
            ".config.before-skin-manager.{}.tmp",
            std::process::id()
        ));
        if fs::write(&tmp, &original).is_ok() {
            if fs::rename(&tmp, &backup).is_err() {
                let _ = fs::write(&backup, &original);
                let _ = fs::remove_file(&tmp);
            }
        } else {
            let _ = fs::write(&backup, &original);
        }
    }

    let mut content_out = content.clone();

    if let Some(header_pos) = find_desktop_header(&content_out) {
        let insert_at = header_pos.end;
        let rest = &content_out[insert_at..];
        let next = find_next_section(rest);
        let (section, after) = if let Some(n) = next {
            (&rest[..n], &rest[n..])
        } else {
            (rest, "")
        };
        validate_desktop_section(section)?;
        let mut lines: Vec<&str> = section.lines().collect();
        while lines
            .last()
            .map(|l| l.trim().is_empty())
            .unwrap_or(false)
        {
            lines.pop();
        }
        // Drop previous pin markers + appearance* scalars (not only bare keys).
        lines.retain(|line| !is_our_theme_line(line));
        let mut out_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        out_lines.extend(settings);
        content_out = format!(
            "{}{}{}{}",
            &content_out[..insert_at],
            out_lines.join(nl),
            nl,
            after
        );
    } else {
        let trimmed = content_out.trim_end();
        content_out = format!(
            "{trimmed}{nl}{nl}[desktop]{nl}{}{nl}",
            settings.join(nl)
        );
    }

    // Newer Codex rewrites chrome themes as dotted subtables
    // (`[desktop.appearanceLightChromeTheme]`) after the user opens 外观.
    // Those tables shadow our inline `appearanceLightChromeTheme = { … }` and
    // leave code/chrome pins looking "stuck" until a manual UI click.
    content_out = strip_desktop_appearance_subtables(&content_out, nl);

    // Serialize with provider/proxy live writers (same config.toml).
    crate::live_config::with_live_lock(&path, |_| {
        atomic_write_config(&path, &original, &content_out).map_err(|e| e.to_string())
    })
    .map_err(EngineError::msg)?;
    Ok(serde_json::json!({ "ok": true, "path": path.to_string_lossy(), "atomic": true }))
}

/// Remove `[desktop.appearance*]` sibling tables while preserving other
/// `[desktop.*]` sections (e.g. open-in-target-preferences).
fn strip_desktop_appearance_subtables(text: &str, nl: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            let header = t.trim_start_matches('[').trim_end_matches(']').trim();
            // Match desktop.appearanceLightChromeTheme / appearanceDark* / quoted forms.
            let bare = header.trim_matches('"').trim_matches('\'');
            let is_appearance_sub = bare
                .strip_prefix("desktop.")
                .map(|rest| {
                    let r = rest.trim_matches('"').trim_matches('\'');
                    r.starts_with("appearance")
                })
                .unwrap_or(false);
            skipping = is_appearance_sub;
            if skipping {
                continue;
            }
        }
        if skipping {
            continue;
        }
        out.push(line);
    }
    // Preserve a trailing newline when the original had one.
    let mut joined = out.join(nl);
    if text.ends_with('\n') || text.ends_with("\r\n") {
        if !joined.ends_with(nl) {
            joined.push_str(nl);
        }
    }
    joined
}

/// Strip skin-written appearance* keys from [desktop].
pub fn restore_desktop_theme(state_root: &Path) -> Value {
    let path = config_path();
    if !path.is_file() {
        return serde_json::json!({ "restored": false, "reason": "config missing" });
    }
    let original = match read_config_bytes_strict(&path) {
        Ok(b) => b,
        Err(e) => {
            return serde_json::json!({ "restored": false, "reason": e.to_string() });
        }
    };
    let current = match String::from_utf8(original.clone()) {
        Ok(s) => s,
        Err(_) => {
            return serde_json::json!({ "restored": false, "reason": "invalid utf-8" });
        }
    };
    let nl = detect_newline(&current);

    // Sibling `[desktop.*]` tables are fine: we only strip appearance* from the
    // exact `[desktop]` section body (section ends at the next table header).

    let Some(header_pos) = find_desktop_header(&current) else {
        return serde_json::json!({ "restored": false, "reason": "no desktop section" });
    };
    let insert_at = header_pos.end;
    let rest = &current[insert_at..];
    let next = find_next_section(rest);
    let (section, after) = if let Some(n) = next {
        (&rest[..n], &rest[n..])
    } else {
        (rest, "")
    };
    if validate_desktop_section(section).is_err() {
        return serde_json::json!({
            "restored": false,
            "reason": "ambiguous desktop section; refuse edit"
        });
    }

    let mut lines: Vec<&str> = section.lines().collect();
    while lines
        .last()
        .map(|l| l.trim().is_empty())
        .unwrap_or(false)
    {
        lines.pop();
    }
    lines.retain(|line| !is_our_theme_line(line));

    let mut restored_from: Option<String> = None;
    let mut candidates = vec![backup_path(state_root)];
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let home = PathBuf::from(home);
        if cfg!(windows) {
            if let Ok(local) = std::env::var("LOCALAPPDATA") {
                let local = PathBuf::from(local);
                candidates.push(local.join("CodexDreamSkin").join("config.before-dream-skin.toml"));
                candidates.push(local.join("CodexCnSkin").join("config.before-cn-skin.toml"));
            }
        } else {
            candidates.push(
                home.join("Library")
                    .join("Application Support")
                    .join("CodexDreamSkin")
                    .join("config.before-dream-skin.toml"),
            );
            candidates.push(
                home.join("Library")
                    .join("Application Support")
                    .join("CodexCnSkin")
                    .join("config.before-cn-skin.toml"),
            );
        }
    }
    for backup in candidates {
        if !backup.is_file() {
            continue;
        }
        if let Ok(bytes) = fs::read(&backup) {
            if bytes.contains(&0) {
                continue;
            }
            let text = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                String::from_utf8_lossy(&bytes[3..]).into_owned()
            } else {
                match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(_) => continue,
                }
            };
            if let Some(m) = find_desktop_header(&text) {
                let b_rest = &text[m.end..];
                let b_next = find_next_section(b_rest);
                let b_section = if let Some(n) = b_next {
                    &b_rest[..n]
                } else {
                    b_rest
                };
                let has_skin_theme = b_section.lines().any(is_our_theme_line);
                if !has_skin_theme {
                    restored_from = Some(backup.to_string_lossy().to_string());
                    break;
                }
            }
        }
    }

    let out = strip_desktop_appearance_subtables(
        &format!(
            "{}{}{}{}",
            &current[..insert_at],
            lines.join(nl),
            nl,
            after
        ),
        nl,
    );
    if let Err(e) = crate::live_config::with_live_lock(&path, |_| {
        atomic_write_config(&path, &original, &out).map_err(|err| err.to_string())
    }) {
        return serde_json::json!({ "restored": false, "reason": e });
    }
    serde_json::json!({
        "restored": true,
        "restoredFrom": restored_from,
        "strippedThemeKeys": true,
        "atomic": true,
        "path": path.to_string_lossy(),
    })
}

struct HeaderPos {
    end: usize,
}

fn find_desktop_header(content: &str) -> Option<HeaderPos> {
    // Exact line-start `[desktop]` (not `[desktop.foo]`).
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i == 0 || bytes[i - 1] == b'\n' {
            if content[i..].starts_with("[desktop]") {
                let mut j = i + "[desktop]".len();
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'#' {
                    while j < bytes.len() && bytes[j] != b'\n' && bytes[j] != b'\r' {
                        j += 1;
                    }
                }
                if j < bytes.len() && bytes[j] == b'\r' {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'\n' {
                    return Some(HeaderPos { end: j + 1 });
                }
                if j >= bytes.len() {
                    return Some(HeaderPos { end: j });
                }
            }
        }
        i += 1;
    }
    None
}

fn find_next_section(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i == 0 || bytes[i - 1] == b'\n' {
            if bytes[i] == b'[' {
                if rest[i..].find(']').is_some() {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

fn line_starts_with_key(line: &str, key: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with('#') {
        return false;
    }
    // appearanceTheme = ...  or  "appearanceTheme" = ...
    if t.starts_with(key) {
        return t[key.len()..].trim_start().starts_with('=');
    }
    let quoted = format!("\"{key}\"");
    if t.starts_with(&quoted) {
        return t[quoted.len()..].trim_start().starts_with('=');
    }
    false
}

/// Parse a simple TOML string scalar: `"dark"` / `'light'` / unquoted.
fn parse_toml_string_value(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    // Double-quoted
    if let Some(rest) = t.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }
    // Single-quoted
    if let Some(rest) = t.strip_prefix('\'') {
        let end = rest.find('\'')?;
        return Some(rest[..end].to_string());
    }
    // Unquoted: strip trailing comment / take first token.
    let no_comment = t.split('#').next().unwrap_or(t).trim();
    let token = no_comment.split_whitespace().next().unwrap_or(no_comment);
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Read `[desktop].appearanceTheme` from config.toml (best-effort).
/// Returns `"light"` / `"dark"` when present; `None` if missing or unreadable.
pub fn read_appearance_theme() -> Option<String> {
    let path = config_path();
    let text = fs::read_to_string(&path).ok()?;
    let header = find_desktop_header(&text)?;
    let rest = &text[header.end..];
    let next = find_next_section(rest).unwrap_or(rest.len());
    let section = &rest[..next];
    for line in section.lines() {
        if !line_starts_with_key(line, "appearanceTheme") {
            continue;
        }
        let after_eq = line.split_once('=')?.1;
        let val = parse_toml_string_value(after_eq)?.to_ascii_lowercase();
        if val == "light" || val == "dark" {
            return Some(val);
        }
        // Unknown value — still surface raw lowercased token for diagnostics.
        return Some(val);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that touch process-wide `CODEX_CONFIG_PATH`.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolves_appearance_pin_fixed_and_auto() {
        let dark = serde_json::json!({ "appearanceTheme": "dark" });
        assert_eq!(resolve_appearance_pin(&dark), "dark");
        let light = serde_json::json!({ "appearance": "light" });
        assert_eq!(resolve_appearance_pin(&light), "light");
        let auto = serde_json::json!({ "appearance": "auto" });
        assert_eq!(resolve_appearance_pin(&auto), "auto");
        let empty = serde_json::json!({});
        assert_eq!(resolve_appearance_pin(&empty), "auto");
    }

    #[test]
    fn fixed_pin_writes_appearance_theme_line() {
        let theme = serde_json::json!({
            "appearanceTheme": "dark",
            "appearanceDarkCodeThemeId": "codex",
            "appearanceDarkChromeTheme": "{}"
        });
        let lines = build_desktop_theme_settings(&theme);
        assert!(
            lines.iter().any(|l| l.contains("appearanceTheme = \"dark\"")),
            "lines={lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("appearance-pin=v2")),
            "expected pin marker lines={lines:?}"
        );
    }

    #[test]
    fn auto_pin_omits_forced_appearance_theme() {
        let theme = serde_json::json!({
            "appearance": "auto",
            "appearanceLightCodeThemeId": "codex",
            "appearanceLightChromeTheme": "{}"
        });
        let lines = build_desktop_theme_settings(&theme);
        assert!(
            !lines.iter().any(|l| line_starts_with_key(l, "appearanceTheme")),
            "auto must not force appearanceTheme lines={lines:?}"
        );
    }

    #[test]
    fn reads_appearance_theme_from_config() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "cgtools-theme-read-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.toml");
        fs::write(
            &cfg,
            "[desktop]\nappearanceTheme = \"dark\"\nother = 1\n",
        )
        .unwrap();
        std::env::set_var("CODEX_CONFIG_PATH", &cfg);
        assert_eq!(read_appearance_theme().as_deref(), Some("dark"));
        fs::write(
            &cfg,
            "[desktop]\n\"appearanceTheme\" = \"light\"\n",
        )
        .unwrap();
        assert_eq!(read_appearance_theme().as_deref(), Some("light"));
        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("CODEX_CONFIG_PATH");
    }

    #[test]
    fn strips_and_patches_desktop_section() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "cgtools-theme-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.toml");
        fs::write(
            &cfg,
            "[desktop]\nappearanceTheme = \"dark\"\nother = 1\n\n[other]\nx = 1\n",
        )
        .unwrap();
        std::env::set_var("CODEX_CONFIG_PATH", &cfg);
        let theme = serde_json::json!({
            "appearanceTheme": "light",
            "appearanceLightCodeThemeId": "codex",
            "appearanceLightChromeTheme": "{ accent = \"#B65CFF\" }"
        });
        let r = apply_desktop_theme(&theme, &dir).unwrap();
        assert!(
            r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) || r.get("created").is_some(),
            "apply result: {r}"
        );
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("appearanceTheme = \"light\""), "text={text}");
        assert!(text.contains("other = 1"));
        let restored = restore_desktop_theme(&dir);
        assert_eq!(
            restored.get("restored").and_then(|v| v.as_bool()),
            Some(true),
            "restore: {restored}"
        );
        let text2 = fs::read_to_string(&cfg).unwrap();
        assert!(!text2.contains("appearanceTheme"), "text2={text2}");
        assert!(text2.contains("other = 1"));
        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("CODEX_CONFIG_PATH");
    }

    #[test]
    fn patches_dark_chrome_theme_keys() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "cgtools-theme-dark-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.toml");
        fs::write(&cfg, "[features]\nmemories = true\n").unwrap();
        std::env::set_var("CODEX_CONFIG_PATH", &cfg);
        let theme = serde_json::json!({
            "appearanceTheme": "dark",
            "appearanceDarkCodeThemeId": "codex",
            "appearanceDarkChromeTheme": "{ accent = \"#A83A2E\", ink = \"#E8E4DC\", surface = \"#141A24\", opaqueWindows = true }"
        });
        let r = apply_desktop_theme(&theme, &dir).unwrap();
        assert!(
            r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) || r.get("created").is_some(),
            "apply result: {r}"
        );
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("appearanceTheme = \"dark\""), "text={text}");
        assert!(
            text.contains("appearanceDarkCodeThemeId = \"codex\""),
            "text={text}"
        );
        assert!(
            text.contains("appearanceDarkChromeTheme ="),
            "text={text}"
        );
        // Dark-only skins should mirror chrome into the light pair as fallback.
        assert!(
            text.contains("appearanceLightChromeTheme ="),
            "text={text}"
        );
        assert!(text.contains("surface = \"#141A24\""), "text={text}");
        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("CODEX_CONFIG_PATH");
    }

    #[test]
    fn patches_desktop_with_sibling_subtables() {
        // Real Codex configs often have [desktop.open-in-target-preferences] etc.
        // We must still write appearance* into the exact [desktop] section.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "cgtools-theme-sub-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.toml");
        fs::write(
            &cfg,
            "[desktop]\nconversationDetailMode = \"STEPS_COMMANDS\"\nappearanceTheme = \"light\"\n[desktop.open-in-target-preferences]\nglobal = \"fileManager\"\n\n[features]\nmemories = true\n",
        )
        .unwrap();
        std::env::set_var("CODEX_CONFIG_PATH", &cfg);
        let theme = serde_json::json!({
            "appearanceTheme": "dark",
            "appearanceDarkCodeThemeId": "codex",
            "appearanceDarkChromeTheme": "{ accent = \"#A83A2E\", surface = \"#141A24\" }"
        });
        let r = apply_desktop_theme(&theme, &dir).unwrap();
        assert!(
            r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            "apply result: {r}"
        );
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("appearanceTheme = \"dark\""), "text={text}");
        assert!(
            text.contains("appearanceDarkChromeTheme ="),
            "text={text}"
        );
        assert!(
            text.contains("[desktop.open-in-target-preferences]"),
            "subtable preserved: text={text}"
        );
        assert!(
            text.contains("global = \"fileManager\""),
            "subtable body preserved: text={text}"
        );
        assert!(
            text.contains("conversationDetailMode = \"STEPS_COMMANDS\""),
            "other desktop keys preserved: text={text}"
        );
        let restored = restore_desktop_theme(&dir);
        assert_eq!(
            restored.get("restored").and_then(|v| v.as_bool()),
            Some(true),
            "restore: {restored}"
        );
        let text2 = fs::read_to_string(&cfg).unwrap();
        assert!(
            !text2.contains("appearanceTheme"),
            "appearance stripped: text2={text2}"
        );
        assert!(
            text2.contains("[desktop.open-in-target-preferences]"),
            "subtable still there after restore: text2={text2}"
        );
        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("CODEX_CONFIG_PATH");
    }

    #[test]
    fn strips_host_appearance_chrome_subtables() {
        // Codex 26.7+ may rewrite chrome as [desktop.appearanceLightChromeTheme]
        // after a settings click; that must not shadow skin inline pins.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "cgtools-theme-chrome-sub-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.toml");
        fs::write(
            &cfg,
            r##"[desktop]
conversationDetailMode = "STEPS_COMMANDS"
appearanceTheme = "light"
appearanceLightCodeThemeId = "old"
appearanceLightChromeTheme = { accent = "#111111" }
[desktop.appearanceLightChromeTheme]
accent = "#0169cc"
surface = "#ffffff"
[desktop.open-in-target-preferences]
global = "fileManager"
"##,
        )
        .unwrap();
        std::env::set_var("CODEX_CONFIG_PATH", &cfg);
        let theme = serde_json::json!({
            "appearanceTheme": "light",
            "appearanceLightCodeThemeId": "codex",
            "appearanceLightChromeTheme": "{ accent = \"#6A9EAB\", surface = \"#EEF3F2\" }"
        });
        let r = apply_desktop_theme(&theme, &dir).unwrap();
        assert!(
            r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            "apply result: {r}"
        );
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(
            text.contains("appearanceLightCodeThemeId = \"codex\""),
            "text={text}"
        );
        assert!(
            text.contains("accent = \"#6A9EAB\""),
            "skin chrome should win: text={text}"
        );
        assert!(
            !text.contains("[desktop.appearanceLightChromeTheme]"),
            "host chrome subtable must be stripped: text={text}"
        );
        assert!(
            text.contains("[desktop.open-in-target-preferences]"),
            "non-appearance subtable kept: text={text}"
        );
        // Pin markers must not stack forever.
        let pins = text.matches("chatgpt-tools:appearance-pin").count();
        assert_eq!(pins, 1, "expected single pin marker, text={text}");
        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("CODEX_CONFIG_PATH");
    }

    #[test]
    fn find_desktop_not_subtable() {
        let c = "[desktop.foo]\nx=1\n[desktop]\ny=1\n";
        let h = find_desktop_header(c).unwrap();
        assert!(c[h.end..].starts_with("y=1") || c[h.end..].contains("y=1"));
    }
}
