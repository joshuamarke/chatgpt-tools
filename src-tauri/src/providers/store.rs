//! JSON persistence for provider profiles.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use super::models::{AppKind, AppProviderStore, Provider, ProvidersFile};

pub const CODEX_OFFICIAL_ID: &str = "codex-official";
pub const GROK_OFFICIAL_ID: &str = "grok-official";

pub fn providers_file_path() -> PathBuf {
    crate::sessions::paths::app_state_dir().join("providers.json")
}

pub fn load() -> Result<ProvidersFile, String> {
    let path = providers_file_path();
    if !path.exists() {
        let mut file = ProvidersFile {
            version: 1,
            ..Default::default()
        };
        ensure_official_seeds(&mut file);
        reconcile_current_pointers(&mut file);
        let _ = save(&file);
        return Ok(file);
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("读取供应商配置失败: {e}"))?;
    if text.trim().is_empty() {
        let mut file = ProvidersFile {
            version: 1,
            ..Default::default()
        };
        ensure_official_seeds(&mut file);
        reconcile_current_pointers(&mut file);
        let _ = save(&file);
        return Ok(file);
    }
    let mut file: ProvidersFile =
        serde_json::from_str(&text).map_err(|e| format!("解析供应商配置失败: {e}"))?;
    if file.version == 0 {
        file.version = 1;
    }
    let before = serde_json::to_string(&file).unwrap_or_default();
    ensure_official_seeds(&mut file);
    migrate_grok_archive_identities(&mut file);
    file.codex.normalize_failover_order();
    file.grok.normalize_failover_order();
    reconcile_current_pointers(&mut file);
    if file.version < 2 {
        file.version = 2;
    }
    let after = serde_json::to_string(&file).unwrap_or_default();
    // Persist refreshed Official defaults / FO migration / current alignment.
    if before != after {
        let _ = save(&file);
    }
    Ok(file)
}

pub fn save(file: &ProvidersFile) -> Result<(), String> {
    let path = providers_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(file).map_err(|e| format!("序列化失败: {e}"))?;
    atomic_write(&path, text.as_bytes())
}

fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("写入临时文件失败: {e}"))?;
        f.write_all(bytes)
            .map_err(|e| format!("写入临时文件失败: {e}"))?;
        f.sync_all().map_err(|e| format!("同步文件失败: {e}"))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("替换配置文件失败: {e}")
    })
}

/// Ensure built-in official entries exist and stay aligned with official defaults.
/// Refreshes settings/notes on every load so third-party live imports never stick
/// as the Official profile content.
///
/// Does **not** mark Official as `current` — that is decided by
/// [`reconcile_current_with_live`] against real live files.
fn ensure_official_seeds(file: &mut ProvidersFile) {
    seed_codex_official(&mut file.codex);
    seed_grok_official(&mut file.grok);
}

fn seed_codex_official(store: &mut AppProviderStore) {
    const ID: &str = CODEX_OFFICIAL_ID;
    const NAME: &str = "OpenAI Official";
    const WEBSITE: &str = "https://chatgpt.com/codex";
    const NOTES: &str = "Codex / ChatGPT 官方渠道。启用后恢复内置路由（清除第三方代理），使用客户端登录或 Platform API；MCP、插件等本地设置会保留。";

    if let Some(p) = store.providers.iter_mut().find(|p| p.id == ID) {
        // Preserve ChatGPT OAuth material if we previously backfilled it.
        let preserved_auth = p
            .settings_config
            .get("auth")
            .cloned()
            .filter(|a| a.as_object().is_some_and(|o| {
                o.keys().any(|k| k != "OPENAI_API_KEY" && k != "auth_mode")
                    || o.get("tokens").is_some()
                    || o.get("access_token").is_some()
                    || o.get("refresh_token").is_some()
            }));
        let mut settings = super::codex::official_settings_config();
        if let Some(auth) = preserved_auth {
            if let Some(obj) = settings.as_object_mut() {
                obj.insert("auth".into(), auth);
            }
        }
        p.name = NAME.into();
        p.settings_config = settings;
        p.category = Some("official".into());
        p.website_url = Some(WEBSITE.into());
        p.notes = Some(NOTES.into());
        p.sort_index = Some(0);
        return;
    }

    let mut p = Provider::new(ID.into(), NAME.into(), super::codex::official_settings_config());
    p.category = Some("official".into());
    p.website_url = Some(WEBSITE.into());
    p.notes = Some(NOTES.into());
    p.sort_index = Some(0);
    store.providers.insert(0, p);
    // Do not set current here — Official seed ≠ “currently enabled”.
}

/// Rewrite leftover Grok supplier-name table keys onto the public
/// `chatgpt-tools-proxy` identity. Display names stay on `Provider.name`.
fn migrate_grok_archive_identities(file: &mut ProvidersFile) {
    for provider in file.grok.providers.iter_mut() {
        if provider.is_official() {
            continue;
        }
        let Some(config) = provider
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Some(next) = super::grok::migrate_archive_identity(&config, &provider.name) else {
            continue;
        };
        if let Some(obj) = provider.settings_config.as_object_mut() {
            obj.insert("config".into(), serde_json::Value::String(next));
        }
    }
}

