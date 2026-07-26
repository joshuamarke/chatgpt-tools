//! Secure skin package download: catalog-only, host allowlist, sha256, validate.

use super::cache::{
    cache_skins_dir, cache_tmp_dir, ensure_cloud_layout, list_cached_skins, now_unix_ms,
    read_cache_meta, write_text_atomic,
};
use super::catalog::{load_catalog_disk, refresh_catalog, version_cmp};
use super::config::{validate_download_url, CloudConfig, MAX_PACKAGE_BYTES};
use super::http::get_bytes_allowlisted;
use crate::cdp;
use crate::engine::EngineError;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Download skin by id using **catalog package metadata only** (never arbitrary URL from UI).
pub fn download_skin(cfg: &CloudConfig, skin_id: &str) -> Result<Value, EngineError> {
    if !cfg.enabled {
        return Err(EngineError::msg("云端已关闭"));
    }
    let safe_id = cdp::native_safe_skin_id(skin_id);
    if safe_id.is_empty() {
        return Err(EngineError::msg("无效皮肤 id"));
    }
    ensure_cloud_layout()?;

    // Fresh catalog preferred; disk fallback inside refresh
    let catalog = refresh_catalog(cfg)
        .map(|s| s.value)
        .or_else(|_| load_catalog_disk().ok_or_else(|| EngineError::msg("无 catalog，无法下载")))?;

    let entry = catalog
        .get("skins")
        .and_then(|s| s.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(skin_id) || s.get("id").and_then(|v| v.as_str()) == Some(safe_id.as_str()))
                .cloned()
        })
        .ok_or_else(|| EngineError::msg(format!("catalog 中无皮肤：{skin_id}")))?;

    if entry.get("status").and_then(|v| v.as_str()) == Some("deprecated") {
        return Err(EngineError::msg("该皮肤已下架"));
    }
    if entry.get("bundledWithApp").and_then(|v| v.as_bool()) == Some(true)
        && entry.get("package").is_none()
    {
        return Err(EngineError::msg("该皮肤为安装包内置，无需下载"));
    }

    let eng_min = entry
        .get("engineProtocolMin")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if eng_min > cfg.engine_protocol {
        return Err(EngineError::msg(format!(
            "需要引擎协议 {eng_min}，当前 {}",
            cfg.engine_protocol
        )));
    }
    if let Some(min_app) = entry.get("minAppVersion").and_then(|v| v.as_str()) {
        if version_cmp(&cfg.app_version, min_app) < 0 {
            return Err(EngineError::msg(format!(
                "需要应用版本 ≥ {min_app}，当前 {}",
                cfg.app_version
            )));
        }
    }

    let package = entry
        .get("package")
        .ok_or_else(|| EngineError::msg("catalog 条目缺少 package"))?;
    let format = package
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("cgskin");
    if format != "cgskin" && format != "zip" {
        return Err(EngineError::msg(format!("不支持的包格式: {format}")));
    }

    let sha256_expected = package
        .get("sha256")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if sha256_expected.len() != 64 || !sha256_expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(EngineError::msg(
            "catalog package.sha256 无效（需要 64 位 hex）",
        ));
    }
    // Reject all-zero placeholder
    if sha256_expected.chars().all(|c| c == '0') {
        return Err(EngineError::msg(
            "catalog package.sha256 仍是占位符，拒绝下载",
        ));
    }

    let size = package.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
    if size > MAX_PACKAGE_BYTES {
        return Err(EngineError::msg(format!(
            "package.size 超过硬限 {} 字节",
            MAX_PACKAGE_BYTES
        )));
    }

    let version = entry
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0")
        .to_string();

    // Cache hit: same id + version + sha256
    if let Some(meta) = read_cache_meta(&safe_id) {
        let same_ver = meta.get("version").and_then(|v| v.as_str()) == Some(version.as_str());
        let same_hash = meta
            .get("sha256")
            .and_then(|v| v.as_str())
            .map(|s| s.eq_ignore_ascii_case(&sha256_expected))
            .unwrap_or(false);
        let dir = cache_skins_dir().join(&safe_id);
        if same_ver && same_hash && dir.join("skin.json").is_file() {
            return Ok(json!({
                "ok": true,
                "skinId": safe_id,
                "cached": true,
                "version": version,
                "sha256": sha256_expected,
                "dir": dir.to_string_lossy(),
                "message": "已使用本地缓存，跳过下载",
            }));
        }
    }

    let mut urls: Vec<String> = Vec::new();
    if let Some(u) = package.get("url").and_then(|v| v.as_str()) {
        urls.push(u.to_string());
    }
    if let Some(mirrors) = package.get("mirrors").and_then(|v| v.as_array()) {
        for m in mirrors {
            if let Some(u) = m.as_str() {
                if !u.is_empty() && !urls.iter().any(|x| x == u) {
                    urls.push(u.to_string());
                }
            }
        }
    }
    if urls.is_empty() {
        return Err(EngineError::msg("package 无可用 url"));
    }

    // Pre-validate all candidates against allowlist
    for u in &urls {
        let _ = validate_download_url(u, cfg)?;
    }

    let expected_size = if size > 0 { Some(size) } else { None };
    let mut last_err = EngineError::msg("所有镜像下载失败");
    let mut bytes: Option<Vec<u8>> = None;
    let mut used_url = String::new();

    for u in &urls {
        match get_bytes_allowlisted(cfg, u, expected_size) {
            Ok(resp) => {
                let hash = sha256_hex(&resp.bytes);
                if hash != sha256_expected {
                    last_err = EngineError::msg(format!(
                        "sha256 校验失败（来源 {}）",
                        shorten_url(u)
                    ));
                    continue;
                }
                bytes = Some(resp.bytes);
                used_url = resp.final_url;
                break;
            }
            Err(e) => {
                last_err = e;
            }
        }
    }

    let bytes = bytes.ok_or(last_err)?;

    // Write temp package
    let tmp_pkg = cache_tmp_dir().join(format!(
        "{}-{}-{}.cgskin",
        safe_id,
        version.replace(['/', '\\'], "-"),
        now_unix_ms()
    ));
    fs::write(&tmp_pkg, &bytes).map_err(|e| EngineError::msg(format!("写临时包: {e}")))?;

    // Extract + validate using same rules as import
    let tmp_extract = cache_tmp_dir().join(format!("extract-{}-{}", safe_id, now_unix_ms()));
    let _ = fs::remove_dir_all(&tmp_extract);
    fs::create_dir_all(&tmp_extract).map_err(|e| EngineError::msg(e.to_string()))?;

    let install_result = (|| {
        cdp::extract_zip_package(&tmp_pkg, &tmp_extract)?;
        let skin_dir = cdp::resolve_skin_dir_extracted(&tmp_extract)?;
        let mut manifest: Value = serde_json::from_str(
            &fs::read_to_string(skin_dir.join("skin.json"))
                .map_err(|e| EngineError::msg(format!("skin.json: {e}")))?,
        )
        .map_err(|e| EngineError::msg(format!("skin.json parse: {e}")))?;

        if let Some(assets) = manifest.get_mut("assets").and_then(|a| a.as_object_mut()) {
            assets.remove("inject");
            assets.remove("useLegacyInject");
        }
        cdp::validate_skin_manifest_pub(&manifest, &skin_dir)?;

        let raw_id = manifest
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id = cdp::native_safe_skin_id(&raw_id);
        if id.is_empty() {
            return Err(EngineError::msg("皮肤 id 不合法"));
        }
        // Must match requested / catalog id
        if id != safe_id && raw_id != skin_id {
            return Err(EngineError::msg(format!(
                "包内 id「{raw_id}」与 catalog「{skin_id}」不一致"
            )));
        }
        if let Some(obj) = manifest.as_object_mut() {
            obj.insert("id".into(), json!(id));
        }

        // Reject leftover inject scripts
        let leftover = skin_dir.join("assets").join("renderer-inject.js");
        if leftover.is_file() {
            let _ = fs::remove_file(&leftover);
        }

        let target = cache_skins_dir().join(&id);
        // Atomic replace: write to staging then rename
        let staging = cache_skins_dir().join(format!(".staging-{id}-{}", now_unix_ms()));
        let _ = fs::remove_dir_all(&staging);
        copy_dir_recursive(&skin_dir, &staging)?;
        fs::write(
            staging.join("skin.json"),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&manifest).unwrap_or_default()
            ),
        )
        .map_err(|e| EngineError::msg(format!("write skin.json: {e}")))?;

        let installed: Value = serde_json::from_str(
            &fs::read_to_string(staging.join("skin.json"))
                .map_err(|e| EngineError::msg(e.to_string()))?,
        )
        .map_err(|e| EngineError::msg(e.to_string()))?;
        cdp::validate_skin_manifest_pub(&installed, &staging)?;

        let meta = json!({
            "version": version,
            "sha256": sha256_expected,
            "downloadedAt": now_unix_ms().to_string(),
            "sourceUrl": used_url,
            "size": bytes.len(),
            "channel": cfg.channel,
            "catalogId": skin_id,
        });
        write_text_atomic(
            &staging.join(".cache-meta.json"),
            &format!(
                "{}\n",
                serde_json::to_string_pretty(&meta).unwrap_or_default()
            ),
        )?;

        if target.is_dir() {
            let bak = cache_skins_dir().join(format!(".bak-{id}-{}", now_unix_ms()));
            let _ = fs::rename(&target, &bak);
            if let Err(e) = fs::rename(&staging, &target) {
                let _ = fs::rename(&bak, &target);
                let _ = fs::remove_dir_all(&staging);
                return Err(EngineError::msg(format!("安装缓存失败: {e}")));
            }
            let _ = fs::remove_dir_all(&bak);
        } else if let Err(e) = fs::rename(&staging, &target) {
            let _ = fs::remove_dir_all(&staging);
            return Err(EngineError::msg(format!("安装缓存失败: {e}")));
        }

        Ok(json!({
            "ok": true,
            "skinId": id,
            "name": installed.get("name").and_then(|v| v.as_str()).unwrap_or(&id),
            "cached": false,
            "version": version,
            "sha256": sha256_expected,
            "dir": target.to_string_lossy(),
            "size": bytes.len(),
            "sourceUrl": used_url,
            "message": "已下载并缓存",
        }))
    })();

    let _ = fs::remove_file(&tmp_pkg);
    let _ = fs::remove_dir_all(&tmp_extract);
    // touch list
    let _ = list_cached_skins();
    install_result
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

fn shorten_url(u: &str) -> String {
    if u.len() <= 64 {
        u.to_string()
    } else {
        format!("{}…", &u[..61])
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), EngineError> {
    fs::create_dir_all(dst).map_err(|e| EngineError::msg(format!("mkdir: {e}")))?;
    for ent in fs::read_dir(src).map_err(|e| EngineError::msg(e.to_string()))? {
        let ent = ent.map_err(|e| EngineError::msg(e.to_string()))?;
        let from = ent.path();
        let to = dst.join(ent.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| EngineError::msg(format!("copy: {e}")))?;
        }
    }
    Ok(())
}
