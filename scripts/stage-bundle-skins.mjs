/**
 * Production bundle: only ship the default skin(s) inside the installer.
 * Other skins can be imported locally; installers only ship the default set.
 *
 * Dev (`tauri dev`) still uses the full repo `skins/` tree �?this script
 * only runs as `beforeBuildCommand`.
 *
 * Override:
 *   CODEX_SKIN_BUNDLE_SKINS=qingkong,dream  (comma-separated ids)
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const srcSkins = path.join(root, "skins");
const outRoot = path.join(root, "bundle-resources", "skins");

/** Default installable skin shipped with the app binary. */
const DEFAULT_BUNDLE_SKINS = ["qingkong"];

function parseBundleIds() {
  const raw = (process.env.CODEX_SKIN_BUNDLE_SKINS || "").trim();
  if (!raw) return [...DEFAULT_BUNDLE_SKINS];
  return raw
    .split(/[,;\s]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function rmrf(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
}

function copyDir(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  for (const ent of fs.readdirSync(src, { withFileTypes: true })) {
    if (ent.name === "." || ent.name === ".." || ent.name.startsWith(".")) continue;
    const from = path.join(src, ent.name);
    const to = path.join(dest, ent.name);
    if (ent.isDirectory()) copyDir(from, to);
    else fs.copyFileSync(from, to);
  }
}

const ids = parseBundleIds();
if (!ids.length) {
  console.error("[stage-bundle-skins] no skin ids configured");
  process.exit(1);
}

rmrf(outRoot);
fs.mkdirSync(outRoot, { recursive: true });

const staged = [];
for (const id of ids) {
  if (id.startsWith("_") || id.startsWith(".")) {
    console.warn(`[stage-bundle-skins] skip scaffold id: ${id}`);
    continue;
  }
  const src = path.join(srcSkins, id);
  const manifest = path.join(src, "skin.json");
  if (!fs.existsSync(manifest)) {
    console.error(`[stage-bundle-skins] missing skin: ${id} (${manifest})`);
    process.exit(1);
  }
  const dest = path.join(outRoot, id);
  copyDir(src, dest);
  staged.push(id);
}

// Marker so operators can see what shipped in the package
fs.writeFileSync(
  path.join(outRoot, ".bundle-manifest.json"),
  `${JSON.stringify(
    {
      bundledSkinIds: staged,
      generatedAt: new Date().toISOString(),
      note: "Other skins are not bundled; import locally or ship via your own distribution.",
    },
    null,
    2
  )}\n`,
  "utf8"
);

console.log(
  `[stage-bundle-skins] staged ${staged.length} skin(s) �?bundle-resources/skins: ${staged.join(", ")}`
);