fn seed_grok_official(store: &mut AppProviderStore) {
    const ID: &str = GROK_OFFICIAL_ID;
    const NAME: &str = "Grok Official";
    const WEBSITE: &str = "https://x.ai/grok";
    const NOTES: &str = "Grok Build 官方渠道（默认 grok-4.5）。启用后清除第三方中转，走 grok login 或 XAI_API_KEY；UI / MCP 等本地设置会保留。";

    if let Some(p) = store.providers.iter_mut().find(|p| p.id == ID) {
        p.name = NAME.into();
        p.settings_config = super::grok::official_settings_config();
        p.category = Some("official".into());
        p.website_url = Some(WEBSITE.into());
        p.notes = Some(NOTES.into());
        p.sort_index = Some(0);
        return;
    }

    let mut p = Provider::new(ID.into(), NAME.into(), super::grok::official_settings_config());
    p.category = Some("official".into());
    p.website_url = Some(WEBSITE.into());
    p.notes = Some(NOTES.into());
    p.sort_index = Some(0);
    store.providers.insert(0, p);
    // Do not set current here — Official seed ≠ “currently enabled”.
}

fn reconcile_current_pointers(file: &mut ProvidersFile) {
    reconcile_current_with_live(AppKind::Codex, &mut file.codex);
    reconcile_current_with_live(AppKind::Grok, &mut file.grok);
}

/// Soft-align `current` with live config so the UI never claims Official is
/// “启用中” just because the built-in seed was inserted.
///
/// Rules:
/// - Orphan `current` (missing provider) → clear
/// - Non-official `current` → leave alone (user choice; real drift stays visible)
/// - Empty / Official `current`:
///   - Prefer any archive that already matches live (third-party first)
///   - Else if live is official-shaped or missing → point at Official
///   - Else (third-party live, no matching archive) → clear, unless local
///     routing takeover is on (keep pointer so proxy still has an upstream id)
pub(crate) fn reconcile_current_with_live(kind: AppKind, store: &mut AppProviderStore) {
    if !store.current.is_empty() && store.providers.iter().all(|p| p.id != store.current) {
        store.current.clear();
    }

    let official_id = match kind {
        AppKind::Codex => CODEX_OFFICIAL_ID,
        AppKind::Grok => GROK_OFFICIAL_ID,
    };

    if let Some(cur) = store.providers.iter().find(|p| p.id == store.current) {
        if !cur.is_official() {
            // Explicit third-party / custom selection — never auto-rewrite.
            return;
        }
    }

    // current is empty or Official (or was just cleared as orphan).
    let matching: Vec<(String, bool)> = match kind {
        AppKind::Codex => {
            let snap = super::codex::read_live_snapshot();
            store
                .providers
                .iter()
                .filter(|p| super::codex::matches_live(p, &snap))
                .map(|p| (p.id.clone(), p.is_official()))
                .collect()
        }
        AppKind::Grok => {
            let snap = super::grok::read_live_snapshot();
            store
                .providers
                .iter()
                .filter(|p| super::grok::matches_live(p, &snap))
                .map(|p| (p.id.clone(), p.is_official()))
                .collect()
        }
    };

    if let Some((id, _)) = matching.iter().find(|(_, official)| !*official) {
        store.current = id.clone();
        return;
    }
    if matching.iter().any(|(id, _)| id == official_id) {
        store.current = official_id.into();
        return;
    }

    let live_official_or_missing = match kind {
        AppKind::Codex => {
            if !super::codex::config_path().exists() {
                true
            } else {
                let text = super::codex::read_config_text().unwrap_or_default();
                super::codex::is_official_live_config(&text)
            }
        }
        AppKind::Grok => {
            if !super::grok::config_path().exists() {
                true
            } else {
                let text = super::grok::read_config_text().unwrap_or_default();
                super::grok::is_official_live_config(&text)
            }
        }
    };

    if live_official_or_missing {
        store.current = official_id.into();
        return;
    }

    // Third-party live with no matching archive: do not pretend Official is on.
    if store.takeover_enabled && !store.current.is_empty() {
        // Keep routing pointer; live_status will surface desync / drift separately.
        return;
    }
    store.current.clear();
}

pub fn find_provider<'a>(store: &'a AppProviderStore, id: &str) -> Option<&'a Provider> {
    store.providers.iter().find(|p| p.id == id)
}

pub fn find_provider_mut<'a>(store: &'a mut AppProviderStore, id: &str) -> Option<&'a mut Provider> {
    store.providers.iter_mut().find(|p| p.id == id)
}

pub fn next_id(prefix: &str) -> String {
    let raw = uuid::Uuid::new_v4().to_string();
    let short = raw.split('-').next().unwrap_or(&raw);
    format!("{prefix}-{short}")
}

/// Whether third-party Codex switches should leave `auth.json` OAuth alone.
pub fn preserve_codex_official_auth() -> bool {
    load()
        .map(|f| f.preserve_codex_official_auth)
        .unwrap_or(true)
}

pub fn set_preserve_codex_official_auth(enabled: bool) -> Result<bool, String> {
    let mut file = load()?;
    file.preserve_codex_official_auth = enabled;
    save(&file)?;
    Ok(enabled)
}

pub fn sanitize_id_fragment(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' || ch.is_whitespace() {
            if !out.ends_with('-') {
                out.push('-');
            }
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "provider".into()
    } else {
        trimmed.chars().take(32).collect()
    }
}

pub fn mask_api_key(key: &str) -> Option<String> {
    let t = key.trim();
    if t.is_empty() {
        return None;
    }
    if t.len() <= 8 {
        return Some("••••".into());
    }
    let head: String = t.chars().take(4).collect();
    let tail: String = t.chars().rev().take(4).collect::<String>().chars().rev().collect();
    Some(format!("{head}…{tail}"))
}
