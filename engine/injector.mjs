/**
 * ChatGPT Tools CDP injector — hardened session + shared payload pipeline.
 *
 * Modes: --watch | --once | --verify | --remove | --check-payload | --self-test
 *
 * Long-lived watch + control file:
 *   manager writes control.json { cmd: "switch", skinDir, requestId }
 *   watch reloads staged payload and delta-applies to open sessions (no respawn).
 */
import fs from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import {
  buildStagedPayload,
  checkSkinPayload,
  loadSkinBundle,
} from "./payload.mjs";
import { ENGINE_VERSION } from "./version.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const { WebSocket } = require("./ws-polyfill.cjs");

const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]", "::1"]);
/** CDP browser UUID from /devtools/browser/<id> — never full Chrome version strings. */
const BROWSER_ID_PATTERN = /^[A-Za-z0-9._-]{1,200}$/;
/** Fallback full rebuild interval when fs.watch is unavailable. */
const STRONG_THEME_AUDIT_MS = 120000;
/** Large original art is intentional; CDP evaluate timeout scales with payload size.
 *  Relaxed for multi-MB wallpapers (up to 16 MB): longer base + more budget per byte. */
const ART_EVAL_BASE_TIMEOUT_MS = 30000;
const ART_EVAL_BYTES_PER_MS = 250;
const CONTROL_POLL_MS = 280;

class CdpIdentityMismatchError extends Error {}

/**
 * Extract stable browser id from /json/version payload.
 * Must be the path segment of webSocketDebuggerUrl, e.g.
 *   ws://127.0.0.1:9335/devtools/browser/ad1a38f4-…  →  ad1a38f4-…
 * Never join Browser|ws|protocol (contains / and | → rejected by CLI regex).
 */
function browserIdFromVersion(version, port) {
  const urlText = version?.webSocketDebuggerUrl || version?.["webSocketDebuggerUrl"];
  if (!urlText || typeof urlText !== "string") {
    throw new Error("CDP version missing webSocketDebuggerUrl");
  }
  const url = assertLoopbackWsUrl(urlText, port);
  const match = url.pathname.match(/^\/devtools\/browser\/([A-Za-z0-9._-]{1,200})$/);
  if (!match || url.search || url.hash || !BROWSER_ID_PATTERN.test(match[1])) {
    throw new Error(`Rejected an invalid CDP browser identity URL: ${url.pathname}`);
  }
  return match[1];
}

function parseArgs(argv) {
  const options = {
    port: 9335,
    mode: "watch",
    timeoutMs: 30000,
    screenshot: null,
    reload: false,
    skinDir: null,
    soft: false,
    browserId: null,
    pauseFile: null,
    controlFile: null,
    preferDelta: true,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--port") options.port = Number(argv[++i]);
    else if (arg === "--once") options.mode = "once";
    else if (arg === "--watch") options.mode = "watch";
    else if (arg === "--verify") options.mode = "verify";
    else if (arg === "--remove") options.mode = "remove";
    else if (arg === "--check-payload") options.mode = "check-payload";
    else if (arg === "--self-test") options.mode = "self-test";
    else if (arg === "--timeout-ms") options.timeoutMs = Number(argv[++i]);
    else if (arg === "--screenshot") options.screenshot = path.resolve(argv[++i]);
    else if (arg === "--reload") options.reload = true;
    else if (arg === "--skin-dir") options.skinDir = path.resolve(argv[++i]);
    else if (arg === "--soft") options.soft = true;
    else if (arg === "--browser-id") options.browserId = argv[++i];
    else if (arg === "--pause-file") options.pauseFile = path.resolve(argv[++i]);
    else if (arg === "--control-file") options.controlFile = path.resolve(argv[++i]);
    else if (arg === "--no-delta") options.preferDelta = false;
    else throw new Error(`Unknown argument: ${arg}`);
  }
  if (!Number.isInteger(options.port) || options.port < 1024 || options.port > 65535) {
    throw new Error(`Invalid port: ${options.port}`);
  }
  if (
    !Number.isInteger(options.timeoutMs) ||
    options.timeoutMs < 250 ||
    options.timeoutMs > 120000
  ) {
    throw new Error(`Invalid timeout: ${options.timeoutMs}`);
  }
  if (options.mode !== "self-test" && options.mode !== "check-payload" && !options.skinDir) {
    throw new Error("--skin-dir is required");
  }
  if (options.browserId != null && options.browserId !== "") {
    // Accept raw UUID, or tolerate accidental full WS URL / legacy compound keys.
    options.browserId = normalizeBrowserIdArg(options.browserId, options.port);
    if (!BROWSER_ID_PATTERN.test(options.browserId)) {
      throw new Error(`Invalid --browser-id: ${options.browserId}`);
    }
  } else {
    options.browserId = null;
  }
  return options;
}

/** Normalize legacy compound ids or full debugger URLs into a bare browser UUID. */
function normalizeBrowserIdArg(raw, port) {
  const text = String(raw || "").trim();
  if (!text) return text;
  if (BROWSER_ID_PATTERN.test(text)) return text;
  // Full WS URL
  if (text.startsWith("ws://") || text.startsWith("wss://")) {
    try {
      return browserIdFromVersion({ webSocketDebuggerUrl: text }, port);
    } catch {
      /* fall through */
    }
  }
  // Legacy compound: Browser|ws://...|protocol
  const wsMatch = text.match(/ws:\/\/[^|\s]+\/devtools\/browser\/([A-Za-z0-9._-]{1,200})/i);
  if (wsMatch && BROWSER_ID_PATTERN.test(wsMatch[1])) return wsMatch[1];
  // Path-only: /devtools/browser/<id>
  const pathMatch = text.match(/\/devtools\/browser\/([A-Za-z0-9._-]{1,200})/);
  if (pathMatch && BROWSER_ID_PATTERN.test(pathMatch[1])) return pathMatch[1];
  return text;
}

