/**
 * Hard-remove any known Codex / ChatGPT Tools skin markers, then optionally reload.
 * Uses a registry-aware cleanup so new skins do not require editing this file.
 */
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";

const require = createRequire(import.meta.url);
const { WebSocket } = require("./ws-polyfill.cjs");

function parseArgs(argv) {
  const options = { port: 9335, timeoutMs: 8000, reload: true };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--port") options.port = Number(argv[++i]);
    else if (arg === "--timeout-ms") options.timeoutMs = Number(argv[++i]);
    else if (arg === "--no-reload") options.reload = false;
  }
  return options;
}

class CdpSession {
  constructor(target) {
    this.ws = new WebSocket(target.webSocketDebuggerUrl);
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Map();
    this.closed = false;
  }

  async open() {
    await new Promise((resolve, reject) => {
      this.ws.addEventListener("open", resolve, { once: true });
      this.ws.addEventListener("error", reject, { once: true });
    });
    this.ws.addEventListener("message", (event) => {
      let message;
      try {
        message = JSON.parse(String(event.data));
      } catch {
        return;
      }
      if (!message || typeof message !== "object") return;
      if (message.id) {
        const waiter = this.pending.get(message.id);
        if (!waiter) return;
        this.pending.delete(message.id);
        if (message.error) waiter.reject(new Error(message.error.message));
        else waiter.resolve(message.result);
        return;
      }
      for (const listener of this.listeners.get(message.method) ?? []) {
        listener(message.params ?? {});
      }
    });
    this.ws.addEventListener("close", () => {
      this.closed = true;
      for (const waiter of this.pending.values()) waiter.reject(new Error("closed"));
      this.pending.clear();
    });
    await this.send("Runtime.enable");
    await this.send("Page.enable");
    return this;
  }

  on(method, listener) {
    const list = this.listeners.get(method) ?? [];
    list.push(listener);
    this.listeners.set(method, list);
  }

  send(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = this.nextId++;
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  async evaluate(expression) {
    const result = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.text || "evaluate failed");
    }
    return result.result?.value;
  }

  close() {
    if (!this.closed) this.ws.close();
    this.closed = true;
  }
}

