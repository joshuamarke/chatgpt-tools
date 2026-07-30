/**
 * Build Tauri updater static JSON from GitHub Release assets.
 *
 * Writes:
 *   latest.json         — platform.url = direct GitHub download
 *   latest.mirror.json  — platform.url = mirrorPrefix + GitHub download
 *                         (for networks that cannot reach github.com)
 *
 * Expected assets (after release-assets workflow):
 *   ChatGPTTools-{ver}-windows-x64-setup.exe (+ .sig)
 *   ChatGPTTools-{ver}-macos-arm64.dmg (+ .sig)
 *   ChatGPTTools-{ver}-macos-x64.dmg (+ .sig)
 *
 * Usage (CI):
 *   node scripts/build-latest-json.mjs \
 *     --repo owner/name --tag v1.1.13 \
 *     --sig-dir dist/sigs \
 *     --out latest.json \
 *     --mirror-out latest.mirror.json \
 *     --mirror-prefix https://ghfast.top/
 */
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

function arg(name, fallback = "") {
  const i = process.argv.indexOf(`--${name}`);
  if (i >= 0 && process.argv[i + 1]) return process.argv[i + 1];
  return fallback;
}

function hasFlag(name) {
  return process.argv.includes(`--${name}`);
}

const repo = arg("repo", process.env.REPO || process.env.GITHUB_REPOSITORY || "");
const tag = arg("tag", process.env.TAG || process.env.GITHUB_REF_NAME || "");
const notesFile = arg("notes-file", "");
const sigDir = arg("sig-dir", "");
const outPath = arg("out", "latest.json");
const mirrorOutPath = arg(
  "mirror-out",
  process.env.LATEST_MIRROR_OUT || "latest.mirror.json"
);
const releaseJsonPath = arg("release-json", "release.json");
/** Prefix prepended to full https://github.com/... URLs (no trailing path only). */
const mirrorPrefix = normalizeMirrorPrefix(
  arg(
    "mirror-prefix",
    process.env.GITHUB_RELEASE_MIRROR_PREFIX || "https://ghfast.top/"
  )
);

if (!repo || !tag) {
  console.error("build-latest-json: --repo and --tag are required");
  process.exit(1);
}

const version = String(tag).replace(/^v/i, "");
const baseUrl = `https://github.com/${repo}/releases/download/${tag}`;

function normalizeMirrorPrefix(raw) {
  const s = String(raw || "").trim();
  if (!s) return "";
  return s.endsWith("/") ? s : `${s}/`;
}

/** Turn a direct GitHub URL into a mirror URL. */
function mirrorUrl(directUrl) {
  if (!mirrorPrefix || !directUrl) return directUrl;
  // Avoid double-prefixing
  if (directUrl.startsWith(mirrorPrefix)) return directUrl;
  return `${mirrorPrefix}${directUrl}`;
}

function loadRelease() {
  if (fs.existsSync(releaseJsonPath)) {
    return JSON.parse(fs.readFileSync(releaseJsonPath, "utf8"));
  }
  if (process.env.GH_TOKEN || process.env.GITHUB_TOKEN) {
    const raw = execFileSync(
      "gh",
      ["release", "view", tag, "--repo", repo, "--json", "assets,body,tagName,url,publishedAt"],
      { encoding: "utf8" }
    );
    return JSON.parse(raw);
  }
  return { assets: [], body: "", tagName: tag };
}

const release = loadRelease();
const assets = Array.isArray(release.assets) ? release.assets : [];
const assetNames = new Set(assets.map((a) => a.name).filter(Boolean));

function findAsset(...predicates) {
  for (const name of assetNames) {
    const lower = name.toLowerCase();
    if (predicates.every((p) => (typeof p === "function" ? p(lower, name) : lower.includes(p)))) {
      return name;
    }
  }
  return null;
}

function readSigFor(assetName) {
  if (!assetName) return "";
  const candidates = [];
  if (sigDir) {
    candidates.push(path.join(sigDir, `${assetName}.sig`));
    candidates.push(
      path.join(sigDir, assetName.replace(/\.(exe|msi|dmg|app\.tar\.gz)$/i, "") + ".sig")
    );
  }
  candidates.push(`${assetName}.sig`);
  for (const p of candidates) {
    if (fs.existsSync(p)) {
      return fs.readFileSync(p, "utf8").trim();
    }
  }
  const sigAsset = `${assetName}.sig`;
  if (assetNames.has(sigAsset) && sigDir) {
    const p = path.join(sigDir, sigAsset);
    if (fs.existsSync(p)) return fs.readFileSync(p, "utf8").trim();
  }
  return "";
}