function assertLoopbackWsUrl(urlText, port) {
  let url;
  try {
    url = new URL(urlText);
  } catch {
    throw new Error("Invalid WebSocket debugger URL");
  }
  if (url.protocol !== "ws:" && url.protocol !== "wss:") {
    throw new Error("Debugger URL must use ws/wss");
  }
  if (!LOOPBACK_HOSTS.has(url.hostname)) {
    throw new Error(`Debugger URL host is not loopback: ${url.hostname}`);
  }
  const urlPort = url.port ? Number(url.port) : url.protocol === "wss:" ? 443 : 80;
  if (urlPort !== port) {
    throw new Error(`Debugger URL port mismatch: expected ${port}, got ${urlPort}`);
  }
  return url;
}

class CdpSession {
  constructor(target, port) {
    assertLoopbackWsUrl(target.webSocketDebuggerUrl, port);
    this.target = target;
    this.port = port;
    this.ws = new WebSocket(target.webSocketDebuggerUrl);
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Map();
    this.closed = false;
    this.defaultTimeoutMs = 12000;
  }

  async open() {
    await new Promise((resolve, reject) => {
      const onOpen = () => {
        cleanup();
        resolve();
      };
      const onError = (error) => {
        cleanup();
        reject(error instanceof Error ? error : new Error("WebSocket error"));
      };
      const timer = setTimeout(() => {
        cleanup();
        reject(new Error("CDP WebSocket open timed out"));
      }, 8000);
      const cleanup = () => {
        clearTimeout(timer);
        this.ws.removeEventListener("open", onOpen);
        this.ws.removeEventListener("error", onError);
      };
      this.ws.addEventListener("open", onOpen, { once: true });
      this.ws.addEventListener("error", onError, { once: true });
    });
    this.ws.addEventListener("message", (event) => this.onMessage(event));
    this.ws.addEventListener("close", () => {
      this.closed = true;
      for (const waiter of this.pending.values()) {
        waiter.reject(new Error("CDP socket closed"));
      }
      this.pending.clear();
    });
    await this.send("Runtime.enable");
    await this.send("Page.enable");
    return this;
  }

  onMessage(event) {
    let message;
    try {
      message = JSON.parse(String(event.data));
    } catch {
      this.close();
      return;
    }
    if (!message || typeof message !== "object" || Array.isArray(message)) {
      this.close();
      return;
    }
    if (message.id) {
      const waiter = this.pending.get(message.id);
      if (!waiter) return;
      this.pending.delete(message.id);
      if (message.error) {
        waiter.reject(new Error(`${message.error.message} (${message.error.code})`));
      } else {
        waiter.resolve(message.result);
      }
      return;
    }
    if (typeof message.method === "string") {
      for (const listener of this.listeners.get(message.method) ?? []) {
        listener(message.params ?? {});
      }
    }
  }

  on(method, listener) {
    const listeners = this.listeners.get(method) ?? [];
    listeners.push(listener);
    this.listeners.set(method, listeners);
  }

  send(method, params = {}, timeoutMs = this.defaultTimeoutMs) {
    if (this.closed) return Promise.reject(new Error("CDP session is closed"));
    return new Promise((resolve, reject) => {
      const id = this.nextId++;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP command timed out: ${method}`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
      try {
        this.ws.send(JSON.stringify({ id, method, params }));
      } catch (error) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(error);
      }
    });
  }

  async evaluate(expression) {
    const result = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
      userGesture: false,
    });
    if (result.exceptionDetails) {
      const detail =
        result.exceptionDetails.exception?.description ?? result.exceptionDetails.text;
      throw new Error(`Renderer evaluation failed: ${detail}`);
    }
    return result.result?.value;
  }

  close() {
    if (!this.closed) {
      try {
        this.ws.close();
      } catch {
        /* ignore */
      }
    }
    this.closed = true;
  }
}

async function fetchJsonLoopback(port, pathname, timeoutMs = 3000) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(`http://127.0.0.1:${port}${pathname}`, {
      signal: controller.signal,
      redirect: "error",
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return await response.json();
  } finally {
    clearTimeout(timer);
  }
}

async function readBrowserIdentity(port) {
  const version = await fetchJsonLoopback(port, "/json/version");
  const browserId = browserIdFromVersion(version, port);
  return {
    browserId,
    raw: version,
    webSocketDebuggerUrl: version?.webSocketDebuggerUrl || null,
    browser: version?.Browser || version?.browser || null,
  };
}

async function listAppTargets(port, expectedBrowserId = null) {
  if (expectedBrowserId) {
    const identity = await readBrowserIdentity(port);
    if (identity.browserId !== expectedBrowserId) {
      throw new CdpIdentityMismatchError(
        "CDP browser identity changed; refusing to attach to a recycled port"
      );
    }
  }
  const targets = await fetchJsonLoopback(port, "/json/list");
  if (!Array.isArray(targets)) throw new Error("Invalid /json/list payload");
  return targets.filter(
    (item) =>
      item &&
      item.type === "page" &&
      typeof item.webSocketDebuggerUrl === "string" &&
      String(item.url || "").startsWith("app://")
  );
}

async function waitForTargets(port, timeoutMs, expectedBrowserId = null) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const pages = await listAppTargets(port, expectedBrowserId);
      if (pages.length) return pages;
      lastError = new Error("No app:// page targets");
    } catch (error) {
      if (error instanceof CdpIdentityMismatchError) throw error;
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 350));
  }
  throw new Error(
    `No Codex renderer target on 127.0.0.1:${port}: ${lastError?.message ?? "timed out"}`
  );
}

