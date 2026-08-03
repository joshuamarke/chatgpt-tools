//! Native skin package import / export / inspect (parity with manager.js + adm-zip).

use super::image::MAX_ART_BYTES;
use super::library::{self, install_skin_tree};
use super::native::{
    ensure_state_dir, get_skin, list_skins, safe_skin_id_pub, state_root, ENGINE_PROTOCOL,
    ENGINE_VERSION,
};
use crate::engine::EngineError;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const SKIN_PACKAGE_VERSION: u32 = 1;

fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn sha256_file(path: &Path) -> Result<String, EngineError> {
    let bytes = fs::read(path).map_err(|e| EngineError::msg(format!("read {}: {e}", path.display())))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

fn rm_dir_recursive(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

/// Validate v2 skin package layout (css + art + plugin + markers).
pub fn validate_skin_manifest(manifest: &Value, skin_dir: &Path) -> Result<(), EngineError> {
    if !manifest.is_object() {
        return Err(EngineError::msg("skin.json 无效"));
    }
    if manifest.get("id").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
        return Err(EngineError::msg("skin.json 缺少 id"));
    }
    if manifest.get("name").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
        return Err(EngineError::msg("skin.json 缺少 name"));
    }
    let css = manifest
        .pointer("/assets/css")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let art = manifest
        .pointer("/assets/art")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let plugin = manifest
        .pointer("/assets/plugin")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if css.is_empty() {
        return Err(EngineError::msg("skin.json 缺少 assets.css"));
    }
    if plugin.is_empty() {
        return Err(EngineError::msg(
            "skin.json 需要 assets.plugin（共享 runtime，不再使用 assets.inject）",
        ));
    }
    // art.mode=none → pure style skin, assets.art optional
    let art_mode = manifest
        .pointer("/art/mode")
        .and_then(|v| v.as_str())
        .or_else(|| manifest.pointer("/theme/art/mode").and_then(|v| v.as_str()))
        .unwrap_or("wallpaper")
        .trim()
        .to_ascii_lowercase();
    let needs_art = art_mode != "none";
    if needs_art && art.is_empty() {
        return Err(EngineError::msg(
            "skin.json 缺少 assets.art（纯样式皮肤请设 art.mode 为 \"none\"）",
        ));
    }
    for key in ["rootClass", "styleId", "stateKey"] {
        if manifest
            .pointer(&format!("/markers/{key}"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            return Err(EngineError::msg("skin.json 缺少 markers 字段"));
        }
    }
    let mut required: Vec<&str> = vec![css, plugin];
    if needs_art {
        required.push(art);
    }
    for rel in required {
        if !skin_dir.join(rel).is_file() {
            return Err(EngineError::msg(format!("缺少资源文件：{rel}")));
        }
    }
    let plugin_text = fs::read_to_string(skin_dir.join(plugin))
        .map_err(|e| EngineError::msg(format!("plugin.json 无效：{e}")))?;
    let plugin_json: Value = serde_json::from_str(&plugin_text)
        .map_err(|e| EngineError::msg(format!("plugin.json 无效：{e}")))?;
    if !plugin_json
        .get("chromeHtml")
        .and_then(|v| v.as_str())
        .is_some()
    {
        return Err(EngineError::msg("plugin.json 需要 chromeHtml 字符串"));
    }
    if needs_art {
        let art_meta = fs::metadata(skin_dir.join(art))
            .map_err(|e| EngineError::msg(format!("立绘不可读：{e}")))?;
        if art_meta.len() < 1 {
            return Err(EngineError::msg("立绘文件为空"));
        }
        if art_meta.len() > MAX_ART_BYTES {
            return Err(EngineError::msg(format!(
                "立绘超过 {} MB 注入上限；请使用 ≤ {} MB 的 PNG/JPEG/WebP（上限内支持高质量原图）",
                MAX_ART_BYTES / 1024 / 1024,
                MAX_ART_BYTES / 1024 / 1024
            )));
        }
    }
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut ZipWriter<File>,
    dir: &Path,
    zip_prefix: &str,
    options: SimpleFileOptions,
) -> Result<(), EngineError> {
    for ent in fs::read_dir(dir).map_err(|e| EngineError::msg(e.to_string()))? {
        let ent = ent.map_err(|e| EngineError::msg(e.to_string()))?;
        let full = ent.path();
        let name = ent.file_name().to_string_lossy().to_string();
        let zpath = if zip_prefix.is_empty() {
            name.clone()
        } else {
            format!("{zip_prefix}/{name}")
        };
        if full.is_dir() {
            add_dir_to_zip(zip, &full, &zpath, options)?;
        } else {
            zip.start_file(zpath.replace('\\', "/"), options)
                .map_err(|e| EngineError::msg(format!("zip start_file: {e}")))?;
            let bytes = fs::read(&full)
                .map_err(|e| EngineError::msg(format!("read {}: {e}", full.display())))?;
            zip.write_all(&bytes)
                .map_err(|e| EngineError::msg(format!("zip write: {e}")))?;
        }
    }
    Ok(())
}

/// Export skin directory to `.skin` / `.zip` with `skin/` root + package.json meta.
/// Legacy `.cgskin` extension is still accepted as an output path.
pub fn export_skin_native(skin_id: &str, output_path: &str) -> Result<Value, EngineError> {
    let skin = get_skin(skin_id)?;
    let dir = PathBuf::from(
        skin.get("dir")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::msg("skin missing dir"))?,
    );
    let manifest: Value = fs::read_to_string(dir.join("skin.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| skin.clone());
    validate_skin_manifest(&manifest, &dir)?;

    let mut out = output_path.to_string();
    let lower = out.to_ascii_lowercase();
    if !lower.ends_with(".skin") && !lower.ends_with(".zip") && !lower.ends_with(".cgskin") {
        out.push_str(".skin");
    }
    if let Some(parent) = Path::new(&out).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| EngineError::msg(format!("mkdir export parent: {e}")))?;
        }
    }

    let file = File::create(&out).map_err(|e| EngineError::msg(format!("create {out}: {e}")))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    add_dir_to_zip(&mut zip, &dir, "skin", options)?;

    let meta = json!({
        "format": "chatgpt-skin",
        "version": SKIN_PACKAGE_VERSION,
        "exportedAt": now_iso(),
        "skinId": skin.get("id").and_then(|v| v.as_str()).unwrap_or(skin_id),
        "skinName": skin.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        "engineVersion": ENGINE_VERSION,
        "protocol": ENGINE_PROTOCOL,
    });
    zip.start_file("package.json", options)
        .map_err(|e| EngineError::msg(format!("zip package.json: {e}")))?;
    zip.write_all(serde_json::to_string_pretty(&meta).unwrap_or_default().as_bytes())
        .map_err(|e| EngineError::msg(format!("zip write meta: {e}")))?;
    zip.finish()
        .map_err(|e| EngineError::msg(format!("zip finish: {e}")))?;

    Ok(json!({
        "ok": true,
        "path": out,
        "skinId": skin.get("id").and_then(|v| v.as_str()).unwrap_or(skin_id),
        "name": skin.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        "engine": "native-rust",
    }))
}

