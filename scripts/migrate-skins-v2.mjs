/**
 * One-shot: convert bundled skins to pure plugin + shared-core layout.
 * - Require assets.plugin
 * - Drop assets.inject / useLegacyInject
 * - Normalize plugin.chromeHtml
 * - Delete renderer-inject.js stubs
 * - Fix artMime from file magic
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const skinsRoot = path.join(root, "skins");

function magicMime(buf) {
  if (buf.length >= 3 && buf[0] === 0xff && buf[1] === 0xd8) return "image/jpeg";
  if (buf.length >= 8 && buf[0] === 0x89 && buf[1] === 0x50) return "image/png";
  if (
    buf.length >= 12 &&
    buf.subarray(0, 4).toString() === "RIFF" &&
    buf.subarray(8, 12).toString() === "WEBP"
  ) {
    return "image/webp";
  }
  const head = buf.subarray(0, 200).toString("utf8");
  if (head.includes("<svg") || head.includes("<?xml")) return "image/svg+xml";
  return null;
}

function normalizeChrome(html) {
  return String(html || "")
    .replace(/\r\n/g, "\n")
    .replace(/\n[ \t]+/g, "\n")
    .replace(/\n{3,}/g, "\n")
    .trim();
}

function escapeHtml(text) {
  return String(text || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

const dirs = fs
  .readdirSync(skinsRoot)
  .filter((d) => fs.statSync(path.join(skinsRoot, d)).isDirectory());

for (const dirName of dirs) {
  const skinDir = path.join(skinsRoot, dirName);
  const skinPath = path.join(skinDir, "skin.json");
  if (!fs.existsSync(skinPath)) continue;
  const manifest = JSON.parse(fs.readFileSync(skinPath, "utf8"));

  if (!manifest.assets) manifest.assets = {};
  const artRel = manifest.assets.art;
  if (artRel) {
    const artPath = path.join(skinDir, artRel);
    if (fs.existsSync(artPath)) {
      const mime = magicMime(fs.readFileSync(artPath));
      if (mime) manifest.assets.artMime = mime;
    }
  }

  if (!manifest.assets.plugin) manifest.assets.plugin = "assets/plugin.json";
  delete manifest.assets.inject;
  delete manifest.assets.useLegacyInject;

  if (!manifest.appearance) manifest.appearance = "auto";
  if (!manifest.art || typeof manifest.art !== "object") {
    manifest.art = {
      focusX: 0.72,
      focusY: 0.45,
      safeArea: "left",
      taskMode: "ambient",
    };
  }
  if (!manifest.version) manifest.version = "2.0.0";

  const pluginPath = path.join(skinDir, manifest.assets.plugin);
  let plugin = { version: "2.0.0", chromeHtml: "", skipAnalysis: false };
  if (fs.existsSync(pluginPath)) {
    plugin = { ...plugin, ...JSON.parse(fs.readFileSync(pluginPath, "utf8")) };
  }
  plugin.version = String(plugin.version || "2.0.0");
  plugin.chromeHtml = normalizeChrome(plugin.chromeHtml);
  plugin.skipAnalysis = plugin.skipAnalysis === true;
  if (!plugin.chromeHtml) {
    plugin.chromeHtml = `<div class="skin-brand"><b>${escapeHtml(
      manifest.name || manifest.id
    )}</b><small>ChatGPT Tools</small></div>`;
  }
  fs.mkdirSync(path.dirname(pluginPath), { recursive: true });
  fs.writeFileSync(pluginPath, JSON.stringify(plugin, null, 2) + "\n", "utf8");

  const injectStub = path.join(skinDir, "assets", "renderer-inject.js");
  if (fs.existsSync(injectStub)) {
    fs.unlinkSync(injectStub);
    console.log("removed", path.relative(root, injectStub));
  }

  fs.writeFileSync(skinPath, JSON.stringify(manifest, null, 2) + "\n", "utf8");
  console.log(
    "ok",
    dirName,
    "id=" + manifest.id,
    "mime=" + manifest.assets.artMime,
    "chrome=" + plugin.chromeHtml.length
  );
}

console.log("migrated", dirs.length, "skins");