async function probeSession(session) {
  return session.evaluate(`(() => {
    const shell = Boolean(document.querySelector('main.main-surface') || document.querySelector('main') || document.querySelector('[role="main"]'));
    const sidebar = Boolean(document.querySelector('aside.app-shell-left-panel'));
    const composer = Boolean(document.querySelector('.composer-surface-chrome'));
    const main = Boolean(document.querySelector('[role="main"]'));
    return {
      markers: { shell, sidebar, composer, main },
      // Sidebar is optional (collapsed rail). Pets/blank windows have no shell.
      codex: location.protocol === 'app:' && shell && (composer || main || sidebar),
    };
  })()`);
}

async function connectTarget(target, port) {
  return new CdpSession(target, port).open();
}

function artEvaluateTimeoutMs(staged) {
  const bytes = Number(staged?.artPayloadBytes || staged?.artBytes || 0);
  if (!bytes) return ART_EVAL_BASE_TIMEOUT_MS;
  return Math.min(
    180000,
    Math.max(ART_EVAL_BASE_TIMEOUT_MS, Math.round(bytes / ART_EVAL_BYTES_PER_MS) + 12000)
  );
}

async function applyToSession(session, payload, timeoutMs = session.defaultTimeoutMs) {
  const result = await session.send(
    "Runtime.evaluate",
    {
      expression: payload,
      awaitPromise: true,
      returnByValue: true,
      userGesture: false,
    },
    timeoutMs
  );
  if (result.exceptionDetails) {
    const detail =
      result.exceptionDetails.exception?.description ?? result.exceptionDetails.text;
    throw new Error(`Renderer evaluation failed: ${detail}`);
  }
  return result.result?.value;
}

/**
 * Probe whether the slim core host bridge is already on the page.
 */
async function probeHostResident(session) {
  try {
    return await session.evaluate(`(() => {
      const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
      if (!host || typeof host.applySkin !== "function") {
        return { resident: false };
      }
      const active = typeof host.getActive === "function" ? host.getActive() : null;
      return {
        resident: true,
        coreRevision: host.coreRevision || null,
        revision: active?.revision || null,
        skinId: active?.skinId || null,
        artReady: Boolean(active?.artReady),
      };
    })()`);
  } catch {
    return { resident: false };
  }
}

/**
 * Two-phase inject with optional delta (no core re-ship when host is resident).
 * Soft verify only needs shell; art is best-effort and does not block success.
 */
async function applyStagedToSession(
  session,
  staged,
  { art = true, preferDelta = true } = {}
) {
  let shellMode = "full";
  let shellResult = null;

  if (preferDelta && staged.deltaShellPayload) {
    const host = await probeHostResident(session);
    const coreOk =
      !staged.coreRevision ||
      !host.coreRevision ||
      host.coreRevision === staged.coreRevision;
    if (host.resident && coreOk) {
      try {
        shellResult = await applyToSession(session, staged.deltaShellPayload, 12000);
        if (shellResult?.ok) {
          shellMode = "delta";
        } else if (shellResult?.needsFullShell) {
          shellResult = null;
        }
      } catch {
        shellResult = null;
      }
    }
  }

  if (!shellResult?.ok) {
    const shellScript = staged.shellPayload || staged.payload;
    shellResult = await applyToSession(session, shellScript, 15000);
    shellMode = shellResult?.mode === "delta" ? "delta" : "full";
  }

  let artResult = null;
  if (art && staged.artPayload) {
    try {
      artResult = await applyToSession(
        session,
        staged.artPayload,
        artEvaluateTimeoutMs(staged)
      );
    } catch (error) {
      artResult = { ok: false, reason: "art-evaluate-failed", message: error.message };
    }
  }
  const artOk =
    artResult == null
      ? !staged.artPayload
      : Boolean(artResult?.ok === true || artResult?.already === true);
  return {
    shell: shellResult,
    art: artResult,
    shellMode,
    deferredArt: Boolean(staged.deferredArt),
    artOk,
    artPending: Boolean(staged.artPayload) && !artOk,
  };
}

/** Normalize buildStagedPayload / buildPayload results for watch reload checks. */
function stagedFromBuilt(built) {
  // Staged shell is present even when art.mode=none (empty artPayload is valid).
  if (built?.shellPayload && (built.phase === "staged" || "hasArt" in (built || {}))) {
    return {
      ...built,
      artPayload: built.artPayload || null,
      deferredArt: Boolean(built.hasArt && built.artPayload),
    };
  }
  if (built?.shellPayload && built?.artPayload) return built;
  // Monolithic fallback — treat full payload as shell only (no separate art).
  return {
    ...built,
    shellPayload: built.payload,
    deltaShellPayload: built.deltaShellPayload || null,
    artPayload: null,
    deferredArt: false,
    payload: built.payload,
  };
}

async function writeControlResult(controlFile, result) {
  if (!controlFile) return;
  const resultPath = `${controlFile}.result`;
  try {
    await fs.writeFile(resultPath, JSON.stringify(result, null, 2) + "\n", "utf8");
  } catch {
    /* ignore */
  }
}

