/**
 * Strict image dimension parser (PNG / JPEG / WebP).
 * Rejects decompression bombs (16384px / 50MP) before payload build.
 */
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SOF_MARKERS = new Set([
  0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7,
  0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf,
]);

export const MAX_IMAGE_DIMENSION = 16384;
export const MAX_IMAGE_PIXELS = 50_000_000;
/**
 * Hard cap for injectable art (bytes). Larger files are rejected.
 * Large originals (multi-MB) are supported up to this cap for wallpaper quality;
 * shell inject still succeeds first; art is deferred with a scaled CDP timeout.
 */
export const MAX_ART_BYTES = 16 * 1024 * 1024;
/**
 * Soft advisory target for list previews / inspect hints only — not a hard limit.
 * High-fidelity wallpapers are first-class up to MAX_ART_BYTES; use
 * assets/screenshot.* for UI thumbnails when art is large.
 * Raised from 1.5 MB so multi-MB backgrounds are not treated as "should compress".
 */
export const RECOMMENDED_ART_BYTES = 8 * 1024 * 1024;

function uint16be(bytes, offset) {
  return bytes[offset] * 256 + bytes[offset + 1];
}

function uint16le(bytes, offset) {
  return bytes[offset] + bytes[offset + 1] * 256;
}

function uint24le(bytes, offset) {
  return bytes[offset] + bytes[offset + 1] * 256 + bytes[offset + 2] * 65536;
}

function uint32be(bytes, offset) {
  return (
    bytes[offset] * 0x1000000 +
    bytes[offset + 1] * 0x10000 +
    bytes[offset + 2] * 0x100 +
    bytes[offset + 3]
  );
}

function uint32le(bytes, offset) {
  return (
    bytes[offset] +
    bytes[offset + 1] * 0x100 +
    bytes[offset + 2] * 0x10000 +
    bytes[offset + 3] * 0x1000000
  );
}

function ascii(bytes, offset, length) {
  return String.fromCharCode(...bytes.subarray(offset, offset + length));
}

function pngDimensions(bytes) {
  const signature = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  if (
    bytes.length < 24 ||
    signature.some((value, index) => bytes[index] !== value) ||
    uint32be(bytes, 8) !== 13 ||
    ascii(bytes, 12, 4) !== "IHDR"
  ) {
    return null;
  }
  const width = uint32be(bytes, 16);
  const height = uint32be(bytes, 20);
  return width > 0 && height > 0 ? { width, height } : null;
}

function jpegDimensions(bytes) {
  if (bytes.length < 12 || bytes[0] !== 0xff || bytes[1] !== 0xd8) return null;
  let offset = 2;
  while (offset + 9 < bytes.length) {
    if (bytes[offset] !== 0xff) {
      offset += 1;
      continue;
    }
    while (offset < bytes.length && bytes[offset] === 0xff) offset += 1;
    const marker = bytes[offset++];
    if (marker === 0xd9 || marker === 0xda) break;
    if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd8)) continue;
    if (offset + 2 > bytes.length) break;
    const length = uint16be(bytes, offset);
    if (length < 2 || offset + length > bytes.length) break;
    if (SOF_MARKERS.has(marker) && length >= 7) {
      const height = uint16be(bytes, offset + 3);
      const width = uint16be(bytes, offset + 5);
      return width > 0 && height > 0 ? { width, height } : null;
    }
    offset += length;
  }
  return null;
}

function webpDimensions(bytes) {
  if (bytes.length < 20 || ascii(bytes, 0, 4) !== "RIFF" || ascii(bytes, 8, 4) !== "WEBP") {
    return null;
  }
  const riffEnd = Math.min(bytes.length, uint32le(bytes, 4) + 8);
  let offset = 12;
  while (offset + 8 <= riffEnd) {
    const type = ascii(bytes, offset, 4);
    const size =
      bytes[offset + 4] +
      bytes[offset + 5] * 256 +
      bytes[offset + 6] * 65536 +
      bytes[offset + 7] * 0x1000000;
    const data = offset + 8;
    if (data + size > riffEnd) break;
    if (type === "VP8X" && size >= 10) {
      return { width: uint24le(bytes, data + 4) + 1, height: uint24le(bytes, data + 7) + 1 };
    }
    if (type === "VP8L" && size >= 5 && bytes[data] === 0x2f) {
      const width = 1 + bytes[data + 1] + ((bytes[data + 2] & 0x3f) << 8);
      const height =
        1 +
        (bytes[data + 2] >> 6) +
        (bytes[data + 3] << 2) +
        ((bytes[data + 4] & 0x0f) << 10);
      return { width, height };
    }
    if (
      type === "VP8 " &&
      size >= 10 &&
      bytes[data + 3] === 0x9d &&
      bytes[data + 4] === 0x01 &&
      bytes[data + 5] === 0x2a
    ) {
      return {
        width: uint16le(bytes, data + 6) & 0x3fff,
        height: uint16le(bytes, data + 8) & 0x3fff,
      };
    }
    offset = data + size + (size % 2);
  }
  return null;
}