async function waitTargets(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/list`, { redirect: "error" });
      if (res.ok) {
        const list = await res.json();
        const pages = list.filter(
          (t) => t.type === "page" && String(t.url || "").startsWith("app://")
        );
        if (pages.length) return pages;
      }
    } catch {
      /* retry */
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error("no targets");
}

/**
 * Generic purge: registry from shared core + legacy hardcoded markers.
 * New skins that use renderer-core register under __CHATGPT_TOOLS_SKIN_REGISTRY__.
 */
const PURGE = `(() => {
  const cleaned = [];
  const registry = window.__CHATGPT_TOOLS_SKIN_REGISTRY__;
  if (registry && typeof registry === 'object') {
    for (const [key, entry] of Object.entries(registry)) {
      try {
        if (entry?.disabledKey) window[entry.disabledKey] = true;
        if (typeof entry?.cleanup === 'function') entry.cleanup();
        cleaned.push(key);
      } catch {}
      try { delete registry[key]; } catch {}
    }
  }

  // Legacy / one-off markers (pre-shared-runtime skins and external tools)
  const legacy = [
    { disabled: '__CODEX_DREAM_SKIN_DISABLED__', state: '__CODEX_DREAM_SKIN_STATE__', root: 'codex-dream-skin', art: '--dream-art', style: 'codex-dream-skin-style', chrome: 'codex-dream-skin-chrome', homes: ['dream-home','dream-home-shell'] },
    { disabled: '__CODEX_CN_SKIN_DISABLED__', state: '__CODEX_CN_SKIN_STATE__', root: 'codex-cn-skin', art: '--cn-art', style: 'codex-cn-skin-style', chrome: 'codex-cn-skin-chrome', homes: ['cn-home','cn-home-shell'] },
    { disabled: '__CODEX_LINGLONG_SKIN_DISABLED__', state: '__CODEX_LINGLONG_SKIN_STATE__', root: 'codex-linglong-skin', art: '--linglong-art', style: 'codex-linglong-skin-style', chrome: 'codex-linglong-skin-chrome', homes: ['linglong-home','linglong-home-shell'] },
    { disabled: '__CODEX_MORTAL_SKIN_DISABLED__', state: '__CODEX_MORTAL_SKIN_STATE__', root: 'codex-mortal-skin', art: '--mortal-art', style: 'codex-mortal-skin-style', chrome: 'codex-mortal-skin-chrome', homes: ['mortal-home','mortal-home-shell'] },
    { disabled: '__CODEX_CYBERPUNK_SKIN_DISABLED__', state: '__CODEX_CYBERPUNK_SKIN_STATE__', root: 'codex-cyberpunk-skin', art: '--cyberpunk-art', style: 'codex-cyberpunk-skin-style', chrome: 'codex-cyberpunk-skin-chrome', homes: ['cyberpunk-home','cyberpunk-home-shell'] },
    { disabled: '__CODEX_EVA_SKIN_DISABLED__', state: '__CODEX_EVA_SKIN_STATE__', root: 'codex-eva-skin', art: '--eva-art', style: 'codex-eva-skin-style', chrome: 'codex-eva-skin-chrome', homes: ['eva-home','eva-home-shell'] },
    { disabled: '__CODEX_MIKU_SKIN_DISABLED__', state: '__CODEX_MIKU_SKIN_STATE__', root: 'codex-miku-skin', art: '--miku-art', style: 'codex-miku-skin-style', chrome: 'codex-miku-skin-chrome', homes: ['miku-home','miku-home-shell'] },
    { disabled: '__CODEX_JIUYI_SKIN_DISABLED__', state: '__CODEX_JIUYI_SKIN_STATE__', root: 'codex-jiuyi-skin', art: '--jiuyi-art', style: 'codex-jiuyi-skin-style', chrome: 'codex-jiuyi-skin-chrome', homes: ['jiuyi-home','jiuyi-home-shell'] },
  ];

  for (const item of legacy) {
    try { window[item.disabled] = true; } catch {}
    try {
      const state = window[item.state];
      if (state?.cleanup) state.cleanup();
      else {
        if (state?.observer) state.observer.disconnect();
        if (state?.rootObserver) state.rootObserver.disconnect();
        if (state?.resizeObserver) state.resizeObserver.disconnect();
        if (state?.timer) clearInterval(state.timer);
        if (state?.scheduler?.timeout) clearTimeout(state.scheduler.timeout);
        if (state?.scheduler?.frame != null && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(state.scheduler.frame);
        if (state?.artUrl) URL.revokeObjectURL(state.artUrl);
      }
    } catch {}
    try { delete window[item.state]; } catch {}
    document.documentElement?.classList.remove(item.root);
    document.documentElement?.style.removeProperty(item.art);
    document.getElementById(item.style)?.remove();
    document.getElementById(item.chrome)?.remove();
    for (const cls of item.homes) {
      document.querySelectorAll('.' + cls).forEach((n) => n.classList.remove(cls));
    }
  }

  // Adaptive theme classes from shared core
  document.documentElement?.classList.remove(
    'dream-theme-light','dream-theme-dark','dream-art-wide','dream-art-standard',
    'dream-focus-left','dream-focus-center','dream-focus-right',
    'dream-safe-left','dream-safe-center','dream-safe-right','dream-safe-none',
    'dream-task-ambient','dream-task-banner','dream-task-off'
  );
  for (const prop of ['--dream-art','--dream-art-position','--dream-focus-x','--dream-focus-y','--dream-accent','--dream-accent-ink','--dream-image-luma']) {
    document.documentElement?.style.removeProperty(prop);
  }
  document.documentElement?.removeAttribute('data-chatgpt-tools-skin');
  document.documentElement?.removeAttribute('data-dream-shell');

  // Any style / chrome nodes tagged by shared runtime
  document.querySelectorAll('style[data-skin-revision], style[id*="-skin-style"]').forEach((n) => n.remove());
  document.querySelectorAll('[id*="-skin-chrome"]').forEach((n) => n.remove());

  return {
    registryCleaned: cleaned,
    styleGone: !document.querySelector('style[data-skin-revision]'),
  };
})()`;

async function purgeSession(session) {
  return session.evaluate(PURGE);
}

const options = parseArgs(process.argv.slice(2));
const targets = await waitTargets(options.port, options.timeoutMs);
const results = [];
for (const target of targets) {
  const session = await new CdpSession(target).open();
  try {
    const result = await purgeSession(session);
    if (options.reload) {
      try {
        await session.send("Page.reload", { ignoreCache: false });
      } catch {
        /* ignore */
      }
    }
    results.push({ targetId: target.id, result });
  } finally {
    session.close();
  }
}
console.log(JSON.stringify({ ok: true, port: options.port, targets: results }, null, 2));
