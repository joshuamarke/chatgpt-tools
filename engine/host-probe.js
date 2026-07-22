/**
 * Host (ChatGPT / Codex desktop) lifecycle probe.
 *
 * Slow machines often show process list lag or CDP up before any app:// page.
 * Treat three independent signals so UI/apply never rely on a single false negative.
 *
 *   processRunning  — OS process named ChatGPT/Codex (best-effort, L2)
 *   debugPortOpen   — loopback CDP HTTP answers (/json/version)
 *   rendererReady   — at least one app:// page target (injectable)
 *
 * lifecycle (stable, after hysteresis):
 *   offline   — confirmed no process, no port
 *   starting  — process and/or port, but no app:// renderer yet
 *   ready     — injectable renderer present (sticky against brief CDP blips)
 */
const { execFile } = require("child_process");
const { promisify } = require("util");
const fs = require("fs");
const path = require("path");

const execFileAsync = promisify(execFile);

const DEFAULT_PORT = Number(process.env.CODEX_SKIN_PORT || 9335);

const STICKY_READY_MS = 5000;
const OFFLINE_HOLD_MS = 3000;
const OFFLINE_CONFIRM = 2;
const CDP_CACHE_MS = 1000;
const PROCESS_CACHE_MS = 3000;
const SNAPSHOT_CACHE_MS = 1000;

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function fetchJsonLocal(port, pathname, timeoutMs = 2000) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(`http://127.0.0.1:${port}${pathname}`, {
      signal: controller.signal,
      redirect: "error",
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return await res.json();
  } finally {
    clearTimeout(timer);
  }
}

/** CDP HTTP is listening (version endpoint). Does not require app:// pages. */
async function isDebugPortOpen(port, timeoutMs = 2000) {
  try {
    const version = await fetchJsonLocal(port, "/json/version", timeoutMs);
    return Boolean(
      version &&
        (version.webSocketDebuggerUrl ||
          version["webSocketDebuggerUrl"] ||
          version.Browser ||
          version.browser)
    );
  } catch {
    return false;
  }
}

/** At least one injectable app:// page target. */
async function isRendererReady(port, timeoutMs = 2000) {
  try {
    const targets = await fetchJsonLocal(port, "/json/list", timeoutMs);
    if (!Array.isArray(targets)) return false;
    return targets.some(
      (t) => t?.type === "page" && String(t.url || "").startsWith("app://")
    );
  } catch {
    return false;
  }
}

function powershellExe() {
  const root = process.env.SystemRoot || "C:\\Windows";
  const candidates = [
    path.join(root, "System32", "WindowsPowerShell", "v1.0", "powershell.exe"),
    path.join(root, "SysWOW64", "WindowsPowerShell", "v1.0", "powershell.exe"),
    "powershell.exe",
  ];
  for (const c of candidates) {
    try {
      if (c === "powershell.exe" || fs.existsSync(c)) return c;
    } catch {
      /* ignore */
    }
  }
  return "powershell.exe";
}

/**
 * Best-effort main process PIDs. tasklist first; Get-Process/CIM only if empty.
 */
