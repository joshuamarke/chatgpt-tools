use std::collections::BTreeSet;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use serde_json::{json, Map, Value};
use toml_edit::{DocumentMut, Item, Table};

fn atomic_write_config(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid config path"))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.cgt-tmp",
        path.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("config.toml")
    ));
    std::fs::write(&temp, bytes).with_context(|| format!("failed to write {}", temp.display()))?;
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error).with_context(|| format!("failed to replace {}", path.display()))
        }
    }
}

const OPENAI_CURATED_MARKETPLACE: &str = "openai-curated";
const OPENAI_API_CURATED_MARKETPLACE: &str = "openai-api-curated";
const OPENAI_CURATED_REMOTE_MARKETPLACE: &str = "openai-curated-remote";
const OPENAI_PLUGINS_ZIP_URL: &str =
    "https://codeload.github.com/openai/plugins/zip/refs/heads/main";
const OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES: usize = 128 * 1024 * 1024;
/// User-facing network requirement for「修复插件市场」(GitHub codeload).
pub const PLUGIN_MARKETPLACE_NETWORK_HINT: &str =
    "需要能访问 GitHub（codeload.github.com / openai/plugins）。请确认网络可连通 OpenAI 官方相关源，必要时使用代理。";

pub fn ensure_openai_curated_marketplace_config(home: &Path) -> anyhow::Result<bool> {
    let Some(marketplace_root) = local_openai_curated_marketplace_root(home)? else {
        return Ok(false);
    };
    let mut changed = ensure_marketplace_configs(
        home,
        &[OPENAI_CURATED_MARKETPLACE, OPENAI_API_CURATED_MARKETPLACE],
        &marketplace_root,
    )?;
    if let Some(remote_marketplace_root) = local_openai_curated_remote_marketplace_root(home)? {
        changed |= ensure_marketplace_configs(
            home,
            &[OPENAI_CURATED_REMOTE_MARKETPLACE],
            &remote_marketplace_root,
        )?;
    }
    Ok(changed)
}

pub fn ensure_openai_curated_remote_marketplace_config(home: &Path) -> anyhow::Result<bool> {
    let Some(marketplace_root) = local_openai_curated_remote_marketplace_root(home)? else {
        return Ok(false);
    };
    ensure_marketplace_configs(
        home,
        &[OPENAI_CURATED_REMOTE_MARKETPLACE],
        &marketplace_root,
    )
}

pub fn ensure_openai_curated_remote_marketplace_available(
    home: &Path,
) -> anyhow::Result<MarketplaceEnsureResult> {
    if local_openai_curated_remote_marketplace_root(home)?.is_none() {
        return Ok(MarketplaceEnsureResult {
            initialized: false,
            configured: false,
        });
    }
    let configured = ensure_openai_curated_remote_marketplace_config(home)?;
    Ok(MarketplaceEnsureResult {
        initialized: false,
        configured,
    })
}