async function readControlCommand(controlFile) {
  if (!controlFile) return null;
  try {
    const text = await fs.readFile(controlFile, "utf8");
    const json = JSON.parse(text);
    if (!json || typeof json !== "object") return null;
    return json;
  } catch {
    return null;
  }
}

async function clearControlCommand(controlFile) {
  if (!controlFile) return;
  try {
    await fs.unlink(controlFile);
  } catch {
    /* ignore */
  }
}

async function removeFromSession(session, markers) {
  // Prefer full state.cleanup; fallback matches purge-all DOM semantics (no reload).
  return session.evaluate(`(() => {
    const markers = ${JSON.stringify({
      disabledKey: markers.disabledKey,
      stateKey: markers.stateKey,
      rootClass: markers.rootClass,
      artVar: markers.artVar,
      styleId: markers.styleId,
      chromeId: markers.chromeId,
      homeClass: markers.homeClass,
      homeShellClass: markers.homeShellClass,
      homeUtilityClass: markers.homeUtilityClass,
    })};
    try { window[markers.disabledKey] = true; } catch {}
    const state = window[markers.stateKey];
    if (state?.cleanup) {
      try { return state.cleanup(); } catch {}
    }
    const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
    if (host?.cleanup) {
      try { return host.cleanup(); } catch {}
    }
    const root = document.documentElement;
    const themeClasses = [
      'skins-theme-light','skins-theme-dark','skins-art-wide','skins-art-standard','skins-art-none',
      'skins-focus-left','skins-focus-center','skins-focus-right',
      'skins-safe-left','skins-safe-center','skins-safe-right','skins-safe-none',
      'skins-task-ambient','skins-task-banner','skins-task-off',
      'dream-theme-light','dream-theme-dark','dream-art-wide','dream-art-standard',
      'dream-focus-left','dream-focus-center','dream-focus-right',
      'dream-safe-left','dream-safe-center','dream-safe-right','dream-safe-none',
      'dream-task-ambient','dream-task-banner','dream-task-off'
    ];
    root?.classList.remove(markers.rootClass, ...themeClasses);
    for (const prop of [
      markers.artVar, '--skins-art', '--skins-art-position', '--skins-focus-x',
      '--skins-focus-y', '--skins-accent', '--skins-accent-ink', '--skins-image-luma',
      '--dream-art', '--dream-art-position', '--dream-focus-x',
      '--dream-focus-y', '--dream-accent', '--dream-accent-ink', '--dream-image-luma'
    ].filter(Boolean)) {
      root?.style.removeProperty(prop);
    }
    root?.removeAttribute('data-chatgpt-tools-skin');
    root?.removeAttribute('data-skins-shell');
    root?.removeAttribute('data-skins-art-mode');
    root?.removeAttribute('data-skins-art-paint');
    root?.removeAttribute('data-skin-contract');
    root?.removeAttribute('data-dream-shell');
    document.getElementById(markers.styleId)?.remove();
    document.getElementById(markers.chromeId)?.remove();
    document.querySelectorAll('style[data-skin-revision], style[id*="-skin-style"]').forEach((n) => n.remove());
    document.querySelectorAll('[id*="-skin-chrome"]').forEach((n) => n.remove());
    const home = markers.homeClass;
    const shell = markers.homeShellClass || (home ? home + '-shell' : null);
    const utility = markers.homeUtilityClass || (home ? home + '-utility' : null);
    for (const cls of [home, shell, utility].filter(Boolean)) {
      document.querySelectorAll('.' + cls).forEach((n) => n.classList.remove(cls));
    }
    try { delete window[markers.stateKey]; } catch {}
    return true;
  })()`);
}

/**
 * soft: root class + style present (fast UX).
 * hard: also chrome pointer-events + main shell; sidebar optional; home cards optional.
 */
async function verifySession(session, markers, soft = false) {
  return session.evaluate(`((soft, markers) => {
    const box = (node) => {
      if (!node) return null;
      const r = node.getBoundingClientRect();
      return { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) };
    };
    const root = document.documentElement;
    const style = document.getElementById(markers.styleId);
    const chrome = document.getElementById(markers.chromeId);
    const home = document.querySelector('.' + markers.homeClass);
    const shellMain = document.querySelector('main.main-surface') || document.querySelector('main') || document.querySelector('[role="main"]');
    const suggestions = home?.querySelector('.group\\\\/home-suggestions') ?? null;
    const cards = suggestions ? [...suggestions.querySelectorAll('button')].map(box) : [];
    const composer = document.querySelector('.composer-surface-chrome');
    const sidebar = document.querySelector('aside.app-shell-left-panel');
    const result = {
      installed: root.classList.contains(markers.rootClass),
      version: window[markers.stateKey]?.version ?? null,
      revision: window[markers.stateKey]?.revision ?? null,
      stylePresent: Boolean(style),
      chromePresent: Boolean(chrome),
      chromePointerEvents: chrome ? getComputedStyle(chrome).pointerEvents : null,
      homePresent: Boolean(home),
      suggestionsPresent: Boolean(suggestions),
      shellPresent: Boolean(shellMain),
      cards,
      composer: box(composer),
      sidebar: box(sidebar),
      viewport: { width: innerWidth, height: innerHeight },
    };
    const state = window[markers.stateKey];
    result.artReady = Boolean(state?.artReady);
    result.artOk = Boolean(state?.artReady && state?.artUrl);
    result.artPending = Boolean(state && !state.artReady);
    result.revision = state?.revision ?? result.revision;
    if (soft) {
      result.pass = result.installed && result.stylePresent;
    } else {
      const homeOk = !result.homePresent || !result.suggestionsPresent ||
        (result.cards.length >= 2 && result.cards.length <= 4);
      result.pass = result.installed && result.stylePresent && result.shellPresent &&
        (!result.chromePresent || result.chromePointerEvents === 'none') &&
        (Boolean(result.composer) || result.shellPresent) &&
        homeOk;
    }
    return result;
  })(${soft ? "true" : "false"}, ${JSON.stringify({
    rootClass: markers.rootClass,
    styleId: markers.styleId,
    chromeId: markers.chromeId,
    homeClass: markers.homeClass,
    stateKey: markers.stateKey,
  })})`);
}