async function findHostMainPids() {
  if (process.platform === "darwin") {
    try {
      const { stdout } = await execFileAsync(
        "pgrep",
        [
          "-f",
          "/Applications/ChatGPT\\.app/Contents/MacOS/ChatGPT|/Applications/Codex\\.app/Contents/MacOS/(ChatGPT|Codex)|/ChatGPT\\.app/Contents/MacOS/ChatGPT",
        ],
        { timeout: 8000 }
      );
      return String(stdout)
        .trim()
        .split(/\s+/)
        .filter(Boolean)
        .map(Number)
        .filter((n) => Number.isFinite(n) && n > 0);
    } catch {
      return [];
    }
  }

  if (process.platform !== "win32") return [];

  const pids = new Set();

  // 1) tasklist — fast
  try {
    const { stdout } = await execFileAsync(
      "tasklist",
      ["/FO", "CSV", "/NH"],
      { windowsHide: true, timeout: 10000, encoding: "utf8" }
    );
    for (const line of String(stdout || "").split(/\r?\n/)) {
      if (!/ChatGPT\.exe|Codex\.exe/i.test(line)) continue;
      const m = line.match(/","(\d+)"/);
      if (m) pids.add(Number(m[1]));
      else {
        const parts = line.split('","');
        if (parts.length >= 2) {
          const id = Number(String(parts[1]).replace(/"/g, ""));
          if (Number.isFinite(id)) pids.add(id);
        }
      }
    }
  } catch {
    /* ignore */
  }

  // 2) Get-Process only when tasklist missed
  if (pids.size === 0) {
    try {
      const { stdout } = await execFileAsync(
        powershellExe(),
        [
          "-NoProfile",
          "-NonInteractive",
          "-Command",
          "Get-Process -Name ChatGPT,Codex -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id",
        ],
        { windowsHide: true, timeout: 12000, encoding: "utf8" }
      );
      for (const line of String(stdout || "").split(/\r?\n/)) {
        const id = Number(line.trim());
        if (Number.isFinite(id) && id > 0) pids.add(id);
      }
    } catch {
      /* ignore */
    }
  }

  // 3) CIM by path when still empty
  if (pids.size === 0) {
    try {
      const { stdout } = await execFileAsync(
        powershellExe(),
        [
          "-NoProfile",
          "-NonInteractive",
          "-Command",
          `Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
            Where-Object {
              $_.Name -match '^(ChatGPT|Codex)\\.exe$' -or
              ($_.ExecutablePath -and $_.ExecutablePath -match 'OpenAI\\.(Codex|ChatGPT)|\\\\ChatGPT\\.exe$|\\\\Codex\\.exe$')
            } | Select-Object -ExpandProperty ProcessId`,
        ],
        { windowsHide: true, timeout: 15000, encoding: "utf8" }
      );
      for (const line of String(stdout || "").split(/\r?\n/)) {
        const id = Number(line.trim());
        if (Number.isFinite(id) && id > 0) pids.add(id);
      }
    } catch {
      /* ignore */
    }
  }

  return [...pids];
}

/** @type {{ at: number, pids: number[] } | null} */
let processCache = null;
/** @type {{ at: number, port: number, debugPortOpen: boolean, rendererReady: boolean } | null} */
let cdpCache = null;
/** @type {{ stable: string, lastReadyAt: number, offlineSince: number | null, offlineHits: number }} */
let hyst = {
  stable: "offline",
  lastReadyAt: 0,
  offlineSince: null,
  offlineHits: 0,
};
/** @type {{ at: number, snap: object } | null} */
let lastSnap = null;

function classifyRaw(processRunning, portOpen, rendererReady) {
  if (rendererReady) return "ready";
  if (processRunning || portOpen) return "starting";
  return "offline";
}

function applyHysteresis(raw, debugPortOpen) {
  const now = Date.now();
  if (raw === "ready") {
    hyst.lastReadyAt = now;
    hyst.offlineSince = null;
    hyst.offlineHits = 0;
    hyst.stable = "ready";
    return { lifecycle: "ready", confidence: "high" };
  }
  if (raw === "starting") {
    hyst.offlineSince = null;
    hyst.offlineHits = 0;
    if (
      hyst.stable === "ready" &&
      now - hyst.lastReadyAt < STICKY_READY_MS &&
      (debugPortOpen || now - hyst.lastReadyAt < 2000)
    ) {
      return { lifecycle: "ready", confidence: "probing" };
    }
    hyst.stable = "starting";
    return { lifecycle: "starting", confidence: "high" };
  }
  // offline raw
  hyst.offlineHits += 1;
  if (hyst.offlineSince == null) hyst.offlineSince = now;
  const held = now - hyst.offlineSince >= OFFLINE_HOLD_MS;
  const confirmed =
    (hyst.offlineHits >= OFFLINE_CONFIRM && held) ||
    hyst.offlineHits >= OFFLINE_CONFIRM + 1;

  if (hyst.stable === "ready" && now - hyst.lastReadyAt < STICKY_READY_MS && !confirmed) {
    return { lifecycle: "ready", confidence: "probing" };
  }
  if (confirmed || hyst.stable === "offline") {
    hyst.stable = "offline";
    return { lifecycle: "offline", confidence: confirmed ? "high" : "probing" };
  }
  return { lifecycle: hyst.stable, confidence: "probing" };
}

function buildSnap(port, pids, portOpen, rendererReady, lifecycle, lifecycleRaw, confidence, probeAgeMs) {
  const processRunning = pids.length > 0;
  const canHotApply = rendererReady || (lifecycle === "ready" && portOpen);
  const needsRestartForInject = processRunning && !portOpen;
  const hostUp =
    processRunning ||
    portOpen ||
    rendererReady ||
    lifecycle === "ready" ||
    lifecycle === "starting";
  return {
    port,
    pids,
    processRunning,
    debugPortOpen: portOpen,
    rendererReady,
    debugReady: rendererReady || lifecycle === "ready",
    codexRunning: hostUp,
    lifecycle,
    lifecycleRaw,
    lifecycleLabel: lifecycle,
    confidence,
    canHotApply,
    needsRestartForInject,
    hostEngaged: lifecycle !== "offline",
    probeAgeMs: probeAgeMs || 0,
    signals: {
      process: processRunning,
      port: portOpen,
      renderer: rendererReady,
    },
  };
}

function invalidateHostProbeCache() {
  processCache = null;
  cdpCache = null;
  lastSnap = null;
}

function noteHostReady(port = DEFAULT_PORT) {
  hyst.stable = "ready";
  hyst.lastReadyAt = Date.now();
  hyst.offlineSince = null;
  hyst.offlineHits = 0;
  cdpCache = {
    at: Date.now(),
    port,
    debugPortOpen: true,
    rendererReady: true,
  };
  const snap = buildSnap(port, [], true, true, "ready", "ready", "high", 0);
  lastSnap = { at: Date.now(), snap };
}

/**
 * Full lifecycle snapshot for status / ensureDebugPort / apply.
 * @param {number} [port]
 * @param {{ fetchTimeoutMs?: number, force?: boolean, fullProcess?: boolean }} [opts]
 */
async function probeHostLifecycle(port = DEFAULT_PORT, opts = {}) {
  const fetchTimeoutMs = opts.fetchTimeoutMs ?? 2500;
  const force = Boolean(opts.force);
  const now = Date.now();

  if (!force && lastSnap && lastSnap.snap.port === port && now - lastSnap.at < SNAPSHOT_CACHE_MS) {
    return {
      ...lastSnap.snap,
      probeAgeMs: now - lastSnap.at,
      confidence: lastSnap.snap.confidence === "high" ? "stale" : lastSnap.snap.confidence,
    };
  }

  let portOpen;
  let rendererReady;
  if (
    !force &&
    cdpCache &&
    cdpCache.port === port &&
    now - cdpCache.at < CDP_CACHE_MS
  ) {
    portOpen = cdpCache.debugPortOpen;
    rendererReady = cdpCache.rendererReady;
  } else {
    const [po, rr] = await Promise.all([
      isDebugPortOpen(port, fetchTimeoutMs),
      isRendererReady(port, fetchTimeoutMs),
    ]);
    portOpen = po || rr;
    rendererReady = rr;
    cdpCache = {
      at: Date.now(),
      port,
      debugPortOpen: portOpen,
      rendererReady,
    };
  }

  const hostUpCdp = portOpen || rendererReady;
  let pids = [];
  if (
    !force &&
    hostUpCdp &&
    processCache &&
    now - processCache.at < PROCESS_CACHE_MS
  ) {
    pids = processCache.pids;
  } else if (
    force ||
    !hostUpCdp ||
    !processCache ||
    now - processCache.at >= PROCESS_CACHE_MS
  ) {
    // Skip expensive process scan when renderer is ready and not forced
    if (hostUpCdp && !force && processCache) {
      pids = processCache.pids;
    } else if (hostUpCdp && !force && rendererReady) {
      pids = processCache?.pids || [];
      if (!processCache) {
        // one light fill later is ok; avoid blocking ready status
        pids = [];
      }
    } else {
      pids = await findHostMainPids();
      processCache = { at: Date.now(), pids };
    }
  } else {
    pids = processCache?.pids || [];
  }

  const processRunning = pids.length > 0;
  const lifecycleRaw = classifyRaw(processRunning, portOpen, rendererReady);
  const { lifecycle, confidence } = applyHysteresis(lifecycleRaw, portOpen);
  const snap = buildSnap(
    port,
    pids,
    portOpen,
    rendererReady,
    lifecycle,
    lifecycleRaw,
    confidence,
    0
  );
  lastSnap = { at: Date.now(), snap };
  return snap;
}

/**
 * Wait until lifecycle reaches one of the allowed phases, or timeout.
 */
async function waitForHostLifecycle(
  port,
  {
    want = ["ready"],
    timeoutMs = 45000,
    pollMs = 400,
    onTick = null,
  } = {}
) {
  const wantSet = new Set(want);
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    last = await probeHostLifecycle(port, { fetchTimeoutMs: 2000, force: true });
    if (typeof onTick === "function") {
      try {
        onTick(last);
      } catch {
        /* ignore */
      }
    }
    if (wantSet.has(last.lifecycle)) return last;
    await sleep(pollMs);
  }
  return last || (await probeHostLifecycle(port, { force: true }));
}

/**
 * Adaptive apply/launch budgets.
 */
function resolveTimingBudget(seedProbe = null) {
  const envScale = Number(process.env.CODEX_SKIN_SLOW_SCALE || 0);
  const starting = seedProbe?.lifecycle === "starting";
  const scale = Math.min(3, Math.max(1, envScale || (starting ? 1.6 : 1)));
  return {
    scale,
    waitDebugPortMs: Math.round(28000 * scale),
    waitRendererMs: Math.round(45000 * scale),
    softOnceTimeoutMs: Math.round(8000 * scale),
    softOnceExecMs: Math.round(20000 * scale),
    softVerifyTimeoutMs: Math.round(12000 * scale),
    launchSettleMs: Math.round(900 * scale),
    stopSettleMs: Math.round(700 * scale),
    pollMs: scale > 1.3 ? 500 : 350,
  };
}

/** Compact host status for GUI polling (Node CLI fallback). */
async function getHostStatus(port = DEFAULT_PORT, { force = false } = {}) {
  const snap = await probeHostLifecycle(port, { force });
  return {
    ok: true,
    ...snap,
    engine: "node",
  };
}

module.exports = {
  sleep,
  isDebugPortOpen,
  isRendererReady,
  findHostMainPids,
  probeHostLifecycle,
  waitForHostLifecycle,
  resolveTimingBudget,
  invalidateHostProbeCache,
  noteHostReady,
  getHostStatus,
  DEFAULT_PORT,
  // test helpers
  _resetProbeStateForTests() {
    processCache = null;
    cdpCache = null;
    lastSnap = null;
    hyst = {
      stable: "offline",
      lastReadyAt: 0,
      offlineSince: null,
      offlineHits: 0,
    };
  },
  _applyHysteresisForTests: applyHysteresis,
  _setHystForTests(next) {
    hyst = { ...hyst, ...next };
  },
};
