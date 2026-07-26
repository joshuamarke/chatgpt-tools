/**
 * Skin payload builder: fingerprint, art limits, shared renderer-core assembly.
 * Protocol 2 — skins only supply CSS + art + plugin.json (no per-skin inject).
 *
 * Inject stages (performance):
 *   shell payload — core + CSS + empty art (first paint / cold install)
 *   art payload   — applyArt(dataUrl) wallpaper patch
 *   delta shell   — host.applySkin({css,markers,theme,plugin}) without re-shipping core
 * Monolithic buildPayload() remains for diagnostics / backward compatibility.
 */
import fs from "node:fs/promises";
import path from "node:path";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import {
  assertArtBytes,
  detectMimeFromBytes,
  mimeFromExtension,
  readImageMetadata,
  RECOMMENDED_ART_BYTES,
} from "./image-metadata.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const CORE_PATH = path.join(here, "runtime", "renderer-core.js");
const IMMERSIVE_CSS_PATH = path.join(here, "runtime", "immersive-skin.css");

const payloadCache = new Map();
const PAYLOAD_CACHE_MAX = 8;

/** Path to framework baseline CSS (authors should not copy into skins). */
export function immersiveContractPath() {
  return IMMERSIVE_CSS_PATH;
}

/**
 * Prepend framework baseline before skin personalization.
 * Engine does not force-override author rules — later rules win at equal specificity.
 */
export function mergeSkinCss(baselineCss, skinCss) {
  const base = typeof baselineCss === "string" ? baselineCss.trim() : "";
  const skin = typeof skinCss === "string" ? skinCss.trim() : "";
  const parts = [];
  if (base) {
    parts.push(
      "/* ===== framework baseline (engine/runtime/immersive-skin.css) ===== */",
      "/* Capability only — author-owned skin CSS follows; engine does not restrict. */",
      base
    );
  }
  if (skin) {
    parts.push(
      "/* ===== skin personalization (skins/<id>/assets) ===== */",
      skin
    );
  }
  return parts.join("\n\n");
}

async function loadImmersiveBaseline() {
  try {
    return await fs.readFile(IMMERSIVE_CSS_PATH, "utf8");
  } catch {
    return "";
  }
}

/** Tokens replaced in renderer-core.js IIFE arguments (must use replaceAll). */
const PLACEHOLDERS = [
  "__SKIN_CSS_JSON__",
  "__SKIN_ART_JSON__",
  "__SKIN_THEME_JSON__",
  "__SKIN_MARKERS_JSON__",
  "__SKIN_PLUGIN_JSON__",
  "__SKIN_REVISION_JSON__",
];

const THEME_CHOICES = {
  appearance: new Set(["auto", "light", "dark"]),
  safeArea: new Set(["auto", "left", "right", "center", "none"]),
  taskMode: new Set(["auto", "ambient", "banner", "off"]),
  /** wallpaper = inject + framework may paint body; token-only = inject vars only; none = no art */
  artMode: new Set(["wallpaper", "token-only", "none"]),
  /** body = immersive paints body; none/custom = skin paints (main/chrome/any selector) */
  artPaint: new Set(["body", "none", "custom"]),
};

/**
 * Resolve art.mode / art.paint from skin.json.
 * Defaults keep legacy behaviour: mode=wallpaper, paint=body.
 * - none → no assets.art required; hasArt=false
 * - token-only → art required; paint defaults to custom (engine does not paint body)
 * - wallpaper → art required; paint defaults to body
 */
