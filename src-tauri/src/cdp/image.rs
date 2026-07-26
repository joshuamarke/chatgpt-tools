//! Strict image dimension parser (PNG / JPEG / WebP) + art size limits.
//! Port of `engine/image-metadata.mjs`.

use thiserror::Error;

pub const MAX_IMAGE_DIMENSION: u32 = 16384;
pub const MAX_IMAGE_PIXELS: u64 = 50_000_000;
pub const MAX_ART_BYTES: u64 = 16 * 1024 * 1024;
/// Soft advisory only — high-fidelity wallpapers up to MAX_ART_BYTES are allowed.
/// Raised from 1.5 MB so multi-MB backgrounds are not treated as "should compress".
pub const RECOMMENDED_ART_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("{0}")]
    Message(String),
}

impl ImageError {
    fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub ratio: f64,
    pub wide: bool,
    pub aspect: String,
    pub task_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<bool>,
}

fn u16_be(b: &[u8], o: usize) -> u16 {
    (b[o] as u16) * 256 + b[o + 1] as u16
}
fn u16_le(b: &[u8], o: usize) -> u16 {
    b[o] as u16 + (b[o + 1] as u16) * 256
}
fn u24_le(b: &[u8], o: usize) -> u32 {
    b[o] as u32 + (b[o + 1] as u32) * 256 + (b[o + 2] as u32) * 65536
}
fn u32_be(b: &[u8], o: usize) -> u32 {
    (b[o] as u32) * 0x100_0000
        + (b[o + 1] as u32) * 0x1_0000
        + (b[o + 2] as u32) * 0x100
        + b[o + 3] as u32
}
fn u32_le(b: &[u8], o: usize) -> u32 {
    b[o] as u32
        + (b[o + 1] as u32) * 0x100
        + (b[o + 2] as u32) * 0x1_0000
        + (b[o + 3] as u32) * 0x100_0000
}
fn ascii(b: &[u8], o: usize, n: usize) -> String {
    b.get(o..o + n)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .unwrap_or_default()
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let sig: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != sig || u32_be(bytes, 8) != 13 || ascii(bytes, 12, 4) != "IHDR"
    {
        return None;
    }
    let w = u32_be(bytes, 16);
    let h = u32_be(bytes, 20);
    if w > 0 && h > 0 {
        Some((w, h))
    } else {
        None
    }
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 12 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }
    let sof: [u8; 12] = [
        0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce,
    ];
    // also 0xcf
    let mut offset = 2usize;
    while offset + 9 < bytes.len() {
        if bytes[offset] != 0xff {
            offset += 1;
            continue;
        }
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        if offset >= bytes.len() {
            break;
        }
        let marker = bytes[offset];
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }
        if offset + 2 > bytes.len() {
            break;
        }
        let length = u16_be(bytes, offset) as usize;
        if length < 2 || offset + length > bytes.len() {
            break;
        }
        let is_sof = sof.contains(&marker) || marker == 0xcf;
        if is_sof && length >= 7 {
            let height = u16_be(bytes, offset + 3) as u32;
            let width = u16_be(bytes, offset + 5) as u32;
            if width > 0 && height > 0 {
                return Some((width, height));
            }
        }
        offset += length;
    }
    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 20 || ascii(bytes, 0, 4) != "RIFF" || ascii(bytes, 8, 4) != "WEBP" {
        return None;
    }
    let riff_end = (u32_le(bytes, 4) as usize + 8).min(bytes.len());
    let mut offset = 12usize;
    while offset + 8 <= riff_end {
        let ty = ascii(bytes, offset, 4);
        let size = u32_le(bytes, offset + 4) as usize;
        let data = offset + 8;
        if data + size > riff_end {
            break;
        }
        if ty == "VP8X" && size >= 10 {
            return Some((u24_le(bytes, data + 4) + 1, u24_le(bytes, data + 7) + 1));
        }
        if ty == "VP8L" && size >= 5 && bytes[data] == 0x2f {
            let width = 1
                + bytes[data + 1] as u32
                + ((bytes[data + 2] as u32 & 0x3f) << 8);
            let height = 1
                + (bytes[data + 2] as u32 >> 6)
                + ((bytes[data + 3] as u32) << 2)
                + ((bytes[data + 4] as u32 & 0x0f) << 10);
            return Some((width, height));
        }
        if ty == "VP8 "
            && size >= 10
            && bytes[data + 3] == 0x9d
            && bytes[data + 4] == 0x01
            && bytes[data + 5] == 0x2a
        {
            return Some((
                (u16_le(bytes, data + 6) as u32) & 0x3fff,
                (u16_le(bytes, data + 8) as u32) & 0x3fff,
            ));
        }
        offset = data + size + (size % 2);
    }
    None
}

