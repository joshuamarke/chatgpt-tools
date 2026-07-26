const fs = require("fs");
const fsp = require("fs/promises");
const path = require("path");
const os = require("os");
const crypto = require("crypto");
const { spawn, execFile } = require("child_process");
const { promisify } = require("util");
const {
  ENGINE_NAME,
  ENGINE_VERSION,
  ENGINE_PROTOCOL,
} = require("./version.js");
const hostProbe = require("./host-probe.js");

const execFileAsync = promisify(execFile);

// 打包后 engine/skins 在 app.asar.unpacked，外部 node 进程必须用真实磁盘路径
function toRealResourcePath(p) {
  if (!p || typeof p !== "string") return p;
  if (!/[\\/]app\.asar([\\/]|$)/.test(p)) return p;
  const unpacked = p
    .replace(/[\\/]app\.asar([\\/])/g, `${path.sep}app.asar.unpacked$1`)
    .replace(/[\\/]app\.asar$/g, `${path.sep}app.asar.unpacked`);
  try {
    if (fs.existsSync(unpacked)) return unpacked;
  } catch {}
  return p;
}

function resolveRoot() {
  // Tauri / external override
  const fromEnv = process.env.CODEX_SKIN_ROOT;
  if (fromEnv && String(fromEnv).trim()) {
    return toRealResourcePath(path.resolve(String(fromEnv).trim()));
  }
  // 安装版 Electron：优先 resources/app.asar.unpacked
  if (process.resourcesPath && process.versions?.electron) {
    const unpacked = path.join(process.resourcesPath, "app.asar.unpacked");
    try {
      if (
        fs.existsSync(path.join(unpacked, "engine")) ||
        fs.existsSync(path.join(unpacked, "skins"))
      ) {
        return unpacked;
      }
    } catch {}
  }
  // Tauri resource dir sibling (production)
  if (process.resourcesPath) {
    const candidates = [
      path.join(process.resourcesPath, "resources"),
      process.resourcesPath,
      path.join(process.resourcesPath, "..", "resources"),
    ];
    for (const c of candidates) {
      try {
        if (fs.existsSync(path.join(c, "engine")) || fs.existsSync(path.join(c, "skins"))) {
          return toRealResourcePath(c);
        }
      } catch {}
    }
  }
  return toRealResourcePath(path.resolve(__dirname, ".."));
}

const ROOT = resolveRoot();
const BUNDLED_SKINS_DIR = toRealResourcePath(path.join(ROOT, "skins"));
const ENGINE_DIR = toRealResourcePath(path.join(ROOT, "engine"));

/**
 * 状态根目录：%LOCALAPPDATA%\ChatGPTTools（Windows）
 * 旧目录 CodexSkin / CodexSkinManager 会在首次启动时合并迁移到 ChatGPTTools。
 */
function resolveStateRoot() {
  if (process.env.CODEX_SKIN_MANAGER_STATE && String(process.env.CODEX_SKIN_MANAGER_STATE).trim()) {
    return path.resolve(String(process.env.CODEX_SKIN_MANAGER_STATE).trim());
  }
  const name = process.env.CODEX_SKIN_STATE_NAME || "ChatGPTTools";
  const base =
    process.platform === "win32"
      ? path.join(process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local"), name)
      : path.join(os.homedir(), "Library", "Application Support", name);

  if (name === "ChatGPTTools" && process.platform === "win32") {
    const local = process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local");
    const legacyDirs = ["CodexSkin", "CodexSkinManager"].map((d) => path.join(local, d));
    try {
      // 优先整目录重命名第一个存在的旧目录
      if (!fs.existsSync(base)) {
        for (const legacy of legacyDirs) {
          if (fs.existsSync(legacy)) {
            fs.renameSync(legacy, base);
            break;
          }
        }
      }
      // 其余旧目录合并进新目录（不覆盖已有）
      if (fs.existsSync(base)) {
        for (const legacy of legacyDirs) {
          if (!fs.existsSync(legacy) || path.resolve(legacy) === path.resolve(base)) continue;
          for (const f of ["state.json", "settings.json", "config.before-skin-manager.toml"]) {
            const src = path.join(legacy, f);
            const dst = path.join(base, f);
            if (fs.existsSync(src) && !fs.existsSync(dst)) {
              try {
                fs.copyFileSync(src, dst);
              } catch {}
            }
          }
          for (const sub of ["skins", "runtime-skins"]) {
            const srcDir = path.join(legacy, sub);
            const dstDir = path.join(base, sub);
            if (!fs.existsSync(srcDir)) continue;
            fs.mkdirSync(dstDir, { recursive: true });
            for (const ent of fs.readdirSync(srcDir, { withFileTypes: true })) {
              const from = path.join(srcDir, ent.name);
              const to = path.join(dstDir, ent.name);
              if (fs.existsSync(to)) continue;
              try {
                if (ent.isDirectory()) copyDirRecursive(from, to);
                else fs.copyFileSync(from, to);
              } catch {}
            }
          }
        }
      }
      // 修正 state.json 里仍指向旧目录的路径
      try {
        const stateFile = path.join(base, "state.json");
        if (fs.existsSync(stateFile)) {
          let raw = fs.readFileSync(stateFile, "utf8");
          let changed = false;
          for (const old of ["CodexSkinManager", "CodexSkin"]) {
            if (raw.includes(old) && old !== "ChatGPTTools") {
              raw = raw.split(old).join("ChatGPTTools");
              changed = true;
            }
          }
          if (changed) fs.writeFileSync(stateFile, raw);
        }
      } catch {}
    } catch (e) {
      try {
        console.error("[chatgpt-tools] migrate legacy state → ChatGPTTools:", e.message || e);
      } catch {}
    }
  }
  return base;
}

const STATE_ROOT = resolveStateRoot();
// 用户导入的皮肤放可写目录（打包后 app 目录可能只读）
const USER_SKINS_DIR = path.join(STATE_ROOT, "skins");
const STATE_PATH = path.join(STATE_ROOT, "state.json");
const SETTINGS_PATH = path.join(STATE_ROOT, "settings.json");
const LOCK_PATH = path.join(STATE_ROOT, ".engine.lock");
const PAUSE_PATH = path.join(STATE_ROOT, "paused.flag");
/** Long-lived injector control channel (switch/reapply without respawn). */
const CONTROL_PATH = path.join(STATE_ROOT, "injector.control.json");
const CONTROL_RESULT_PATH = path.join(STATE_ROOT, "injector.control.json.result");
const CONFIG_PATH = process.env.CODEX_CONFIG_PATH || path.join(os.homedir(), ".codex", "config.toml");
const BACKUP_PATH = path.join(STATE_ROOT, "config.before-skin-manager.toml");
// 所有皮肤共用一个调试端口，切换皮肤时不必反复重启 ChatGPT
const SHARED_PORT = Number(process.env.CODEX_SKIN_PORT || 9335);
const SKIN_PACKAGE_VERSION = 1;
const MAX_ART_BYTES = 16 * 1024 * 1024;
const DIAG_LOG_MAX_BYTES = 2 * 1024 * 1024;
const INJECTOR_LOG_MAX_BYTES = 4 * 1024 * 1024;

function readSettings() {
  ensureStateDir();
  try {
    return JSON.parse(fs.readFileSync(SETTINGS_PATH, "utf8"));
  } catch {
    return {};
  }
}

function writeSettings(next) {
  ensureStateDir();
  fs.writeFileSync(SETTINGS_PATH, JSON.stringify(next || {}, null, 2) + "\n");
  return next || {};
}

function getConfiguredAppPath() {
  const fromEnv = process.env.CODEX_APP_PATH;
  if (fromEnv && String(fromEnv).trim()) return String(fromEnv).trim();
  const fromFile = readSettings().appPath;
  return fromFile && String(fromFile).trim() ? String(fromFile).trim() : null;
}

function setConfiguredAppPath(appPath) {
  const settings = readSettings();
  if (appPath && String(appPath).trim()) settings.appPath = String(appPath).trim();
  else delete settings.appPath;
  writeSettings(settings);
  return { ok: true, appPath: settings.appPath || null };
}

function pathLooksLikeExe(p) {
  if (!p || typeof p !== "string") return false;
  try {
    if (fs.existsSync(p)) return true;
  } catch {}
  // WindowsApps 在部分机器上 existsSync 会失败，但 Store 激活仍可用
  return /[\\/]WindowsApps[\\/].+\.exe$/i.test(p);
}

function ensureStateDir() {
  fs.mkdirSync(STATE_ROOT, { recursive: true });
  fs.mkdirSync(USER_SKINS_DIR, { recursive: true });
}

function skinDirs() {
  return [BUNDLED_SKINS_DIR, USER_SKINS_DIR];
}

function safeSkinId(id) {
  return String(id || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

function readState() {
  ensureStateDir();
  try {
    return JSON.parse(fs.readFileSync(STATE_PATH, "utf8"));
  } catch {
    return null;
  }
}

/**
 * Atomic state write (Dream Skin #71 habit): same-dir temp + replace.
 * Never leave a half-written state.json that later looks like success.
 */
function writeState(state) {
  ensureStateDir();
  const text = JSON.stringify(state, null, 2) + "\n";
  const dir = path.dirname(STATE_PATH);
  const tmp = path.join(dir, `.state.json.chatgpt-tools.${process.pid}.${Date.now()}.tmp`);
  const bak = path.join(dir, `.state.json.chatgpt-tools.${process.pid}.bak`);
  fs.writeFileSync(tmp, text, { encoding: "utf8" });
  try {
    if (fs.existsSync(STATE_PATH)) {
      try {
        fs.renameSync(STATE_PATH, bak);
      } catch {
        fs.copyFileSync(STATE_PATH, bak);
        try {
          fs.unlinkSync(STATE_PATH);
        } catch {}
      }
    }
    fs.renameSync(tmp, STATE_PATH);
    try {
      if (fs.existsSync(bak)) fs.unlinkSync(bak);
    } catch {
      // Post-commit cleanup must never mask success.
    }
  } catch (e) {
    try {
      if (fs.existsSync(bak) && !fs.existsSync(STATE_PATH)) fs.renameSync(bak, STATE_PATH);
    } catch {}
    try {
      if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
    } catch {}
    throw e;
  }
}

/** Archive active state (restore path) instead of silent truncate. */
function archiveStateFile() {
  if (!fs.existsSync(STATE_PATH)) return null;
  const dir = path.dirname(STATE_PATH);
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const archive = path.join(dir, `state.stale-${stamp}-${process.pid}.json`);
  try {
    fs.renameSync(STATE_PATH, archive);
    return archive;
  } catch {
    try {
      fs.unlinkSync(STATE_PATH);
    } catch {}
    return null;
  }
}

function readSkinFromDir(dir, source) {
  const manifestPath = path.join(dir, "skin.json");
  if (!fs.existsSync(manifestPath)) return null;
  try {
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    if (!manifest.id) manifest.id = path.basename(dir);
    return {
      ...manifest,
      dir,
      source, // bundled | user
      builtin: source === "bundled",
    };
  } catch {
    return null;
  }
}

function listSkins() {
  ensureStateDir();
  const map = new Map();
  // 内置先加载，用户皮肤同 id 可覆盖
  for (const [dirRoot, source] of [
    [BUNDLED_SKINS_DIR, "bundled"],
    [USER_SKINS_DIR, "user"],
  ]) {
    if (!fs.existsSync(dirRoot)) continue;
    for (const d of fs.readdirSync(dirRoot, { withFileTypes: true })) {
      if (!d.isDirectory()) continue;
      // `_template` and other author scaffolding — not installable skins
      if (d.name.startsWith(".") || d.name.startsWith("_")) continue;
      const skin = readSkinFromDir(path.join(dirRoot, d.name), source);
      if (skin?.id) map.set(skin.id, skin);
    }
  }
  return [...map.values()].sort((a, b) => a.name.localeCompare(b.name, "zh"));
}

function getSkin(id) {
  const skin = listSkins().find((s) => s.id === id);
  if (!skin) throw new Error(`Skin not found: ${id}`);
  return skin;
}

/**
 * art.mode: wallpaper | token-only | none
 * - none → assets.art optional (pure style skin)
 * - wallpaper / token-only → assets.art required
 */
function resolveArtModeFromManifest(manifest) {
  const art = manifest?.art && typeof manifest.art === "object" ? manifest.art : {};
  const themeArt =
    manifest?.theme?.art && typeof manifest.theme.art === "object" ? manifest.theme.art : {};
  const raw = String(themeArt.mode ?? art.mode ?? "wallpaper").trim().toLowerCase();
  if (raw === "none" || raw === "token-only" || raw === "wallpaper") return raw;
  return "wallpaper";
}

function validateSkinManifest(manifest, skinDir) {
  if (!manifest || typeof manifest !== "object") throw new Error("skin.json 无效");
  if (!manifest.id) throw new Error("skin.json 缺少 id");
  if (!manifest.name) throw new Error("skin.json 缺少 name");
  if (!manifest.assets?.css) {
    throw new Error("skin.json 缺少 assets.css");
  }
  // v2: shared renderer-core only — per-skin inject scripts are not used
  if (!manifest.assets?.plugin) {
    throw new Error("skin.json 需要 assets.plugin（共享 runtime，不再使用 assets.inject）");
  }
  if (!manifest.markers?.rootClass || !manifest.markers?.styleId || !manifest.markers?.stateKey) {
    throw new Error("skin.json 缺少 markers 字段");
  }
  const artMode = resolveArtModeFromManifest(manifest);
  const needsArt = artMode !== "none";
  const artRel =
    typeof manifest.assets.art === "string" && manifest.assets.art.trim()
      ? manifest.assets.art.trim()
      : null;
  if (needsArt && !artRel) {
    throw new Error(
      'skin.json 缺少 assets.art（纯样式皮肤请设 art.mode 为 "none"）'
    );
  }
  const required = [manifest.assets.css, manifest.assets.plugin];
  if (needsArt && artRel) required.push(artRel);
  for (const rel of required) {
    const abs = path.join(skinDir, rel);
    if (!fs.existsSync(abs)) throw new Error(`缺少资源文件：${rel}`);
  }
  // plugin.json must parse and include chromeHtml
  try {
    const plugin = JSON.parse(
      fs.readFileSync(path.join(skinDir, manifest.assets.plugin), "utf8")
    );
    if (typeof plugin.chromeHtml !== "string") {
      throw new Error("plugin.json 需要 chromeHtml 字符串");
    }
  } catch (e) {
    if (e.message && e.message.includes("chromeHtml")) throw e;
    throw new Error(`plugin.json 无效：${e.message || e}`);
  }
  if (needsArt && artRel) {
    const artAbs = path.join(skinDir, artRel);
    try {
      const size = fs.statSync(artAbs).size;
      if (size < 1) throw new Error("立绘文件为空");
      if (size > MAX_ART_BYTES) {
        throw new Error(
          `立绘超过 ${MAX_ART_BYTES / 1024 / 1024} MB 注入上限；请使用 ≤ ${MAX_ART_BYTES / 1024 / 1024} MB 的 PNG/JPEG/WebP（上限内支持高质量原图）`
        );
      }
    } catch (e) {
      if (e.message && e.message.includes("注入上限")) throw e;
      if (e.message && e.message.includes("为空")) throw e;
    }
  }
  return true;
}

/**
 * Async process mutex — never busy-spin. Concurrent apply/restore wait with sleep.
 * Stale locks (dead pid or age > 90s) are stolen so a crashed CLI cannot wedge the GUI.
 */
async function withEngineLock(fn) {
  ensureStateDir();
  const token = `${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  const deadline = Date.now() + 60000;

  const tryStealStale = () => {
    if (!fs.existsSync(LOCK_PATH)) return;
    try {
      const existing = JSON.parse(fs.readFileSync(LOCK_PATH, "utf8"));
      const age = Date.now() - Number(existing.at || 0);
      let alive = false;
      if (existing.pid) {
        try {
          process.kill(existing.pid, 0);
          alive = true;
        } catch {
          alive = false;
        }
      }
      if (!alive || age > 90000) {
        try {
          fs.unlinkSync(LOCK_PATH);
          appendDiag(
            `engine.lock stolen (alive=${alive} ageMs=${age} prevPid=${existing.pid || "?"})`
          );
        } catch {}
      }
    } catch {
      try {
        fs.unlinkSync(LOCK_PATH);
      } catch {}
    }
  };

  while (Date.now() < deadline) {
    tryStealStale();
    try {
      const fd = fs.openSync(LOCK_PATH, "wx");
      fs.writeFileSync(
        fd,
        JSON.stringify({ pid: process.pid, token, at: Date.now(), version: ENGINE_VERSION }, null, 2)
      );
      fs.closeSync(fd);
      const release = () => {
        try {
          if (fs.existsSync(LOCK_PATH)) {
            const cur = JSON.parse(fs.readFileSync(LOCK_PATH, "utf8"));
            if (cur.token === token) fs.unlinkSync(LOCK_PATH);
          }
        } catch {}
      };
      try {
        return await fn();
      } finally {
        release();
      }
    } catch (e) {
      if (e && (e.code === "EEXIST" || e.code === "EPERM")) {
        await hostProbe.sleep(120);
        continue;
      }
      throw e;
    }
  }
  throw new Error("引擎忙：其他换肤操作尚未结束，请稍后重试");
}

function setPaused(paused) {
  ensureStateDir();
  if (paused) fs.writeFileSync(PAUSE_PATH, `${new Date().toISOString()}\n`);
  else {
    try {
      if (fs.existsSync(PAUSE_PATH)) fs.unlinkSync(PAUSE_PATH);
    } catch {}
  }
  return { ok: true, paused: Boolean(paused), pauseFile: PAUSE_PATH };
}

function isPaused() {
  try {
    return fs.existsSync(PAUSE_PATH);
  } catch {
    return false;
  }
}

/**
 * CDP browser UUID from /json/version webSocketDebuggerUrl path.
 * Aligns with Dream Skin / injector BROWSER_ID_PATTERN (no / | :).
 */
async function readCdpBrowserId(port) {
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 1500);
    const res = await fetch(`http://127.0.0.1:${port}/json/version`, {
      signal: controller.signal,
      redirect: "error",
    });
    clearTimeout(timer);
    if (!res.ok) return null;
    const version = await res.json();
    const urlText = version?.webSocketDebuggerUrl || version?.["webSocketDebuggerUrl"];
    if (!urlText || typeof urlText !== "string") return null;
    let url;
    try {
      url = new URL(urlText);
    } catch {
      return null;
    }
    const match = url.pathname.match(/^\/devtools\/browser\/([A-Za-z0-9._-]{1,200})$/);
    if (!match) {
      // Tolerate legacy compound keys stored in state (extract if present)
      const legacy = String(urlText).match(
        /\/devtools\/browser\/([A-Za-z0-9._-]{1,200})/
      );
      return legacy ? legacy[1] : null;
    }
    return match[1];
  } catch {
    return null;
  }
}

