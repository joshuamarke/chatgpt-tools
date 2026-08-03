//! Single skin **library** model (Scheme B).
//!
//! | Mode | Local list source |
//! |------|-------------------|
//! | Dev (`debug_assertions`, unless forced) | repo `skins/` workspace (live edits) + library-only extras |
//! | Release | `%STATE%/library/<id>/` only; bundled skins are **seeded** once |
//!
//! Install origins share one tree: `seed` | `import` | `cloud` | `design`.
//! Legacy `state/skins` and `state/cache/skins` are migrated into `library/` once.

use crate::engine::{self, EngineError};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::host;

/// Unified install metadata filename under each library skin dir.
pub const INSTALL_META_FILE: &str = ".install-meta.json";

pub fn safe_skin_id(id: &str) -> String {
    let s: String = id
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = s.trim_matches('-');
    trimmed.chars().take(64).collect()
}

static MIGRATION_DONE: AtomicBool = AtomicBool::new(false);

pub fn state_root() -> PathBuf {
    host::state_root()
}

/// Unique install root for all non-workspace skins.
pub fn library_dir() -> PathBuf {
    state_root().join("library")
}

/// Compile-time repo root (`src-tauri` → parent). Used so dev workspace always
/// sees the full `skins/` tree even if runtime `project_root` was rebound.
fn compile_time_repo_skins_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("skins"))
        .unwrap_or_else(|| PathBuf::from("skins"))
}

/// Seed source (release) / authoring workspace (dev): `…/skins`.
///
/// In workspace mode prefer the **repo** `skins/` directory (all author skins).
/// `project_root()/skins` may be the staged install set under resource_dir.
pub fn bundled_skins_dir() -> PathBuf {
    if is_dev_workspace() {
        let repo = compile_time_repo_skins_dir();
        if repo.is_dir() {
            return repo;
        }
    }
    engine::project_root().join("skins")
}

/// Dev workspace mode: GUI lists + applies repo `skins/` directly (instant authoring).
///
/// Override:
/// - `CODEX_SKIN_LIBRARY_MODE=workspace|dev` → force workspace
/// - `CODEX_SKIN_LIBRARY_MODE=library|prod` → force library+seed (even in debug)
pub fn is_dev_workspace() -> bool {
    if let Ok(mode) = std::env::var("CODEX_SKIN_LIBRARY_MODE") {
        let m = mode.trim().to_ascii_lowercase();
        if m == "workspace" || m == "dev" {
            return true;
        }
        if m == "library" || m == "prod" || m == "release" {
            return false;
        }
    }
    cfg!(debug_assertions)
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn version_cmp(a: &str, b: &str) -> i32 {
    let parse = |s: &str| -> Vec<u64> {
        s.trim()
            .trim_start_matches('v')
            .split(|c| c == '.' || c == '-' || c == '+')
            .take(3)
            .map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
            })
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let pa = parse(a);
    let pb = parse(b);
    for i in 0..3 {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x < y {
            return -1;
        }
        if x > y {
            return 1;
        }
    }
    0
}

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), EngineError> {
    fs::create_dir_all(dst)
        .map_err(|e| EngineError::msg(format!("mkdir {}: {e}", dst.display())))?;
    for ent in fs::read_dir(src).map_err(|e| EngineError::msg(format!("read_dir: {e}")))? {
        let ent = ent.map_err(|e| EngineError::msg(e.to_string()))?;
        let name = ent.file_name();
        let name_str = name.to_string_lossy();
        // Skip VCS / staging noise; keep install meta when copying installs.
        if name_str == "." || name_str == ".." {
            continue;
        }
        let from = ent.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .map_err(|e| EngineError::msg(format!("copy {}: {e}", from.display())))?;
        }
    }
    Ok(())
}

pub fn rm_dir_recursive(path: &Path) -> Result<(), EngineError> {
    if path.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| EngineError::msg(format!("remove {}: {e}", path.display())))?;
    }
    Ok(())
}

