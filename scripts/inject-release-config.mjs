/**
 * Inject production cloud / updater endpoints at **package time only**.
 *
 * Sources (priority high → low):
 *   1) process env (CI Secrets / shell)
 *   2) keys/release.env  (local, gitignored)
 *   3) GitHub Releases default from repo-meta / GITHUB_REPOSITORY
 *      → https://github.com/{owner}/{repo}/releases/latest/download/latest.json
 *
 * Cloud CDN URL is optional (open-source builds can ship with bundled skins only).
 * Updater endpoints default to the public GitHub latest.json when the repo is known.
 *
 * Writes (gitignored):
 *   src-tauri/gen/release-config.json          → embedded by build.rs
 *   src-tauri/gen/tauri.release-overlay.json   → `tauri build --config …`
 *
 * Env:
 *   CODEX_SKIN_CLOUD_URL          optional production cloud API base
 *   CODEX_SKIN_CLOUD_EXTRA_HOSTS  optional extra allowlist hosts
 *   TAURI_UPDATER_ENDPOINTS       optional override; else GitHub latest.json
 *   REQUIRE_RELEASE_SECRETS=1     fail if no updater endpoint can be resolved
 *   REQUIRE_CLOUD_URL=1           also require CODEX_SKIN_CLOUD_URL
 *   SKIP_RELEASE_INJECT=1         empty inject (smoke builds)
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const genDir = path.join(root, "src-tauri", "gen");
const releaseEnvPath = path.join(root, "keys", "release.env");
const metaPath = path.join(root, "src", "repo-meta.json");
const outConfig = path.join(genDir, "release-config.json");
const outOverlay = path.join(genDir, "tauri.release-overlay.json");

function loadDotEnvFile(filePath) {
  if (!fs.existsSync(filePath)) return {};
  const out = {};
  const text = fs.readFileSync(filePath, "utf8");
  for (const line of text.split(/\r?\n/)) {
    const t = line.trim();
    if (!t || t.startsWith("#")) continue;
    const eq = t.indexOf("=");
    if (eq <= 0) continue;
    const key = t.slice(0, eq).trim();
    let val = t.slice(eq + 1).trim();
    if (
      (val.startsWith('"') && val.endsWith('"')) ||
      (val.startsWith("'") && val.endsWith("'"))
    ) {
      val = val.slice(1, -1);
    }
    out[key] = val;
  }
  return out;
}

function env(name, fileEnv) {
  const fromProc = (process.env[name] || "").trim();
  if (fromProc) return fromProc;
  return String(fileEnv[name] || "").trim();
}

function parseEndpoints(raw) {
  if (!raw) return [];
  const t = raw.trim();
  if (t.startsWith("[")) {
    try {
      const arr = JSON.parse(t);
      if (Array.isArray(arr)) {
        return arr.map((x) => String(x || "").trim()).filter(Boolean);
      }
    } catch {
      /* fall through */
    }
  }
  return t
    .split(/[\n,;]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function parseHosts(raw) {
  if (!raw) return [];
  return raw
    .split(/[\n,;]+/)
    .map((s) => s.trim().toLowerCase())
    .filter(Boolean);
}

function hostFromUrl(u) {
  try {
    const parsed = new URL(u);
    return (parsed.hostname || "").toLowerCase();
  } catch {
    return "";
  }
}

function stampRepoMeta() {
  const stamp = path.join(root, "scripts", "stamp-repo-meta.mjs");
  const r = spawnSync(process.execPath, [stamp], {
    cwd: root,
    env: process.env,
    encoding: "utf8",
  });
  if (r.stdout) process.stdout.write(r.stdout);
  if (r.stderr) process.stderr.write(r.stderr);
  if (r.status !== 0) {
    console.warn("[inject-release-config] stamp-repo-meta failed (non-fatal)");
  }
}

function readRepoMeta() {
  try {
    return JSON.parse(fs.readFileSync(metaPath, "utf8"));
  } catch {
    return null;
  }
}

/** Public GitHub Releases updater manifest (not a secret). */
function defaultGithubUpdaterEndpoints() {
  const meta = readRepoMeta();
  if (meta?.latestJsonUrl) return [String(meta.latestJsonUrl).trim()].filter(Boolean);
  const full =
    (meta?.repository || "").trim() ||
    (meta?.owner && meta?.name ? `${meta.owner}/${meta.name}` : "") ||
    (process.env.GITHUB_REPOSITORY || "").trim();
  if (!full || !full.includes("/")) return [];
  return [`https://github.com/${full}/releases/latest/download/latest.json`];
}

function writeOutputs({ cloudUrl, endpoints, hosts, note }) {
  fs.mkdirSync(genDir, { recursive: true });

  const releaseConfig = {
    cloudBaseUrl: cloudUrl || "",
    cloudAllowedHosts: [...hosts],
    updaterEndpoints: endpoints,
    generatedAt: new Date().toISOString(),
    note: note || "Generated at package time - do not commit.",
  };

  fs.writeFileSync(outConfig, `${JSON.stringify(releaseConfig, null, 2)}\n`, "utf8");

  const overlay = {
    plugins: {
      updater: {
        endpoints: endpoints.length ? endpoints : [],
      },
    },
  };
  fs.writeFileSync(outOverlay, `${JSON.stringify(overlay, null, 2)}\n`, "utf8");

  const redactedUrl = cloudUrl
    ? `${cloudUrl.slice(0, 8)}…(${cloudUrl.length} chars)`
    : "(empty — runtime falls back to local preview URL)";
  console.log(
    `[inject-release-config] cloudBaseUrl=${redactedUrl}; hosts=${hosts.size}; updaterEndpoints=${endpoints.length}`
  );
  if (endpoints.length) {
    // Public GitHub URLs are fine to log; still keep it short.
    console.log(
      `[inject-release-config] endpoints[0]=${endpoints[0].replace(/^https:\/\//, "")}`
    );
  }
  console.log(`[inject-release-config] wrote ${path.relative(root, outConfig)}`);
  console.log(`[inject-release-config] wrote ${path.relative(root, outOverlay)}`);
}

// Always refresh repo-meta (UI GitHub link + default updater URL).
stampRepoMeta();

if (process.env.SKIP_RELEASE_INJECT === "1") {
  // Still stamp meta for the UI; skip embedding production endpoints.
  writeOutputs({
    cloudUrl: "",
    endpoints: [],
    hosts: new Set(),
    note: "Skipped inject (SKIP_RELEASE_INJECT=1) — empty package-time secrets.",
  });
  console.log("[inject-release-config] skipped secret load (SKIP_RELEASE_INJECT=1)");
  process.exit(0);
}

const fileEnv = loadDotEnvFile(releaseEnvPath);
const cloudUrl = env("CODEX_SKIN_CLOUD_URL", fileEnv);
let endpoints = parseEndpoints(env("TAURI_UPDATER_ENDPOINTS", fileEnv));
const extraHosts = parseHosts(env("CODEX_SKIN_CLOUD_EXTRA_HOSTS", fileEnv));

if (!endpoints.length) {
  endpoints = defaultGithubUpdaterEndpoints();
  if (endpoints.length) {
    console.log(
      "[inject-release-config] TAURI_UPDATER_ENDPOINTS empty → default GitHub Releases latest.json"
    );
  }
}

const requireSecrets =
  process.env.REQUIRE_RELEASE_SECRETS === "1" ||
  process.env.REQUIRE_RELEASE_SECRETS === "true";
const requireCloud =
  process.env.REQUIRE_CLOUD_URL === "1" || process.env.REQUIRE_CLOUD_URL === "true";

if (requireSecrets) {
  const missing = [];
  if (!endpoints.length) {
    missing.push(
      "TAURI_UPDATER_ENDPOINTS (or set GITHUB_REPOSITORY / src/repo-meta.json owner)"
    );
  }
  if (requireCloud && !cloudUrl) {
    missing.push("CODEX_SKIN_CLOUD_URL");
  }
  if (missing.length) {
    console.error(
      `[inject-release-config] missing required config: ${missing.join(", ")}\n` +
        `  Set CI Secrets, keys/release.env, or create the GitHub repo and stamp meta.\n` +
        `  See keys/release.env.example and keys/README.md`
    );
    process.exit(1);
  }
}

// Derive allowlist hosts from cloud URL + updater endpoint hosts + extras
const hosts = new Set(extraHosts);
const cloudHost = hostFromUrl(cloudUrl);
if (cloudHost) hosts.add(cloudHost);
for (const ep of endpoints) {
  const h = hostFromUrl(ep);
  if (h) hosts.add(h);
}
// Always allow GitHub download hosts for updater / release assets
for (const h of [
  "github.com",
  "objects.githubusercontent.com",
  "release-assets.githubusercontent.com",
  "raw.githubusercontent.com",
]) {
  hosts.add(h);
}

writeOutputs({ cloudUrl, endpoints, hosts });