pub fn extract_zip_to_pub(package_path: &Path, tmp_root: &Path) -> Result<Vec<String>, EngineError> {
    extract_zip_to(package_path, tmp_root)
}

pub fn resolve_skin_dir_from_extracted_pub(tmp_root: &Path) -> Result<PathBuf, EngineError> {
    resolve_skin_dir_from_extracted(tmp_root)
}

fn extract_zip_to(package_path: &Path, tmp_root: &Path) -> Result<Vec<String>, EngineError> {
    let file =
        File::open(package_path).map_err(|e| EngineError::msg(format!("open package: {e}")))?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| EngineError::msg(format!("zip open: {e}")))?;
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| EngineError::msg(format!("zip entry: {e}")))?;
        let name = entry.name().replace('\\', "/").to_string();
        // Zip-slip guard
        if name.contains("..") {
            return Err(EngineError::msg(format!("拒绝不安全的 zip 路径: {name}")));
        }
        let out_path = tmp_root.join(&name);
        if entry.is_dir() || name.ends_with('/') {
            fs::create_dir_all(&out_path).ok();
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let mut outfile = File::create(&out_path)
            .map_err(|e| EngineError::msg(format!("extract create: {e}")))?;
        std::io::copy(&mut entry, &mut outfile)
            .map_err(|e| EngineError::msg(format!("extract copy: {e}")))?;
        names.push(name);
    }
    if names.is_empty() {
        return Err(EngineError::msg("皮肤包是空的"));
    }
    Ok(names)
}

fn resolve_skin_dir_from_extracted(tmp_root: &Path) -> Result<PathBuf, EngineError> {
    let skin_dir = tmp_root.join("skin");
    if skin_dir.join("skin.json").is_file() {
        return Ok(skin_dir);
    }
    if tmp_root.join("skin.json").is_file() {
        return Ok(tmp_root.to_path_buf());
    }
    if let Ok(entries) = fs::read_dir(tmp_root) {
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_dir() && p.join("skin.json").is_file() {
                return Ok(p);
            }
        }
    }
    Err(EngineError::msg("皮肤包中未找到 skin.json"))
}