/// Ensure library dir exists; migrate legacy roots; seed bundled in release mode.
pub fn ensure_library() -> Result<(), EngineError> {
    fs::create_dir_all(library_dir())
        .map_err(|e| EngineError::msg(format!("create library: {e}")))?;
    if !MIGRATION_DONE.swap(true, Ordering::SeqCst) {
        migrate_legacy_into_library();
    }
    if !is_dev_workspace() {
        seed_bundled_into_library()?;
    }
    Ok(())
}

fn read_json(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

pub fn read_install_meta(skin_dir: &Path) -> Option<Value> {
    read_json(&skin_dir.join(INSTALL_META_FILE))
        // Legacy filenames from pre-library layout
        .or_else(|| read_json(&skin_dir.join(".cache-meta.json")))
        .or_else(|| read_json(&skin_dir.join(".import-meta.json")))
}

pub fn write_install_meta(skin_dir: &Path, meta: &Value) -> Result<(), EngineError> {
    let text = serde_json::to_string_pretty(meta)
        .map_err(|e| EngineError::msg(format!("serialize install meta: {e}")))?;
    fs::write(skin_dir.join(INSTALL_META_FILE), format!("{text}\n"))
        .map_err(|e| EngineError::msg(format!("write install meta: {e}")))?;
    // Drop legacy sidecars so one meta file remains authoritative.
    let _ = fs::remove_file(skin_dir.join(".cache-meta.json"));
    let _ = fs::remove_file(skin_dir.join(".import-meta.json"));
    Ok(())
}

/// Map install origin → GUI `source` string (keeps existing labels working).
fn origin_to_source(origin: &str) -> (&'static str, bool) {
    match origin {
        "seed" | "workspace" | "bundled" => ("bundled", true),
        "cloud" | "cache" => ("cache", false),
        "import" | "user" | "design" => ("user", false),
        _ => ("user", false),
    }
}

fn infer_origin_from_dir(dir: &Path, fallback: &str) -> String {
    if let Some(meta) = read_install_meta(dir) {
        if let Some(o) = meta.get("origin").and_then(|v| v.as_str()) {
            if !o.is_empty() {
                return o.to_string();
            }
        }
        // Legacy cache meta always had sha256/version from CDN.
        if meta.get("sha256").is_some() && meta.get("downloadedAt").is_some() {
            return "cloud".into();
        }
        if meta.get("importedAt").is_some() || meta.get("from").is_some() {
            return "import".into();
        }
    }
    fallback.to_string()
}

fn read_skin_card(dir: &Path, origin: &str) -> Option<Value> {
    let manifest_path = dir.join("skin.json");
    if !manifest_path.is_file() {
        return None;
    }
    let text = fs::read_to_string(&manifest_path).ok()?;
    let mut manifest: Value = serde_json::from_str(&text).ok()?;
    if manifest.get("id").and_then(|v| v.as_str()).is_none() {
        if let Some(obj) = manifest.as_object_mut() {
            obj.insert(
                "id".into(),
                json!(dir
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()),
            );
        }
    }
    let (source, builtin) = origin_to_source(origin);
    let meta = read_install_meta(dir);
    if let Some(obj) = manifest.as_object_mut() {
        obj.insert("dir".into(), json!(dir.to_string_lossy()));
        obj.insert("source".into(), json!(source));
        obj.insert("origin".into(), json!(origin));
        obj.insert("builtin".into(), json!(builtin));
        obj.insert("installState".into(), json!("ready"));
        if let Some(m) = meta {
            if let Some(v) = m.get("version").cloned() {
                obj.insert("cacheVersion".into(), v.clone());
                // Prefer package version from meta when skin.json is stale after cloud update.
                if origin == "cloud" {
                    if obj.get("version").and_then(|x| x.as_str()).unwrap_or("").is_empty() {
                        obj.insert("version".into(), v);
                    }
                }
            }
            if let Some(v) = m.get("sha256").cloned() {
                obj.insert("cacheSha256".into(), v);
            }
        }
    }
    Some(manifest)
}

fn scan_skin_root(root: &Path, default_origin: &str, map: &mut BTreeMap<String, Value>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }
        let origin = infer_origin_from_dir(&path, default_origin);
        if let Some(skin) = read_skin_card(&path, &origin) {
            if let Some(id) = skin.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()) {
                if !id.is_empty() {
                    map.insert(id, skin);
                }
            }
        }
    }
}