export function resolveArtPolicy(manifest = {}) {
  const art = manifest.art && typeof manifest.art === "object" ? manifest.art : {};
  const themeBlock = manifest.theme && typeof manifest.theme === "object" ? manifest.theme : {};
  const mergedArt = {
    ...art,
    ...(themeBlock.art && typeof themeBlock.art === "object" ? themeBlock.art : {}),
  };
  const mode = normalizedChoice(
    mergedArt.mode,
    "art.mode",
    THEME_CHOICES.artMode,
    "wallpaper"
  );
  let paintRaw = mergedArt.paint;
  if (paintRaw == null || paintRaw === "") {
    paintRaw = mode === "none" ? "none" : mode === "token-only" ? "custom" : "body";
  }
  const paint = normalizedChoice(paintRaw, "art.paint", THEME_CHOICES.artPaint, "body");
  const needsArt = mode !== "none";
  return { mode, paint, needsArt };
}

function normalizedChoice(value, field, allowed, fallback) {
  if (value == null || value === "") return fallback;
  const text = String(value).trim();
  if (!allowed.has(text)) {
    throw new Error(`${field} must be one of ${[...allowed].join("|")}`);
  }
  return text;
}

function normalizedUnit(value, field) {
  if (value == null || value === "") return null;
  const number = Number(value);
  if (!Number.isFinite(number) || number < 0 || number > 1) {
    throw new Error(`${field} must be a number in 0..1`);
  }
  return number;
}

/**
 * Normalize skin.json theme fields used by renderer-core.
 */
export function normalizeThemeConfig(manifest = {}) {
  const art = manifest.art && typeof manifest.art === "object" ? manifest.art : {};
  const themeBlock = manifest.theme && typeof manifest.theme === "object" ? manifest.theme : {};
  const mergedArt = {
    ...art,
    ...(themeBlock.art && typeof themeBlock.art === "object" ? themeBlock.art : {}),
  };
  const artPolicy = resolveArtPolicy(manifest);
  const appearance = normalizedChoice(
    themeBlock.appearance ?? manifest.appearance,
    "appearance",
    THEME_CHOICES.appearance,
    "auto"
  );
  return {
    id: manifest.id || "custom",
    name: manifest.name || manifest.id || "Skin",
    version: String(manifest.version || "2.0.0"),
    appearance,
    accent: typeof manifest.accent === "string" ? manifest.accent : themeBlock.accent || null,
    palette:
      themeBlock.palette && typeof themeBlock.palette === "object" ? themeBlock.palette : {},
    art: {
      mode: artPolicy.mode,
      paint: artPolicy.paint,
      focusX: normalizedUnit(mergedArt.focusX, "art.focusX"),
      focusY: normalizedUnit(mergedArt.focusY, "art.focusY"),
      safeArea: normalizedChoice(
        mergedArt.safeArea,
        "art.safeArea",
        THEME_CHOICES.safeArea,
        "auto"
      ),
      taskMode: normalizedChoice(
        mergedArt.taskMode,
        "art.taskMode",
        THEME_CHOICES.taskMode,
        "auto"
      ),
    },
    skipAnalysis:
      themeBlock.skipAnalysis === true ||
      manifest.skipAnalysis === true ||
      artPolicy.mode === "none",
  };
}

export function normalizeMarkers(manifest = {}) {
  const m = manifest.markers || {};
  if (!m.rootClass || !m.styleId || !m.stateKey) {
    throw new Error("skin.json markers require rootClass, styleId, stateKey");
  }
  const homeClass = m.homeClass || "skin-home";
  return {
    id: manifest.id || "custom",
    rootClass: m.rootClass,
    homeClass,
    homeShellClass: m.homeShellClass || `${homeClass}-shell`,
    homeUtilityClass: m.homeUtilityClass || `${homeClass}-utility`,
    styleId: m.styleId,
    chromeId: m.chromeId || `${m.styleId}-chrome`.replace(/-style$/, "-chrome"),
    stateKey: m.stateKey,
    disabledKey: m.disabledKey || m.stateKey.replace(/_STATE__$/, "_DISABLED__"),
    artVar: m.artVar || "--skin-art",
  };
}

/**
 * Load required assets/plugin.json (chrome decoration only).
 */
