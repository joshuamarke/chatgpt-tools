//! Native custom wallpaper skin designer (parity with manager.createWallpaperSkin).
//! Pure filesystem — no CDP, no Node.

use super::image::MAX_ART_BYTES;
use super::library::{self, install_skin_tree};
use super::native::{ensure_state_dir, get_skin, safe_skin_id_pub};
use super::package::validate_skin_manifest;
use crate::engine::EngineError;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), EngineError> {
    fs::create_dir_all(dst).map_err(|e| EngineError::msg(format!("mkdir {}: {e}", dst.display())))?;
    for ent in fs::read_dir(src).map_err(|e| EngineError::msg(format!("read_dir: {e}")))? {
        let ent = ent.map_err(|e| EngineError::msg(e.to_string()))?;
        let from = ent.path();
        let to = dst.join(ent.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .map_err(|e| EngineError::msg(format!("copy {}: {e}", from.display())))?;
        }
    }
    Ok(())
}

fn rm_dir_recursive(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

fn payload_str(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else if v.is_number() || v.is_boolean() {
                Some(v.to_string())
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn payload_num(payload: &Value, key: &str, default: f64) -> f64 {
    payload
        .get(key)
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_i64().map(|i| i as f64))
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .unwrap_or(default)
}

fn payload_opt_f64(payload: &Value, key: &str) -> Option<f64> {
    payload.get(key).and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_i64().map(|i| i as f64))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn is_hex_color(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 7 || b[0] != b'#' {
        return false;
    }
    b[1..].iter().all(|c| c.is_ascii_hexdigit())
}

fn hex_or<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    match value {
        Some(v) if is_hex_color(v) => v,
        _ => fallback,
    }
}

/// Create a user skin from a template + wallpaper image (GUI designer).
pub fn design_wallpaper_native(payload: &Value) -> Result<Value, EngineError> {
    ensure_state_dir()?;

    let base_skin_id = payload_str(payload, "baseSkinId")
        .or_else(|| payload_str(payload, "base_skin_id"))
        .unwrap_or_else(|| "dream".into());
    if base_skin_id.is_empty() {
        return Err(EngineError::msg("请选择目标皮肤模板"));
    }

    let image_path = payload_str(payload, "imagePath")
        .or_else(|| payload_str(payload, "image_path"))
        .ok_or_else(|| EngineError::msg("请选择一张壁纸"))?;
    let image = PathBuf::from(&image_path);
    if !image.is_file() {
        return Err(EngineError::msg("请选择一张壁纸"));
    }
    let meta = fs::metadata(&image).map_err(|e| EngineError::msg(format!("读取壁纸失败: {e}")))?;
    if meta.len() < 1 {
        return Err(EngineError::msg("请选择有效的壁纸文件"));
    }
    if meta.len() > MAX_ART_BYTES {
        return Err(EngineError::msg(format!(
            "壁纸必须不超过 {} MB（当前 {:.1} MB）",
            MAX_ART_BYTES / 1024 / 1024,
            meta.len() as f64 / 1024.0 / 1024.0
        )));
    }

    let ext = image
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (ext_norm, mime) = match ext.as_str() {
        "png" => ("png", "image/png"),
        "jpg" | "jpeg" => ("jpg", "image/jpeg"),
        "webp" => ("webp", "image/webp"),
        _ => return Err(EngineError::msg("仅支持 PNG、JPEG 或 WebP 壁纸")),
    };

    let base = get_skin(&base_skin_id)?;
    let base_dir = PathBuf::from(
        base.get("dir")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::msg("模板皮肤缺少目录"))?,
    );
    let base_name = base
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&base_skin_id)
        .to_string();
    let base_id = base
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(&base_skin_id)
        .to_string();
    let base_art_rel = base
        .pointer("/assets/art")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let name_raw = payload_str(payload, "name").unwrap_or_else(|| format!("{base_name} · 自定义"));
    let safe_name: String = name_raw.chars().take(80).collect();
    let safe_name = if safe_name.trim().is_empty() {
        format!("{base_name} · 自定义")
    } else {
        safe_name
    };

    let mut id = safe_skin_id_pub(&format!("{base_id}-{safe_name}"));
    if id.is_empty() {
        id = format!(
            "custom-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
    }
    if id == base_id {
        id = format!(
            "{base_id}-custom-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
    }

    let lib_dir = library::library_dir();
    let target_dir = lib_dir.join(&id);
    if target_dir.exists() {
        return Err(EngineError::msg(format!(
            "皮肤「{safe_name}」已存在，请换一个名称"
        )));
    }

    let position = payload_str(payload, "position").unwrap_or_else(|| "right center".into());
    let valid_position = if regex_position_ok(&position) {
        position
    } else {
        "right center".into()
    };
    let fit = payload_str(payload, "fit").unwrap_or_else(|| "cover".into());
    let valid_fit = if fit == "contain" { "contain" } else { "cover" };

    let pos_x = valid_position
        .split_whitespace()
        .next()
        .unwrap_or("right")
        .to_string();

    let focus_x = match payload_opt_f64(payload, "focusX").or_else(|| payload_opt_f64(payload, "focus_x"))
    {
        Some(v) if v.is_finite() => v.clamp(0.0, 1.0),
        _ => match pos_x.as_str() {
            "left" => 0.28,
            "center" => 0.5,
            _ => 0.72,
        },
    };
    let focus_y_explicit = payload_opt_f64(payload, "focusY")
        .or_else(|| payload_opt_f64(payload, "focus_y"))
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(0.0, 1.0));
    let inferred_focus_y = focus_y_explicit.unwrap_or(0.45);

    let appearance = payload_str(payload, "appearance").unwrap_or_else(|| "auto".into());
    let appearance_choice = match appearance.as_str() {
        "light" | "dark" | "auto" => appearance,
        _ => "auto".into(),
    };
    let safe_area = payload_str(payload, "safeArea")
        .or_else(|| payload_str(payload, "safe_area"))
        .unwrap_or_else(|| "auto".into());
    let safe_area_choice = match safe_area.as_str() {
        "auto" | "left" | "right" | "center" | "none" => safe_area,
        _ => "auto".into(),
    };
    let task_mode = payload_str(payload, "taskMode")
        .or_else(|| payload_str(payload, "task_mode"))
        .unwrap_or_else(|| "auto".into());
    let task_mode_choice = match task_mode.as_str() {
        "auto" | "ambient" | "banner" | "off" => task_mode,
        _ => "auto".into(),
    };

    let accent = payload_str(payload, "accent");
    let background = payload_str(payload, "background");
    let text = payload_str(payload, "text");
    let panel = payload_str(payload, "panel");
    let font = payload_str(payload, "font").unwrap_or_else(|| "system".into());
    let radius = payload_num(payload, "radius", 16.0);
    let overlay = payload_num(payload, "overlay", 12.0);
    let opacity = payload_num(payload, "opacity", 92.0);

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let tmp = lib_dir.join(format!(".wallpaper-{}-{}", std::process::id(), stamp));

    let result = (|| -> Result<Value, EngineError> {
        if tmp.exists() {
            rm_dir_recursive(&tmp);
        }
        copy_dir_recursive(&base_dir, &tmp)?;

        let art_name = format!("wallpaper.{ext_norm}");
        let art_rel = format!("assets/{art_name}");
        let new_art = tmp.join(&art_rel);
        if let Some(parent) = new_art.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| EngineError::msg(format!("mkdir assets: {e}")))?;
        }
        fs::copy(&image, &new_art).map_err(|e| EngineError::msg(format!("复制壁纸失败: {e}")))?;

        if !base_art_rel.is_empty() {
            let old_art = tmp.join(&base_art_rel);
            if old_art.exists() {
                let old_canon = old_art.canonicalize().unwrap_or(old_art.clone());
                let new_canon = new_art.canonicalize().unwrap_or(new_art.clone());
                if old_canon != new_canon {
                    let _ = fs::remove_file(&old_art);
                }
            }
        }

        let manifest_path = tmp.join("skin.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .map_err(|e| EngineError::msg(format!("读 skin.json: {e}")))?;
        let mut manifest: Value = serde_json::from_str(&manifest_text)
            .map_err(|e| EngineError::msg(format!("parse skin.json: {e}")))?;

        {
            let obj = manifest
                .as_object_mut()
                .ok_or_else(|| EngineError::msg("skin.json 无效"))?;
            obj.insert("id".into(), json!(id));
            obj.insert("name".into(), json!(safe_name));
            obj.insert("nameEn".into(), json!(safe_name));
            obj.insert(
                "description".into(),
                json!(format!(
                    "基于「{base_name}」模板的自定义皮肤，可调整壁纸、颜色、字体与自适应构图。"
                )),
            );
            let ver = obj
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("2.0.0")
                .to_string();
            obj.insert("version".into(), json!(ver));

            let mut tags: Vec<String> = obj
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            for t in ["自定义皮肤", "自适应"] {
                if !tags.iter().any(|x| x == t) {
                    tags.push(t.into());
                }
            }
            obj.insert("tags".into(), json!(tags));

            let base_cats: Vec<String> = obj
                .get("categories")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| c.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let categories = if base_cats.is_empty() {
                vec!["art".to_string()]
            } else {
                let mut c = base_cats;
                c.sort();
                c.dedup();
                c
            };
            obj.insert("categories".into(), json!(categories));

            let assets = obj
                .entry("assets")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| EngineError::msg("assets 无效"))?;
            assets.insert("art".into(), json!(art_rel));
            assets.insert("artMime".into(), json!(mime));
            if assets
                .get("plugin")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                assets.insert("plugin".into(), json!("assets/plugin.json"));
            }

            if appearance_choice != "auto" {
                obj.insert("appearance".into(), json!(appearance_choice));
            } else if obj
                .get("appearance")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                obj.insert("appearance".into(), json!("auto"));
            }

            let base_art = obj
                .get("art")
                .cloned()
                .filter(|v| v.is_object())
                .unwrap_or_else(|| json!({}));
            let resolved_safe_area = if safe_area_choice != "auto" {
                safe_area_choice.clone()
            } else {
                match pos_x.as_str() {
                    "left" => "right".into(),
                    "right" => "left".into(),
                    _ => base_art
                        .get("safeArea")
                        .and_then(|v| v.as_str())
                        .unwrap_or("center")
                        .to_string(),
                }
            };
            let resolved_task_mode = if task_mode_choice != "auto" {
                task_mode_choice.clone()
            } else {
                base_art
                    .get("taskMode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("auto")
                    .to_string()
            };
            let focus_y_final = if focus_y_explicit.is_some() {
                inferred_focus_y
            } else {
                base_art
                    .get("focusY")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(inferred_focus_y)
            };
            obj.insert(
                "art".into(),
                json!({
                    "focusX": focus_x,
                    "focusY": focus_y_final,
                    "safeArea": resolved_safe_area,
                    "taskMode": resolved_task_mode,
                    "fit": valid_fit,
                    "position": valid_position,
                }),
            );

            if let Some(ref a) = accent {
                if is_hex_color(a) {
                    obj.insert("accent".into(), json!(a));
                }
            }

            let desktop = obj
                .entry("desktopTheme")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| EngineError::msg("desktopTheme 无效"))?;
            if appearance_choice == "dark" {
                desktop.insert("appearanceTheme".into(), json!("dark"));
            } else if appearance_choice == "light" {
                desktop.insert("appearanceTheme".into(), json!("light"));
            }
        }

        let root_class = manifest
            .pointer("/markers/rootClass")
            .and_then(|v| v.as_str())
            .unwrap_or("skins-root")
            .to_string();
        let art_var = manifest
            .pointer("/markers/artVar")
            .and_then(|v| v.as_str())
            .unwrap_or("--skins-art")
            .to_string();
        let css_rel = manifest
            .pointer("/assets/css")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::msg("模板缺少 assets.css"))?
            .to_string();
        let css_path = tmp.join(&css_rel);
        let css = fs::read_to_string(&css_path)
            .map_err(|e| EngineError::msg(format!("读 CSS: {e}")))?;

        let accent_hex = hex_or(
            accent.as_deref(),
            manifest
                .get("accent")
                .and_then(|v| v.as_str())
                .unwrap_or("#8b7cff"),
        );
        // When accent invalid, fall back already applied; still prefer manifest accent for CSS.
        let accent_css = if accent.as_ref().map(|a| is_hex_color(a)).unwrap_or(false) {
            accent.as_deref().unwrap()
        } else {
            accent_hex
        };
        let bg_css = hex_or(background.as_deref(), "#f7f8fc");
        let text_css = hex_or(text.as_deref(), "#202536");
        let panel_css = hex_or(panel.as_deref(), "#ffffff");
        let safe_radius = radius.clamp(0.0, 32.0);
        let safe_overlay = overlay.clamp(0.0, 70.0);
        let safe_opacity = opacity.clamp(55.0, 100.0);
        let font_stack = match font.as_str() {
            "sans" => r#""Inter", "PingFang SC", "Microsoft YaHei", sans-serif"#,
            "serif" => r#""Songti SC", "STSong", serif"#,
            "mono" => r#""SF Mono", "Cascadia Code", monospace"#,
            _ => r#"system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif"#,
        };

        let custom_css = format!(
            r#"

/* Custom Skin Designer: overrides on top of template «{base_id}». */
html.{root} {{
  --designer-accent: {accent};
  --designer-bg: {bg};
  --designer-text: {text};
  --designer-panel: {panel};
  --designer-panel-alpha: {alpha};
  --designer-radius: {radius}px;
  --designer-overlay: {overlay};
  --skins-art-position: {position};
  --skins-accent: var(--designer-accent);
  --skins-text: var(--designer-text);
  --skins-canvas: var(--designer-bg);
  --skins-surface-raised: color-mix(in srgb, var(--designer-panel) calc(var(--designer-panel-alpha) * 100%), transparent);
}}
html.{root} body {{
  color: var(--designer-text) !important;
  font-family: {font} !important;
  background-color: var(--designer-bg) !important;
  background-size: {fit} !important;
  background-position: var(--skins-art-position, {position}) !important;
  background-repeat: no-repeat !important;
}}
/* Dim layer over wallpaper without replacing template layout */
html.{root} body::after {{
  content: "";
  position: fixed;
  inset: 0;
  z-index: 0;
  pointer-events: none;
  background: rgba(0,0,0,{overlay}) !important;
}}
/* Soften panels; keep template background-image / framework wide-art rules */
html.{root} main.main-surface {{
  border-radius: var(--designer-radius) !important;
}}
html.{root}.skins-art-wide main.main-surface {{
  background-color: color-mix(in srgb, var(--designer-panel) calc(var(--designer-panel-alpha) * 100%), transparent) !important;
}}
html.{root}.skins-art-standard main.main-surface {{
  background-color: color-mix(in srgb, var(--designer-panel) calc(var(--designer-panel-alpha) * 100%), transparent) !important;
  background-size: {fit} !important;
  background-position: var(--skins-art-position, {position}) !important;
  background-repeat: no-repeat !important;
}}
html.{root} button, html.{root} [role="button"] {{ border-radius: var(--designer-radius) !important; }}
html.{root} a, html.{root} [data-state="active"], html.{root} [aria-current="page"] {{
  color: var(--designer-accent) !important;
}}
/* Runtime injects art via {art_var} (and --skins-art alias). */
"#,
            root = root_class,
            accent = accent_css,
            bg = bg_css,
            text = text_css,
            panel = panel_css,
            alpha = safe_opacity / 100.0,
            radius = safe_radius,
            overlay = safe_overlay / 100.0,
            position = valid_position,
            font = font_stack,
            fit = valid_fit,
            art_var = art_var,
            base_id = base_id,
        );
        fs::write(&css_path, format!("{css}{custom_css}"))
            .map_err(|e| EngineError::msg(format!("写 CSS: {e}")))?;

        let plugin_path = tmp.join("assets").join("plugin.json");
        if !plugin_path.is_file() {
            let chrome = format!(
                r#"<div class="skin-brand"><b>{}</b><small>自定义皮肤 · {}</small></div>"#,
                escape_html(&safe_name),
                escape_html(&base_name)
            );
            let plugin = json!({
                "version": "2.0.0",
                "chromeHtml": chrome,
                "skipAnalysis": false,
            });
            if let Some(parent) = plugin_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| EngineError::msg(format!("mkdir plugin: {e}")))?;
            }
            fs::write(
                &plugin_path,
                format!("{}\n", serde_json::to_string_pretty(&plugin).unwrap_or_default()),
            )
            .map_err(|e| EngineError::msg(format!("写 plugin.json: {e}")))?;
        }

        fs::write(
            &manifest_path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&manifest)
                    .map_err(|e| EngineError::msg(format!("serialize manifest: {e}")))?
            ),
        )
        .map_err(|e| EngineError::msg(format!("写 skin.json: {e}")))?;

        validate_skin_manifest(&manifest, &tmp)?;

        let meta = json!({
            "origin": "design",
            "baseSkinId": base_id,
            "version": manifest.get("version").cloned().unwrap_or(json!("1.0.0")),
        });
        let installed_dir = install_skin_tree(&tmp, &id, "design", meta)
            .map_err(|e| EngineError::msg(format!("安装自定义皮肤失败: {e}")))?;
        rm_dir_recursive(&tmp);

        let art = manifest.get("art").cloned().unwrap_or(json!({}));
        Ok(json!({
            "ok": true,
            "skinId": id,
            "name": safe_name,
            "baseSkinId": base_id,
            "dir": installed_dir.to_string_lossy(),
            "appearance": appearance_choice,
            "art": art,
            "nativeEngine": true,
            "nodeRequired": false,
        }))
    })();

    // Always scrub staging dir on failure (success path renames/removes it).
    if tmp.exists() {
        rm_dir_recursive(&tmp);
    }
    result
}

fn regex_position_ok(position: &str) -> bool {
    // left|center|right optionally + top|center|bottom
    let parts: Vec<&str> = position.split_whitespace().collect();
    if parts.is_empty() || parts.len() > 2 {
        return false;
    }
    let x_ok = matches!(parts[0], "left" | "center" | "right");
    if !x_ok {
        return false;
    }
    if parts.len() == 2 {
        return matches!(parts[1], "top" | "center" | "bottom");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::regex_position_ok;

    #[test]
    fn position_parser() {
        assert!(regex_position_ok("right center"));
        assert!(regex_position_ok("left"));
        assert!(!regex_position_ok("somewhere"));
    }
}
