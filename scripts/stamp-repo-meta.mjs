/**
 * Resolve GitHub owner/repo and write `src/repo-meta.json`.
 *
 * Priority:
 *   1) GITHUB_REPOSITORY (CI / Actions, e.g. "acme/chatgpt-tools")
 *   2) REPO_OWNER + REPO_NAME env
 *   3) Existing src/repo-meta.json owner/name (if owner already set)
 *   4) package.json "repository" field
 *
 * Safe to run repeatedly. Does not invent a fake owner.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const metaPath = path.join(root, "src", "repo-meta.json");
const pkgPath = path.join(root, "package.json");

function isPlaceholderOwner(owner) {
  return /^(owner|your[_-]?user|your[_-]?name|your[_-]?github|example|changeme|todo|xxx)$/i.test(
    String(owner || "").trim()
  );
}

function parseGithubRepoSlug(raw) {
  const s = String(raw || "").trim();
  if (!s) return null;
  // owner/name
  let m = s.match(/^([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)$/);
  if (m) {
    if (isPlaceholderOwner(m[1])) return null;
    return { owner: m[1], name: m[2] };
  }
  // https://github.com/owner/name(.git)?
  m = s.match(/github\.com[/:]([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+?)(?:\.git)?\/?$/i);
  if (m) {
    if (isPlaceholderOwner(m[1])) return null;
    return { owner: m[1], name: m[2] };
  }
  return null;
}

function readJson(p) {
  try {
    return JSON.parse(fs.readFileSync(p, "utf8"));
  } catch {
    return null;
  }
}

function resolve() {
  const fromCi = parseGithubRepoSlug(process.env.GITHUB_REPOSITORY);
  if (fromCi) return { ...fromCi, source: "GITHUB_REPOSITORY" };

  const envOwner = (process.env.REPO_OWNER || "").trim();
  const envName = (process.env.REPO_NAME || "").trim();
  if (envOwner && envName) {
    return { owner: envOwner, name: envName, source: "REPO_OWNER/REPO_NAME" };
  }

  const existing = readJson(metaPath);
  if (existing?.owner && existing?.name) {
    return {
      owner: String(existing.owner).trim(),
      name: String(existing.name).trim(),
      source: "src/repo-meta.json",
    };
  }

  const pkg = readJson(pkgPath);
  const repoField = pkg?.repository;
  const repoUrl =
    typeof repoField === "string"
      ? repoField
      : repoField && typeof repoField === "object"
        ? repoField.url || ""
        : "";
  const fromPkg = parseGithubRepoSlug(repoUrl);
  if (fromPkg) return { ...fromPkg, source: "package.json#repository" };

  return {
    owner: "",
    name: (existing?.name || pkg?.name || "chatgpt-tools").trim() || "chatgpt-tools",
    source: "fallback-name-only",
  };
}

const resolved = resolve();
const owner = resolved.owner;
const name = resolved.name;
const full = owner && name ? `${owner}/${name}` : "";
const url = full ? `https://github.com/${full}` : "";
const releasesUrl = full ? `${url}/releases` : "";
const latestJsonUrl = full
  ? `https://github.com/${full}/releases/latest/download/latest.json`
  : "";

const latestMirrorJsonUrl = full
  ? `https://github.com/${full}/releases/latest/download/latest.mirror.json`
  : "";
const mirrorPrefixes = ["https://ghfast.top/", "https://ghproxy.net/"];
const updaterEndpoints = full
  ? [
      latestJsonUrl,
      ...mirrorPrefixes.map((p) => `${p}${latestMirrorJsonUrl}`),
    ]
  : [];

const meta = {
  owner,
  name,
  url,
  releasesUrl,
  latestJsonUrl,
  latestMirrorJsonUrl,
  updaterEndpoints,
  repository: full,
  source: resolved.source,
  stampedAt: new Date().toISOString(),
};

fs.mkdirSync(path.dirname(metaPath), { recursive: true });
fs.writeFileSync(metaPath, `${JSON.stringify(meta, null, 2)}\n`, "utf8");

// Keep package.json repository in sync when we know the full slug
if (full && process.env.STAMP_PACKAGE_JSON !== "0") {
  const pkg = readJson(pkgPath);
  if (pkg && typeof pkg === "object") {
    const nextUrl = `git+https://github.com/${full}.git`;
    const cur =
      typeof pkg.repository === "object" && pkg.repository
        ? pkg.repository.url
        : typeof pkg.repository === "string"
          ? pkg.repository
          : "";
    if (cur !== nextUrl || pkg.homepage !== url) {
      pkg.repository = { type: "git", url: nextUrl };
      pkg.homepage = url;
      pkg.bugs = { url: `${url}/issues` };
      fs.writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`, "utf8");
      console.log(`[stamp-repo-meta] updated package.json repository → ${full}`);
    }
  }
}

console.log(
  `[stamp-repo-meta] ${full || `(name=${name}, owner unset)`} ← ${resolved.source}`
);
console.log(`[stamp-repo-meta] wrote ${path.relative(root, metaPath)}`);