async function loadPlugin(skinDir, manifest) {
  const assets = manifest.assets || {};
  const pluginRel = assets.plugin || "assets/plugin.json";
  const pluginPath = path.join(skinDir, pluginRel);
  let text;
  try {
    text = await fs.readFile(pluginPath, "utf8");
  } catch {
    throw new Error(
      `skin requires ${pluginRel} (shared runtime; per-skin inject is no longer supported)`
    );
  }
  let json;
  try {
    json = JSON.parse(text);
  } catch (error) {
    throw new Error(`Invalid plugin.json: ${error.message}`);
  }
  if (typeof json.chromeHtml !== "string") {
    throw new Error("plugin.json requires string field chromeHtml");
  }
  return {
    version: String(json.version || manifest.version || "2.0.0"),
    chromeHtml: json.chromeHtml,
    skipAnalysis: json.skipAnalysis === true,
    labels: json.labels && typeof json.labels === "object" ? json.labels : {},
  };
}

/**
 * Substitute every placeholder occurrence. String.replace only swaps the first
 * match — the core template documents the tokens in a header comment, so a
 * single replace left the IIFE arguments unbroken and produced invalid JS.
 */
export function assemblePayload(coreTemplate, replacements) {
  let payload = coreTemplate;
  for (const [token, value] of Object.entries(replacements)) {
    if (!payload.includes(token)) {
      throw new Error(`renderer-core missing placeholder ${token}`);
    }
    payload = payload.split(token).join(value);
  }
  for (const token of PLACEHOLDERS) {
    if (payload.includes(token)) {
      throw new Error(`payload still contains unresolved placeholder ${token}`);
    }
  }
  return payload;
}

export function assertPayloadSyntax(payload) {
  try {
    // eslint-disable-next-line no-new-func
    new Function(payload);
  } catch (error) {
    throw new Error(`assembled payload is not valid JavaScript: ${error.message}`);
  }
}

export function artDataUrlFromBundle(bundle) {
  if (!bundle?.artBytes?.length) return "";
  return `data:${bundle.mime};base64,${bundle.artBytes.toString("base64")}`;
}

/**
 * Small CDP script that attaches wallpaper after shell runtime is installed.
 * Does not re-ship renderer-core or CSS. Prefers stable host bridge when present.
 */
export function assembleArtPayload(markers, artDataUrl, revision) {
  const stateKey = markers?.stateKey || "__CODEX_SKIN_STATE__";
  const disabledKey = markers?.disabledKey || "__CODEX_SKIN_DISABLED__";
  const payload = `(() => {
  const hostKey = "__CHATGPT_TOOLS_SKIN_HOST__";
  const stateKey = ${JSON.stringify(stateKey)};
  const disabledKey = ${JSON.stringify(disabledKey)};
  const revision = ${JSON.stringify(revision)};
  const artDataUrl = ${JSON.stringify(artDataUrl)};
  if (window[disabledKey]) return { ok: false, reason: "disabled", revision };
  const host = window[hostKey];
  if (host && typeof host.applyArt === "function") {
    return host.applyArt(artDataUrl, revision);
  }
  const state = window[stateKey];
  if (!state) return { ok: false, reason: "no-state", revision };
  if (state.revision != null && revision != null && state.revision !== revision) {
    return { ok: false, reason: "revision-mismatch", stateRevision: state.revision, revision };
  }
  if (typeof state.applyArt === "function") {
    return state.applyArt(artDataUrl, revision);
  }
  return { ok: false, reason: "no-applyArt", revision };
})()`;
  assertPayloadSyntax(payload);
  return payload;
}

/**
 * Delta shell: swap CSS / markers / theme / chrome via resident host.
 * Omits renderer-core entirely (~CSS size only). Falls back to full shell
 * only if host is missing (caller should detect ok:false and re-ship shell).
 */