fn scan_risks(inject_code: &str, leftover_inject: bool, art_bytes: u64) -> Vec<String> {
    let mut risks = Vec::new();
    let lower = inject_code.to_ascii_lowercase();
    if regex_simple_any(
        &lower,
        &[
            "fetch(",
            "xmlhttprequest",
            "websocket",
            "navigator.sendbeacon",
        ],
    ) {
        risks.push("装饰层可能发起网络请求".into());
    }
    if regex_simple_any(
        &lower,
        &["localstorage", "indexeddb", "document.cookie", "sessionstorage"],
    ) {
        risks.push("装饰层可能读写本地存储".into());
    }
    if lower.contains("eval(")
        || lower.contains("new function")
        || lower.contains("function(\"return")
        || lower.contains("function('return")
    {
        risks.push("包含动态执行代码（eval 等）".into());
    }
    if inject_code.contains("child_process")
        || inject_code.contains("require(")
        || inject_code.contains("process.")
        || inject_code.contains("fs.")
        || inject_code.contains("nw.")
        || inject_code.contains("electron")
    {
        risks.push("疑似尝试访问系统能力".into());
    }
    if leftover_inject {
        risks.push("包内仍含 renderer-inject.js（引擎 v2 已忽略；建议删除）".into());
    }
    if art_bytes > MAX_ART_BYTES {
        risks.push(format!(
            "立绘超过 {} MB 注入上限",
            MAX_ART_BYTES / 1024 / 1024
        ));
    } else if art_bytes > 8 * 1024 * 1024 {
        risks.push(
            "立绘较大（>8MB）：引擎支持高质量原图，但 shell 后贴图会更慢；请为列表提供 assets/screenshot"
                .into(),
        );
    }
    if risks.is_empty() {
        risks.push("未发现明显高危模式（不能保证绝对安全）".into());
    }
    risks
}

fn regex_simple_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