function downloadSigAssets() {
  if (!sigDir) return;
  fs.mkdirSync(sigDir, { recursive: true });
  for (const a of assets) {
    const name = a?.name;
    if (!name || !name.endsWith(".sig")) continue;
    try {
      execFileSync(
        "gh",
        ["release", "download", tag, "--repo", repo, "--pattern", name, "--dir", sigDir, "--clobber"],
        { stdio: "inherit" }
      );
    } catch (e) {
      console.warn(`warn: failed to download ${name}:`, e?.message || e);
    }
  }
  for (const a of assets) {
    const name = a?.name;
    if (!name || name.endsWith(".sig")) continue;
    if (!/\.(exe|msi|dmg)$/i.test(name)) continue;
    const sigName = `${name}.sig`;
    if (!assetNames.has(sigName)) continue;
    try {
      execFileSync(
        "gh",
        [
          "release",
          "download",
          tag,
          "--repo",
          repo,
          "--pattern",
          sigName,
          "--dir",
          sigDir,
          "--clobber",
        ],
        { stdio: "inherit" }
      );
    } catch {
      /* ignore */
    }
  }
}

if (sigDir) downloadSigAssets();

const winSetup =
  findAsset((l) => l.includes("windows") && l.includes("setup") && l.endsWith(".exe")) ||
  findAsset((l) => l.endsWith("-setup.exe")) ||
  findAsset((l) => l.includes("windows") && l.endsWith(".exe") && !l.includes("portable"));

const macArm =
  findAsset(
    (l) =>
      l.includes("macos") &&
      (l.includes("arm64") || l.includes("aarch64")) &&
      l.endsWith(".dmg")
  ) || findAsset((l) => l.includes("aarch64") && l.endsWith(".dmg"));

const macX64 =
  findAsset(
    (l) => l.includes("macos") && (l.includes("x64") || l.includes("x86_64")) && l.endsWith(".dmg")
  ) || findAsset((l) => l.includes("x86_64") && l.endsWith(".dmg"));

const platformsDirect = {};

function addPlatform(key, assetName) {
  if (!assetName) return;
  const signature = readSigFor(assetName);
  if (!signature) {
    console.warn(`warn: missing signature for ${assetName} — platform ${key} skipped`);
    return;
  }
  const url = `${baseUrl}/${encodeURIComponent(assetName)}`;
  platformsDirect[key] = { signature, url };
}

addPlatform("windows-x86_64", winSetup);
addPlatform("darwin-aarch64", macArm);
addPlatform("darwin-x86_64", macX64);

let notes = "";
if (notesFile && fs.existsSync(notesFile)) {
  notes = fs.readFileSync(notesFile, "utf8").trim();
} else if (release.body) {
  notes = String(release.body).trim();
}

const pubDate = release.publishedAt || release.createdAt || new Date().toISOString();

function buildPayload(platforms) {
  return {
    version,
    notes,
    pub_date: pubDate,
    platforms,
    url: release.url || `https://github.com/${repo}/releases/tag/${tag}`,
    assets: [...assetNames]
      .filter((n) => n !== "latest.json" && n !== "latest.mirror.json")
      .map((name) => {
        const direct = `${baseUrl}/${encodeURIComponent(name)}`;
        return { name, url: direct, mirrorUrl: mirrorUrl(direct) };
      }),
  };
}

const platformsMirror = {};
for (const [key, p] of Object.entries(platformsDirect)) {
  platformsMirror[key] = {
    signature: p.signature,
    url: mirrorUrl(p.url),
  };
}

if (!Object.keys(platformsDirect).length) {
  console.error(
    "build-latest-json: no signed platforms found — refusing to write empty updater manifest"
  );
  if (hasFlag("allow-empty")) {
    fs.writeFileSync(outPath, `${JSON.stringify(buildPayload({}), null, 2)}\n`);
    process.exit(0);
  }
  process.exit(1);
}

const directPayload = buildPayload(platformsDirect);
fs.writeFileSync(outPath, `${JSON.stringify(directPayload, null, 2)}\n`);
console.log(`wrote ${outPath} with platforms: ${Object.keys(platformsDirect).join(", ")}`);

if (mirrorPrefix && mirrorOutPath) {
  const mirrorPayload = buildPayload(platformsMirror);
  mirrorPayload.notes = notes;
  mirrorPayload._mirrorPrefix = mirrorPrefix;
  fs.writeFileSync(mirrorOutPath, `${JSON.stringify(mirrorPayload, null, 2)}\n`);
  console.log(
    `wrote ${mirrorOutPath} (mirrorPrefix=${mirrorPrefix}) platforms: ${Object.keys(platformsMirror).join(", ")}`
  );
}