pub fn openai_curated_marketplace_status(home: &Path) -> MarketplaceStatus {
    let marketplace_root = local_openai_curated_marketplace_root(home).ok().flatten();
    let remote_marketplace_root = local_openai_curated_remote_marketplace_root(home)
        .ok()
        .flatten();
    let config_registered = marketplace_root
        .as_deref()
        .map(|root| {
            marketplace_config_points_to_root(home, OPENAI_CURATED_MARKETPLACE, root)
                && marketplace_config_points_to_root(home, OPENAI_API_CURATED_MARKETPLACE, root)
                && remote_marketplace_root
                    .as_deref()
                    .map(|remote_root| {
                        marketplace_config_points_to_root(
                            home,
                            OPENAI_CURATED_REMOTE_MARKETPLACE,
                            remote_root,
                        )
                    })
                    .unwrap_or(true)
        })
        .unwrap_or(false);
    MarketplaceStatus {
        marketplace_root,
        config_registered,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketplaceStatus {
    pub marketplace_root: Option<PathBuf>,
    pub config_registered: bool,
}

impl MarketplaceStatus {
    pub fn needs_repair(&self) -> bool {
        self.marketplace_root.is_none() || !self.config_registered
    }
}

pub async fn initialize_openai_curated_marketplace_and_configure(
    home: &Path,
) -> anyhow::Result<MarketplaceEnsureResult> {
    let mut initialized = false;
    if local_openai_curated_marketplace_root(home)?.is_none() {
        initialize_openai_curated_marketplace_from_github(home).await?;
        initialized = true;
    }
    let configured = ensure_openai_curated_marketplace_config(home)?;
    Ok(MarketplaceEnsureResult {
        initialized,
        configured,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketplaceEnsureResult {
    pub initialized: bool,
    pub configured: bool,
}

fn local_openai_curated_marketplace_root(home: &Path) -> anyhow::Result<Option<PathBuf>> {
    let root = home.join(".tmp").join("plugins");
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root))
}

fn local_openai_curated_remote_marketplace_root(home: &Path) -> anyhow::Result<Option<PathBuf>> {
    let root = home.join(".tmp").join("plugins-remote");
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_REMOTE_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root))
}

async fn initialize_openai_curated_marketplace_from_github(home: &Path) -> anyhow::Result<()> {
    let bytes = download_openai_plugins_zip().await?;
    install_openai_plugins_zip(home, &bytes)
}

async fn download_openai_plugins_zip() -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(format!("ChatGPTTools/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client for plugin marketplace download")?;
    let bytes = match client
        .get(OPENAI_PLUGINS_ZIP_URL)
        .header(reqwest::header::ACCEPT, "application/zip")
        .send()
        .await
    {
        Ok(resp) => match resp.error_for_status() {
            Ok(ok) => ok
                .bytes()
                .await
                .context("failed to read openai/plugins marketplace download body")?,
            Err(e) => {
                anyhow::bail!(
                    "下载 openai/plugins 失败（HTTP）：{e}。{PLUGIN_MARKETPLACE_NETWORK_HINT}"
                );
            }
        },
        Err(e) => {
            anyhow::bail!(
                "无法连接 GitHub 下载 openai/plugins：{e}。{PLUGIN_MARKETPLACE_NETWORK_HINT}"
            );
        }
    };
    if bytes.len() > OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES {
        anyhow::bail!(
            "openai/plugins marketplace download is too large: {} bytes",
            bytes.len()
        );
    }
    Ok(bytes.to_vec())
}

fn install_openai_plugins_zip(home: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let destination = home.join(".tmp").join("plugins");
    let staging_parent = home.join(".tmp");
    std::fs::create_dir_all(&staging_parent)
        .with_context(|| format!("failed to create {}", staging_parent.display()))?;
    let staging = staging_parent.join(format!(
        "plugins-download-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .with_context(|| format!("failed to remove stale {}", staging.display()))?;
    }
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("failed to create {}", staging.display()))?;

    let result = extract_openai_plugins_zip(bytes, &staging)
        .and_then(|_| validate_openai_plugins_marketplace_root(&staging))
        .and_then(|_| replace_directory(&staging, &destination));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn extract_openai_plugins_zip(bytes: &[u8], destination: &Path) -> anyhow::Result<()> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("failed to read openai/plugins zip")?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry {index}"))?;
        let Some(relative_path) = zip_entry_relative_path(file.name()) else {
            continue;
        };
        let output_path = destination.join(relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path)
                .with_context(|| format!("failed to create {}", output_path.display()))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .with_context(|| format!("failed to read zip entry {}", file.name()))?;
        std::fs::write(&output_path, contents)
            .with_context(|| format!("failed to write {}", output_path.display()))?;
    }
    Ok(())
}

fn zip_entry_relative_path(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    let mut components = path.components();
    match components.next()? {
        Component::Normal(_) => {}
        _ => return None,
    }
    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

fn validate_openai_plugins_marketplace_root(root: &Path) -> anyhow::Result<()> {
    let marketplace = local_openai_curated_marketplace_root_from_root(root)?
        .ok_or_else(|| anyhow::anyhow!("downloaded openai/plugins marketplace is invalid"))?;
    if marketplace != root {
        anyhow::bail!("downloaded openai/plugins marketplace root mismatch");
    }
    Ok(())
}

fn local_openai_curated_marketplace_root_from_root(root: &Path) -> anyhow::Result<Option<PathBuf>> {
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root.to_path_buf()))
}

fn replace_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    replace_directory_with_backup_name(source, destination, "plugins.previous")
}

