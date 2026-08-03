//! Catalog fetch, disk cache, merge with local skins.

use super::cache::{
    catalog_etag_path, catalog_path, ensure_cloud_layout, read_etag, read_json, write_etag,
    write_text_atomic,
};
use super::config::CloudConfig;
use super::http::{get_text, join_url};
use crate::engine::EngineError;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct CatalogSnapshot {
    pub value: Value,
    #[allow(dead_code)]
    pub from_network: bool,
    #[allow(dead_code)]
    pub not_modified: bool,
}

pub fn load_catalog_disk() -> Option<Value> {
    read_json(&catalog_path())
}

/// Refresh catalog from CDN (or keep disk on 304 / network error if disk exists when soft).
pub fn refresh_catalog(cfg: &CloudConfig) -> Result<CatalogSnapshot, EngineError> {
    if !cfg.enabled {
        return Err(EngineError::msg("云端已关闭"));
    }
    ensure_cloud_layout()?;

    let etag = read_etag(&catalog_etag_path());
    let urls = [
        join_url(&cfg.base_url, &format!("{}/catalog.json", cfg.channel)),
        join_url(&cfg.base_url, &format!("{}/catalog", cfg.channel)),
    ];

    let mut last_err = EngineError::msg("catalog 请求失败");
    for url in &urls {
        match get_text(cfg, url, etag.as_deref()) {
            Ok(resp) if resp.not_modified => {
                if let Some(v) = load_catalog_disk() {
                    return Ok(CatalogSnapshot {
                        value: v,
                        from_network: false,
                        not_modified: true,
                    });
                }
                last_err = EngineError::msg("304 但本地无 catalog 缓存");
            }
            Ok(resp) => {
                let body = resp
                    .body
                    .ok_or_else(|| EngineError::msg("catalog 空响应"))?;
                let value: Value = serde_json::from_str(&body)
                    .map_err(|e| EngineError::msg(format!("catalog JSON: {e}")))?;
                validate_catalog_shape(&value)?;
                write_text_atomic(&catalog_path(), &format!("{}\n", pretty(&value)))?;
                if let Some(et) = resp.etag {
                    let _ = write_etag(&catalog_etag_path(), &et);
                }
                return Ok(CatalogSnapshot {
                    value,
                    from_network: true,
                    not_modified: false,
                });
            }
            Err(e) => last_err = e,
        }
    }

    // Soft fallback to disk
    if let Some(v) = load_catalog_disk() {
        return Ok(CatalogSnapshot {
            value: v,
            from_network: false,
            not_modified: false,
        });
    }
    Err(last_err)
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn validate_catalog_shape(v: &Value) -> Result<(), EngineError> {
    let protocol = v.get("protocol").and_then(|p| p.as_u64()).unwrap_or(0);
    if protocol != 1 {
        return Err(EngineError::msg(format!(
            "不支持的 catalog protocol: {protocol}"
        )));
    }
    if !v.get("skins").and_then(|s| s.as_array()).is_some() {
        return Err(EngineError::msg("catalog 缺少 skins 数组"));
    }
    Ok(())
}

/// Compare loose semver-like strings (major.minor.patch); non-numeric → equal 0.
pub fn version_cmp(a: &str, b: &str) -> i32 {
    let parse = |s: &str| -> Vec<u64> {
        s.trim()
            .trim_start_matches('v')
            .split(|c| c == '.' || c == '-' || c == '+')
            .take(3)
            .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
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

fn local_version_of(skin: &Value) -> String {
    skin.get("version")
        .and_then(|v| v.as_str())
        .or_else(|| skin.get("cacheVersion").and_then(|v| v.as_str()))
        .unwrap_or("0")
        .to_string()
}

/// Merge remote catalog entries into status skins array (mutates status).
/// Local skins already come from the unified library (or dev workspace).
/// Adds remote-only cards and flags updateAvailable.
pub fn merge_remote_into_status(status: &mut Value, catalog: Option<&Value>, cfg: &CloudConfig) {
    let Some(catalog) = catalog else {
        return;
    };
    let Some(remote_skins) = catalog.get("skins").and_then(|s| s.as_array()) else {
        return;
    };
    let Some(local_skins) = status.get_mut("skins").and_then(|s| s.as_array_mut()) else {
        return;
    };

    // Index local by id
    let mut local_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for skin in local_skins.iter_mut() {
        let id = skin
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        local_ids.insert(id.clone());

        if let Some(remote) = remote_skins.iter().find(|r| {
            r.get("id").and_then(|v| v.as_str()) == Some(id.as_str())
        }) {
            let remote_ver = remote
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0");
            let local_ver = local_version_of(skin);
            let update = version_cmp(&local_ver, remote_ver) < 0
                && remote.get("bundledWithApp").and_then(|v| v.as_bool()) != Some(true)
                && remote.get("package").is_some();

            if let Some(obj) = skin.as_object_mut() {
                obj.insert("remoteVersion".into(), json!(remote_ver));
                obj.insert(
                    "installState".into(),
                    if update {
                        json!("updateAvailable")
                    } else {
                        json!(obj
                            .get("installState")
                            .and_then(|v| v.as_str())
                            .unwrap_or("ready"))
                    },
                );
                if update {
                    obj.insert("updateAvailable".into(), json!(true));
                }
                // attach package meta for download (not trusted alone — download re-reads catalog)
                if let Some(pkg) = remote.get("package").cloned() {
                    obj.insert("remotePackage".into(), pkg);
                }
                if let Some(tags) = remote.get("tags").cloned() {
                    if obj.get("tags").and_then(|t| t.as_array()).map(|a| a.is_empty()).unwrap_or(true)
                    {
                        obj.insert("tags".into(), tags);
                    }
                }
                // Prefer local skin.json categories; fill from catalog when missing/empty
                if let Some(cats) = remote.get("categories").cloned() {
                    let local_empty = obj
                        .get("categories")
                        .and_then(|t| t.as_array())
                        .map(|a| a.is_empty())
                        .unwrap_or(true);
                    if local_empty {
                        obj.insert("categories".into(), cats);
                    }
                }
            }
        } else if let Some(obj) = skin.as_object_mut() {
            if !obj.contains_key("installState") {
                obj.insert("installState".into(), json!("ready"));
            }
        }
    }

    // Append remote-only (not bundled-only placeholders that exist locally)
    for remote in remote_skins {
        let id = remote
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() || local_ids.contains(&id) {
            continue;
        }
        let status_str = remote
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("active");
        if status_str == "deprecated" {
            continue;
        }
        let eng_min = remote
            .get("engineProtocolMin")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        if eng_min > cfg.engine_protocol {
            continue;
        }
        let min_app = remote
            .get("minAppVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("0");
        if version_cmp(&cfg.app_version, min_app) < 0 {
            continue;
        }

        let bundled = remote
            .get("bundledWithApp")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // If marked bundled but not present locally, still show as remote if package exists
        let has_package = remote.get("package").is_some();
        if bundled && !has_package {
            continue;
        }
        if !has_package {
            continue;
        }

        let preview_url = remote
            .pointer("/preview/url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut card = json!({
            "id": id,
            "name": remote.get("name").and_then(|v| v.as_str()).unwrap_or(&id),
            "nameEn": remote.get("nameEn"),
            "version": remote.get("version"),
            "description": remote.get("description").and_then(|v| v.as_str()).unwrap_or(""),
            "tags": remote.get("tags").cloned().unwrap_or(json!([])),
            "categories": remote.get("categories").cloned().unwrap_or(json!([])),
            "accent": remote.get("accent"),
            "previewGradient": remote.get("previewGradient"),
            "featured": remote.get("featured").and_then(|v| v.as_bool()).unwrap_or(false),
            "source": "remote",
            "builtin": false,
            "installState": "remote",
            "remoteVersion": remote.get("version"),
            "updateAvailable": false,
            "active": false,
            "dir": null,
        });
        if !preview_url.is_empty() {
            if let Some(obj) = card.as_object_mut() {
                // Host will cache → previewUrl data-URL; raw URL kept for ensure path.
                // Do not put remote http(s) into previewUrl (WebView CSP blocks it).
                obj.insert("remotePreviewUrl".into(), json!(preview_url));
            }
        }
        if let Some(prev) = remote.get("preview").cloned() {
            if let Some(obj) = card.as_object_mut() {
                obj.insert("remotePreview".into(), prev);
            }
        }
        if let Some(pkg) = remote.get("package").cloned() {
            if let Some(obj) = card.as_object_mut() {
                obj.insert("remotePackage".into(), pkg);
            }
        }
        local_skins.push(card);
    }

    // Sort: active/local ready first, then name
    local_skins.sort_by(|a, b| {
        let rank = |s: &Value| -> u8 {
            match s.get("source").and_then(|v| v.as_str()).unwrap_or("") {
                "user" => 0,
                "cache" => 1,
                "bundled" => 2,
                "remote" => 3,
                _ => 4,
            }
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| {
                let na = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let nb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
                na.cmp(nb)
            })
    });

    if let Some(obj) = status.as_object_mut() {
        obj.insert(
            "cloudCatalog".into(),
            json!({
                "channel": catalog.get("channel"),
                "generatedAt": catalog.get("generatedAt"),
                "defaultSkinId": catalog.get("defaultSkinId"),
                "minAppVersion": catalog.get("minAppVersion"),
                "skinCount": remote_skins.len(),
            }),
        );
    }
}