/// List skins for GUI / apply.
///
/// Dev workspace: repo `skins/` wins for same id (live authoring).  
/// Library-only ids (imports during dev) still appear.  
/// Release: library only (after seed).
pub fn list_skins() -> Vec<Value> {
    let _ = ensure_library();
    let mut map: BTreeMap<String, Value> = BTreeMap::new();

    if is_dev_workspace() {
        // Live workspace first — never shadowed by AppData.
        scan_skin_root(&bundled_skins_dir(), "workspace", &mut map);
        // Extra installs (import/design/cloud while testing) that are not in repo.
        let workspace_ids: std::collections::HashSet<String> = map.keys().cloned().collect();
        let mut lib_map = BTreeMap::new();
        scan_skin_root(&library_dir(), "import", &mut lib_map);
        for (id, skin) in lib_map {
            if !workspace_ids.contains(&id) {
                map.insert(id, skin);
            }
        }
    } else {
        scan_skin_root(&library_dir(), "import", &mut map);
    }

    let mut skins: Vec<_> = map.into_values().collect();
    skins.sort_by(|a, b| {
        let na = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let nb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        na.cmp(nb)
    });
    skins
}

pub fn get_skin(skin_id: &str) -> Result<Value, EngineError> {
    list_skins()
        .into_iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(skin_id))
        .ok_or_else(|| EngineError::msg(format!("Skin not found: {skin_id}")))
}