export function assembleDeltaShellPayload({ css, markers, theme, plugin, revision }) {
  const payload = `(() => {
  const hostKey = "__CHATGPT_TOOLS_SKIN_HOST__";
  const host = window[hostKey];
  const delta = {
    css: ${JSON.stringify(css)},
    markers: ${JSON.stringify(markers)},
    theme: ${JSON.stringify(theme)},
    plugin: ${JSON.stringify(plugin)},
    revision: ${JSON.stringify(revision)},
  };
  if (!host || typeof host.applySkin !== "function") {
    return { ok: false, reason: "no-host", needsFullShell: true, revision: delta.revision };
  }
  try {
    return host.applySkin(delta);
  } catch (error) {
    return {
      ok: false,
      reason: "delta-throw",
      message: String(error && error.message ? error.message : error),
      needsFullShell: true,
      revision: delta.revision,
    };
  }
})()`;
  assertPayloadSyntax(payload);
  return payload;
}

/** Placeholder metadata when art.mode=none (no wallpaper file). */
function noArtMetadata() {
  return {
    width: 1920,
    height: 1080,
    ratio: 16 / 9,
    wide: true,
    aspect: "wide",
    taskMode: "ambient",
  };
}

export async function loadSkinBundle(skinDir) {
  const manifestPath = path.join(skinDir, "skin.json");
  const manifestText = await fs.readFile(manifestPath, "utf8");
  const manifest = JSON.parse(manifestText);
  if (!manifest.assets?.css) {
    throw new Error("skin.json requires assets.css");
  }
  if (!manifest.assets?.plugin) {
    throw new Error("skin.json requires assets.plugin (shared renderer-core)");
  }
  const artPolicy = resolveArtPolicy(manifest);
  const artRel =
    typeof manifest.assets.art === "string" && manifest.assets.art.trim()
      ? manifest.assets.art.trim()
      : null;
  if (artPolicy.needsArt && !artRel) {
    throw new Error(
      "skin.json requires assets.art unless art.mode is \"none\" (pure style skin)"
    );
  }
  const markers = normalizeMarkers(manifest);
  const theme = normalizeThemeConfig(manifest);
  const cssPath = path.join(skinDir, manifest.assets.css);
  const artPath = artRel ? path.join(skinDir, artRel) : null;

  const [skinCss, coreTemplate, plugin, cssStat, baselineCss] = await Promise.all([
    fs.readFile(cssPath, "utf8"),
    fs.readFile(CORE_PATH, "utf8"),
    loadPlugin(skinDir, manifest),
    fs.stat(cssPath),
    loadImmersiveBaseline(),
  ]);
  const css = mergeSkinCss(baselineCss, skinCss);

  let artBytes = Buffer.alloc(0);
  let artStat = { size: 0, mtimeMs: 0 };
  let artMetadata = noArtMetadata();
  let mime = "image/png";
  let artKey = "no-art";

  if (artPolicy.needsArt && artPath) {
    artBytes = await fs.readFile(artPath);
    artStat = await fs.stat(artPath);
    assertArtBytes(artBytes.length, `Art for ${manifest.id || skinDir}`);
    const extension = path.extname(artPath).toLowerCase();
    artMetadata = readImageMetadata(artBytes, extension);
    if (!artMetadata) {
      throw new Error(
        `Art metadata is invalid or exceeds the 16384px / 50MP safety limit (${manifest.id || artPath})`
      );
    }
    // Prefer real file magic — several bundled arts are JPEG stored as .png
    mime =
      detectMimeFromBytes(artBytes, extension) ||
      manifest.assets.artMime ||
      mimeFromExtension(extension) ||
      "image/png";
    artKey = createHash("sha256").update(artBytes).digest("hex").slice(0, 20);
  }

  const styleRevision = createHash("sha256").update(css, "utf8").digest("hex").slice(0, 16);
  const coreRevision = createHash("sha256")
    .update(coreTemplate, "utf8")
    .digest("hex")
    .slice(0, 16);
  const pluginJson = JSON.stringify(plugin);
  const revision = createHash("sha256")
    .update(manifestText, "utf8")
    .update("\0")
    .update(css, "utf8")
    .update("\0")
    .update(artBytes.length ? artBytes : Buffer.from(artKey))
    .update("\0")
    .update(pluginJson, "utf8")
    .update("\0")
    .update(coreRevision, "utf8")
    .digest("hex")
    .slice(0, 24);

  theme.artKey = artKey;
  theme.artMetadata = artMetadata;
  theme.version = plugin.version || theme.version;
  theme.coreRevision = coreRevision;
  if (plugin.skipAnalysis) theme.skipAnalysis = true;
  if (artPolicy.mode === "none") theme.skipAnalysis = true;

  const hasArt = artPolicy.needsArt && artBytes.length > 0;
  const fingerprint = revision;
  const sourceStamp = `${cssStat.size}:${cssStat.mtimeMs}:${artStat.size}:${artStat.mtimeMs}:${revision}`;

  return {
    manifest,
    markers,
    theme,
    plugin,
    css,
    coreTemplate,
    artBytes,
    artPath,
    mime,
    artMetadata,
    artKey,
    styleRevision,
    coreRevision,
    revision,
    fingerprint,
    sourceStamp,
    hasArt,
    artMode: artPolicy.mode,
    artPaint: artPolicy.paint,
    recommended: !hasArt || artBytes.length <= RECOMMENDED_ART_BYTES,
    payloadBytesEstimate: Math.ceil(artBytes.length * 1.37) + css.length + coreTemplate.length,
    shellBytesEstimate: css.length + coreTemplate.length + pluginJson.length + 256,
    deltaShellBytesEstimate: css.length + pluginJson.length + 512,
  };
}