async function waitForVerifiedSession(session, markers, timeoutMs, soft = false) {
  const deadline = Date.now() + timeoutMs;
  let lastResult;
  while (Date.now() < deadline) {
    lastResult = await verifySession(session, markers, soft);
    if (lastResult.pass) return lastResult;
    await new Promise((resolve) => setTimeout(resolve, soft ? 120 : 350));
  }
  return lastResult;
}

/** Non-interactive screenshot (no Escape / mouse). */
async function capture(session, outputPath) {
  await fs.mkdir(path.dirname(outputPath), { recursive: true });
  const result = await session.send("Page.captureScreenshot", {
    format: "png",
    fromSurface: true,
    captureBeyondViewport: false,
  });
  await fs.writeFile(outputPath, Buffer.from(result.data, "base64"));
}

/**
 * Early document script installs shell only (no multi-MB art).
 * Art is re-applied after load via applyStagedToSession.
 */
export function earlyPayloadFor(shellPayload, revision, markers) {
  return `(() => {
    const generationKey = "__CHATGPT_TOOLS_EARLY_GENERATION__";
    const appliedKey = "__CHATGPT_TOOLS_EARLY_APPLIED__";
    const generation = ${JSON.stringify(revision)};
    window[generationKey] = generation;
    let observer = null;
    let timeout = null;
    const stop = () => {
      observer?.disconnect();
      observer = null;
      if (timeout) clearTimeout(timeout);
      timeout = null;
    };
    const install = () => {
      if (window[generationKey] !== generation) { stop(); return true; }
      const root = document.documentElement;
      if (!root || !document.body) return false;
      const shell = document.querySelector('main.main-surface') ||
        document.querySelector('main') ||
        document.querySelector('[role="main"]');
      if (!shell) return false;
      stop();
      ${shellPayload};
      window[appliedKey] = generation;
      return true;
    };
    if (install()) return;
    if (typeof MutationObserver === "function" && document.documentElement) {
      observer = new MutationObserver(install);
      observer.observe(document.documentElement, { childList: true, subtree: true });
    }
    timeout = setTimeout(stop, 10000);
  })()`;
}

async function registerEarlyPayload(session, shellPayload, revision, markers) {
  try {
    const source = earlyPayloadFor(shellPayload, revision, markers);
    const result = await session.send("Page.addScriptToEvaluateOnNewDocument", { source });
    return result?.identifier ?? true;
  } catch {
    return null;
  }
}

