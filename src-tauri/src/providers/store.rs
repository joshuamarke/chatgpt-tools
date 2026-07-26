//! JSON persistence for provider profiles.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use super::models::{AppProviderStore, Provider, ProvidersFile};

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
    let after = serde_json::to_string(&file).unwrap_or_default();
    // Persist refreshed Official defaults so disk never keeps stale empty/proxy shapes.
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
fn ensure_official_seeds(file: &mut ProvidersFile) {
    seed_codex_official(&mut file.codex);
    seed_grok_official(&mut file.grok);
}

fn seed_codex_official(store: &mut AppProviderStore) {
    const ID: &str = "codex-official";
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
    if store.current.is_empty() {
        store.current = ID.into();
    }
}

fn seed_grok_official(store: &mut AppProviderStore) {
    const ID: &str = "grok-official";
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
    if store.current.is_empty() {
        store.current = ID.into();
    }
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