/// Inspect package without installing.
pub fn inspect_skin_native(package_path: &str) -> Result<Value, EngineError> {
    let package = PathBuf::from(package_path);
    if !package.is_file() {
        return Err(EngineError::msg("找不到皮肤包文件"));
    }
    ensure_state_dir()?;
    let tmp_root = state_root().join(format!(
        ".inspect-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    rm_dir_recursive(&tmp_root);
    fs::create_dir_all(&tmp_root).map_err(|e| EngineError::msg(e.to_string()))?;

    let result = (|| {
        let names = extract_zip_to(&package, &tmp_root)?;
        let skin_dir = resolve_skin_dir_from_extracted(&tmp_root)?;
        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(skin_dir.join("skin.json"))
                .map_err(|e| EngineError::msg(format!("skin.json: {e}")))?,
        )
        .map_err(|e| EngineError::msg(format!("skin.json parse: {e}")))?;
        validate_skin_manifest(&manifest, &skin_dir)?;

        let css_rel = manifest.pointer("/assets/css").and_then(|v| v.as_str()).unwrap_or("");
        let art_rel = manifest.pointer("/assets/art").and_then(|v| v.as_str()).unwrap_or("");
        let plugin_rel = manifest
            .pointer("/assets/plugin")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let css_path = skin_dir.join(css_rel);
        let art_path = skin_dir.join(art_rel);
        let plugin_path = skin_dir.join(plugin_rel);
        let leftover = skin_dir.join("assets").join("renderer-inject.js");

        let mut scan_parts = String::new();
        if plugin_path.is_file() {
            if let Ok(t) = fs::read_to_string(&plugin_path) {
                scan_parts.push_str(&t);
            }
        }
        if leftover.is_file() {
            if let Ok(t) = fs::read_to_string(&leftover) {
                scan_parts.push('\n');
                scan_parts.push_str(&t);
            }
        }
        let art_bytes = art_path.metadata().map(|m| m.len()).unwrap_or(0);
        let risks = scan_risks(&scan_parts, leftover.is_file(), art_bytes);
        let id = safe_skin_id_pub(
            manifest.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        );

        Ok(json!({
            "ok": true,
            "path": package_path,
            "fileName": package.file_name().and_then(|s| s.to_str()).unwrap_or(""),
            "skinId": id,
            "name": manifest.get("name").and_then(|v| v.as_str()).unwrap_or(&id),
            "description": manifest.get("description").and_then(|v| v.as_str()).unwrap_or(""),
            "files": names.iter().take(40).cloned().collect::<Vec<_>>(),
            "fileCount": names.len(),
            "hasInject": false,
            "hasPlugin": plugin_path.is_file(),
            "injectPath": null,
            "pluginPath": plugin_rel,
            "pluginSha256": if plugin_path.is_file() { sha256_file(&plugin_path).ok() } else { None },
            "injectSha256": null,
            "injectBytes": 0,
            "cssBytes": css_path.metadata().map(|m| m.len()).unwrap_or(0),
            "artBytes": art_bytes,
            "risks": risks,
            "warning": "皮肤装饰会注入 ChatGPT 页面。共享 runtime 由引擎提供；请只导入信任来源。plugin.json 仅应含 chromeHtml 等装饰字段。",
            "engine": "native-rust",
        }))
    })();

    rm_dir_recursive(&tmp_root);
    result
}

/// Import `.skin` / `.zip` (and legacy `.cgskin`) into user skins directory.
pub fn import_skin_native(package_path: &str, overwrite: bool) -> Result<Value, EngineError> {
    let package = PathBuf::from(package_path);
    if !package.is_file() {
        return Err(EngineError::msg("找不到皮肤包文件"));
    }
    ensure_state_dir()?;
    let tmp_root = state_root().join(format!(
        ".import-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    rm_dir_recursive(&tmp_root);
    fs::create_dir_all(&tmp_root).map_err(|e| EngineError::msg(e.to_string()))?;

    let result = (|| {
        let _ = extract_zip_to(&package, &tmp_root)?;
        let skin_dir = resolve_skin_dir_from_extracted(&tmp_root)?;
        let mut manifest: Value = serde_json::from_str(
            &fs::read_to_string(skin_dir.join("skin.json"))
                .map_err(|e| EngineError::msg(format!("skin.json: {e}")))?,
        )
        .map_err(|e| EngineError::msg(format!("skin.json parse: {e}")))?;

        // Drop legacy inject references
        if let Some(assets) = manifest.get_mut("assets").and_then(|a| a.as_object_mut()) {
            assets.remove("inject");
            assets.remove("useLegacyInject");
        }

        validate_skin_manifest(&manifest, &skin_dir)?;
        let raw_id = manifest
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id = safe_skin_id_pub(&raw_id);
        if id.is_empty() {
            return Err(EngineError::msg("皮肤 id 不合法"));
        }
        if let Some(obj) = manifest.as_object_mut() {
            obj.insert("id".into(), json!(id));
        }

        let plugin_rel = manifest
            .pointer("/assets/plugin")
            .and_then(|v| v.as_str())
            .unwrap_or("assets/plugin.json")
            .to_string();
        let plugin_path = skin_dir.join(&plugin_rel);
        let plugin_sha = if plugin_path.is_file() {
            sha256_file(&plugin_path).ok()
        } else {
            None
        };

        let leftover = skin_dir.join("assets").join("renderer-inject.js");
        let _ = fs::remove_file(&leftover);

        let lib_target = library::library_skin_dir(&id);
        if lib_target.is_dir() && !overwrite {
            return Err(EngineError::msg(format!("皮肤「{id}」已存在")));
        }

        fs::write(
            skin_dir.join("skin.json"),
            format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap_or_default()),
        )
        .map_err(|e| EngineError::msg(format!("write skin.json: {e}")))?;

        let meta = json!({
            "importedAt": now_iso(),
            "from": package.file_name().and_then(|s| s.to_str()).unwrap_or(""),
            "pluginSha256": plugin_sha,
            "engineProtocol": ENGINE_PROTOCOL,
            "version": manifest.get("version").cloned().unwrap_or(json!("0")),
            "warning": "decoration from plugin.json is injected into ChatGPT renderer",
        });
        let target_dir = install_skin_tree(&skin_dir, &id, "import", meta)?;
        let installed: Value = serde_json::from_str(
            &fs::read_to_string(target_dir.join("skin.json"))
                .map_err(|e| EngineError::msg(e.to_string()))?,
        )
        .map_err(|e| EngineError::msg(e.to_string()))?;
        validate_skin_manifest(&installed, &target_dir)?;

        // Touch list for diagnostics
        let _ = list_skins();

        Ok(json!({
            "ok": true,
            "skinId": id,
            "name": installed.get("name").and_then(|v| v.as_str()).unwrap_or(&id),
            "dir": target_dir.to_string_lossy(),
            "overwritten": overwrite,
            "pluginSha256": plugin_sha,
            "engine": "native-rust",
        }))
    })();

    rm_dir_recursive(&tmp_root);
    result
}