pub fn classify_image_dimensions(width: u32, height: u32) -> Option<ImageMetadata> {
    if width < 1
        || height < 1
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || (width as u64) * (height as u64) > MAX_IMAGE_PIXELS
    {
        return None;
    }
    let ratio = width as f64 / height as f64;
    if !ratio.is_finite() {
        return None;
    }
    let aspect = if ratio >= 2.25 {
        "ultrawide"
    } else if ratio >= 1.45 {
        "wide"
    } else if ratio >= 1.08 {
        "landscape"
    } else if ratio >= 0.9 {
        "square"
    } else {
        "portrait"
    };
    let task_mode = if ratio >= 2.25 { "banner" } else { "ambient" };
    Some(ImageMetadata {
        width,
        height,
        ratio,
        wide: ratio >= 1.75,
        aspect: aspect.into(),
        task_mode: task_mode.into(),
        vector: None,
    })
}

pub fn read_image_metadata(bytes: &[u8], extension: &str) -> Option<ImageMetadata> {
    let normalized = extension.to_ascii_lowercase();
    let head = ascii(bytes, 0, bytes.len().min(200));
    if normalized == ".svg"
        || head.starts_with("<svg")
        || head.starts_with("<?xml")
        || (bytes.first() == Some(&0x3c) && head.contains("<svg"))
    {
        return Some(ImageMetadata {
            width: 2560,
            height: 1440,
            ratio: 16.0 / 9.0,
            wide: true,
            aspect: "wide".into(),
            task_mode: "ambient".into(),
            vector: Some(true),
        });
    }
    let dims = png_dimensions(bytes)
        .or_else(|| jpeg_dimensions(bytes))
        .or_else(|| webp_dimensions(bytes));
    dims.and_then(|(w, h)| classify_image_dimensions(w, h))
}

pub fn detect_mime_from_bytes(bytes: &[u8], fallback_ext: &str) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] == 0xd8 {
        return "image/jpeg".into();
    }
    if bytes.len() >= 2 && bytes[0] == 0x89 && bytes[1] == 0x50 {
        return "image/png".into();
    }
    if bytes.len() >= 12 && ascii(bytes, 8, 4) == "WEBP" {
        return "image/webp".into();
    }
    mime_from_extension(fallback_ext)
}

pub fn mime_from_extension(extension: &str) -> String {
    match extension.to_ascii_lowercase().as_str() {
        ".jpg" | ".jpeg" => "image/jpeg".into(),
        ".webp" => "image/webp".into(),
        ".svg" => "image/svg+xml".into(),
        _ => "image/png".into(),
    }
}

pub fn assert_art_bytes(size: u64, label: &str) -> Result<(), ImageError> {
    if size < 1 {
        return Err(ImageError::msg(format!("{label} cannot be empty")));
    }
    if size > MAX_ART_BYTES {
        return Err(ImageError::msg(format!(
            "{label} exceeds the {} MB inject limit; please use PNG/JPEG/WebP ≤ {} MB (high-quality originals are supported within this cap)",
            MAX_ART_BYTES / 1024 / 1024,
            MAX_ART_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}