/// Apply uses the resolved dir in place — no runtime-skins mirror.
/// Validates required assets; refreshes are immediate when files change on disk.
pub fn materialize_skin(skin: &Value) -> Result<Value, EngineError> {
    let dir = PathBuf::from(
        skin.get("dir")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::msg("skin missing dir"))?,
    );
    if !dir.is_dir() {
        return Err(EngineError::msg(format!(
            "皮肤目录不存在: {}",
            dir.display()
        )));
    }
    let manifest: Value = fs::read_to_string(dir.join("skin.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({}));
    for key in ["css", "plugin"] {
        let rel = manifest
            .pointer(&format!("/assets/{key}"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if rel.is_empty() || !dir.join(rel).is_file() {
            return Err(EngineError::msg(format!(
                "皮肤资源缺失: assets.{key} ({rel})"
            )));
        }
    }
    // art optional when art.mode=none
    let art_mode = manifest
        .pointer("/art/mode")
        .and_then(|v| v.as_str())
        .unwrap_or("wallpaper");
    if art_mode != "none" {
        let rel = manifest
            .pointer("/assets/art")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !rel.is_empty() && !dir.join(rel).is_file() {
            return Err(EngineError::msg(format!("皮肤立绘缺失: {rel}")));
        }
    }
    Ok(skin.clone())
}

/// Install (or replace) a skin tree into the library.
pub fn install_skin_tree(
    src: &Path,
    id: &str,
    origin: &str,
    extra_meta: Value,
) -> Result<PathBuf, EngineError> {
    let safe = safe_skin_id(id);
    if safe.is_empty() {
        return Err(EngineError::msg("皮肤 id 不合法"));
    }
    // Only mkdir — do not call ensure_library() (seed uses this and would recurse).
    fs::create_dir_all(library_dir())
        .map_err(|e| EngineError::msg(format!("create library: {e}")))?;
    let target = library_dir().join(&safe);
    let staging = library_dir().join(format!(".staging-{safe}-{}", now_unix_ms()));
    let _ = rm_dir_recursive(&staging);
    copy_dir_recursive(src, &staging)?;

    let mut meta = extra_meta;
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("origin".into(), json!(origin));
        obj.insert("installedAt".into(), json!(now_unix_ms().to_string()));
        obj.entry("id".to_string())
            .or_insert_with(|| json!(safe.clone()));
    } else {
        meta = json!({
            "origin": origin,
            "installedAt": now_unix_ms().to_string(),
            "id": safe,
        });
    }
    write_install_meta(&staging, &meta)?;

    if target.is_dir() {
        let bak = library_dir().join(format!(".bak-{safe}-{}", now_unix_ms()));
        let _ = fs::rename(&target, &bak);
        if let Err(e) = fs::rename(&staging, &target) {
            let _ = fs::rename(&bak, &target);
            let _ = rm_dir_recursive(&staging);
            return Err(EngineError::msg(format!("安装到 library 失败: {e}")));
        }
        let _ = rm_dir_recursive(&bak);
    } else if let Err(e) = fs::rename(&staging, &target) {
        let _ = rm_dir_recursive(&staging);
        return Err(EngineError::msg(format!("安装到 library 失败: {e}")));
    }

    // Drop legacy parallel copies for this id (post-library world).
    let _ = rm_dir_recursive(&state_root().join("skins").join(&safe));
    let _ = rm_dir_recursive(&state_root().join("cache").join("skins").join(&safe));
    let _ = rm_dir_recursive(&state_root().join("runtime-skins").join(&safe));

    Ok(target)
}

fn bundled_has_id(id: &str) -> bool {
    let root = bundled_skins_dir();
    if root.join(id).join("skin.json").is_file() {
        return true;
    }
    // id may differ from directory name — rare; best-effort scan not needed for migrate.
    false
}

fn migrate_one_root(legacy_root: &Path, default_origin: &str) {
    let Ok(entries) = fs::read_dir(legacy_root) else {
        return;
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if !path.join("skin.json").is_file() {
            continue;
        }
        let has_import_meta = path.join(".import-meta.json").is_file()
            || read_json(&path.join(INSTALL_META_FILE))
                .and_then(|m| m.get("origin").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .map(|o| o == "import" || o == "design" || o == "user")
                .unwrap_or(false);
        let has_cloud_meta = path.join(".cache-meta.json").is_file()
            || read_json(&path.join(INSTALL_META_FILE))
                .and_then(|m| m.get("origin").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .map(|o| o == "cloud" || o == "cache")
                .unwrap_or(false);

        let id = read_json(&path.join("skin.json"))
            .and_then(|m| {
                m.get("id")
                    .and_then(|v| v.as_str())
                    .map(safe_skin_id)
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| safe_skin_id(&name));
        if id.is_empty() {
            continue;
        }

        // Drop bare copies of bundled skins (common after manual mirror / old tools).
        // Real imports have .import-meta; cloud installs have .cache-meta.
        // Release will re-seed from package; dev uses workspace directly.
        if !has_import_meta && !has_cloud_meta && bundled_has_id(&id) {
            let _ = fs::remove_dir_all(&path);
            continue;
        }

        let origin = if has_cloud_meta {
            "cloud".to_string()
        } else if has_import_meta {
            infer_origin_from_dir(&path, "import")
        } else {
            infer_origin_from_dir(&path, default_origin)
        };

        let dest = library_dir().join(&id);
        if dest.join("skin.json").is_file() {
            // Already in library — remove legacy duplicate.
            let _ = fs::remove_dir_all(&path);
            continue;
        }
        // Prefer rename (same volume); fall back to copy.
        if fs::rename(&path, &dest).is_err() {
            if copy_dir_recursive(&path, &dest).is_ok() {
                let _ = fs::remove_dir_all(&path);
            } else {
                continue;
            }
        }
        let mut meta = read_install_meta(&dest).unwrap_or_else(|| json!({}));
        if let Some(obj) = meta.as_object_mut() {
            if !obj.contains_key("origin") {
                obj.insert("origin".into(), json!(origin));
            }
            obj.entry("migratedFrom".to_string())
                .or_insert_with(|| json!(legacy_root.to_string_lossy()));
        }
        let _ = write_install_meta(&dest, &meta);
    }
}

fn migrate_legacy_into_library() {
    // user overrides first (historical highest priority), then cloud cache.
    migrate_one_root(&state_root().join("skins"), "import");
    migrate_one_root(&state_root().join("cache").join("skins"), "cloud");
}

fn seed_bundled_into_library() -> Result<(), EngineError> {
    let root = bundled_skins_dir();
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(());
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }
        if !path.join("skin.json").is_file() {
            continue;
        }
        let bundled_manifest = read_json(&path.join("skin.json")).unwrap_or(json!({}));
        let id = bundled_manifest
            .get("id")
            .and_then(|v| v.as_str())
            .map(safe_skin_id)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| safe_skin_id(&name));
        if id.is_empty() {
            continue;
        }
        let bundled_ver = bundled_manifest
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0");
        let dest = library_dir().join(&id);
        if dest.join("skin.json").is_file() {
            let meta = read_install_meta(&dest).unwrap_or(json!({}));
            let origin = meta
                .get("origin")
                .and_then(|v| v.as_str())
                .unwrap_or("import");
            // Only refresh pure seeds — never clobber user/cloud/design installs.
            if origin != "seed" && origin != "bundled" {
                continue;
            }
            let local_ver = read_json(&dest.join("skin.json"))
                .and_then(|m| m.get("version").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| "0".into());
            if version_cmp(bundled_ver, &local_ver) <= 0 {
                continue;
            }
        }
        let meta = json!({
            "origin": "seed",
            "version": bundled_ver,
            "seededFrom": path.to_string_lossy(),
        });
        let _ = install_skin_tree(&path, &id, "seed", meta);
    }
    Ok(())
}

/// Delete an installed library skin (not workspace/bundled seed cards in dev).
pub fn delete_skin(skin_id: &str) -> Result<Value, EngineError> {
    let skin = get_skin(skin_id)?;
    let source = skin.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let origin = skin
        .get("origin")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let builtin = skin
        .get("builtin")
        .and_then(|v| v.as_bool())
        .unwrap_or(source == "bundled");

    if source == "remote" {
        return Err(EngineError::msg("云端皮肤尚未下载，无需删除"));
    }

    // Dev workspace skins live in the repo — not deletable via GUI.
    if origin == "workspace" || (is_dev_workspace() && builtin && source == "bundled") {
        let lib = library_dir().join(safe_skin_id(skin_id));
        // If a library shadow somehow existed, remove it (list already prefers workspace).
        if lib.is_dir() {
            rm_dir_recursive(&lib)?;
            return Ok(json!({
                "ok": true,
                "skinId": skin_id,
                "removed": "library-shadow",
                "engine": "native-rust",
            }));
        }
        return Err(EngineError::msg("开发工作区皮肤不能删除，请直接编辑仓库 skins/"));
    }

    if builtin || origin == "seed" || source == "bundled" {
        return Err(EngineError::msg("内置皮肤不能删除，只能导出"));
    }

    let safe = safe_skin_id(skin_id);
    let lib = library_dir().join(&safe);
    if !lib.is_dir() {
        return Err(EngineError::msg("未找到可删除的已安装皮肤"));
    }
    rm_dir_recursive(&lib)?;
    let _ = rm_dir_recursive(&state_root().join("runtime-skins").join(&safe));
    Ok(json!({
        "ok": true,
        "skinId": skin_id,
        "removed": origin,
        "engine": "native-rust",
    }))
}

/// Library entries with cloud origin (and legacy cache meta).
pub fn list_cloud_installed_skins() -> Vec<Value> {
    let _ = ensure_library();
    list_skins()
        .into_iter()
        .filter(|s| {
            let o = s.get("origin").and_then(|v| v.as_str()).unwrap_or("");
            let src = s.get("source").and_then(|v| v.as_str()).unwrap_or("");
            o == "cloud" || src == "cache"
        })
        .collect()
}

/// Remove one cloud-origin skin, or all cloud-origin skins when `skin_id` is None.
pub fn clear_cloud_skins(skin_id: Option<&str>) -> Result<Value, EngineError> {
    ensure_library()?;
    if let Some(id) = skin_id {
        let safe = safe_skin_id(id);
        if safe.is_empty() {
            return Err(EngineError::msg("无效皮肤 id"));
        }
        let dir = library_dir().join(&safe);
        if !dir.is_dir() {
            return Ok(json!({ "ok": true, "removed": [] }));
        }
        let origin = infer_origin_from_dir(&dir, "import");
        if origin != "cloud" && origin != "cache" {
            return Err(EngineError::msg("该皮肤不是云端安装，无法用缓存清理删除"));
        }
        rm_dir_recursive(&dir)?;
        return Ok(json!({ "ok": true, "removed": [safe] }));
    }
    let mut removed = Vec::new();
    if let Ok(entries) = fs::read_dir(library_dir()) {
        for ent in entries.flatten() {
            let p = ent.path();
            if !p.is_dir() {
                continue;
            }
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            let origin = infer_origin_from_dir(&p, "import");
            if origin == "cloud" || origin == "cache" {
                let _ = rm_dir_recursive(&p);
                removed.push(name);
            }
        }
    }
    Ok(json!({ "ok": true, "removed": removed }))
}

/// Read cloud install meta for download cache-hit checks.
pub fn read_library_cloud_meta(skin_id: &str) -> Option<Value> {
    let safe = safe_skin_id(skin_id);
    if safe.is_empty() {
        return None;
    }
    let dir = library_dir().join(&safe);
    if !dir.join("skin.json").is_file() {
        return None;
    }
    let meta = read_install_meta(&dir)?;
    let origin = meta
        .get("origin")
        .and_then(|v| v.as_str())
        .unwrap_or("import");
    if origin == "cloud" || origin == "cache" || meta.get("sha256").is_some() {
        Some(meta)
    } else {
        None
    }
}

pub fn library_skin_dir(skin_id: &str) -> PathBuf {
    library_dir().join(safe_skin_id(skin_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_id_sanitizes() {
        assert_eq!(safe_skin_id("Jiuyi"), "jiuyi");
        assert_eq!(safe_skin_id("a b/c"), "a-b-c");
    }

    #[test]
    fn origin_source_mapping() {
        assert_eq!(origin_to_source("seed"), ("bundled", true));
        assert_eq!(origin_to_source("workspace"), ("bundled", true));
        assert_eq!(origin_to_source("cloud"), ("cache", false));
        assert_eq!(origin_to_source("import"), ("user", false));
        assert_eq!(origin_to_source("design"), ("user", false));
    }

    #[test]
    fn version_order() {
        assert!(version_cmp("2.0.0", "1.9.9") > 0);
        assert!(version_cmp("1.0.0", "1.0.0") == 0);
        assert!(version_cmp("1.2.0", "1.10.0") < 0);
    }

    #[test]
    fn workspace_skins_dir_is_full_repo_tree() {
        // debug unit tests → workspace mode; must not resolve to staged
        // bundle-resources/skins (often only qingkong).
        if !is_dev_workspace() {
            return;
        }
        let dir = bundled_skins_dir();
        assert!(
            dir.join("jiuyi").join("skin.json").is_file(),
            "expected repo skins/jiuyi under {}, got missing — GUI would hide author skins",
            dir.display()
        );
        assert!(
            dir.join("dream").join("skin.json").is_file(),
            "expected repo skins/dream under {}",
            dir.display()
        );
        // Must not be the install-only stage tree
        let s = dir.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
        assert!(
            !s.contains("bundle-resources"),
            "workspace skins resolved to staged bundle path: {s}"
        );
    }
}
