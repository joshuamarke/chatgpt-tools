/**
 * Electron ELECTRON_RUN_AS_NODE 使用 Node 20，没有全局 WebSocket。
 * 安装版注入脚本必须能在无系统 Node 时工作。
 * 同时把 `ws` 包适配成浏览器风格 API（addEventListener / event.data）。
 */
const path = require("path");
const fs = require("fs");
const Module = require("module");

function tryRequire(id) {
  try {
    return require(id);
  } catch {
    return null;
  }
}

function loadWsConstructor() {
  if (typeof globalThis.WebSocket === "function") {
    return globalThis.WebSocket;
  }

  const candidates = [];
  candidates.push("ws");
  candidates.push(path.join(__dirname, "node_modules", "ws"));
  candidates.push(path.join(__dirname, "..", "node_modules", "ws"));
  if (process.resourcesPath) {
    candidates.push(path.join(process.resourcesPath, "app.asar.unpacked", "node_modules", "ws"));
    candidates.push(path.join(process.resourcesPath, "app.asar.unpacked", "engine", "node_modules", "ws"));
    candidates.push(path.join(process.resourcesPath, "app.asar", "node_modules", "ws"));
  }
  if (process.execPath) {
    const base = path.dirname(process.execPath);
    candidates.push(path.join(base, "resources", "app.asar.unpacked", "node_modules", "ws"));
  }

  for (const c of candidates) {
    const mod = tryRequire(c);
    if (mod) return mod.WebSocket || mod.default || mod;
  }

  for (const c of candidates) {
    try {
      if (c === "ws") continue;
      const pkg = path.join(c, "package.json");
      if (!fs.existsSync(pkg)) continue;
      const req = Module.createRequire(pkg);
      const mod = req(".");
      if (mod) return mod.WebSocket || mod.default || mod;
    } catch {}
  }

  throw new Error(
    "WebSocket 不可用：当前是 Electron-as-Node(Node20)。请确保已打包 ws 模块。"
  );
}

const RawWebSocket = loadWsConstructor();

/** 包装成浏览器 WebSocket 风格，兼容 injector 里的 addEventListener */
function WebSocket(url, protocols) {
  // 全局 WebSocket 已是浏览器风格
  if (typeof globalThis.WebSocket === "function" && RawWebSocket === globalThis.WebSocket) {
    return new RawWebSocket(url, protocols);
  }

  const ws = protocols !== undefined ? new RawWebSocket(url, protocols) : new RawWebSocket(url);

  // 已有 addEventListener 则直接用
  if (typeof ws.addEventListener === "function") {
    return ws;
  }

  const listeners = {
    open: new Set(),
    message: new Set(),
    error: new Set(),
    close: new Set(),
  };

  ws.addEventListener = function addEventListener(type, handler) {
    if (!listeners[type] || typeof handler !== "function") return;
    listeners[type].add(handler);
  };
  ws.removeEventListener = function removeEventListener(type, handler) {
    if (!listeners[type]) return;
    listeners[type].delete(handler);
  };

  ws.on("open", () => {
    for (const h of listeners.open) {
      try {
        h();
      } catch {}
    }
  });
  ws.on("message", (data, isBinary) => {
    const payload = isBinary ? data : typeof data === "string" ? data : String(data);
    const event = { data: payload };
    for (const h of listeners.message) {
      try {
        h(event);
      } catch {}
    }
  });
  ws.on("error", (err) => {
    for (const h of listeners.error) {
      try {
        h(err);
      } catch {}
    }
  });
  ws.on("close", () => {
    for (const h of listeners.close) {
      try {
        h();
      } catch {}
    }
  });

  return ws;
}

module.exports = { WebSocket, loadWsConstructor };