function cacheGet(key) {
  const cached = payloadCache.get(key);
  if (!cached) return null;
  return { ...cached, cacheHit: true };
}

function cacheSet(key, result) {
  payloadCache.set(key, result);
  while (payloadCache.size > PAYLOAD_CACHE_MAX) {
    payloadCache.delete(payloadCache.keys().next().value);
  }
}

/**
 * Phase-1 shell: renderer-core + CSS + chrome, art argument is empty string.
 * First paint does not wait on multi-MB base64 wallpaper.
 */
export async function buildShellPayload(skinDir, preloaded = null) {
  const bundle = preloaded || (await loadSkinBundle(skinDir));
  const cacheKey = `shell:${bundle.fingerprint}`;
  const hit = cacheGet(cacheKey);
  if (hit) return hit;

  const shellPayload = assemblePayload(bundle.coreTemplate, {
    __SKIN_CSS_JSON__: JSON.stringify(bundle.css),
    __SKIN_ART_JSON__: JSON.stringify(""),
    __SKIN_THEME_JSON__: JSON.stringify(bundle.theme),
    __SKIN_MARKERS_JSON__: JSON.stringify(bundle.markers),
    __SKIN_PLUGIN_JSON__: JSON.stringify(bundle.plugin),
    __SKIN_REVISION_JSON__: JSON.stringify(bundle.revision),
  });
  assertPayloadSyntax(shellPayload);

  const result = {
    payload: shellPayload,
    shellPayload,
    fingerprint: bundle.fingerprint,
    revision: bundle.revision,
    markers: bundle.markers,
    theme: bundle.theme,
    manifest: bundle.manifest,
    artMetadata: bundle.artMetadata,
    sourceStamp: bundle.sourceStamp,
    payloadBytes: Buffer.byteLength(shellPayload, "utf8"),
    shellBytes: Buffer.byteLength(shellPayload, "utf8"),
    recommended: bundle.recommended,
    phase: "shell",
    deferredArt: true,
    cacheHit: false,
  };
  cacheSet(cacheKey, result);
  return result;
}