fn replace_directory_with_backup_name(
    source: &Path,
    destination: &Path,
    backup_name: &str,
) -> anyhow::Result<()> {
    let backup = destination.with_file_name(backup_name);
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .with_context(|| format!("failed to remove {}", backup.display()))?;
    }
    if destination.exists() {
        std::fs::rename(destination, &backup).with_context(|| {
            format!(
                "failed to move {} to {}",
                destination.display(),
                backup.display()
            )
        })?;
    }
    match std::fs::rename(source, destination) {
        Ok(()) => {
            if backup.exists() {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(error) => {
            if backup.exists() {
                let _ = std::fs::rename(&backup, destination);
            }
            Err(error).with_context(|| {
                format!(
                    "failed to move {} to {}",
                    source.display(),
                    destination.display()
                )
            })
        }
    }
}

fn ensure_marketplace_configs(
    home: &Path,
    marketplace_names: &[&str],
    marketplace_root: &Path,
) -> anyhow::Result<bool> {
    let config_path = home.join("config.toml");
    let existing = match std::fs::read(&config_path) {
        Ok(bytes) => String::from_utf8(bytes)
            .with_context(|| format!("failed to read UTF-8 {}", config_path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    let without_bom = existing.trim_start_matches('\u{feff}');
    let updated =
        merge_marketplace_configs_into_text(without_bom, marketplace_names, marketplace_root)?;
    if updated.as_bytes() == without_bom.as_bytes() {
        return Ok(false);
    }
    atomic_write_config(&config_path, updated.as_bytes())?;
    Ok(true)
}

fn merge_marketplace_configs_into_text(
    config_text: &str,
    marketplace_names: &[&str],
    marketplace_root: &Path,
) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(config_text)?;
    let marketplaces = table_mut_or_insert(&mut doc, "marketplaces")?;
    for marketplace_name in marketplace_names {
        if marketplaces
            .get(marketplace_name)
            .and_then(Item::as_table)
            .is_none()
        {
            marketplaces[marketplace_name] = toml_edit::table();
        }
        marketplaces[marketplace_name]["source_type"] = toml_edit::value("local");
        marketplaces[marketplace_name]["source"] =
            toml_edit::value(windows_extended_path(marketplace_root));
    }
    Ok(ensure_trailing_newline(doc.to_string()))
}

fn marketplace_config_points_to_root(home: &Path, marketplace_name: &str, root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(home.join("config.toml")) else {
        return false;
    };
    let Ok(doc) = text.trim_start_matches('\u{feff}').parse::<DocumentMut>() else {
        return false;
    };
    let Some(table) = doc
        .get("marketplaces")
        .and_then(Item::as_table)
        .and_then(|marketplaces| marketplaces.get(marketplace_name))
        .and_then(Item::as_table)
    else {
        return false;
    };
    let source_type = table
        .get("source_type")
        .and_then(Item::as_str)
        .unwrap_or_default();
    let source = table
        .get("source")
        .and_then(Item::as_str)
        .unwrap_or_default();
    source_type == "local" && normalize_windows_extended_path(source) == root.to_string_lossy()
}

fn normalize_windows_extended_path(value: &str) -> String {
    value.strip_prefix(r"\\?\").unwrap_or(value).to_string()
}

fn windows_extended_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if value.starts_with(r"\\?\") {
        value.into_owned()
    } else {
        format!(r"\\?\{value}")
    }
}

fn parse_toml_document(contents: &str) -> anyhow::Result<DocumentMut> {
    let contents = contents.trim_start_matches('\u{feff}');
    if contents.trim().is_empty() {
        Ok(DocumentMut::new())
    } else {
        contents
            .parse::<DocumentMut>()
            .map_err(|error| anyhow::anyhow!("config.toml TOML parse failed: {error}"))
    }
}

fn table_mut_or_insert<'a>(doc: &'a mut DocumentMut, key: &str) -> anyhow::Result<&'a mut Table> {
    if !doc.as_table().contains_key(key) {
        doc[key] = toml_edit::table();
    }
    if doc.get(key).and_then(Item::as_table).is_none() {
        doc[key] = toml_edit::table();
    }
    doc.get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("{key} must be a TOML table"))
}

fn ensure_trailing_newline(mut contents: String) -> String {
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents
}

/// Local marketplace JSON catalogs for renderer inject (third-party merge source).
pub fn local_plugin_marketplaces_json(home: &Path) -> Value {
    let installed = installed_plugins_from_config(home);
    let marketplace_dir = home
        .join(".tmp")
        .join("plugins")
        .join(".agents")
        .join("plugins");
    let candidates = [
        marketplace_dir.join("marketplace.json"),
        marketplace_dir.join("api_marketplace.json"),
        home.join(".tmp")
            .join("plugins-remote")
            .join(".agents")
            .join("plugins")
            .join("marketplace.json"),
    ];
    let marketplaces = candidates
        .iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            let mut marketplace: Value = serde_json::from_str(&text).ok()?;
            expand_local_plugin_marketplace(&mut marketplace, path, home, &installed);
            if let Some(object) = marketplace.as_object_mut() {
                object
                    .entry("path")
                    .or_insert_with(|| Value::String(path.to_string_lossy().to_string()));
            }
            Some(marketplace)
        })
        .collect::<Vec<_>>();
    Value::Array(marketplaces)
}