export function classifyImageDimensions({ width, height }) {
  const ratio = width / height;
  if (
    !Number.isSafeInteger(width) ||
    !Number.isSafeInteger(height) ||
    width < 1 ||
    height < 1 ||
    width > MAX_IMAGE_DIMENSION ||
    height > MAX_IMAGE_DIMENSION ||
    width * height > MAX_IMAGE_PIXELS ||
    !Number.isFinite(ratio)
  ) {
    return null;
  }
  const aspect =
    ratio >= 2.25
      ? "ultrawide"
      : ratio >= 1.45
        ? "wide"
        : ratio >= 1.08
          ? "landscape"
          : ratio >= 0.9
            ? "square"
            : "portrait";
  return {
    width,
    height,
    ratio,
    wide: ratio >= 1.75,
    aspect,
    taskMode: ratio >= 2.25 ? "banner" : "ambient",
  };
}

export function readImageMetadata(value, extension = "") {
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
  const normalized = String(extension || "").toLowerCase();
  // SVG: no raster dimensions; allow with a synthetic profile for adaptive layout.
  if (
    normalized === ".svg" ||
    ascii(bytes, 0, 4) === "<svg" ||
    ascii(bytes, 0, 5) === "<?xml" ||
    (bytes[0] === 0x3c && ascii(bytes, 0, Math.min(200, bytes.length)).includes("<svg"))
  ) {
    return {
      width: 2560,
      height: 1440,
      ratio: 16 / 9,
      wide: true,
      aspect: "wide",
      taskMode: "ambient",
      vector: true,
    };
  }
  // Prefer magic bytes over extension (many assets are JPEG mislabeled as .png).
  let dimensions =
    pngDimensions(bytes) || jpegDimensions(bytes) || webpDimensions(bytes);
  if (!dimensions && normalized) {
    if (normalized === ".png") dimensions = pngDimensions(bytes);
    else if (normalized === ".jpg" || normalized === ".jpeg") dimensions = jpegDimensions(bytes);
    else if (normalized === ".webp") dimensions = webpDimensions(bytes);
  }
  return dimensions ? classifyImageDimensions(dimensions) : null;
}

/** Detect real MIME from bytes when filename extension is wrong. */
export function detectMimeFromBytes(bytes, fallbackExt = "") {
  const u8 = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  if (u8[0] === 0xff && u8[1] === 0xd8) return "image/jpeg";
  if (u8[0] === 0x89 && u8[1] === 0x50) return "image/png";
  if (ascii(u8, 8, 4) === "WEBP") return "image/webp";
  if (fallbackExt) return mimeFromExtension(fallbackExt);
  return "application/octet-stream";
}

export function mimeFromExtension(extension) {
  const ext = String(extension || "").toLowerCase();
  if (ext === ".jpg" || ext === ".jpeg") return "image/jpeg";
  if (ext === ".webp") return "image/webp";
  if (ext === ".svg") return "image/svg+xml";
  return "image/png";
}

export function assertArtBytes(size, label = "Theme image") {
  if (!Number.isFinite(size) || size < 1) {
    throw new Error(`${label} cannot be empty`);
  }
  if (size > MAX_ART_BYTES) {
    throw new Error(
      `${label} exceeds the ${MAX_ART_BYTES / 1024 / 1024} MB inject limit; please use PNG/JPEG/WebP ≤ ${MAX_ART_BYTES / 1024 / 1024} MB (high-quality originals are supported within this cap)`
    );
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, imagePath] = process.argv.slice(2);
  if (mode !== "--check" || !imagePath) {
    console.error("Usage: image-metadata.mjs --check <image>");
    process.exitCode = 2;
  } else {
    try {
      const resolved = path.resolve(imagePath);
      const bytes = await fs.readFile(resolved);
      assertArtBytes(bytes.length, "Image");
      const metadata = readImageMetadata(bytes, path.extname(resolved));
      if (!metadata) {
        throw new Error("Image metadata is invalid or exceeds the 16384px / 50MP safety limit");
      }
      console.log(JSON.stringify({ ...metadata, bytes: bytes.length }));
    } catch (error) {
      console.error(error?.message ?? String(error));
      process.exitCode = 2;
    }
  }
}