/**
 * Phase-2 art patch only (applyArt). Call after shell is verified.
 * When art.mode=none (or no bytes), returns empty art payload — caller skips CDP.
 */
export async function buildArtPayload(skinDir, preloaded = null) {
  const bundle = preloaded || (await loadSkinBundle(skinDir));
  const cacheKey = `art:${bundle.fingerprint}`;
  const hit = cacheGet(cacheKey);
  if (hit) return hit;

  const hasArt = Boolean(bundle.hasArt && bundle.artBytes?.length);
  const artDataUrl = hasArt ? artDataUrlFromBundle(bundle) : "";
  const artPayload = hasArt
    ? assembleArtPayload(bundle.markers, artDataUrl, bundle.revision)
    : "";

  const result = {
    payload: artPayload,
    artPayload,
    artDataUrl,
    fingerprint: bundle.fingerprint,
    revision: bundle.revision,
    markers: bundle.markers,
    theme: bundle.theme,
    manifest: bundle.manifest,
    artMetadata: bundle.artMetadata,
    sourceStamp: bundle.sourceStamp,
    payloadBytes: Buffer.byteLength(artPayload, "utf8"),
    artBytes: bundle.artBytes.length,
    hasArt,
    recommended: bundle.recommended,
    phase: "art",
    cacheHit: false,
  };
  cacheSet(cacheKey, result);
  return result;
}

/**
 * Delta shell only (no core). Used when page host is already resident.
 */
export async function buildDeltaShellPayload(skinDir, preloaded = null) {
  const bundle = preloaded || (await loadSkinBundle(skinDir));
  const cacheKey = `delta:${bundle.fingerprint}`;
  const hit = cacheGet(cacheKey);
  if (hit) return hit;

  const deltaShellPayload = assembleDeltaShellPayload({
    css: bundle.css,
    markers: bundle.markers,
    theme: bundle.theme,
    plugin: bundle.plugin,
    revision: bundle.revision,
  });

  const result = {
    payload: deltaShellPayload,
    deltaShellPayload,
    fingerprint: bundle.fingerprint,
    revision: bundle.revision,
    coreRevision: bundle.coreRevision,
    markers: bundle.markers,
    theme: bundle.theme,
    manifest: bundle.manifest,
    artMetadata: bundle.artMetadata,
    sourceStamp: bundle.sourceStamp,
    payloadBytes: Buffer.byteLength(deltaShellPayload, "utf8"),
    deltaShellBytes: Buffer.byteLength(deltaShellPayload, "utf8"),
    recommended: bundle.recommended,
    phase: "delta-shell",
    deferredArt: true,
    cacheHit: false,
  };
  cacheSet(cacheKey, result);
  return result;
}

/**
 * Preferred inject path: shell + deferred art as separate CDP evaluates.
 * Also builds deltaShell for hot-switch when slim core is already on the page.
 */
export async function buildStagedPayload(skinDir, preloaded = null) {
  const bundle = preloaded || (await loadSkinBundle(skinDir));
  const cacheKey = `staged:${bundle.fingerprint}`;
  const hit = cacheGet(cacheKey);
  if (hit) return hit;

  const shell = await buildShellPayload(skinDir, bundle);
  const art = await buildArtPayload(skinDir, bundle);
  const delta = await buildDeltaShellPayload(skinDir, bundle);

  const hasArt = Boolean(bundle.hasArt && bundle.artBytes?.length);
  const result = {
    // Default "payload" is shell for soft-verify / early document inject.
    payload: shell.shellPayload || shell.payload,
    shellPayload: shell.shellPayload || shell.payload,
    deltaShellPayload: delta.deltaShellPayload || delta.payload,
    artPayload: art.artPayload || art.payload,
    artDataUrl: art.artDataUrl,
    fingerprint: bundle.fingerprint,
    revision: bundle.revision,
    coreRevision: bundle.coreRevision,
    markers: bundle.markers,
    theme: bundle.theme,
    manifest: bundle.manifest,
    artMetadata: bundle.artMetadata,
    sourceStamp: bundle.sourceStamp,
    payloadBytes: shell.payloadBytes,
    shellBytes: shell.payloadBytes,
    deltaShellBytes: delta.payloadBytes,
    artPayloadBytes: art.payloadBytes,
    artBytes: bundle.artBytes.length,
    totalBytes: shell.payloadBytes + art.payloadBytes,
    recommended: bundle.recommended,
    phase: "staged",
    deferredArt: hasArt,
    hasArt,
    artMode: bundle.artMode,
    artPaint: bundle.artPaint,
    supportsDelta: true,
    cacheHit: false,
  };
  cacheSet(cacheKey, result);
  return result;
}

