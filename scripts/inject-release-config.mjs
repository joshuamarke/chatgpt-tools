/**
 * Inject package-time updater endpoints (and optional extra hosts).
 *
 * Sources (priority high → low):
 *   1) process env (CI Secrets / shell) — TAURI_UPDATER_ENDPOINTS full override
 *   2) keys/release.env  (local only, entire keys/ is gitignored)
 *   3) Default chain (GitHub direct → GitHub-proxy mirrors):
 *        https://github.com/{owner}/{repo}/releases/latest/download/latest.json
 *        {mirror}https://github.com/.../latest.mirror.json   (exe/dmg URLs also mirrored)
 *
 * Tauri tries endpoints in order; first successful check wins. The mirror
 * manifest uses proxied asset URLs so installers also fall back off GitHub.
 *
 * Env:
 *   CODEX_SKIN_CLOUD_URL             production skin CDN base (e.g. https://cdn.aiku.cc.cd/v1)
 *   CODEX_SKIN_CLOUD_EXTRA_HOSTS     optional extra allowlist hosts
 *   TAURI_UPDATER_ENDPOINTS          optional full override (comma / JSON array)
 *   GITHUB_RELEASE_MIRROR_PREFIXES   comma list of proxy prefixes (default: ghfast.top)
 *   REQUIRE_RELEASE_SECRETS=1        fail if no updater endpoint can be resolved
 *   REQUIRE_CLOUD_URL=1              fail if CODEX_SKIN_CLOUD_URL missing (Release builds)
 *   SKIP_RELEASE_INJECT=1            empty inject (smoke builds)
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

/** Default public mirror prefixes (prepend to full https://github.com/... URL). */
function defaultMirrorPrefixes(fileEnv) {
  const raw = env("GITHUB_RELEASE_MIRROR_PREFIXES", fileEnv);
  if (raw) {
    return raw
      .split(/[\n,;]+/)
      .map((s) => s.trim())
      .filter(Boolean)
      .map((s) => (s.endsWith("/") ? s : `${s}/`));
  }
  // Community GitHub release accelerators (China / restricted networks).
  // Order = fallback priority after direct GitHub.
  return ["https://ghfast.top/", "https://ghproxy.net/"];
}

function resolveRepoSlug() {
  const meta = readRepoMeta();
  const full =
    (meta?.repository || "").trim() ||
    (meta?.owner && meta?.name ? `${meta.owner}/${meta.name}` : "") ||
    (process.env.GITHUB_REPOSITORY || "").trim();
  if (full && full.includes("/")) return full;
  return "";
}

/**
 * Public GitHub Releases updater manifests (not secrets).
 * 1) Direct latest.json (platform.url = github.com downloads)
 * 2+) Mirror of latest.mirror.json (platform.url already proxied for exe/dmg)
 */
function defaultGithubUpdaterEndpoints(fileEnv = {}) {
  const full = resolveRepoSlug();
  if (!full) return [];
  const direct = `https://github.com/${full}/releases/latest/download/latest.json`;
  const mirrorManifest = `https://github.com/${full}/releases/latest/download/latest.mirror.json`;
  const endpoints = [direct];
  for (const prefix of defaultMirrorPrefixes(fileEnv)) {
    endpoints.push(`${prefix}${mirrorManifest}`);
  }
  return endpoints;
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
  endpoints = defaultGithubUpdaterEndpoints(fileEnv);
  if (endpoints.length) {
    console.log(
      `[inject-release-config] TAURI_UPDATER_ENDPOINTS empty → GitHub direct + ${endpoints.length - 1} mirror(s)`
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
        `  Set TAURI_UPDATER_ENDPOINTS, or ensure GITHUB_REPOSITORY / src/repo-meta.json is set.`
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
// Mirror proxy hosts derived from default prefixes
for (const prefix of defaultMirrorPrefixes(fileEnv)) {
  const base = prefix.startsWith("http") ? prefix.replace(/\/?$/, "") : `https://${prefix.replace(/\/?$/, "")}`;
  const hh = hostFromUrl(base);
  if (hh) hosts.add(hh);
}

writeOutputs({ cloudUrl, endpoints, hosts });