fn expand_local_plugin_marketplace(
    marketplace: &mut Value,
    marketplace_path: &Path,
    home: &Path,
    installed_plugins: &BTreeSet<String>,
) {
    let marketplace_name = marketplace
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let Some(plugins) = marketplace.get_mut("plugins").and_then(Value::as_array_mut) else {
        return;
    };
    let marketplace_root = marketplace_path
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".tmp").join("plugins"));
    for plugin in plugins {
        let Some(plugin_object) = plugin.as_object_mut() else {
            continue;
        };
        let plugin_name = plugin_object
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                plugin_object
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| id.split('@').next())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if plugin_name.is_empty() {
            continue;
        }
        let manifest_path = marketplace_root
            .join("plugins")
            .join(&plugin_name)
            .join(".codex-plugin")
            .join("plugin.json");
        let plugin_root = marketplace_root.join("plugins").join(&plugin_name);
        if let Some(manifest) = plugin_manifest(&manifest_path) {
            for (key, value) in manifest {
                plugin_object.entry(key).or_insert(value);
            }
        }
        absolutize_plugin_icon_paths(plugin_object, &plugin_root);
        plugin_object
            .entry("name".to_string())
            .or_insert_with(|| Value::String(plugin_name.clone()));
        plugin_object
            .entry("id".to_string())
            .or_insert_with(|| Value::String(format!("{plugin_name}@{marketplace_name}")));
        plugin_object
            .entry("marketplaceName".to_string())
            .or_insert_with(|| Value::String(marketplace_name.clone()));
        plugin_object
            .entry("marketplacePath".to_string())
            .or_insert_with(|| Value::String(marketplace_name.clone()));
        plugin_object
            .entry("keywords".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        plugin_object.insert(
            "installed".to_string(),
            Value::Bool(
                installed_plugins.contains(&format!("{plugin_name}@{marketplace_name}")),
            ),
        );
    }
}

fn absolutize_plugin_icon_paths(plugin: &mut Map<String, Value>, plugin_root: &Path) {
    for key in ["composerIconPath", "logoPath"] {
        absolutize_string_field(plugin, key, plugin_root);
    }
    let Some(interface) = plugin.get_mut("interface").and_then(Value::as_object_mut) else {
        return;
    };
    for key in ["composerIcon", "composerIconUrl", "logo", "logoUrl"] {
        absolutize_string_field(interface, key, plugin_root);
    }
}

fn absolutize_string_field(object: &mut Map<String, Value>, key: &str, root: &Path) {
    let Some(value) = object.get(key).and_then(Value::as_str).map(str::to_string) else {
        return;
    };
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("data:")
        || trimmed.starts_with("http:")
        || trimmed.starts_with("https:")
        || trimmed.starts_with("file:")
        || Path::new(trimmed).is_absolute()
    {
        return;
    }
    let relative = trimmed.strip_prefix("./").unwrap_or(trimmed);
    object.insert(
        key.to_string(),
        Value::String(root.join(relative).to_string_lossy().to_string()),
    );
}

fn plugin_manifest(path: &Path) -> Option<Map<String, Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()?
        .as_object()
        .cloned()
}

fn installed_plugins_from_config(home: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return BTreeSet::new();
    };
    let Some(plugins) = doc.get("plugins").and_then(Item::as_table) else {
        return BTreeSet::new();
    };
    plugins
        .iter()
        .filter_map(|(id, item)| {
            let enabled = item
                .get("enabled")
                .and_then(Item::as_bool)
                .unwrap_or(false);
            enabled.then(|| id.to_string())
        })
        .collect()
}

/// Best-effort when third-party unlock is on: only re-register config if local
/// curated already exists. Never auto-download — user must click「修复插件市场」
/// (GitHub openai/plugins) on a fresh machine.
pub fn ensure_plugin_marketplaces_for_third_party(home: &Path) -> Value {
    let local_cfg = ensure_openai_curated_marketplace_config(home)
        .map(|changed| json!({ "ok": true, "changed": changed }))
        .unwrap_or_else(|e| json!({ "ok": false, "error": e.to_string() }));
    // Legacy .tmp/plugins-remote: re-register only, never invent from built-in zip.
    let remote_cfg = ensure_openai_curated_remote_marketplace_available(home)
        .map(|r| json!({ "ok": true, "configured": r.configured }))
        .unwrap_or_else(|e| json!({ "ok": false, "error": e.to_string() }));
    let status = openai_curated_marketplace_status(home);
    json!({
        "localConfig": local_cfg,
        "remoteConfig": remote_cfg,
        "localNeedsRepair": status.needs_repair(),
        "localRoot": status.marketplace_root.map(|p| p.to_string_lossy().to_string()),
        "networkHint": PLUGIN_MARKETPLACE_NETWORK_HINT,
    })
}