/**
 * Monolithic payload (core + CSS + art in one evaluate). Kept for diagnostics
 * and any caller that still expects a single script.
 */
export async function buildPayload(skinDir, preloaded = null) {
  const bundle = preloaded || (await loadSkinBundle(skinDir));
  const cacheKey = `full:${bundle.fingerprint}`;
  const hit = cacheGet(cacheKey);
  if (hit) return hit;

  const hasArt = Boolean(bundle.hasArt && bundle.artBytes?.length);
  const artDataUrl = hasArt ? artDataUrlFromBundle(bundle) : "";
  const finalPayload = assemblePayload(bundle.coreTemplate, {
    __SKIN_CSS_JSON__: JSON.stringify(bundle.css),
    __SKIN_ART_JSON__: JSON.stringify(artDataUrl),
    __SKIN_THEME_JSON__: JSON.stringify(bundle.theme),
    __SKIN_MARKERS_JSON__: JSON.stringify(bundle.markers),
    __SKIN_PLUGIN_JSON__: JSON.stringify(bundle.plugin),
    __SKIN_REVISION_JSON__: JSON.stringify(bundle.revision),
  });
  assertPayloadSyntax(finalPayload);

  const result = {
    payload: finalPayload,
    fingerprint: bundle.fingerprint,
    revision: bundle.revision,
    markers: bundle.markers,
    theme: bundle.theme,
    manifest: bundle.manifest,
    artMetadata: bundle.artMetadata,
    sourceStamp: bundle.sourceStamp,
    payloadBytes: Buffer.byteLength(finalPayload, "utf8"),
    recommended: bundle.recommended,
    phase: "full",
    deferredArt: false,
    hasArt,
    artMode: bundle.artMode,
    artPaint: bundle.artPaint,
    cacheHit: false,
  };

  cacheSet(cacheKey, result);
  return result;
}

export function clearPayloadCache() {
  payloadCache.clear();
}

export async function checkSkinPayload(skinDir) {
  const bundle = await loadSkinBundle(skinDir);
  const staged = await buildStagedPayload(skinDir, bundle);
  const full = await buildPayload(skinDir, bundle);
  return {
    pass: true,
    skinId: bundle.manifest.id,
    fingerprint: staged.fingerprint,
    revision: staged.revision,
    coreRevision: bundle.coreRevision,
    payloadBytes: full.payloadBytes,
    shellBytes: staged.shellBytes,
    deltaShellBytes: staged.deltaShellBytes,
    artPayloadBytes: staged.artPayloadBytes,
    totalStagedBytes: staged.totalBytes,
    artBytes: bundle.artBytes.length,
    hasArt: Boolean(bundle.hasArt),
    artMode: bundle.artMode,
    artPaint: bundle.artPaint,
    recommended: bundle.recommended,
    artMetadata: bundle.artMetadata,
    appearance: bundle.theme.appearance,
    art: bundle.theme.art,
    mime: bundle.mime,
    pluginVersion: bundle.plugin.version,
    deferredArt: Boolean(bundle.hasArt),
    supportsDelta: true,
    phase: "staged",
  };
}