/** Normalize any legacy browser-id value to UUID form for child argv. */
function normalizeBrowserIdForArg(raw) {
  if (raw == null || raw === "") return null;
  const text = String(raw).trim();
  if (/^[A-Za-z0-9._-]{1,200}$/.test(text)) return text;
  const m = text.match(/\/devtools\/browser\/([A-Za-z0-9._-]{1,200})/);
  return m ? m[1] : null;
}

function injectorCommandMatches(cmdline, injectorPath, port) {
  if (!cmdline || typeof cmdline !== "string") return false;
  const norm = cmdline.replace(/\//g, "\\").toLowerCase();
  // Our watcher always runs injector.mjs
  if (!norm.includes("injector.mjs")) return false;
  // Prefer matching our engine path when known
  const inj = String(injectorPath || "").replace(/\//g, "\\").toLowerCase();
  if (inj) {
    const base = path.basename(inj).toLowerCase();
    const dirHint = path
      .dirname(inj)
      .replace(/\//g, "\\")
      .toLowerCase()
      .split("\\")
      .filter(Boolean)
      .slice(-2)
      .join("\\");
    if (dirHint && norm.includes(dirHint)) return true;
    if (base && norm.includes(base)) return true;
  }
  // Fallback: any injector.mjs with our debug port
  if (port != null && norm.includes(String(port))) return true;
  // Last resort: injector.mjs + skin-dir / watch (our CLI shape)
  return norm.includes("--watch") || norm.includes("--skin-dir") || norm.includes("chatgpt-tools");
}

function readWindowsProcessCommandLine(pid) {
  try {
    const { stdout } = require("child_process").spawnSync(
      powershellExe(),
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `(Get-CimInstance Win32_Process -Filter "ProcessId = ${Number(pid)}").CommandLine`,
      ],
      { encoding: "utf8", windowsHide: true, timeout: 5000 }
    );
    return String(stdout || "").trim();
  } catch {
    return "";
  }
}

function processLooksLikeOurInjector(pid, state) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
  } catch {
    return false;
  }
  const injector = state?.injectorScript || path.join(ENGINE_DIR, "injector.mjs");
  // Dream Stop-DreamSkinRecordedInjector: require injector.mjs + --watch + port when known.
  const requireWatch = true;
  if (process.platform === "win32") {
    const cmd = readWindowsProcessCommandLine(pid);
    if (!cmd) {
      // No cmdline: refuse to treat as ours (never kill-by-pid alone).
      // Dead path is handled by caller; alive-without-identity must not match.
      return false;
    }
    if (!injectorCommandMatches(cmd, injector, state?.port)) return false;
    if (requireWatch && !/(?:^|\s)--watch(?:\s|=|$)/i.test(cmd) && !cmd.toLowerCase().includes("--watch")) {
      // Accept once/verify short processes only if state explicitly stored them;
      // recorded long-lived injectors always use --watch.
      if (state?.injectorScript) {
        /* allow if path+port already matched */
      } else {
        return false;
      }
    }
    if (state?.port != null) {
      const portRe = new RegExp(
        `(?:^|\\s)--port(?:=|\\s+)${String(state.port).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?:\\s|$)`,
        "i"
      );
      if (!portRe.test(cmd) && !cmd.includes(String(state.port))) return false;
    }
    if (state?.browserId) {
      const bid = String(state.browserId).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const browserRe = new RegExp(
        `(?:^|\\s)--browser-id(?:=|\\s+)${bid}(?:\\s|$)`,
        "i"
      );
      // Soft: only enforce when present in state AND cmdline has a browser-id flag.
      if (/--browser-id/i.test(cmd) && !browserRe.test(cmd)) return false;
    }
    return true;
  }
  try {
    const cmdline = fs.readFileSync(`/proc/${pid}/cmdline`, "utf8").replace(/\0/g, " ");
    if (!injectorCommandMatches(cmdline, injector, state?.port)) return false;
    if (state?.port != null && !cmdline.includes(String(state.port))) return false;
    return true;
  } catch {
    try {
      const { stdout } = require("child_process").spawnSync(
        "ps",
        ["-p", String(pid), "-o", "command="],
        { encoding: "utf8" }
      );
      const cmdline = String(stdout || "");
      if (!injectorCommandMatches(cmdline, injector, state?.port)) return false;
      if (state?.port != null && !cmdline.includes(String(state.port))) return false;
      return true;
    } catch {
      return false;
    }
  }
}

function copyDirRecursive(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const from = path.join(src, entry.name);
    const to = path.join(dest, entry.name);
    if (entry.isDirectory()) copyDirRecursive(from, to);
    else fs.copyFileSync(from, to);
  }
}

function rmDirRecursive(dir) {
  if (!fs.existsSync(dir)) return;
  fs.rmSync(dir, { recursive: true, force: true });
}

function exportSkin(skinId, outputPath) {
  const AdmZip = require("adm-zip");
  const skin = getSkin(skinId);
  validateSkinManifest(skin, skin.dir);

  const zip = new AdmZip();
  // 包内统一以 skin/ 为根，方便导入
  const addDir = (dir, zipPrefix) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      const zpath = `${zipPrefix}/${entry.name}`.replace(/\\/g, "/");
      if (entry.isDirectory()) addDir(full, zpath);
      else zip.addLocalFile(full, path.posix.dirname(zpath), entry.name);
    }
  };
  addDir(skin.dir, "skin");

  // 额外写入包装信息
  const meta = {
    format: "chatgpt-skin",
    version: SKIN_PACKAGE_VERSION,
    exportedAt: new Date().toISOString(),
    skinId: skin.id,
    skinName: skin.name,
  };
  zip.addFile("package.json", Buffer.from(JSON.stringify(meta, null, 2), "utf8"));

  const out = outputPath.endsWith(".cgskin") || outputPath.endsWith(".zip")
    ? outputPath
    : `${outputPath}.cgskin`;
  fs.mkdirSync(path.dirname(out), { recursive: true });
  zip.writeZip(out);
  return { ok: true, path: out, skinId: skin.id, name: skin.name };
}

function resolveSkinDirFromExtracted(tmpRoot) {
  let skinDir = path.join(tmpRoot, "skin");
  if (fs.existsSync(path.join(skinDir, "skin.json"))) return skinDir;
  if (fs.existsSync(path.join(tmpRoot, "skin.json"))) return tmpRoot;
  const found = fs
    .readdirSync(tmpRoot, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => path.join(tmpRoot, d.name))
    .find((d) => fs.existsSync(path.join(d, "skin.json")));
  if (!found) throw new Error("皮肤包中未找到 skin.json");
  return found;
}

function sha256File(filePath) {
  const hash = crypto.createHash("sha256");
  hash.update(fs.readFileSync(filePath));
  return hash.digest("hex");
}