async function fileExists(filePath) {
  if (!filePath) return false;
  try {
    return (await fs.stat(filePath)).isFile();
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

async function runOneShot(options) {
  const soft = options.soft || options.mode === "once";
  let browserId = options.browserId;
  if (!browserId && (options.mode === "verify" || options.mode === "once" || options.mode === "remove")) {
    try {
      browserId = (await readBrowserIdentity(options.port)).browserId;
    } catch {
      browserId = null;
    }
  }

  const loaded =
    options.mode === "remove"
      ? {
          markers: (await loadSkinBundle(options.skinDir)).markers,
          payload: null,
          shellPayload: null,
          artPayload: null,
          revision: null,
          deferredArt: false,
        }
      : stagedFromBuilt(await buildStagedPayload(options.skinDir));

  const targets = await waitForTargets(
    options.port,
    Math.min(options.timeoutMs, soft ? 4000 : options.timeoutMs),
    browserId
  );
  const results = [];

  for (const target of targets) {
    const session = await connectTarget(target, options.port);
    try {
      if (options.mode === "remove") {
        await removeFromSession(session, loaded.markers);
      } else if (options.mode === "once") {
        const probe = await probeSession(session);
        if (!probe?.markers?.shell && !options.soft) {
          results.push({
            targetId: target.id,
            result: { pass: false, reason: "no-shell" },
          });
          continue;
        }
        // Soft once: shell first (success not gated on multi-MB art).
        // preferDelta reuses resident host when page already has slim core.
        await applyStagedToSession(session, loaded, {
          art: false,
          preferDelta: options.preferDelta !== false,
        });
      }
      if (options.mode === "once") {
        await new Promise((resolve) => setTimeout(resolve, soft ? 60 : 400));
      }
      if (options.reload) {
        await session.send("Page.reload", { ignoreCache: true });
        await new Promise((resolve) => setTimeout(resolve, 900));
        if (options.mode !== "remove" && loaded.shellPayload) {
          await applyStagedToSession(session, loaded, {
            art: true,
            preferDelta: options.preferDelta !== false,
          });
        }
      }
      let verified;
      let stagedMeta = null;
      if (options.mode === "remove") {
        verified = await session.evaluate(
          `!document.documentElement.classList.contains(${JSON.stringify(loaded.markers.rootClass)})`
        );
      } else if (options.reload || options.mode === "once") {
        const verifyBudget = soft
          ? Math.min(options.timeoutMs, Math.max(2500, Math.round(options.timeoutMs * 0.6)))
          : options.timeoutMs;
        verified = await waitForVerifiedSession(
          session,
          loaded.markers,
          verifyBudget,
          soft
        );
        // Progressive art after shell pass (large originals allowed; may time out → artPending).
        // Only send art patch — do not re-evaluate shell (avoids double work with watch).
        if (verified?.pass && loaded.artPayload) {
          try {
            const artValue = await applyToSession(
              session,
              loaded.artPayload,
              artEvaluateTimeoutMs(loaded)
            );
            const artOk = Boolean(artValue?.ok === true || artValue?.already === true);
            stagedMeta = { artOk, artPending: !artOk, art: artValue };
            verified = {
              ...verified,
              artOk,
              artPending: !artOk,
              artAttached: artOk,
            };
          } catch (error) {
            verified = {
              ...verified,
              artOk: false,
              artPending: true,
              artError: error.message,
            };
          }
        }
      } else {
        verified = await verifySession(session, loaded.markers, soft);
      }
      results.push({
        targetId: target.id,
        // Do not log page titles/URLs (privacy)
        result: verified,
      });
      if (options.screenshot) await capture(session, options.screenshot);
      if (options.mode === "once" && verified?.pass) break;
    } finally {
      session.close();
    }
  }

  console.log(
    JSON.stringify(
      {
        mode: options.mode,
        port: options.port,
        version: ENGINE_VERSION,
        soft,
        browserId: browserId || null,
        targets: results,
      },
      null,
      2
    )
  );

  const failed = results.some((item) => {
    const r = item.result;
    if (typeof r === "boolean") return !r;
    return !r?.pass;
  });
  if ((options.mode === "verify" || options.mode === "once") && (failed || !results.length)) {
    process.exitCode = 2;
  }
}

async function runWatch(options) {
  let identity;
  try {
    identity = await readBrowserIdentity(options.port);
  } catch (error) {
    throw new Error(`Cannot read CDP identity: ${error.message}`);
  }
  const expectedBrowserId = options.browserId || identity.browserId;
  // Staged: shell + deferred art; early document scripts only carry shell.
  let activeSkinDir = options.skinDir;
  let loadedPayload = stagedFromBuilt(await buildStagedPayload(activeSkinDir));
  let lastStrongThemeAuditAt = Date.now();
  let paused = await fileExists(options.pauseFile);
  const sessions = new Map();
  const earlyScripts = new Map();
  const fallbackTargets = new Map();
  const targetFailures = new Map();
  let stopping = false;
  let listFailures = 0;
  let lastListErrorLogAt = 0;
  let lastThemeErrorLogAt = 0;
  let lastControlPollAt = 0;
  let lastHandledRequestId = null;
  /** fs.watch dirty flag — rebuild payload only when skin dir changes. */
  let skinDirDirty = false;
  let skinWatcher = null;

  const stop = () => {
    stopping = true;
  };
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);

  const attachSkinWatcher = async (dir) => {
    if (skinWatcher) {
      try {
        skinWatcher.close();
      } catch {
        /* ignore */
      }
      skinWatcher = null;
    }
    try {
      const fsSync = await import("node:fs");
      skinWatcher = fsSync.watch(dir, { recursive: true }, () => {
        skinDirDirty = true;
      });
      skinWatcher.on?.("error", () => {
        skinDirDirty = true;
      });
    } catch {
      skinWatcher = null;
    }
  };
  await attachSkinWatcher(activeSkinDir);

  const rejectTarget = (targetId, baseDelayMs, error = null) => {
    const previous = targetFailures.get(targetId) ?? { failures: 0, lastLogAt: 0 };
    const failures = previous.failures + 1;
    const delayMs = Math.min(30000, baseDelayMs * 2 ** Math.min(failures - 1, 4));
    const now = Date.now();
    if (error && (failures === 1 || now - previous.lastLogAt >= 30000)) {
      console.error(
        `[skin] inject failed for ${targetId}: ${error.message}; retrying in ${delayMs}ms`
      );
      previous.lastLogAt = now;
    }
    targetFailures.set(targetId, { failures, lastLogAt: previous.lastLogAt, until: now + delayMs });
  };

  const reinjectSession = async (session, { art = true, preferDelta = true } = {}) => {
    if (paused) {
      await removeFromSession(session, loadedPayload.markers);
      return null;
    }
    return applyStagedToSession(session, loadedPayload, { art, preferDelta });
  };

  /**
   * Hot-switch skin dir without process respawn.
   * Prefer delta shell when host is resident; always rebuild staged payloads.
   */
  const switchSkinDir = async (nextDir, requestId = null) => {
    const resolved = path.resolve(nextDir);
    if (!(await fileExists(path.join(resolved, "skin.json")))) {
      throw new Error(`switch: skin.json missing under ${resolved}`);
    }
    const nextPayload = stagedFromBuilt(await buildStagedPayload(resolved));
    const prevId = loadedPayload.manifest?.id;
    const nextId = nextPayload.manifest?.id;
    activeSkinDir = resolved;
    loadedPayload = nextPayload;
    skinDirDirty = false;
    lastStrongThemeAuditAt = Date.now();
    await attachSkinWatcher(activeSkinDir);

    let deltaSessions = 0;
    let fullSessions = 0;
    let artOkCount = 0;
    for (const [id, session] of sessions) {
      try {
        if (paused) {
          await removeFromSession(session, loadedPayload.markers);
          continue;
        }
        const applied = await reinjectSession(session, { art: true, preferDelta: true });
        if (applied?.shellMode === "delta") deltaSessions += 1;
        else fullSessions += 1;
        if (applied?.artOk) artOkCount += 1;
        try {
          await registerEarlyPayload(
            session,
            loadedPayload.shellPayload || loadedPayload.payload,
            loadedPayload.revision,
            loadedPayload.markers
          );
        } catch {
          /* optional */
        }
      } catch (error) {
        console.error(`[skin] hot-switch failed for ${id}: ${error.message}`);
      }
    }
    console.error(
      `[skin] hot-switch ${prevId || "?"} → ${nextId || "?"} ` +
        `delta=${deltaSessions} full=${fullSessions} artOk=${artOkCount} req=${requestId || "-"}`
    );
    return {
      ok: true,
      mode: "hot-switch",
      skinId: nextId,
      skinDir: resolved,
      revision: nextPayload.revision,
      sessions: sessions.size,
      deltaSessions,
      fullSessions,
      artOkCount,
      requestId,
    };
  };

  try {
    while (!stopping) {
      // Control channel: manager writes switch/ping without respawning this process.
      const nowTs = Date.now();
      if (options.controlFile && nowTs - lastControlPollAt >= CONTROL_POLL_MS) {
        lastControlPollAt = nowTs;
        const cmd = await readControlCommand(options.controlFile);
        if (cmd?.requestId && cmd.requestId !== lastHandledRequestId) {
          lastHandledRequestId = cmd.requestId;
          try {
            if (cmd.cmd === "switch" && cmd.skinDir) {
              const result = await switchSkinDir(cmd.skinDir, cmd.requestId);
              await writeControlResult(options.controlFile, {
                ...result,
                at: new Date().toISOString(),
              });
            } else if (cmd.cmd === "ping") {
              await writeControlResult(options.controlFile, {
                ok: true,
                mode: "ping",
                skinId: loadedPayload.manifest?.id || null,
                skinDir: activeSkinDir,
                revision: loadedPayload.revision,
                sessions: sessions.size,
                requestId: cmd.requestId,
                at: new Date().toISOString(),
              });
            } else if (cmd.cmd === "reapply") {
              let deltaSessions = 0;
              let fullSessions = 0;
              for (const session of sessions.values()) {
                const applied = await reinjectSession(session, {
                  art: true,
                  preferDelta: true,
                });
                if (applied?.shellMode === "delta") deltaSessions += 1;
                else fullSessions += 1;
              }
              await writeControlResult(options.controlFile, {
                ok: true,
                mode: "reapply",
                skinId: loadedPayload.manifest?.id || null,
                deltaSessions,
                fullSessions,
                sessions: sessions.size,
                requestId: cmd.requestId,
                at: new Date().toISOString(),
              });
            } else {
              await writeControlResult(options.controlFile, {
                ok: false,
                reason: "unknown-cmd",
                requestId: cmd.requestId,
              });
            }
          } catch (error) {
            await writeControlResult(options.controlFile, {
              ok: false,
              reason: "control-error",
              message: error.message,
              requestId: cmd.requestId,
              at: new Date().toISOString(),
            });
            console.error(`[skin] control command failed: ${error.message}`);
          }
          await clearControlCommand(options.controlFile);
        }
      }

      // Identity re-check periodically via listAppTargets
      let targets = [];
      try {
        targets = await listAppTargets(options.port, expectedBrowserId);
        listFailures = 0;
      } catch (error) {
        if (error instanceof CdpIdentityMismatchError) {
          console.error("[skin] original CDP browser identity closed; watcher is stopping");
          process.exitCode = 3;
          break;
        }
        listFailures += 1;
        const retryMs = Math.min(10000, 1000 * 2 ** Math.min(listFailures - 1, 4));
        if (listFailures === 1 || Date.now() - lastListErrorLogAt >= 30000) {
          console.error(`[skin] ${new Date().toISOString()} ${error.message}; retrying in ${retryMs}ms`);
          lastListErrorLogAt = Date.now();
        }
        // Host may still be starting after slow relaunch — keep watching.
        await new Promise((resolve) => setTimeout(resolve, retryMs));
        continue;
      }

      const nextPaused = await fileExists(options.pauseFile);
      let nextPayload = loadedPayload;
      if (!nextPaused) {
        try {
          const now = Date.now();
          const dueAudit = now - lastStrongThemeAuditAt >= STRONG_THEME_AUDIT_MS;
          let shouldAudit = skinDirDirty || dueAudit || !loadedPayload;
          if (shouldAudit) {
            skinDirDirty = false;
            lastStrongThemeAuditAt = now;
            try {
              const bundle = await loadSkinBundle(activeSkinDir);
              if (bundle.fingerprint === loadedPayload.fingerprint) {
                loadedPayload.sourceStamp = bundle.sourceStamp;
              } else {
                nextPayload = stagedFromBuilt(await buildStagedPayload(activeSkinDir, bundle));
              }
            } catch (error) {
              if (Date.now() - lastThemeErrorLogAt >= 30000) {
                console.error(
                  `[skin] theme update rejected: ${error.message}; keeping the active skin`
                );
                lastThemeErrorLogAt = Date.now();
              }
            }
          }
        } catch (error) {
          if (Date.now() - lastThemeErrorLogAt >= 30000) {
            console.error(
              `[skin] theme update rejected: ${error.message}; keeping the active skin`
            );
            lastThemeErrorLogAt = Date.now();
          }
        }
      }

      const pauseChanged = nextPaused !== paused;
      const payloadChanged = !nextPaused && nextPayload !== loadedPayload;
      loadedPayload = nextPayload;
      paused = nextPaused;

      if (pauseChanged || payloadChanged) {
        for (const [id, session] of sessions) {
          try {
            if (paused) {
              await removeFromSession(session, loadedPayload.markers);
            } else {
              await reinjectSession(session, { art: true, preferDelta: true });
              try {
                await registerEarlyPayload(
                  session,
                  loadedPayload.shellPayload || loadedPayload.payload,
                  loadedPayload.revision,
                  loadedPayload.markers
                );
              } catch {
                /* optional */
              }
            }
          } catch (error) {
            console.error(`[skin] refresh failed for ${id}: ${error.message}`);
          }
        }
      }

      const activeIds = new Set(targets.map((t) => t.id));
      for (const [id, session] of sessions) {
        if (!activeIds.has(id) || session.closed) {
          session.close();
          sessions.delete(id);
          earlyScripts.delete(id);
          fallbackTargets.delete(id);
          targetFailures.delete(id);
        }
      }

      for (const target of targets) {
        if (sessions.has(target.id)) continue;
        const failure = targetFailures.get(target.id);
        if (failure && Date.now() < failure.until) continue;
        try {
          const session = await connectTarget(target, options.port);
          const probe = await probeSession(session);
          // Keep connection for pages that may grow a shell; skip pure garbage.
          if (!probe?.codex && !probe?.markers?.shell) {
            session.close();
            // Slow start: page may not have shell yet — short backoff, not harsh.
            rejectTarget(target.id, 800, new Error("not a Codex shell yet"));
            continue;
          }

          if (!paused) {
            try {
              const earlyId = await registerEarlyPayload(
                session,
                loadedPayload.shellPayload || loadedPayload.payload,
                loadedPayload.revision,
                loadedPayload.markers
              );
              if (earlyId) earlyScripts.set(target.id, earlyId);
              fallbackTargets.set(target.id, !earlyId);
            } catch {
              fallbackTargets.set(target.id, true);
            }
            // Shell first; art second. Delta if host already resident on this page.
            await applyStagedToSession(session, loadedPayload, {
              art: true,
              preferDelta: true,
            });
          }

          session.on("Page.loadEventFired", () => {
            setTimeout(() => {
              if (paused) {
                removeFromSession(session, loadedPayload.markers).catch(() => {});
                return;
              }
              // Full document load: re-apply shell+art (SPA navigations rely on MutationObserver).
              reinjectSession(session, { art: true, preferDelta: true }).catch((error) => {
                console.error(`[skin] reinject failed: ${error.message}`);
              });
            }, 280);
          });

          sessions.set(target.id, session);
          targetFailures.delete(target.id);
          console.log(
            `[skin] injected ${loadedPayload.manifest?.id || "skin"} (shell${
              loadedPayload.deferredArt ? "+art" : ""
            }) -> ${target.id}${paused ? " (paused)" : ""}`
          );
        } catch (error) {
          rejectTarget(target.id, 1000, error);
        }
      }

      // Fewer polls when sessions are healthy; faster when waiting for first target.
      // Control file needs reasonably snappy response — cap poll when idle.
      const pollDelay = sessions.size
        ? Math.min(1200, CONTROL_POLL_MS * 3)
        : targets.length
          ? 450
          : 900;
      await new Promise((resolve) => setTimeout(resolve, pollDelay));
    }
  } finally {
    try {
      skinWatcher?.close?.();
    } catch {
      /* ignore */
    }
    for (const session of sessions.values()) session.close();
  }
}

async function runSelfTest() {
  // Unit-ish checks without a live Codex process
  const fakePort = 9335;
  try {
    assertLoopbackWsUrl("ws://127.0.0.1:9335/devtools/page/abc", fakePort);
  } catch (e) {
    throw new Error(`loopback accept failed: ${e.message}`);
  }
  let rejected = false;
  try {
    assertLoopbackWsUrl("ws://192.168.1.2:9335/devtools/page/abc", fakePort);
  } catch {
    rejected = true;
  }
  if (!rejected) throw new Error("non-loopback URL should be rejected");

  // browser id extraction: UUID only, not Chrome version compound keys
  const sampleId = browserIdFromVersion(
    {
      webSocketDebuggerUrl: `ws://127.0.0.1:${fakePort}/devtools/browser/test-browser-uuid-01`,
      Browser: "Chrome/150.0.0.0",
    },
    fakePort
  );
  if (sampleId !== "test-browser-uuid-01") {
    throw new Error(`browserIdFromVersion failed: ${sampleId}`);
  }
  const normalized = normalizeBrowserIdArg(
    `Chrome/150|ws://127.0.0.1:${fakePort}/devtools/browser/legacy-id-xyz|1.3`,
    fakePort
  );
  if (normalized !== "legacy-id-xyz") {
    throw new Error(`normalizeBrowserIdArg failed: ${normalized}`);
  }

  console.log(
    JSON.stringify({
      pass: true,
      version: ENGINE_VERSION,
      test: "loopback-cdp-validation",
      browserIdSample: sampleId,
    })
  );
}

const options = parseArgs(process.argv.slice(2));

if (options.mode === "self-test") {
  await runSelfTest();
} else if (options.mode === "check-payload") {
  if (!options.skinDir) throw new Error("--skin-dir is required for --check-payload");
  const report = await checkSkinPayload(options.skinDir);
  console.log(JSON.stringify(report));
} else if (options.mode === "watch") {
  await runWatch(options);
} else {
  await runOneShot(options);
}
