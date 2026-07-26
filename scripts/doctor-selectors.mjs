/**
 * Doctor: validate engine/runtime/selectors.json schema & uniqueness,
 * plus nested :has() regression guard across framework + skin CSS.
 * Does not require a live Codex instance (static contract check).
 *
 * Usage: node scripts/doctor-selectors.mjs
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..");
const file = path.join(root, "engine", "runtime", "selectors.json");

const raw = fs.readFileSync(file, "utf8");
const doc = JSON.parse(raw);

const errors = [];
const warnings = [];

if (doc.schema !== "chatgpt-tools-host-selectors/1") {
  errors.push(`unexpected schema: ${doc.schema}`);
}
if (!Array.isArray(doc.selectors) || doc.selectors.length === 0) {
  errors.push("selectors[] missing or empty");
}

const keys = new Set();
const requiredL1 = [];
const selectorByKey = new Map();
for (const entry of doc.selectors || []) {
  if (!entry?.key || !entry?.selector) {
    errors.push(`invalid entry: ${JSON.stringify(entry)}`);
    continue;
  }
  if (keys.has(entry.key)) errors.push(`duplicate key: ${entry.key}`);
  keys.add(entry.key);
  selectorByKey.set(entry.key, String(entry.selector));
  if (!["L1", "L2", "L0"].includes(entry.tier)) {
    warnings.push(`${entry.key}: tier should be L0|L1|L2`);
  }
  if (entry.tier === "L1" && entry.required) requiredL1.push(entry.key);
  if (String(entry.selector).includes("[hash") || /_[a-z0-9]{5,}_/.test(entry.selector)) {
    // heuristic: full CSS module hashes are fragile
    if (!entry.selector.includes("*=")) {
      warnings.push(`${entry.key}: selector may lock a fragile hash — prefer prefix *=`);
    }
  }
}

const must = ["shell-main", "header-tint", "home-icon", "home-route-css"];
for (const k of must) {
  if (!keys.has(k)) errors.push(`missing required key in contract: ${k}`);
}

// home-route-css must stay free of :has() so CSS can nest route gates safely.
const homeRouteCss = selectorByKey.get("home-route-css") || "";
if (homeRouteCss.includes(":has(")) {
  errors.push(
    "home-route-css must not contain :has() — browsers drop nested :has() rules (Dream Skin 1.3.2)"
  );
}
const homeRoute = selectorByKey.get("home-route") || "";
if (homeRoute && !homeRoute.includes("home-icon") && !homeRoute.includes("role")) {
  warnings.push("home-route selector looks unusual — expected home-icon / role=main probe");
}

if (!doc.cssAuthoringMap?.whereToEdit && !doc.cssAuthoringMap?.["where-to-edit"]) {
  warnings.push("cssAuthoringMap.where-to-edit recommended for skin authors");
}

const map = doc.cssAuthoringMap?.["where-to-edit"] || doc.cssAuthoringMap?.whereToEdit || {};
if (map && typeof map === "object") {
  const host = map["host-anchors"] || map.hostAnchors || "";
  if (host && !String(host).includes("selectors.json")) {
    warnings.push("where-to-edit.host-anchors should point at selectors.json");
  }
}

// Cross-check immersive + core still mention primary anchors
const immersive = fs.readFileSync(
  path.join(root, "engine", "runtime", "immersive-skin.css"),
  "utf8"
);
const core = fs.readFileSync(
  path.join(root, "engine", "runtime", "renderer-core.js"),
  "utf8"
);
for (const anchor of ["main.main-surface", "app-header-tint", "home-icon"]) {
  if (!immersive.includes(anchor.split(".").pop()) && !core.includes(anchor)) {
    warnings.push(`runtime may not reference anchor fragment: ${anchor}`);
  }
}
if (!core.includes("main.main-surface") && !core.includes("main-surface")) {
  errors.push("renderer-core.js missing shell-main style probe");
}
if (!core.includes("home-icon")) {
  errors.push("renderer-core.js missing home-icon probe");
}

/**
 * Detect nested :has() which CSS parsers discard entirely.
 * Walk each selector-ish chunk; if a :has(… ) body itself contains :has(, flag it.
 * Does not require a full CSS parser — false positives are rare on our skin CSS.
 */
function findNestedHas(cssText, label) {
  const hits = [];
  const src = String(cssText);
  let i = 0;
  while (i < src.length) {
    const start = src.indexOf(":has(", i);
    if (start < 0) break;
    let depth = 0;
    let j = start + 5; // after ":has("
    let bodyStart = j;
    for (; j < src.length; j++) {
      const ch = src[j];
      if (ch === "(") depth += 1;
      else if (ch === ")") {
        if (depth === 0) {
          const body = src.slice(bodyStart, j);
          if (body.includes(":has(")) {
            // Line number for diagnostics
            const line = src.slice(0, start).split(/\r?\n/).length;
            const snippet = src.slice(start, Math.min(start + 96, j + 1)).replace(/\s+/g, " ");
            hits.push({ label, line, snippet });
          }
          i = j + 1;
          break;
        }
        depth -= 1;
      }
    }
    if (j >= src.length) break;
  }
  return hits;
}

function walkCssFiles(dir, out = []) {
  if (!fs.existsSync(dir)) return out;
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, ent.name);
    if (ent.isDirectory()) walkCssFiles(p, out);
    else if (ent.isFile() && ent.name.endsWith(".css")) out.push(p);
  }
  return out;
}

const cssTargets = [
  path.join(root, "engine", "runtime", "immersive-skin.css"),
  ...walkCssFiles(path.join(root, "skins")),
];

const nestedHas = [];
for (const cssPath of cssTargets) {
  let text;
  try {
    text = fs.readFileSync(cssPath, "utf8");
  } catch {
    continue;
  }
  const rel = path.relative(root, cssPath).replace(/\\/g, "/");
  for (const hit of findNestedHas(text, rel)) {
    nestedHas.push(hit);
  }
}

// Nested :has() is a hard error on framework CSS; skins warn (authors may iterate).
for (const hit of nestedHas) {
  const msg = `nested :has() in ${hit.label}:${hit.line} — ${hit.snippet}`;
  if (hit.label.startsWith("engine/runtime/")) {
    errors.push(msg);
  } else {
    warnings.push(msg + " (browser may drop the whole rule)");
  }
}

// Prefer home-route-css alias in framework CSS when gating with :has(:not(...))
if (immersive.includes(":has(") && immersive.includes("home-route")) {
  // soft note only
}

const report = {
  ok: errors.length === 0,
  file,
  selectorCount: keys.size,
  requiredL1,
  nestedHasCount: nestedHas.length,
  nestedHas: nestedHas.slice(0, 20),
  errors,
  warnings,
};

console.log(JSON.stringify(report, null, 2));
process.exit(errors.length ? 1 : 0);
