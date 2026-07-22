/**
 * Dev helper: ensure engine deps resolve and resource markers exist.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const required = [
  "engine/manager.js",
  "engine/injector.mjs",
  "engine/cli.mjs",
  "engine/payload.mjs",
  "engine/image-metadata.mjs",
  "engine/runtime/renderer-core.js",
  "skins",
  "src/index.html",
];
for (const rel of required) {
  const p = path.join(root, rel);
  if (!fs.existsSync(p)) {
    console.warn(`[ensure-resources] missing: ${rel}`);
  }
}
console.log("[ensure-resources] ok");