function inspectSkinPackage(packagePath) {
  const AdmZip = require("adm-zip");
  if (!packagePath || !fs.existsSync(packagePath)) {
    throw new Error("找不到皮肤包文件");
  }
  ensureStateDir();
  const zip = new AdmZip(packagePath);
  const entries = zip.getEntries().filter((e) => !e.isDirectory);
  if (!entries.length) throw new Error("皮肤包是空的");

  const tmpRoot = path.join(STATE_ROOT, `.inspect-${Date.now()}`);
  rmDirRecursive(tmpRoot);
  fs.mkdirSync(tmpRoot, { recursive: true });
  try {
    zip.extractAllTo(tmpRoot, true);
    const skinDir = resolveSkinDirFromExtracted(tmpRoot);
    const manifest = JSON.parse(fs.readFileSync(path.join(skinDir, "skin.json"), "utf8"));
    validateSkinManifest(manifest, skinDir);

    const cssPath = path.join(skinDir, manifest.assets.css);
    const artPath = path.join(skinDir, manifest.assets.art);
    const pluginPath = path.join(skinDir, manifest.assets.plugin);
    const scanParts = [];
    if (fs.existsSync(pluginPath)) {
      scanParts.push(fs.readFileSync(pluginPath, "utf8"));
    }
    // Flag leftover per-skin inject files (ignored by engine, still risky if someone re-enables)
    const leftoverInject = path.join(skinDir, "assets", "renderer-inject.js");
    if (fs.existsSync(leftoverInject)) {
      scanParts.push(fs.readFileSync(leftoverInject, "utf8"));
    }
    const injectCode = scanParts.join("\n");
    const risks = [];
    const lower = injectCode.toLowerCase();
    if (/fetch\s*\(|xmlhttprequest|websocket|navigator\.sendbeacon/.test(lower)) {
      risks.push("装饰层可能发起网络请求");
    }
    if (/localstorage|indexeddb|document\.cookie|sessionstorage/.test(lower)) {
      risks.push("装饰层可能读写本地存储");
    }
    if (/eval\s*\(|new\s+function|function\s*\(\s*["']return/.test(lower)) {
      risks.push("包含动态执行代码（eval 等）");
    }
    if (/child_process|require\s*\(|process\.|fs\.|nw\.|electron/.test(injectCode)) {
      risks.push("疑似尝试访问系统能力");
    }
    if (fs.existsSync(leftoverInject)) {
      risks.push("包内仍含 renderer-inject.js（引擎 v2 已忽略；建议删除）");
    }
    if (fs.existsSync(artPath)) {
      const artBytes = fs.statSync(artPath).size;
      if (artBytes > MAX_ART_BYTES) {
        risks.push(`立绘超过 ${MAX_ART_BYTES / 1024 / 1024} MB 注入上限`);
      } else if (artBytes > 8 * 1024 * 1024) {
        risks.push(
          "立绘较大（>8MB）：引擎支持高质量原图，但 shell 后贴图会更慢；请为列表提供 assets/screenshot"
        );
      }
    }
    if (!risks.length) risks.push("未发现明显高危模式（不能保证绝对安全）");

    return {
      ok: true,
      path: packagePath,
      fileName: path.basename(packagePath),
      skinId: safeSkinId(manifest.id),
      name: manifest.name || manifest.id,
      description: manifest.description || "",
      files: entries.map((e) => e.entryName).slice(0, 40),
      fileCount: entries.length,
      hasInject: false,
      hasPlugin: fs.existsSync(pluginPath),
      injectPath: null,
      pluginPath: manifest.assets.plugin,
      pluginSha256: fs.existsSync(pluginPath) ? sha256File(pluginPath) : null,
      injectSha256: null,
      injectBytes: 0,
      cssBytes: fs.existsSync(cssPath) ? fs.statSync(cssPath).size : 0,
      artBytes: fs.existsSync(artPath) ? fs.statSync(artPath).size : 0,
      risks,
      warning:
        "皮肤装饰会注入 ChatGPT 页面。共享 runtime 由引擎提供；请只导入信任来源。plugin.json 仅应含 chromeHtml 等装饰字段。",
    };
  } finally {
    rmDirRecursive(tmpRoot);
  }
}

function importSkin(packagePath, { overwrite = true } = {}) {
  const AdmZip = require("adm-zip");
  if (!packagePath || !fs.existsSync(packagePath)) {
    throw new Error("找不到皮肤包文件");
  }
  ensureStateDir();

  const zip = new AdmZip(packagePath);
  const entries = zip.getEntries();
  if (!entries.length) throw new Error("皮肤包是空的");

  const tmpRoot = path.join(STATE_ROOT, `.import-${Date.now()}`);
  rmDirRecursive(tmpRoot);
  fs.mkdirSync(tmpRoot, { recursive: true });

  try {
    zip.extractAllTo(tmpRoot, true);
    const skinDir = resolveSkinDirFromExtracted(tmpRoot);

    const manifest = JSON.parse(fs.readFileSync(path.join(skinDir, "skin.json"), "utf8"));
    validateSkinManifest(manifest, skinDir);
    const id = safeSkinId(manifest.id);
    if (!id) throw new Error("皮肤 id 不合法");
    manifest.id = id;

    // Drop legacy inject references — shared core only
    if (manifest.assets) {
      delete manifest.assets.inject;
      delete manifest.assets.useLegacyInject;
    }
    const pluginRel = manifest.assets.plugin || "assets/plugin.json";
    const pluginPath = path.join(skinDir, pluginRel);
    const pluginSha256 = fs.existsSync(pluginPath) ? sha256File(pluginPath) : null;

    // Remove obsolete inject stubs from the package before install
    const leftoverInject = path.join(skinDir, "assets", "renderer-inject.js");
    try {
      if (fs.existsSync(leftoverInject)) fs.unlinkSync(leftoverInject);
    } catch {}

    const targetDir = path.join(USER_SKINS_DIR, id);
    if (fs.existsSync(targetDir)) {
      if (!overwrite) throw new Error(`皮肤「${id}」已存在`);
      rmDirRecursive(targetDir);
    }

    // 写回规范化 id / assets
    fs.writeFileSync(path.join(skinDir, "skin.json"), JSON.stringify(manifest, null, 2) + "\n");
    copyDirRecursive(skinDir, targetDir);
    validateSkinManifest(
      JSON.parse(fs.readFileSync(path.join(targetDir, "skin.json"), "utf8")),
      targetDir
    );

    // 记录导入摘要，便于用户核对
    const metaPath = path.join(targetDir, ".import-meta.json");
    fs.writeFileSync(
      metaPath,
      JSON.stringify(
        {
          importedAt: new Date().toISOString(),
          from: path.basename(packagePath),
          pluginSha256,
          engineProtocol: ENGINE_PROTOCOL,
          warning: "decoration from plugin.json is injected into ChatGPT renderer",
        },
        null,
        2
      ) + "\n"
    );

    return {
      ok: true,
      skinId: id,
      name: manifest.name,
      dir: targetDir,
      overwritten: overwrite,
      pluginSha256,
    };
  } finally {
    rmDirRecursive(tmpRoot);
  }
}



function createWallpaperSkin({
  baseSkinId = "dream",
  imagePath,
  name,
  position = "right center",
  fit = "cover",
  accent = "#8b7cff",
  background = "#f7f8fc",
  text = "#202536",
  panel = "#ffffff",
  font = "system",
  radius = 16,
  overlay = 12,
  opacity = 92,
  appearance = "auto",
  focusX = null,
  focusY = null,
  safeArea = "auto",
  taskMode = "auto",
} = {}) {
  ensureStateDir();
  if (!baseSkinId) throw new Error("请选择目标皮肤模板");
  const base = getSkin(baseSkinId);
  if (!imagePath || !fs.existsSync(imagePath)) throw new Error("请选择一张壁纸");
  const stat = fs.statSync(imagePath);
  if (!stat.isFile() || stat.size < 1) {
    throw new Error("请选择有效的壁纸文件");
  }
  // Hard cap aligned with inject path (MAX_ART_BYTES = 16 MB)
  const ext = path.extname(imagePath).toLowerCase();
  const mime = {
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".webp": "image/webp",
  }[ext];
  if (!mime) throw new Error("仅支持 PNG、JPEG 或 WebP 壁纸");
  if (stat.size > MAX_ART_BYTES) {
    throw new Error(
      `壁纸必须不超过 ${MAX_ART_BYTES / 1024 / 1024} MB（当前 ${(stat.size / 1024 / 1024).toFixed(1)} MB）`
    );
  }
  const safeName =
    String(name || `${base.name} · 自定义`).trim().slice(0, 80) || `${base.name} · 自定义`;
  let id = safeSkinId(`${base.id}-${safeName}`) || `custom-${Date.now()}`;
  if (id === base.id) id = `${base.id}-custom-${Date.now()}`;
  const targetDir = path.join(USER_SKINS_DIR, id);
  if (fs.existsSync(targetDir)) throw new Error(`皮肤「${safeName}」已存在，请换一个名称`);
  const validPosition = /^(left|center|right)(?:\s+(top|center|bottom))?$/.test(position)
    ? position
    : "right center";
  const validFit = fit === "contain" ? "contain" : "cover";
  const posX = String(validPosition).split(/\s+/)[0];
  const inferredFocusX =
    focusX != null && Number.isFinite(Number(focusX))
      ? Math.min(1, Math.max(0, Number(focusX)))
      : posX === "left"
        ? 0.28
        : posX === "center"
          ? 0.5
          : 0.72;
  const inferredFocusY =
    focusY != null && Number.isFinite(Number(focusY))
      ? Math.min(1, Math.max(0, Number(focusY)))
      : 0.45;
  const appearanceChoice = ["auto", "light", "dark"].includes(appearance) ? appearance : "auto";
  const safeAreaChoice = ["auto", "left", "right", "center", "none"].includes(safeArea)
    ? safeArea
    : "auto";
  const taskModeChoice = ["auto", "ambient", "banner", "off"].includes(taskMode)
    ? taskMode
    : "auto";
  const tmp = path.join(USER_SKINS_DIR, `.wallpaper-${process.pid}-${Date.now()}`);
  try {
    copyDirRecursive(base.dir, tmp);
    const oldArt = path.join(tmp, base.assets.art);
    const artName = `wallpaper${ext === ".jpeg" ? ".jpg" : ext}`;
    const artRel = `assets/${artName}`;
    const newArt = path.join(tmp, artRel);
    fs.copyFileSync(imagePath, newArt);
    if (path.resolve(oldArt) !== path.resolve(newArt)) fs.rmSync(oldArt, { force: true });
    const manifestPath = path.join(tmp, "skin.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    manifest.id = id;
    manifest.name = safeName;
    manifest.nameEn = safeName;
    manifest.description = `基于「${base.name}」模板的自定义皮肤，可调整壁纸、颜色、字体与自适应构图。`;
    manifest.version = String(manifest.version || "2.0.0");
    manifest.tags = [...new Set([...(manifest.tags || []), "自定义皮肤", "自适应"])];
    // Preserve template categories when present; otherwise land in art (custom designs)
    const baseCats = Array.isArray(manifest.categories)
      ? manifest.categories.map((c) => String(c || "").trim()).filter(Boolean)
      : [];
    manifest.categories = baseCats.length ? [...new Set(baseCats)] : ["art"];
    manifest.assets.art = artRel;
    manifest.assets.artMime = mime;
    if (!manifest.assets.plugin) manifest.assets.plugin = "assets/plugin.json";
    // Preserve template appearance unless user explicitly chose light/dark
    if (appearanceChoice !== "auto") {
      manifest.appearance = appearanceChoice;
    } else if (!manifest.appearance) {
      manifest.appearance = "auto";
    }
    // Merge art layout onto template defaults; position select drives focusX
    const baseArt = manifest.art && typeof manifest.art === "object" ? manifest.art : {};
    const resolvedSafeArea =
      safeAreaChoice !== "auto"
        ? safeAreaChoice
        : posX === "left"
          ? "right"
          : posX === "right"
            ? "left"
            : baseArt.safeArea || "center";
    const resolvedTaskMode =
      taskModeChoice !== "auto" ? taskModeChoice : baseArt.taskMode || "auto";
    manifest.art = {
      focusX: inferredFocusX,
      focusY:
        focusY != null && Number.isFinite(Number(focusY))
          ? inferredFocusY
          : baseArt.focusY ?? inferredFocusY,
      safeArea: resolvedSafeArea,
      taskMode: resolvedTaskMode,
      fit: validFit,
      position: validPosition,
    };
    if (/^#[0-9a-f]{6}$/i.test(String(accent))) {
      manifest.accent = String(accent);
    }
    // Desktop chrome: keep template tokens; only force when user chose light/dark
    if (!manifest.desktopTheme) manifest.desktopTheme = {};
    if (appearanceChoice === "dark") {
      manifest.desktopTheme.appearanceTheme = "dark";
    } else if (appearanceChoice === "light") {
      manifest.desktopTheme.appearanceTheme = "light";
    }
    // else: leave template desktopTheme.appearanceTheme untouched
    const root = manifest.markers.rootClass;
    const artVar = manifest.markers.artVar || "--skins-art";
    const cssPath = path.join(tmp, manifest.assets.css);
    const css = fs.readFileSync(cssPath, "utf8");
    const hex = (value, fallback) =>
      /^#[0-9a-f]{6}$/i.test(String(value)) ? String(value) : fallback;
    const safeRadius = Math.max(0, Math.min(32, Number(radius) || 16));
    const safeOverlay = Math.max(0, Math.min(70, Number(overlay) || 0));
    const safeOpacity = Math.max(55, Math.min(100, Number(opacity) || 92));
    const fonts = {
      system: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
      sans: '"Inter", "PingFang SC", "Microsoft YaHei", sans-serif',
      serif: '"Songti SC", "STSong", serif',
      mono: '"SF Mono", "Cascadia Code", monospace',
    };
    const fontStack = fonts[font] || fonts.system;
    // Designer overlay: token + fit/position only.
    // Do NOT repaint main.main-surface with cover art — that breaks template
    // full-window wallpaper (framework paints body; skins like jiuyi own main).
    const customCss = `

/* Custom Skin Designer: overrides on top of template «${base.id}». */
html.${root} {
  --designer-accent: ${hex(accent, manifest.accent || "#8b7cff")};
  --designer-bg: ${hex(background, "#f7f8fc")};
  --designer-text: ${hex(text, "#202536")};
  --designer-panel: ${hex(panel, "#ffffff")};
  --designer-panel-alpha: ${safeOpacity / 100};
  --designer-radius: ${safeRadius}px;
  --designer-overlay: ${safeOverlay / 100};
  --skins-art-position: ${validPosition};
  --skins-accent: var(--designer-accent);
  --skins-text: var(--designer-text);
  --skins-canvas: var(--designer-bg);
  --skins-surface-raised: color-mix(in srgb, var(--designer-panel) calc(var(--designer-panel-alpha) * 100%), transparent);
}
html.${root} body {
  color: var(--designer-text) !important;
  font-family: ${fontStack} !important;
  background-color: var(--designer-bg) !important;
  background-size: ${validFit} !important;
  background-position: var(--skins-art-position, ${validPosition}) !important;
  background-repeat: no-repeat !important;
}
/* Dim layer over wallpaper without replacing template layout */
html.${root} body::after {
  content: "";
  position: fixed;
  inset: 0;
  z-index: 0;
  pointer-events: none;
  background: rgba(0,0,0,${safeOverlay / 100}) !important;
}
/* Soften panels; keep template background-image / framework wide-art rules */
html.${root} main.main-surface {
  border-radius: var(--designer-radius) !important;
}
html.${root}.skins-art-wide main.main-surface {
  background-color: color-mix(in srgb, var(--designer-panel) calc(var(--designer-panel-alpha) * 100%), transparent) !important;
}
html.${root}.skins-art-standard main.main-surface {
  background-color: color-mix(in srgb, var(--designer-panel) calc(var(--designer-panel-alpha) * 100%), transparent) !important;
  background-size: ${validFit} !important;
  background-position: var(--skins-art-position, ${validPosition}) !important;
  background-repeat: no-repeat !important;
}
html.${root} button, html.${root} [role="button"] { border-radius: var(--designer-radius) !important; }
html.${root} a, html.${root} [data-state="active"], html.${root} [aria-current="page"] {
  color: var(--designer-accent) !important;
}
/* Runtime injects art via ${artVar} (and --skins-art alias). */
`;
    fs.writeFileSync(cssPath, `${css}${customCss}`);
    // Ensure plugin.json exists (copy from base or minimal)
    const pluginPath = path.join(tmp, "assets", "plugin.json");
    if (!fs.existsSync(pluginPath)) {
      fs.writeFileSync(
        pluginPath,
        JSON.stringify(
          {
            version: "2.0.0",
            chromeHtml: `<div class="skin-brand"><b>${safeName.replace(/[<>&]/g, "")}</b><small>自定义皮肤 · ${String(base.name || base.id).replace(/[<>&]/g, "")}</small></div>`,
            skipAnalysis: false,
          },
          null,
          2
        ) + "\n"
      );
    }
    fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
    validateSkinManifest(manifest, tmp);
    rmDirRecursive(targetDir);
    fs.renameSync(tmp, targetDir);
    return {
      ok: true,
      skinId: id,
      name: safeName,
      baseSkinId: base.id,
      dir: targetDir,
      appearance: appearanceChoice,
      art: manifest.art,
    };
  } finally {
    rmDirRecursive(tmp);
  }
}

function deleteUserSkin(skinId) {
  const skin = getSkin(skinId);
  if (skin.builtin || skin.source === "bundled") {
    // 若用户覆盖了内置，只删用户目录
    const userDir = path.join(USER_SKINS_DIR, skinId);
    if (fs.existsSync(userDir)) {
      rmDirRecursive(userDir);
      return { ok: true, skinId, removed: "user-override" };
    }
    throw new Error("内置皮肤不能删除，只能导出");
  }
  const userDir = path.join(USER_SKINS_DIR, skinId);
  if (!fs.existsSync(userDir)) throw new Error("未找到可删除的用户皮肤");
  rmDirRecursive(userDir);
  return { ok: true, skinId, removed: "user" };
}

const sleep = hostProbe.sleep;

/**
 * True when an injectable app:// page is present (strict inject readiness).
 * For "is ChatGPT up?" use probeHostLifecycle().codexRunning instead.
 */
async function testDebugPort(port) {
  return hostProbe.isRendererReady(port, 2000);
}

/** Port answers /json/version even if no app:// page yet (slow cold start). */
async function testDebugPortOpen(port) {
  return hostProbe.isDebugPortOpen(port, 2000);
}

function expandConfiguredPath(p) {
  if (!p) return null;
  if (process.platform === "darwin") {
    for (const exe of [
      path.join(p, "Contents/MacOS/ChatGPT"),
      path.join(p, "Contents/MacOS/Codex"),
      p,
    ]) {
      if (fs.existsSync(exe)) return exe;
    }
    return null;
  }
  if (process.platform === "win32") {
    for (const exe of [
      p,
      path.join(p, "ChatGPT.exe"),
      path.join(p, "Codex.exe"),
      path.join(p, "app", "ChatGPT.exe"),
      path.join(p, "app", "Codex.exe"),
    ]) {
      if (pathLooksLikeExe(exe)) return exe;
    }
    return null;
  }
  return fs.existsSync(p) ? p : null;
}

function resolveCodexExe() {
  const configured = getConfiguredAppPath();
  if (configured) {
    const hit = expandConfiguredPath(configured);
    if (hit) return hit;
  }

  if (process.platform === "darwin") {
    const candidates = [
      "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
      "/Applications/Codex.app/Contents/MacOS/Codex",
      "/Applications/Codex.app/Contents/MacOS/ChatGPT",
      path.join(os.homedir(), "Applications/ChatGPT.app/Contents/MacOS/ChatGPT"),
    ];
    for (const c of candidates) if (fs.existsSync(c)) return c;
    return null;
  }

  if (process.platform === "win32") {
    // 同步快速探测；完整探测走 resolveWindowsCodexExe
    return { type: "windows-resolve" };
  }

  return null;
}

function windowsExeCandidates() {
  const local = process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local");
  const pf = process.env.ProgramFiles || "C:\\Program Files";
  const pf86 = process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)";
  const userProfile = process.env.USERPROFILE || os.homedir();
  return [
    path.join(local, "Programs", "ChatGPT", "ChatGPT.exe"),
    path.join(local, "Programs", "Codex", "Codex.exe"),
    path.join(local, "Programs", "chatgpt", "ChatGPT.exe"),
    path.join(local, "Programs", "OpenAI", "ChatGPT", "ChatGPT.exe"),
    path.join(local, "Programs", "OpenAI", "Codex", "Codex.exe"),
    path.join(local, "Microsoft", "WindowsApps", "ChatGPT.exe"),
    path.join(local, "Microsoft", "WindowsApps", "Codex.exe"),
    path.join(pf, "ChatGPT", "ChatGPT.exe"),
    path.join(pf, "Codex", "Codex.exe"),
    path.join(pf, "OpenAI", "ChatGPT", "ChatGPT.exe"),
    path.join(pf, "OpenAI", "Codex", "Codex.exe"),
    path.join(pf86, "ChatGPT", "ChatGPT.exe"),
    path.join(pf86, "Codex", "Codex.exe"),
    path.join(userProfile, "AppData", "Local", "Programs", "ChatGPT", "ChatGPT.exe"),
  ];
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
    } catch {}
  }
  return "powershell.exe";
}

function runPowerShell(script, timeout = 20000) {
  // EncodedCommand 避免 join/引号把脚本拆坏（Win 上常见）
  const encoded = Buffer.from(String(script).replace(/^\uFEFF/, ""), "utf16le").toString("base64");
  return execFileAsync(
    powershellExe(),
    ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", encoded],
    { windowsHide: true, timeout, encoding: "utf8", maxBuffer: 4 * 1024 * 1024 }
  );
}

function firstNonEmptyLine(stdout) {
  return String(stdout || "")
    .replace(/^\uFEFF/, "")
    .trim()
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter((s) => s && !/^WARNING:/i.test(s))
    .find(Boolean);
}

async function resolveWindowsCodexExe() {
  const configured = getConfiguredAppPath();
  if (configured) {
    const hit = expandConfiguredPath(configured);
    if (hit) return hit;
  }

  for (const g of windowsExeCandidates()) {
    if (g && pathLooksLikeExe(g)) return g;
  }

  // 一次 PowerShell 聚合探测：运行中进程 → Store → 开始菜单 → 注册表
  try {
    const { stdout } = await runPowerShell(`
$ErrorActionPreference = 'SilentlyContinue'
function Out-First([string]$p) {
  if ($p -and (Test-Path -LiteralPath $p)) { Write-Output $p; exit 0 }
}

# 1) 已运行进程（最可靠）
Get-CimInstance Win32_Process -Filter "Name = 'ChatGPT.exe' OR Name = 'Codex.exe'" |
  ForEach-Object {
    if ($_.ExecutablePath) { Out-First $_.ExecutablePath }
  }

# 2) Microsoft Store / AppX
$pkgs = @()
foreach ($n in @('OpenAI.Codex','OpenAI.ChatGPT','OpenAI.ChatGPT-Desktop')) {
  $pkgs += Get-AppxPackage -Name $n
}
$pkgs += Get-AppxPackage | Where-Object {
  $_.Name -match 'ChatGPT|Codex' -or $_.PackageFamilyName -match 'OpenAI'
}
$p = $pkgs | Sort-Object Version -Descending | Select-Object -First 1
if ($p) {
  foreach ($rel in @('app\\ChatGPT.exe','app\\Codex.exe','ChatGPT.exe','Codex.exe')) {
    Out-First (Join-Path $p.InstallLocation $rel)
  }
  # 即使 Test-Path 失败，也输出常见路径（部分机子 ACL 导致探测假阴性）
  $guess = Join-Path $p.InstallLocation 'app\\ChatGPT.exe'
  if ($guess) { Write-Output $guess; exit 0 }
}

# 3) 开始菜单快捷方式
$shell = New-Object -ComObject WScript.Shell
$lnkRoots = @(
  (Join-Path $env:APPDATA 'Microsoft\\Windows\\Start Menu\\Programs'),
  (Join-Path $env:ProgramData 'Microsoft\\Windows\\Start Menu\\Programs')
)
Get-ChildItem -Path $lnkRoots -Filter '*.lnk' -Recurse -ErrorAction SilentlyContinue |
  Where-Object { $_.Name -match 'ChatGPT|Codex|OpenAI' } |
  ForEach-Object {
    try {
      $s = $shell.CreateShortcut($_.FullName)
      if ($s.TargetPath) { Out-First $s.TargetPath }
    } catch {}
  }

# 4) 卸载注册表
$keys = @(
  'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*',
  'HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*',
  'HKLM:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'
)
Get-ItemProperty $keys | Where-Object { $_.DisplayName -match 'ChatGPT|Codex|OpenAI' } | ForEach-Object {
  if ($_.DisplayIcon) {
    $icon = ($_.DisplayIcon -replace ',\\d+$','')
    Out-First $icon
  }
  if ($_.InstallLocation) {
    foreach ($name in @('ChatGPT.exe','Codex.exe','app\\ChatGPT.exe','app\\Codex.exe')) {
      Out-First (Join-Path $_.InstallLocation $name)
    }
  }
}
`);
    const exe = firstNonEmptyLine(stdout);
    if (exe && pathLooksLikeExe(exe)) return exe;
    // Store 路径：即使 existsSync 失败也返回，后续走 AUMID 启动
    if (exe && /[\\/]WindowsApps[\\/]/i.test(exe)) return exe;
  } catch {
    // fall through
  }

  return null;
}

/** Multi-strategy process list — see host-probe.js (tasklist + PS + CIM). */
async function findCodexMainPids() {
  return hostProbe.findHostMainPids();
}

async function stopCodex() {
  if (process.platform === "darwin") {
    try {
      await execFileAsync("osascript", ["-e", 'tell application "ChatGPT" to quit']);
    } catch {}
    try {
      await execFileAsync("osascript", ["-e", 'tell application "Codex" to quit']);
    } catch {}
    await sleep(600);
    let pids = await findCodexMainPids();
    for (const pid of pids) {
      try {
        process.kill(pid, "SIGTERM");
      } catch {}
    }
    await sleep(350);
    pids = await findCodexMainPids();
    for (const pid of pids) {
      try {
        process.kill(pid, "SIGKILL");
      } catch {}
    }
    return;
  }
  if (process.platform === "win32") {
    // Store/Electron 若未彻底退出，再次激活不会带上 --remote-debugging-port
    for (const image of ["ChatGPT.exe", "Codex.exe"]) {
      try {
        await execFileAsync("taskkill", ["/F", "/IM", image, "/T"], {
          windowsHide: true,
          timeout: 15000,
        });
      } catch {}
    }
    try {
      await runPowerShell(`
$ErrorActionPreference = 'SilentlyContinue'
Get-CimInstance Win32_Process | Where-Object {
  $_.Name -match '^(ChatGPT|Codex)\\.exe$' -or
  ($_.ExecutablePath -and $_.ExecutablePath -match 'OpenAI\\.(Codex|ChatGPT)|\\\\ChatGPT\\.exe$|\\\\Codex\\.exe$')
} | ForEach-Object {
  Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
}
`);
    } catch {}
    for (let i = 0; i < 40; i++) {
      const left = await findCodexMainPids();
      if (!left.length) break;
      await sleep(150);
    }
    await sleep(500);
  }
}

/** Content stamp so runtime-skins refresh when CSS/plugin/art/skin.json change. */
function skinMaterialStamp(skinDir) {
  try {
    const manifestPath = path.join(skinDir, "skin.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    const parts = [path.resolve(skinDir), "v2"];
    for (const rel of [
      "skin.json",
      manifest.assets?.css,
      manifest.assets?.art,
      manifest.assets?.plugin,
    ].filter(Boolean)) {
      const abs = path.join(skinDir, rel);
      if (!fs.existsSync(abs)) {
        parts.push(`${rel}:missing`);
        continue;
      }
      const st = fs.statSync(abs);
      parts.push(`${rel}:${st.size}:${Math.trunc(st.mtimeMs)}`);
    }
    return parts.join("|");
  } catch (e) {
    return `${path.resolve(skinDir)}|error|${Date.now()}`;
  }
}

/** 把皮肤拷到可写真实目录，避免安装版 asar/权限导致读不到立绘 */
function materializeSkin(skin) {
  if (!skin?.dir || !skin?.id) return skin;
  ensureStateDir();
  const destRoot = path.join(STATE_ROOT, "runtime-skins", safeSkinId(skin.id));
  const stampPath = path.join(destRoot, ".src");
  const stamp = skinMaterialStamp(skin.dir);
  let needCopy = true;
  try {
    if (fs.existsSync(path.join(destRoot, "skin.json")) && fs.existsSync(stampPath)) {
      const prev = fs.readFileSync(stampPath, "utf8").trim();
      if (prev === stamp) needCopy = false;
    }
  } catch {}
  if (needCopy) {
    try {
      fs.rmSync(destRoot, { recursive: true, force: true });
    } catch {}
    copyDirRecursive(skin.dir, destRoot);
    try {
      fs.writeFileSync(stampPath, stamp);
    } catch {}
  }
  // 校验关键资源（v2: css + art + plugin）
  try {
    const manifest = JSON.parse(fs.readFileSync(path.join(destRoot, "skin.json"), "utf8"));
    for (const rel of [manifest.assets?.css, manifest.assets?.art, manifest.assets?.plugin]) {
      if (!rel || !fs.existsSync(path.join(destRoot, rel))) {
        throw new Error(`皮肤资源缺失: ${rel || "?"}`);
      }
    }
  } catch (e) {
    // 强制再拷一次
    try {
      fs.rmSync(destRoot, { recursive: true, force: true });
    } catch {}
    copyDirRecursive(skin.dir, destRoot);
    fs.writeFileSync(stampPath, stamp);
  }
  return { ...skin, dir: destRoot };
}

function rotateLogIfNeeded(filePath, maxBytes) {
  try {
    if (!fs.existsSync(filePath)) return;
    const st = fs.statSync(filePath);
    if (st.size < maxBytes) return;
    const bak = `${filePath}.1`;
    try {
      if (fs.existsSync(bak)) fs.unlinkSync(bak);
    } catch {}
    fs.renameSync(filePath, bak);
  } catch {}
}

function appendDiag(line) {
  try {
    ensureStateDir();
    const logPath = path.join(STATE_ROOT, "diag.log");
    rotateLogIfNeeded(logPath, DIAG_LOG_MAX_BYTES);
    fs.appendFileSync(logPath, `[${new Date().toISOString()}] ${line}\n`);
  } catch {}
}

/**
 * Wait until injectable renderer exists (app:// page).
 * On slow hosts the process/port may appear long before the first page.
 */
async function waitForDebugPort(port, timeoutMs = 45000, pollMs = 400) {
  const result = await hostProbe.waitForHostLifecycle(port, {
    want: ["ready"],
    timeoutMs,
    pollMs,
    onTick: (snap) => {
      if (snap.lifecycle === "starting") {
        /* quiet — only log occasionally via ensureDebugPort */
      }
    },
  });
  return Boolean(result?.rendererReady);
}

/**
 * Ensure CDP is up with remote debugging. Distinguishes:
 *   - process up but no debug port → must relaunch with --remote-debugging-port
 *   - port open but no app:// yet → wait (slow cold start), do not false-fail
 *   - fully ready → return immediately
 */
async function ensureDebugPort(port, { restart = true } = {}) {
  let probe = await hostProbe.probeHostLifecycle(port);
  const budget = hostProbe.resolveTimingBudget(probe);
  appendDiag(
    `ensureDebugPort begin port=${port} lifecycle=${probe.lifecycle} process=${probe.processRunning} portOpen=${probe.debugPortOpen} renderer=${probe.rendererReady} scale=${budget.scale} restart=${restart}`
  );

  // Explicit "restart client" from GUI: always stop + relaunch with debug port,
  // even when the renderer is already ready (desktopTheme only reloads on restart).
  if (restart) {
    appendDiag(
      `ensureDebugPort: forced restart (GUI/auto-restart) port=${port} wasReady=${probe.rendererReady}`
    );
    await stopCodex();
    await sleep(budget.stopSettleMs);
    await launchCodex(port);
    await sleep(budget.launchSettleMs);
    const afterForce = await hostProbe.waitForHostLifecycle(port, {
      want: ["starting", "ready"],
      timeoutMs: budget.waitDebugPortMs,
      pollMs: budget.pollMs,
    });
    if (afterForce?.rendererReady) return true;
    if (afterForce?.debugPortOpen) {
      appendDiag("ensureDebugPort: forced restart, waiting for renderer");
      if (await waitForDebugPort(port, budget.waitRendererMs, budget.pollMs)) return true;
    }
    // Fall through to hard relaunch retry below.
    probe = (await hostProbe.probeHostLifecycle(port)) || probe;
  } else {
    if (probe.rendererReady) return true;

    // Port is open but renderer not yet — common on slow disks. Wait before relaunching.
    if (probe.debugPortOpen && !probe.rendererReady) {
      appendDiag("ensureDebugPort: port open, waiting for app:// renderer (slow start)");
      if (await waitForDebugPort(port, budget.waitRendererMs, budget.pollMs)) return true;
      probe = await hostProbe.probeHostLifecycle(port);
    }

    const running = probe.codexRunning || (await findCodexMainPids()).length > 0;
    if (running && !probe.debugPortOpen) {
      throw new Error("ChatGPT 正在运行，需要重启后才能换肤。请勾选自动重启后再试。");
    }

    // Need a debug-enabled launch: no process, or port wait timed out without process.
    if (running) {
      // running with open port but not ready was already waited; if still not ready, relaunch.
      if (probe.debugPortOpen) {
        appendDiag("ensureDebugPort: running but not ready after wait; relaunch without GUI restart flag");
      }
      await stopCodex();
      await sleep(budget.stopSettleMs);
    }

    await launchCodex(port);
    await sleep(budget.launchSettleMs);
  }

  // Shared tail: wait for port + renderer, then hard relaunch once if needed.
  const afterLaunch = await hostProbe.waitForHostLifecycle(port, {
    want: ["starting", "ready"],
    timeoutMs: budget.waitDebugPortMs,
    pollMs: budget.pollMs,
  });
  if (afterLaunch?.rendererReady) return true;
  if (afterLaunch?.debugPortOpen) {
    appendDiag("ensureDebugPort: launched, waiting for renderer");
    if (await waitForDebugPort(port, budget.waitRendererMs, budget.pollMs)) return true;
  }

  appendDiag("ensureDebugPort: retry hard relaunch");
  await stopCodex();
  await sleep(budget.stopSettleMs + 300);
  await launchCodex(port);
  await sleep(budget.launchSettleMs);

  const final = await hostProbe.waitForHostLifecycle(port, {
    want: ["ready"],
    timeoutMs: budget.waitRendererMs + budget.waitDebugPortMs,
    pollMs: budget.pollMs,
  });
  if (final?.rendererReady) return true;

  const last = final || (await hostProbe.probeHostLifecycle(port));
  appendDiag(
    `ensureDebugPort: failed lifecycle=${last.lifecycle} process=${last.processRunning} portOpen=${last.debugPortOpen}`
  );
  throw new Error(
    process.platform === "win32"
      ? `未能就绪调试端口 ${port}（当前状态: ${last.lifecycle}）。慢速电脑请多等片刻后重试；或勾选自动重启并完全退出 ChatGPT 后再试。日志: ${path.join(STATE_ROOT, "diag.log")}`
      : "ChatGPT 启动超时，请手动打开后再试。"
  );
}

function killPid(pid, force = false) {
  if (!pid) return;
  if (process.platform === "win32") {
    try {
      require("child_process").spawnSync(
        "taskkill",
        force ? ["/PID", String(pid), "/T", "/F"] : ["/PID", String(pid), "/T"],
        { windowsHide: true, stdio: "ignore" }
      );
    } catch {}
    return;
  }
  try {
    process.kill(pid, force ? "SIGKILL" : "SIGTERM");
  } catch {}
}

function stopInjector(state) {
  if (!state?.injectorPid) return { skipped: true, reason: "no-pid" };
  const pid = state.injectorPid;
  // Hard rule (Dream Skin Stop-DreamSkinRecordedInjector):
  // never kill a live PID we cannot identify. Dead → ok; mismatch → refuse
  // (preserve state) so we never "succeed" after killing the wrong process.
  let looksOurs = false;
  try {
    looksOurs = processLooksLikeOurInjector(pid, state);
  } catch {
    looksOurs = false;
  }
  if (!looksOurs) {
    let alive = false;
    try {
      process.kill(pid, 0);
      alive = true;
    } catch {
      alive = false;
    }
    if (!alive) {
      appendDiag(`stopInjector: pid=${pid} already dead`);
      return { ok: true, pid, alreadyDead: true };
    }
    const cmd =
      process.platform === "win32" ? readWindowsProcessCommandLine(pid) : "";
    appendDiag(
      `stopInjector: refuse to kill pid=${pid} (identity mismatch; state preserved)` +
        (cmd ? ` cmd=${cmd.slice(0, 160)}` : " (no cmdline)")
    );
    return { skipped: true, reason: "identity-mismatch", pid, alive: true };
  }
  killPid(pid, false);
  killPid(pid, true);
  // Verify death — never report success if still alive
  let stillAlive = false;
  try {
    process.kill(pid, 0);
    stillAlive = true;
  } catch {
    stillAlive = false;
  }
  if (stillAlive) {
    appendDiag(`stopInjector: pid=${pid} still alive after kill attempts`);
    return { ok: false, reason: "still-alive", pid };
  }
  return { ok: true, pid };
}

/**
 * Codex desktop chrome theme keys in ~/.codex/config.toml [desktop].
 * Light skins use appearanceLight*; dark skins use appearanceDark*.
 * Window caption (min/max/close on Windows) + system dialogs follow these
 * chrome themes after host restart — CSS alone cannot recolor OS title buttons.
 */
function buildDesktopThemeSettings(theme = {}) {
  const appearance = theme.appearanceTheme || "light";
  const lightCode =
    theme.appearanceLightCodeThemeId || theme.appearanceDarkCodeThemeId || "codex";
  const darkCode =
    theme.appearanceDarkCodeThemeId || theme.appearanceLightCodeThemeId || "codex";
  // If only one chrome pair is authored, mirror it so host dialogs never fall
  // back to default white surfaces when appearanceTheme flips.
  const lightChrome =
    theme.appearanceLightChromeTheme != null
      ? theme.appearanceLightChromeTheme
      : theme.appearanceDarkChromeTheme != null
        ? theme.appearanceDarkChromeTheme
        : "{}";
  const darkChrome =
    theme.appearanceDarkChromeTheme != null
      ? theme.appearanceDarkChromeTheme
      : theme.appearanceLightChromeTheme != null
        ? theme.appearanceLightChromeTheme
        : "{}";

  const lines = [`appearanceTheme = "${appearance}"`];

  // Always emit light pair (Codex defaults; other skins rely on this).
  lines.push(`appearanceLightCodeThemeId = "${lightCode}"`);
  lines.push(`appearanceLightChromeTheme = ${lightChrome}`);

  // Emit dark pair when skin is dark or provides dark chrome tokens.
  // Without these, appearanceTheme=dark still paints dialogs/settings with
  // the host's default light surfaces (white bg / black text).
  if (
    appearance === "dark" ||
    theme.appearanceDarkCodeThemeId != null ||
    theme.appearanceDarkChromeTheme != null
  ) {
    lines.push(`appearanceDarkCodeThemeId = "${darkCode}"`);
    lines.push(`appearanceDarkChromeTheme = ${darkChrome}`);
  }

  return lines;
}

function readConfigStrict(filePath) {
  const buf = fs.readFileSync(filePath);
  if (buf.includes(0)) {
    throw new Error("config.toml contains NUL bytes; refusing to modify");
  }
  let start = 0;
  if (buf.length >= 3 && buf[0] === 0xef && buf[1] === 0xbb && buf[2] === 0xbf) start = 3;
  const body = buf.subarray(start);
  const text = body.toString("utf8");
  // Detect invalid UTF-8 replacement from Node is rare with Buffer; check unpaired
  if (body.includes(0xff) && !text) {
    throw new Error("config.toml is not valid UTF-8");
  }
  return { original: body, text, hasBom: start === 3 };
}

function atomicWriteConfig(filePath, originalBody, newText) {
  const now = fs.readFileSync(filePath);
  const nowBody =
    now.length >= 3 && now[0] === 0xef && now[1] === 0xbb && now[2] === 0xbf
      ? now.subarray(3)
      : now;
  if (!nowBody.equals(originalBody)) {
    throw new Error("config.toml changed during edit; refusing concurrent overwrite");
  }
  const dir = path.dirname(filePath);
  const tmp = path.join(dir, `.config.toml.chatgpt-tools.${process.pid}.tmp`);
  fs.writeFileSync(tmp, newText, { encoding: "utf8" });
  const now2 = fs.readFileSync(filePath);
  const now2Body =
    now2.length >= 3 && now2[0] === 0xef && now2[1] === 0xbb && now2[2] === 0xbf
      ? now2.subarray(3)
      : now2;
  if (!now2Body.equals(originalBody)) {
    try {
      fs.unlinkSync(tmp);
    } catch {}
    throw new Error("config.toml changed before replace; aborting");
  }
  const bak = path.join(dir, `.config.toml.chatgpt-tools.${process.pid}.bak`);
  try {
    if (fs.existsSync(filePath)) {
      try {
        fs.renameSync(filePath, bak);
      } catch {
        fs.copyFileSync(filePath, bak);
        fs.unlinkSync(filePath);
      }
    }
    fs.renameSync(tmp, filePath);
    try {
      if (fs.existsSync(bak)) fs.unlinkSync(bak);
    } catch {}
  } catch (e) {
    try {
      if (fs.existsSync(bak) && !fs.existsSync(filePath)) fs.renameSync(bak, filePath);
    } catch {}
    try {
      if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
    } catch {}
    throw e;
  }
}

function applyDesktopTheme(theme) {
  // 桌面 chrome 主题是可选增强；纯 CSS 注入不依赖 config.toml
  // Windows 上很多用户只有 ChatGPT 桌面、没有 ~/.codex/config.toml
  if (!fs.existsSync(CONFIG_PATH)) {
    try {
      fs.mkdirSync(path.dirname(CONFIG_PATH), { recursive: true });
      fs.writeFileSync(CONFIG_PATH, `[desktop]\n${buildDesktopThemeSettings(theme).join("\n")}\n`);
      return { created: true };
    } catch {
      return { skipped: true, reason: "config missing and create failed" };
    }
  }
  ensureStateDir();
  let originalBody;
  let content;
  try {
    const r = readConfigStrict(CONFIG_PATH);
    originalBody = r.original;
    content = r.text;
  } catch (e) {
    appendDiag("applyDesktopTheme refuse: " + e.message);
    return { skipped: true, reason: e.message };
  }
  // Sibling tables like [desktop.open-in-target-preferences] are common in Codex
  // configs. We only edit the exact [desktop] section body (until next [header]);
  // do not refuse the whole file when subtables exist.
  if (!fs.existsSync(BACKUP_PATH)) {
    fs.writeFileSync(BACKUP_PATH, originalBody);
  }
  const settings = buildDesktopThemeSettings(theme);
  const keys = [
    "appearanceTheme",
    "appearanceLightCodeThemeId",
    "appearanceLightChromeTheme",
    "appearanceDarkCodeThemeId",
    "appearanceDarkChromeTheme",
  ];
  const nl = content.includes("\r\n") ? "\r\n" : "\n";
  // Exact [desktop] header only (not [desktop.foo])
  const headerRe = /^\[desktop\][ \t]*(?:#[^\r\n]*)?\r?\n/m;
  let header = content.match(headerRe);
  if (!header) {
    content = content.replace(/\s*$/, "") + `${nl}${nl}[desktop]${nl}` + settings.join(nl) + nl;
  } else {
    const insertAt = header.index + header[0].length;
    const rest = content.slice(insertAt);
    const next = rest.search(/^\[[^\]]+\]/m);
    const section = next === -1 ? rest : rest.slice(0, next);
    const after = next === -1 ? "" : rest.slice(next);
    // Refuse multiline / duplicate target keys
    for (const k of keys) {
      const hits = section.split(/\r?\n/).filter((line) => new RegExp("^" + k + "\\s*=").test(line.trimStart()));
      if (hits.length > 1) {
        return { skipped: true, reason: `duplicate key ${k}` };
      }
      for (const line of hits) {
        const afterEq = line.split("=")[1]?.trim() || "";
        if (afterEq.startsWith('"""') || afterEq.startsWith("'''")) {
          return { skipped: true, reason: `multiline string form for ${k}` };
        }
        // Allow single-line `{ ... }` chrome theme; reject TOML arrays only.
        if (afterEq.startsWith("[")) {
          return { skipped: true, reason: `array form for ${k}` };
        }
      }
    }
    let lines = section.split(/\r?\n/);
    while (lines.length && lines[lines.length - 1].trim() === "") lines.pop();
    lines = lines.filter((line) => !keys.some((k) => new RegExp("^" + k + "\\s*=").test(line.trimStart())));
    lines.push(...settings);
    content = content.slice(0, insertAt) + lines.join(nl) + nl + after;
  }
  try {
    atomicWriteConfig(CONFIG_PATH, originalBody, content);
  } catch (e) {
    appendDiag("applyDesktopTheme atomic fail: " + e.message);
    return { skipped: true, reason: e.message };
  }
  return { ok: true, atomic: true };
}

function isOurThemeLine(line) {
  // 识别由皮肤写入的主题行；完全还原时直接删除，避免“脏备份”还原不干净
  return /^(appearanceTheme|appearanceLightCodeThemeId|appearanceLightChromeTheme|appearanceDarkCodeThemeId|appearanceDarkChromeTheme)\s*=/.test(
    line
  );
}

function restoreDesktopTheme() {
  if (!fs.existsSync(CONFIG_PATH)) return { restored: false, reason: "config missing" };
  let originalBody;
  let currentContent;
  try {
    const r = readConfigStrict(CONFIG_PATH);
    originalBody = r.original;
    currentContent = r.text;
  } catch (e) {
    return { restored: false, reason: e.message };
  }
  // Sibling [desktop.*] tables are preserved; we only strip appearance* from [desktop].
  const nl = currentContent.includes("\r\n") ? "\r\n" : "\n";
  const headerRe = /^\[desktop\][ \t]*(?:#[^\r\n]*)?\r?\n/m;
  const header = currentContent.match(headerRe);
  if (!header) return { restored: false, reason: "no desktop section" };

  const insertAt = header.index + header[0].length;
  const rest = currentContent.slice(insertAt);
  const next = rest.search(/^\[[^\]]+\]/m);
  const section = next === -1 ? rest : rest.slice(0, next);
  const after = next === -1 ? "" : rest.slice(next);

  let lines = section.split(/\r?\n/);
  while (lines.length && lines[lines.length - 1].trim() === "") lines.pop();

  // 1) 先去掉当前皮肤主题
  lines = lines.filter((line) => !isOurThemeLine(line));

  // 2) 若有更干净的历史备份（不含皮肤 accent），可恢复用户原主题
  const candidateBackups = [
    BACKUP_PATH,
    path.join(os.homedir(), "Library", "Application Support", "CodexDreamSkin", "config.before-dream-skin.toml"),
    path.join(os.homedir(), "Library", "Application Support", "CodexCnSkin", "config.before-cn-skin.toml"),
  ];
  if (process.platform === "win32") {
    const local = process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local");
    candidateBackups.push(
      path.join(local, "CodexDreamSkin", "config.before-dream-skin.toml"),
      path.join(local, "CodexCnSkin", "config.before-cn-skin.toml")
    );
  }

  let restoredFrom = null;
  for (const backup of candidateBackups) {
    try {
      if (!fs.existsSync(backup)) continue;
      const text = fs.readFileSync(backup, "utf8");
      const m = text.match(headerRe);
      if (!m) continue;
      const bRest = text.slice(m.index + m[0].length);
      const bNext = bRest.search(/^\[[^\]]+\]/m);
      const bSection = bNext === -1 ? bRest : bRest.slice(0, bNext);
      const bLines = bSection.split(/\r?\n/).filter((line) => isOurThemeLine(line));
      if (!bLines.length) {
        restoredFrom = backup;
        break;
      }
    } catch {}
  }

  // 不把脏备份里的 appearance* 写回去；删除后即使用 Codex 默认配色
  currentContent = currentContent.slice(0, insertAt) + lines.join(nl) + nl + after;
  try {
    atomicWriteConfig(CONFIG_PATH, originalBody, currentContent);
  } catch (e) {
    return { restored: false, reason: e.message };
  }
  return { restored: true, restoredFrom, strippedThemeKeys: true, atomic: true };
}

async function resolveWindowsStoreAumid() {
  try {
    const { stdout } = await runPowerShell(`
$ErrorActionPreference = 'SilentlyContinue'
$pkgs = @()
foreach ($n in @('OpenAI.Codex','OpenAI.ChatGPT','OpenAI.ChatGPT-Desktop')) {
  $pkgs += Get-AppxPackage -Name $n
}
$pkgs += Get-AppxPackage | Where-Object {
  $_.Name -match 'ChatGPT|Codex' -or $_.PackageFamilyName -match 'OpenAI'
}
$p = $pkgs | Sort-Object Version -Descending | Select-Object -First 1
if (-not $p) { return }
$manifest = Join-Path $p.InstallLocation 'AppxManifest.xml'
if (-not (Test-Path -LiteralPath $manifest)) {
  # 常见 AppId 兜底
  Write-Output ($p.PackageFamilyName + '!App')
  return
}
try {
  [xml]$x = Get-Content -LiteralPath $manifest
  $app = @($x.Package.Applications.Application) | Select-Object -First 1
  if ($app -and $app.Id) {
    Write-Output ($p.PackageFamilyName + '!' + $app.Id)
    return
  }
} catch {}
Write-Output ($p.PackageFamilyName + '!App')
`);
    return firstNonEmptyLine(stdout) || null;
  } catch {
    return null;
  }
}

function isWindowsStorePath(exe) {
  return typeof exe === "string" && /[\\/]WindowsApps[\\/]/i.test(exe);
}

async function launchWindowsStoreApp(port, aumidPref = null) {
  const aumid = aumidPref || (await resolveWindowsStoreAumid());
  if (!aumid) return null;
  const arg = `--remote-debugging-port=${port}`;
  // Store 包不能直接 spawn WindowsApps 下的 exe（Access Denied），
  // 必须用 IApplicationActivationManager 并带上调试参数。
  const script = `
$ErrorActionPreference = 'Stop'
if (-not ('ChatGPTToolsAppLauncher' -as [type])) {
  $code = @'
using System;
using System.Runtime.InteropServices;
public class ChatGPTToolsAppLauncher {
  [ComImport, Guid("2e941141-7f97-4756-ba1d-9decde894a3d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IApplicationActivationManager {
    IntPtr ActivateApplication([In] String appUserModelId, [In] String arguments, [In] UInt32 options, [Out] out UInt32 processId);
  }
  [ComImport, Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C")]
  class ApplicationActivationManager {}
  public static uint Launch(string aumid, string args) {
    var mgr = new ApplicationActivationManager();
    var iam = (IApplicationActivationManager)mgr;
    uint pid;
    iam.ActivateApplication(aumid, args, 0, out pid);
    return pid;
  }
}
'@
  Add-Type -TypeDefinition $code
}
$launchPid = [ChatGPTToolsAppLauncher]::Launch('${aumid.replace(/'/g, "''")}', '${arg.replace(/'/g, "''")}')
Write-Output $launchPid
`;
  const { stdout } = await runPowerShell(script, 30000);
  const pid = Number(firstNonEmptyLine(stdout));
  return Number.isFinite(pid) && pid > 0 ? pid : 1;
}

function spawnWithDebugPort(exe, port) {
  const arg = `--remote-debugging-port=${port}`;
  try {
    const child = spawn(exe, [arg], {
      detached: true,
      stdio: "ignore",
      windowsHide: false,
      windowsVerbatimArguments: process.platform === "win32",
    });
    child.unref();
    return child.pid;
  } catch {
    if (process.platform === "win32") {
      const child = spawn(`"${exe}" ${arg}`, {
        detached: true,
        stdio: "ignore",
        windowsHide: false,
        shell: true,
      });
      child.unref();
      return child.pid;
    }
    throw new Error(`无法启动: ${exe}`);
  }
}

async function launchCodex(port) {
  // 用户手动指定的非 Store 路径优先
  const configured = getConfiguredAppPath();
  if (configured) {
    const fixed = expandConfiguredPath(configured);
    if (fixed && !isWindowsStorePath(fixed)) {
      return spawnWithDebugPort(fixed, port);
    }
  }

  // Windows Store：优先 AUMID 激活（不依赖 exe 可读/可 spawn）
  if (process.platform === "win32") {
    const aumid = await resolveWindowsStoreAumid();
    if (aumid) {
      try {
        const pid = await launchWindowsStoreApp(port, aumid);
        if (pid) return pid;
      } catch {
        // 继续尝试 exe
      }
    }
  }

  let exe = resolveCodexExe();
  if (exe && typeof exe === "object" && (exe.type === "windows-store" || exe.type === "windows-resolve")) {
    exe = await resolveWindowsCodexExe();
  }
  if (!exe || typeof exe !== "string") {
    if (process.platform === "win32") {
      const pid = await launchWindowsStoreApp(port);
      if (pid) return pid;
    }
    throw new Error(
      "未找到 Codex / ChatGPT 桌面版。可在界面点「指定客户端」选择 ChatGPT.exe，或设置 CODEX_APP_PATH。"
    );
  }

  // Microsoft Store 版路径在 WindowsApps 下，直接 spawn 会被拒绝
  if (process.platform === "win32" && isWindowsStorePath(exe)) {
    const pid = await launchWindowsStoreApp(port);
    if (pid) return pid;
    throw new Error(
      "检测到 Microsoft Store 版 Codex/ChatGPT，但无法带调试端口启动。请完全退出后重试，或点「指定客户端」。"
    );
  }

  return spawnWithDebugPort(exe, port);
}

function resolveNodeRuntime() {
  // 安装版优先用自身 Electron 当 Node，避免系统 node 缺失/版本过旧
  const electronAsNode = {
    bin: process.execPath,
    env: { ...process.env, ELECTRON_RUN_AS_NODE: "1" },
  };
  if (process.versions?.electron && process.resourcesPath) {
    try {
      if (fs.existsSync(path.join(process.resourcesPath, "app.asar"))) {
        return electronAsNode;
      }
    } catch {}
  }
  const which = process.platform === "win32" ? "where" : "which";
  try {
    const { stdout } = require("child_process").spawnSync(which, ["node"], { encoding: "utf8" });
    const candidate = String(stdout || "")
      .split(/\r?\n/)
      .map((s) => s.trim())
      .find(Boolean);
    if (candidate && fs.existsSync(candidate)) {
      return { bin: candidate, env: { ...process.env } };
    }
  } catch {}
  return electronAsNode;
}

function startInjector(skin, port, { browserId = null } = {}) {
  ensureStateDir();
  // 必须用 ENGINE_DIR（asar.unpacked），外部 node 读不了 app.asar 内脚本
  const injector = path.join(ENGINE_DIR, "injector.mjs");
  const outLog = path.join(STATE_ROOT, "injector.log");
  const errLog = path.join(STATE_ROOT, "injector-error.log");
  rotateLogIfNeeded(outLog, INJECTOR_LOG_MAX_BYTES);
  rotateLogIfNeeded(errLog, INJECTOR_LOG_MAX_BYTES);
  // Clear stale control files so a new watch does not replay old switch commands.
  try {
    if (fs.existsSync(CONTROL_PATH)) fs.unlinkSync(CONTROL_PATH);
  } catch {}
  try {
    if (fs.existsSync(CONTROL_RESULT_PATH)) fs.unlinkSync(CONTROL_RESULT_PATH);
  } catch {}
  const out = fs.openSync(outLog, "a");
  const err = fs.openSync(errLog, "a");
  const runtime = resolveNodeRuntime();
  const safeBrowserId = normalizeBrowserIdForArg(browserId);
  const args = [
    injector,
    "--watch",
    "--port",
    String(port),
    "--skin-dir",
    skin.dir,
    "--pause-file",
    PAUSE_PATH,
    "--control-file",
    CONTROL_PATH,
  ];
  if (safeBrowserId) args.push("--browser-id", safeBrowserId);
  const child = spawn(runtime.bin, args, {
    detached: true,
    stdio: ["ignore", out, err],
    windowsHide: true,
    env: {
      ...runtime.env,
      CODEX_SKIN_ROOT: runtime.env.CODEX_SKIN_ROOT || ROOT,
      CODEX_SKIN_STATE_NAME: process.env.CODEX_SKIN_STATE_NAME || "ChatGPTTools",
    },
  });
  child.unref();
  return {
    pid: child.pid,
    outLog,
    errLog,
    injectorScript: injector,
    nodePath: runtime.bin,
    browserId: safeBrowserId || null,
    controlFile: CONTROL_PATH,
  };
}

/**
 * Send a command to the long-lived watch injector via control file.
 * Returns parsed result JSON or null on timeout / missing watcher.
 */
async function sendInjectorControl(cmd, payload = {}, timeoutMs = 8000) {
  const state = readState();
  if (!state?.injectorPid || !processLooksLikeOurInjector(state.injectorPid, state)) {
    return { ok: false, reason: "no-live-injector" };
  }
  ensureStateDir();
  const requestId = crypto.randomBytes(8).toString("hex");
  try {
    if (fs.existsSync(CONTROL_RESULT_PATH)) fs.unlinkSync(CONTROL_RESULT_PATH);
  } catch {}
  const body = {
    cmd,
    requestId,
    at: new Date().toISOString(),
    ...payload,
  };
  fs.writeFileSync(CONTROL_PATH, JSON.stringify(body, null, 2) + "\n");
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      if (fs.existsSync(CONTROL_RESULT_PATH)) {
        const text = fs.readFileSync(CONTROL_RESULT_PATH, "utf8");
        const json = JSON.parse(text);
        if (json && json.requestId === requestId) {
          return json;
        }
      }
    } catch {
      /* keep polling */
    }
    await sleep(120);
  }
  return { ok: false, reason: "control-timeout", requestId };
}

function injectorIsLive(state) {
  if (!state?.injectorPid) return false;
  try {
    return processLooksLikeOurInjector(state.injectorPid, state);
  } catch {
    return false;
  }
}

async function getStatus() {
  // Never throw on partial probe failures — GUI depends on this for running pills.
  let state = null;
  try {
    state = readState();
  } catch (e) {
    appendDiag("getStatus readState: " + (e.message || e));
  }

  let skins = [];
  try {
    skins = listSkins();
  } catch (e) {
    appendDiag("getStatus listSkins: " + (e.message || e));
  }

  const port = state?.port || SHARED_PORT;
  let host = {
    lifecycle: "offline",
    processRunning: false,
    debugPortOpen: false,
    rendererReady: false,
    debugReady: false,
    codexRunning: false,
    pids: [],
  };
  try {
    host = await hostProbe.probeHostLifecycle(port);
  } catch (e) {
    appendDiag("getStatus probe: " + (e.message || e));
  }

  const debugReady = host.rendererReady;
  const codexRunning = host.codexRunning;
  const activeSkinId = state?.skinId || null;
  const paused = isPaused();

  let injectorAlive = false;
  try {
    injectorAlive = Boolean(
      state?.injectorPid && processLooksLikeOurInjector(state.injectorPid, state)
    );
  } catch {
    injectorAlive = false;
  }

  // Active skin: state matches + not paused + host not offline (stable lifecycle).
  const hostEngaged =
    host.hostEngaged !== undefined
      ? Boolean(host.hostEngaged) || injectorAlive
      : debugReady ||
        host.debugPortOpen ||
        host.processRunning ||
        injectorAlive ||
        host.lifecycle === "ready" ||
        host.lifecycle === "starting";

  return {
    platform: process.platform,
    configPath: CONFIG_PATH,
    stateRoot: STATE_ROOT,
    state,
    debugReady: debugReady || host.lifecycle === "ready",
    /** CDP HTTP up (may still be loading first app:// page). */
    debugPortOpen: host.debugPortOpen,
    processRunning: host.processRunning,
    /** offline | starting | ready (stable, after hysteresis) */
    lifecycle: host.lifecycle,
    lifecycleRaw: host.lifecycleRaw || host.lifecycle,
    lifecycleLabel: host.lifecycleLabel || host.lifecycle,
    confidence: host.confidence || "high",
    canHotApply: Boolean(host.canHotApply ?? debugReady),
    needsRestartForInject: Boolean(
      host.needsRestartForInject ?? (host.processRunning && !host.debugPortOpen)
    ),
    signals: host.signals || {
      process: host.processRunning,
      port: host.debugPortOpen,
      renderer: host.rendererReady,
    },
    codexRunning,
    paused,
    protocol: ENGINE_PROTOCOL,
    engineVersion: ENGINE_VERSION,
    engineName: ENGINE_NAME,
    ok: true,
    shellOk: Boolean(state?.shellOk),
    artOk: Boolean(state?.artOk),
    artPending: Boolean(state?.artPending),
    injectorAlive,
    skins: skins.map((s) => {
      const artRel = s.assets?.art || "";
      const artAbs = artRel ? path.join(s.dir, artRel) : "";
      const hasArt = artAbs && fs.existsSync(artAbs);
      let previewKind = "";
      try {
        resolveSkinAsset(s.id, "screenshot");
        previewKind = "screenshot";
      } catch {
        if (hasArt) previewKind = "art";
      }
      return {
        id: s.id,
        name: s.name,
        nameEn: s.nameEn,
        description: s.description,
        tags: s.tags,
        categories: Array.isArray(s.categories) ? s.categories : [],
        previewGradient: s.previewGradient,
        accent: s.accent,
        active: Boolean(
          activeSkinId && activeSkinId === s.id && !paused && hostEngaged
        ),
        previewUrl: previewKind ? `skin-asset://local/${s.id}/${previewKind}` : "",
        previewKind,
        builtin: Boolean(s.builtin),
        source: s.source || "bundled",
        appearance: s.appearance || s.theme?.appearance || "auto",
      };
    }),
  };
}

/**
 * Resolve a skin asset path.
 * - screenshot: prefer assets/screenshot.{png,jpg,jpeg,webp} (UI thumbnail, small)
 * - art: full illustration used for injection (may be large)
 * - preview: screenshot if present, otherwise art (for status list cards)
 */
function resolveSkinAsset(skinId, kind) {
  const skin = getSkin(skinId);
  if (kind === "screenshot") {
    const candidates = [
      skin.assets?.screenshot,
      "assets/screenshot.png",
      "assets/screenshot.jpg",
      "assets/screenshot.jpeg",
      "assets/screenshot.webp",
    ].filter(Boolean);
    for (const rel of candidates) {
      const abs = path.join(skin.dir, rel);
      if (fs.existsSync(abs) && fs.statSync(abs).isFile()) return abs;
    }
    throw new Error("No screenshot asset");
  }
  if (kind === "art") {
    const rel = skin.assets?.art;
    if (!rel) throw new Error("No art asset");
    const abs = path.join(skin.dir, rel);
    if (!fs.existsSync(abs)) throw new Error(`Art not found: ${abs}`);
    return abs;
  }
  if (kind === "preview") {
    try {
      return resolveSkinAsset(skinId, "screenshot");
    } catch {
      return resolveSkinAsset(skinId, "art");
    }
  }
  throw new Error(`Unknown asset kind: ${kind}`);
}

/**
 * Parse injector once/verify JSON stdout for shell/art outcomes.
 */
function parseInjectorOnceResult(stdout) {
  try {
    const text = String(stdout || "").trim();
    if (!text) return { ok: false, shellOk: false, artOk: false, raw: null };
    const json = JSON.parse(text);
    const targets = Array.isArray(json.targets) ? json.targets : [];
    const anyPass = targets.some((t) => {
      const r = t?.result;
      if (typeof r === "boolean") return r;
      return Boolean(r?.pass);
    });
    const artOk = targets.some((t) => {
      const r = t?.result;
      return r && (r.artOk === true || r.artAttached === true);
    });
    const artPending = targets.some((t) => t?.result?.artPending === true);
    return {
      ok: anyPass,
      shellOk: anyPass,
      artOk,
      artPending: artPending || (anyPass && !artOk),
      soft: Boolean(json.soft),
      browserId: json.browserId || null,
      raw: json,
    };
  } catch {
    return { ok: false, shellOk: false, artOk: false, raw: null };
  }
}

async function runInjectorOnce(
  skin,
  port,
  mode,
  extraArgs = [],
  { browserId = null, timeoutMs = null, execTimeoutMs = null } = {}
) {
  const injector = path.join(ENGINE_DIR, "injector.mjs");
  const runtime = resolveNodeRuntime();
  const safeBrowserId = normalizeBrowserIdForArg(browserId);
  const budget = hostProbe.resolveTimingBudget();
  const cdpTimeout =
    timeoutMs ??
    (mode === "once" ? budget.softOnceTimeoutMs : budget.softVerifyTimeoutMs);
  const execTimeout =
    execTimeoutMs ??
    (mode === "once" ? budget.softOnceExecMs : budget.softVerifyTimeoutMs + 8000);
  const args = [
    injector,
    `--${mode}`,
    "--port",
    String(port),
    "--skin-dir",
    skin.dir,
    "--timeout-ms",
    String(Math.min(120000, Math.max(250, Math.round(cdpTimeout)))),
    ...extraArgs,
  ];
  if (safeBrowserId) args.push("--browser-id", safeBrowserId);
  const { stdout, stderr } = await execFileAsync(runtime.bin, args, {
    timeout: Math.min(180000, Math.max(5000, Math.round(execTimeout))),
    windowsHide: true,
    env: runtime.env,
    maxBuffer: 8 * 1024 * 1024,
  });
  return { stdout, stderr, parsed: parseInjectorOnceResult(stdout) };
}

async function checkSkinPayload(skinId) {
  const skin = getSkin(skinId);
  const runtime = resolveNodeRuntime();
  const injector = path.join(ENGINE_DIR, "injector.mjs");
  const { stdout } = await execFileAsync(
    runtime.bin,
    [injector, "--check-payload", "--skin-dir", skin.dir],
    { timeout: 30000, windowsHide: true, env: runtime.env, maxBuffer: 4 * 1024 * 1024 }
  );
  return JSON.parse(String(stdout || "{}").trim());
}

async function quickPurgePort(port) {
  // 不刷新页面，只清掉所有旧皮肤 DOM/监听，避免切换皮肤时残留
  if (!(await testDebugPort(port))) return false;
  try {
    const runtime = resolveNodeRuntime();
    await execFileAsync(
      runtime.bin,
      [
        path.join(ENGINE_DIR, "purge-all.mjs"),
        "--port",
        String(port),
        "--timeout-ms",
        "2500",
        "--no-reload",
      ],
      { timeout: 6000, windowsHide: true, env: runtime.env }
    );
    return true;
  } catch {
    // 再兜底：按已安装皮肤逐个 remove
    for (const s of listSkins()) {
      try {
        await runInjectorOnce(s, port, "remove");
      } catch {}
    }
    return false;
  }
}

async function applySkin(skinId, { restart = false } = {}) {
  return withEngineLock(async () => {
    const baseSkin = getSkin(skinId);
    validateSkinManifest(baseSkin, baseSkin.dir);
    // 安装版：把皮肤落到可写目录再注入，确保立绘/CSS 可被外部进程读取
    const skin = materializeSkin(baseSkin);
    const port = SHARED_PORT;
    ensureStateDir();
    setPaused(false);

    let seedProbe = null;
    try {
      seedProbe = await hostProbe.probeHostLifecycle(port);
    } catch {
      seedProbe = null;
    }
    const budget = hostProbe.resolveTimingBudget(seedProbe);

    appendDiag(
      `applySkin id=${skin.id} dir=${skin.dir} engine=${ENGINE_DIR} root=${ROOT} restart=${restart} lifecycle=${seedProbe?.lifecycle || "?"} scale=${budget.scale}`
    );

    // Desktop chrome theme MUST be written before host restart: Codex only reloads
    // config.toml [desktop] on process start. CSS inject does not depend on this.
    let themeResult = { skipped: true, reason: "no desktopTheme" };
    try {
      if (skin.desktopTheme && typeof skin.desktopTheme === "object") {
        themeResult = applyDesktopTheme(skin.desktopTheme) || themeResult;
        appendDiag("applyDesktopTheme result: " + JSON.stringify(themeResult));
      }
    } catch (e) {
      themeResult = { skipped: true, reason: e.message };
      appendDiag("applyDesktopTheme skip: " + e.message);
    }

    // Forced client restart: stop injectors first, then relaunch host (no hot path).
    if (restart) {
      const prevForStop = readState();
      if (prevForStop) stopInjector(prevForStop);
      stopExternalSkinInjectors();
      await sleep(80);
      await ensureDebugPort(port, { restart: true });
    } else {
      await ensureDebugPort(port, { restart: false });
    }

    // Re-probe after ensure (restart may have relaunched the client).
    try {
      seedProbe = await hostProbe.probeHostLifecycle(port);
    } catch {
      /* keep previous */
    }

    const prev = readState();
    const liveInjector = !restart && injectorIsLive(prev);
    // Same skin + live watch → reapply only (no stop/spawn). Disabled when restart=true.
    const sameSkinLive =
      liveInjector &&
      prev &&
      prev.skinId === skin.id &&
      prev.skinDir === skin.dir &&
      seedProbe?.rendererReady;
    // Different skin (or same id new dir) + live watch → control-file hot-switch.
    const crossSkinHot =
      liveInjector &&
      prev &&
      seedProbe?.rendererReady &&
      (prev.skinId !== skin.id || prev.skinDir !== skin.dir);

    if (!sameSkinLive && !crossSkinHot) {
      if (prev) stopInjector(prev);
      stopExternalSkinInjectors();
      await sleep(80);
    }

    let browserId = await readCdpBrowserId(port);
    browserId = normalizeBrowserIdForArg(browserId);
    if (!browserId) {
      appendDiag("applySkin: could not read CDP browser UUID; continuing without --browser-id");
    } else {
      appendDiag("applySkin browserId=" + browserId);
    }

    // Cold path / forced restart: purge residual skins then spawn watch.
    // Hot paths keep sessions + use delta/host soft residual cleanup.
    if (!sameSkinLive && !crossSkinHot) {
      await quickPurgePort(port);
    }

    let started = null;
    let errLog = path.join(STATE_ROOT, "injector-error.log");
    let applyMode = "cold";
    let hotSwitchMeta = null;

    if (crossSkinHot) {
      appendDiag(
        `applySkin: hot-switch via control ${prev.skinId} → ${skin.id} (no stop/spawn)`
      );
      const controlTimeout = Math.min(20000, Math.max(6000, budget.softOnceExecMs || 8000));
      const controlResult = await sendInjectorControl(
        "switch",
        { skinDir: skin.dir, skinId: skin.id },
        controlTimeout
      );
      if (controlResult?.ok) {
        applyMode = "hot-switch";
        hotSwitchMeta = controlResult;
        writeState({
          ...prev,
          schema: 2,
          skinId: skin.id,
          port,
          browserId: browserId || prev.browserId || null,
          startedAt: prev.startedAt || new Date().toISOString(),
          platform: process.platform,
          skinDir: skin.dir,
          phase: "applying",
          shellOk: false,
          artOk: false,
          artPending: true,
          applyMode: "hot-switch",
          engineVersion: ENGINE_VERSION,
        });
      } else {
        // Control failed — fall back to cold restart of injector.
        appendDiag(
          `applySkin: hot-switch failed (${controlResult?.reason || controlResult?.message || "?"}); cold fallback`
        );
        try {
          stopInjector(prev);
        } catch {}
        stopExternalSkinInjectors();
        await sleep(80);
        await quickPurgePort(port);
        started = startInjector(skin, port, { browserId });
        errLog = started.errLog;
        applyMode = "cold-fallback";
        writeState({
          schema: 2,
          skinId: skin.id,
          port,
          injectorPid: started.pid,
          injectorScript: started.injectorScript,
          nodePath: started.nodePath,
          browserId: browserId || null,
          startedAt: new Date().toISOString(),
          platform: process.platform,
          skinDir: skin.dir,
          phase: "applying",
          shellOk: false,
          artOk: false,
          artPending: true,
          applyMode: "cold-fallback",
          engineVersion: ENGINE_VERSION,
        });
        await sleep(Math.min(400, budget.launchSettleMs));
      }
    } else if (sameSkinLive) {
      appendDiag("applySkin: hot reapply via control/soft once (watch already running)");
      applyMode = "hot-reapply";
      // Prefer control reapply so watch rebuilds from disk if assets changed.
      const reapply = await sendInjectorControl("reapply", { skinDir: skin.dir }, 5000);
      if (!reapply?.ok) {
        appendDiag(`applySkin: reapply control soft-fail (${reapply?.reason || "?"}); soft once still runs`);
      }
      writeState({
        ...prev,
        schema: 2,
        skinId: skin.id,
        port,
        browserId: browserId || prev.browserId || null,
        startedAt: prev.startedAt || new Date().toISOString(),
        platform: process.platform,
        skinDir: skin.dir,
        phase: "applying",
        shellOk: false,
        artOk: false,
        artPending: true,
        applyMode: "hot-reapply",
      });
    } else {
      // Watch first (owns long-lived sessions + large art reinject on navigation).
      started = startInjector(skin, port, { browserId });
      errLog = started.errLog;
      applyMode = "cold";
      writeState({
        schema: 2,
        skinId: skin.id,
        port,
        injectorPid: started.pid,
        injectorScript: started.injectorScript,
        nodePath: started.nodePath,
        browserId: browserId || null,
        startedAt: new Date().toISOString(),
        platform: process.platform,
        skinDir: skin.dir,
        phase: "applying",
        shellOk: false,
        artOk: false,
        artPending: true,
        applyMode: "cold",
        engineVersion: ENGINE_VERSION,
      });
      // Brief settle so watch can attach before soft once (reduces double work races).
      await sleep(Math.min(400, budget.launchSettleMs));
    }

    // soft once: shell success is enough; art may still be streaming (large originals OK).
    let lastError = null;
    let verified = false;
    let verifyMode = "soft";
    let shellOk = false;
    let artOk = false;
    let artPending = true;
    const onceAttempts = budget.scale > 1.3 ? 8 : 5;
    for (let i = 0; i < onceAttempts; i++) {
      try {
        const result = await runInjectorOnce(skin, port, "once", ["--soft"], {
          browserId,
          timeoutMs: budget.softOnceTimeoutMs,
          execTimeoutMs: budget.softOnceExecMs,
        });
        const parsed = result.parsed || parseInjectorOnceResult(result.stdout);
        if (parsed.ok || parsed.shellOk) {
          verified = true;
          shellOk = true;
          artOk = Boolean(parsed.artOk);
          artPending = Boolean(parsed.artPending) || !artOk;
          break;
        }
        lastError = new Error("soft once did not pass");
        appendDiag(`inject once no-pass#${i}`);
      } catch (e) {
        lastError = e;
        appendDiag("inject once fail#" + i + ": " + e.message);
      }
      await sleep(250 + i * 100);
    }

    if (!verified) {
      const verifyAttempts = budget.scale > 1.3 ? 10 : 6;
      for (let i = 0; i < verifyAttempts; i++) {
        await sleep(300);
        try {
          const result = await runInjectorOnce(skin, port, "verify", ["--soft"], {
            browserId,
            timeoutMs: budget.softVerifyTimeoutMs,
            execTimeoutMs: budget.softVerifyTimeoutMs + 10000,
          });
          const parsed = result.parsed || parseInjectorOnceResult(result.stdout);
          if (parsed.ok || parsed.shellOk) {
            verified = true;
            shellOk = true;
            artOk = Boolean(parsed.artOk);
            artPending = !artOk;
            verifyMode = "soft-verify";
            break;
          }
        } catch (e) {
          lastError = e;
        }
      }
    }

    if (!verified) {
      try {
        stopInjector(readState());
      } catch {}
      try {
        if (fs.existsSync(STATE_PATH)) fs.unlinkSync(STATE_PATH);
      } catch {}
      let tail = "";
      try {
        if (fs.existsSync(errLog)) tail = fs.readFileSync(errLog, "utf8").slice(-500);
      } catch {}
      throw new Error(
        (
          "换肤未完成（样式可能未注入；大立绘会在 shell 之后异步贴图）。" +
          (lastError ? lastError.message : "") +
          " " +
          tail
        ).trim()
      );
    }

    const cur = readState() || {};
    writeState({
      ...cur,
      schema: 2,
      skinId: skin.id,
      port,
      browserId: browserId || cur.browserId || null,
      skinDir: skin.dir,
      phase: "active",
      shellOk,
      artOk,
      artPending,
      applyMode,
      verifiedAt: new Date().toISOString(),
      engineVersion: ENGINE_VERSION,
      injectorPid: cur.injectorPid || started?.pid || prev?.injectorPid,
      injectorScript: cur.injectorScript || started?.injectorScript || prev?.injectorScript,
      nodePath: cur.nodePath || started?.nodePath || prev?.nodePath,
      startedAt: cur.startedAt || new Date().toISOString(),
      platform: process.platform,
    });

    appendDiag(
      `applySkin ok id=${skin.id} shellOk=${shellOk} artOk=${artOk} artPending=${artPending} verify=${verifyMode} apply=${applyMode}`
    );
    return {
      ok: true,
      skinId: skin.id,
      port,
      verified: true,
      verifyMode,
      applyMode,
      hotSwitch: hotSwitchMeta,
      shellOk,
      artOk,
      artPending,
      browserId: browserId || null,
      skinDir: skin.dir,
      lifecycle: "ready",
      engineVersion: ENGINE_VERSION,
      theme: themeResult,
    };
  });
}

async function pauseSkin() {
  return withEngineLock(async () => {
    // Flag first so watch/keep cannot re-paint (Dream live-pause order).
    setPaused(true);
    const state = readState();
    const port = state?.port || SHARED_PORT;
    let browserId = state?.browserId || null;
    try {
      browserId = browserId || (await readCdpBrowserId(port));
    } catch {
      /* host may be offline */
    }
    let hostLive = false;
    try {
      const life = await hostProbe.probeHostLifecycle(port, { force: true });
      hostLive = Boolean(life?.rendererReady || life?.debugPortOpen);
    } catch {
      hostLive = false;
    }
    let removed = { ok: false, skipped: true };
    let removeError = null;
    if (state?.skinDir && fs.existsSync(state.skinDir)) {
      try {
        const out = await runInjectorOnce(
          { dir: state.skinDir, id: state.skinId },
          port,
          "remove",
          [],
          { browserId }
        );
        try {
          removed = JSON.parse(String(out?.stdout || "{}").trim() || "{}");
        } catch {
          // Never invent ok:true from unparsable stdout (Dream: no false success).
          removed = { ok: false, raw: true, parseError: true };
          removeError = "remove 输出无法解析";
        }
        if (removed && removed.ok === false) {
          removeError = removeError || removed.reason || removed.error || "remove did not pass";
        }
      } catch (e) {
        removeError = e.message || String(e);
        appendDiag("pause remove: " + removeError);
      }
    } else if (hostLive) {
      try {
        const purged = await quickPurgePort(port);
        removed = purged
          ? { ok: true, purged: true }
          : { ok: false, purged: false, reason: "purge-failed" };
        if (!purged) removeError = "通用清理未成功";
      } catch (e) {
        appendDiag("pause purge: " + e.message);
        removed = { ok: false, purged: false, error: e.message };
        removeError = e.message;
      }
    }

    // Host offline → pause marker alone is success.
    // Host live → require remove/purge ok (Dream #168 honesty).
    if (hostLive && removed?.ok !== true) {
      const err = new Error(
        `已写入暂停标记，但即时卸下皮肤失败: ${removeError || "unknown"}`
      );
      err.code = "PAUSE_REMOVE_FAILED";
      err.paused = true;
      err.port = port;
      err.removed = removed;
      throw err;
    }

    if (state) {
      try {
        writeState({
          ...state,
          phase: "paused",
          pausedAt: new Date().toISOString(),
        });
      } catch (e) {
        appendDiag("pause writeState: " + e.message);
      }
    }
    return { ok: true, paused: true, port, removed, hostLive };
  });
}

async function resumeSkin({ restart = false } = {}) {
  const state = readState();
  if (!state?.skinId) throw new Error("没有可恢复的皮肤会话，请先应用一套皮肤");
  setPaused(false);
  return applySkin(state.skinId, { restart });
}

async function hardVerifySkin(skinId) {
  const skin = getSkin(skinId);
  const port = SHARED_PORT;
  const browserId = await readCdpBrowserId(port);
  const { stdout } = await runInjectorOnce(skin, port, "verify", [], { browserId });
  try {
    return JSON.parse(String(stdout || "{}").trim());
  } catch {
    return { raw: stdout };
  }
}

function stopProcessTree(pid) {
  killPid(pid, false);
  killPid(pid, true);
}

function stopExternalSkinInjectors() {
  // 顺带停掉早期独立皮肤脚本留下的注入进程，避免“还原后又被重新注入”。
  // Dream habit: only stop PIDs whose recorded identity matches (never kill by PID alone).
  const home = os.homedir();
  const externalStates = [
    path.join(home, "Library", "Application Support", "CodexDreamSkin", "state.json"),
    path.join(home, "Library", "Application Support", "CodexCnSkin", "state.json"),
  ];
  if (process.platform === "win32") {
    const local = process.env.LOCALAPPDATA || path.join(home, "AppData", "Local");
    externalStates.push(
      path.join(local, "CodexDreamSkin", "state.json"),
      path.join(local, "CodexCnSkin", "state.json")
    );
  }
  for (const statePath of externalStates) {
    try {
      if (!fs.existsSync(statePath)) continue;
      const s = JSON.parse(fs.readFileSync(statePath, "utf8"));
      if (s?.injectorPid) {
        // Reuse identity gate: map Dream fields onto our checker shape.
        const identity = {
          injectorPid: s.injectorPid,
          injectorScript: s.injectorPath || s.injectorScript || "",
          port: s.port,
          browserId: s.browserId,
          nodePath: s.nodePath,
        };
        // Prefer path+port+injector.mjs match; if Dream state has skillRoot, seed script path.
        if (!identity.injectorScript && s.skillRoot) {
          identity.injectorScript = path.join(String(s.skillRoot), "scripts", "injector.mjs");
        }
        const stop = stopInjector(identity);
        if (stop?.skipped && stop.reason === "identity-mismatch") {
          appendDiag(
            `stopExternalSkinInjectors: refuse pid=${s.injectorPid} from ${statePath}`
          );
          // Do not delete foreign state when we could not verify the process.
          continue;
        }
      }
      try {
        fs.unlinkSync(statePath);
      } catch {}
    } catch (e) {
      appendDiag("stopExternalSkinInjectors state: " + (e.message || e));
    }
  }

  // 兜底：仅杀 cmdline 同时含 injector.mjs 与已知皮肤项目标识的进程（不按裸 PID）。
  try {
    if (process.platform === "darwin" || process.platform === "linux") {
      const { stdout } = require("child_process").spawnSync(
        "pgrep",
        ["-f", "injector\\.mjs.*(codex-skin-manager|codex-dream-skin|codex-cn-skin|chatgpt-tools)"],
        { encoding: "utf8" }
      );
      String(stdout || "")
        .trim()
        .split(/\s+/)
        .filter(Boolean)
        .forEach((pid) => {
          const n = Number(pid);
          if (!Number.isFinite(n) || n <= 0) return;
          // Best-effort identity: require cmdline still matches after pgrep.
          try {
            const { stdout: cmdOut } = require("child_process").spawnSync(
              "ps",
              ["-p", String(n), "-o", "command="],
              { encoding: "utf8" }
            );
            const cmd = String(cmdOut || "");
            if (!/injector\.mjs/i.test(cmd)) return;
            if (!/codex-(skin-manager|dream-skin|cn-skin)|chatgpt-tools/i.test(cmd)) return;
            stopProcessTree(n);
          } catch {}
        });
    } else if (process.platform === "win32") {
      // Identity-scoped: injector.mjs AND (known skin project OR our engine path).
      require("child_process").spawnSync(
        "powershell.exe",
        [
          "-NoProfile",
          "-Command",
          "Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and ($_.CommandLine -match 'injector\\.mjs') -and ($_.CommandLine -match 'codex-(skin-manager|dream-skin|cn-skin)|chatgpt-tools|ChatGPTTools') -and ($_.CommandLine -match '--watch|--port') } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }",
        ],
        { windowsHide: true }
      );
    }
  } catch {}
}

async function purgeAllSkinsFromPorts(ports) {
  const skins = listSkins();
  const uniqPorts = [...new Set(ports.filter(Boolean))];
  for (const port of uniqPorts) {
    if (!(await testDebugPort(port))) continue;
    for (const skin of skins) {
      try {
        await runInjectorOnce(skin, port, "remove");
      } catch {}
    }
    // 通用硬清理：不管当前是哪套皮肤，DOM 痕迹一并清掉
    try {
      const runtime = resolveNodeRuntime();
      // 用任意 skin-dir 跑 remove 不够时，直接 evaluate 通用清理
      await execFileAsync(
        runtime.bin,
        [
          path.join(ENGINE_DIR, "purge-all.mjs"),
          "--port",
          String(port),
          "--timeout-ms",
          "5000",
        ],
        { timeout: 12000, windowsHide: true, env: runtime.env }
      );
    } catch {}
  }
}

async function resolveCodexExecutablePath() {
  let exe = resolveCodexExe();
  if (exe && typeof exe === "object" && (exe.type === "windows-store" || exe.type === "windows-resolve")) {
    exe = await resolveWindowsCodexExe();
  }
  return typeof exe === "string" ? exe : null;
}

async function softRelaunchCodexNormal() {
  // 官方配色只在启动时读取 config.toml，所以还原主题后需要自动重开一次
  const exe = await resolveCodexExecutablePath();
  await stopCodex();
  await sleep(400);

  if (process.platform === "win32" && (!exe || isWindowsStorePath(exe))) {
    // 还原时不带调试端口，用 Store 正常激活
    const aumid = await resolveWindowsStoreAumid();
    if (aumid) {
      await runPowerShell(`Start-Process "shell:AppsFolder\\${aumid.replace(/'/g, "''")}"`);
    } else if (exe) {
      spawn(exe, [], { detached: true, stdio: "ignore", windowsHide: false }).unref();
    } else {
      return false;
    }
  } else {
    if (!exe) return false;
    const child = spawn(exe, [], {
      detached: true,
      stdio: "ignore",
      windowsHide: false,
    });
    child.unref();
  }

  // 等到进程起来，避免用户感觉“闪一下没了”
  const deadline = Date.now() + 12000;
  while (Date.now() < deadline) {
    const pids = await findCodexMainPids();
    if (pids.length) {
      await sleep(800);
      return true;
    }
    await sleep(250);
  }
  return true;
}

async function restoreSkin({ restoreTheme = true } = {}) {
  return withEngineLock(async () => {
    const state = readState();
    const port = state?.port || SHARED_PORT;
    const host = await hostProbe.probeHostLifecycle(port, { force: true });
    // Use lifecycle, not process-only — slow machines often miss process list mid-start.
    const wasRunning = host.codexRunning;
    const hostLive = Boolean(host?.rendererReady || host?.debugPortOpen);

    // 1) 先停掉所有注入守护，防止清理后又写回（身份不匹配则保留，不杀错进程）
    let injectorStop = { skipped: true };
    if (state) {
      injectorStop = stopInjector(state);
      if (injectorStop?.reason === "identity-mismatch") {
        appendDiag(
          `restoreSkin: injector identity mismatch pid=${state.injectorPid}; continuing without kill`
        );
      }
    }
    stopExternalSkinInjectors();
    setPaused(false);
    await sleep(150);

    const ports = [...new Set([SHARED_PORT, state?.port, 9335, 9336].filter(Boolean))];

    // 2) 在线清理 DOM（尽量先去掉皮肤层）
    let purgeOk = !hostLive;
    try {
      await purgeAllSkinsFromPorts(ports);
      purgeOk = true;
    } catch (e) {
      appendDiag("restore purge: " + (e.message || e));
      purgeOk = false;
    }

    // 3) 还原主题配置（删除皮肤写入的 appearance*）— never claim restored on failure
    let theme = { restored: false, reason: "skipped" };
    if (restoreTheme !== false) {
      theme = restoreDesktopTheme();
    }

    const archivedState = archiveStateFile();

    // 4) 配色必须重开应用才会生效：自动软重启（用户不用自己关开）
    let relaunched = false;
    let refreshed = false;
    if (wasRunning) {
      relaunched = await softRelaunchCodexNormal();
    } else {
      for (const p of ports) {
        if (!(await testDebugPort(p))) continue;
        try {
          const runtime = resolveNodeRuntime();
          await execFileAsync(
            runtime.bin,
            [path.join(ENGINE_DIR, "purge-all.mjs"), "--port", String(p), "--timeout-ms", "6000"],
            { timeout: 15000, windowsHide: true, env: runtime.env }
          );
          refreshed = true;
        } catch {}
      }
    }

    // Honest success: live host + purge failed → partial (session still cleared).
    const partial =
      hostLive &&
      !purgeOk &&
      injectorStop?.reason === "identity-mismatch";
    // If we could not stop a live injector we do not own, refuse full success
    // only when purge also failed — otherwise session is still cleaned.
    const ok = !(hostLive && !purgeOk && !relaunched && !refreshed);

    return {
      ok,
      partial: !ok || partial,
      theme,
      full: ok,
      refreshed,
      relaunched,
      archivedState,
      injectorStop,
      error: ok
        ? null
        : "已尝试恢复，但即时卸下皮肤可能未完成；请确认 ChatGPT 窗口是否仍显示主题",
    };
  });
}

async function detectCodex() {
  let exe = resolveCodexExe();
  if (exe && typeof exe === "object" && (exe.type === "windows-store" || exe.type === "windows-resolve")) {
    exe = await resolveWindowsCodexExe();
  }
  let aumid = null;
  if (process.platform === "win32") {
    try {
      aumid = await resolveWindowsStoreAumid();
    } catch {}
  }
  const host = await hostProbe.probeHostLifecycle(SHARED_PORT, { force: true });
  return {
    platform: process.platform,
    exe: typeof exe === "string" ? exe : null,
    aumid,
    configuredAppPath: getConfiguredAppPath(),
    configExists: fs.existsSync(CONFIG_PATH),
    configPath: CONFIG_PATH,
    engineDir: ENGINE_DIR,
    debugPort: SHARED_PORT,
    debugPortOpen: host.debugPortOpen,
    rendererReady: host.rendererReady,
    processRunning: host.processRunning,
    lifecycle: host.lifecycle,
    lifecycleRaw: host.lifecycleRaw,
    confidence: host.confidence,
    canHotApply: host.canHotApply,
    needsRestartForInject: host.needsRestartForInject,
    codexRunning: host.codexRunning,
    found: Boolean((typeof exe === "string" && exe) || aumid),
  };
}

/** Lightweight host lifecycle for GUI polling (Node fallback). */
async function getHostStatus({ force = false } = {}) {
  const state = readState();
  const port = state?.port || SHARED_PORT;
  return hostProbe.getHostStatus(port, { force });
}

module.exports = {
  listSkins,
  getSkin,
  getStatus,
  getHostStatus,
  applySkin,
  restoreSkin,
  pauseSkin,
  resumeSkin,
  hardVerifySkin,
  checkSkinPayload,
  detectCodex,
  resolveSkinAsset,
  exportSkin,
  importSkin,
  inspectSkinPackage,
  createWallpaperSkin,
  deleteUserSkin,
  setConfiguredAppPath,
  getConfiguredAppPath,
  setPaused,
  isPaused,
  USER_SKINS_DIR,
  BUNDLED_SKINS_DIR,
  STATE_ROOT,
  ROOT,
  PAUSE_PATH,
  ENGINE_PROTOCOL,
  ENGINE_VERSION,
  ENGINE_NAME,
  probeHostLifecycle: (port) => hostProbe.probeHostLifecycle(port || SHARED_PORT),
};
