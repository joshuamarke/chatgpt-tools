/**
 * Generate toolbox/resources/plugin_unlock.js from CodexPlusPlus renderer-inject.js.
 * Run: node scripts/gen-plugin-unlock.mjs
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const srcPath = path.resolve(
  root,
  "..",
  "CodexPlusPlus",
  "assets",
  "inject",
  "renderer-inject.js"
);
const outPath = path.join(
  root,
  "src-tauri",
  "src",
  "toolbox",
  "resources",
  "plugin_unlock.js"
);

if (!fs.existsSync(srcPath)) {
  console.error("CodexPlusPlus source not found:", srcPath);
  process.exit(1);
}

const lines = fs.readFileSync(srcPath, "utf8").split(/\n/);
// 1-based inclusive ranges from earlier research
const ranges = [
  [3993, 4439],
  [4443, 4780],
];
let body = "";
for (const [a, b] of ranges) {
  body += lines.slice(a - 1, b).join("\n") + "\n";
}

body = body
  .replace(
    /window\.__CODEX_PLUS_PLUGIN_MARKETPLACES__/g,
    "(window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACES__ || window.__CODEX_PLUS_PLUGIN_MARKETPLACES__)"
  )
  .replace(/OpenAI插件(\d)\(Codex\+\+\)/g, "OpenAI插件$1(ChatGPT Tools)");

const preamble = `/**
 * ChatGPT Tools — plugin marketplace unlock (third-party API).
 * Generated from CodexPlusPlus renderer-inject marketplace block.
 * Config: window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACE_UNLOCK__ = { enabled, autoExpand }
 * Local catalogs: window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACES__
 */
(() => {
  const SCRIPT_VERSION = "cgt-plugin-unlock-1";
  const codexPluginMarketplaceUnlockVersion = "14";
  const codexPluginAutoExpandVersion = "1";
  const codexPluginAutoExpandMaxClicks = 24;
  const codexPluginAutoExpandClickDelayMs = 220;
  const moreMenuClass = "chatgpt-tools-plugin-more-menu";
  const codexPlusMenuId = "chatgpt-tools-plus-menu";

  function sendCodexPlusDiagnostic(event, payload) {
    try {
      if (!window.__CGT_PLUGIN_DIAG__) window.__CGT_PLUGIN_DIAG__ = [];
      window.__CGT_PLUGIN_DIAG__.push({ t: Date.now(), event, payload });
    } catch (_) {}
  }

  function codexPlusSettings() {
    const cfg = window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACE_UNLOCK__ || {};
    const on = cfg.enabled === true;
    return {
      pluginMarketplaceUnlock: on,
      pluginAutoExpand: on && cfg.autoExpand !== false,
    };
  }

  function pluginPatchDisabledInRelayMode() {
    return false;
  }

  function appServerModelRequestMethod(method, params) {
    if (method === "send-cli-request-for-host" && params && params.method) {
      return String(params.method);
    }
    if (method === "vscode://codex/list-plugins") return "list-plugins";
    if (method === "vscode://codex/plugin/install") return "install-plugin";
    if (method === "vscode://codex/plugin/uninstall") return "uninstall-plugin";
    if (method === "plugin/list") return "list-plugins";
    if (method === "plugin/install") return "install-plugin";
    if (method === "plugin/uninstall") return "uninstall-plugin";
    return String(method || "");
  }

  if (
    !window.__CODEX_PLUS_PLUGIN_MARKETPLACES__ &&
    Array.isArray(window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACES__)
  ) {
    window.__CODEX_PLUS_PLUGIN_MARKETPLACES__ =
      window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACES__;
  }

  async function loadAppServerRequestCandidates() {
    const candidates = [];
    const seen = new Set();
    const push = (c) => {
      if (!c || typeof c.sendRequest !== "function" || seen.has(c)) return;
      seen.add(c);
      candidates.push(c);
    };
    try {
      const roots = [
        window.__CODEX_APP_SERVER__,
        window.__appServer__,
        window.__CODEX_HOST__,
        window.electronBridge,
      ];
      for (const root of roots) {
        if (!root || typeof root !== "object") continue;
        push(root);
        try {
          for (const v of Object.values(root)) push(v);
        } catch (_) {}
      }
    } catch (_) {}
    return {
      modules: [],
      candidates,
      sources: ["global-walk"],
      discovery: "fallback",
    };
  }

`;

const outro = `
  function installAll() {
    const cfg = window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACE_UNLOCK__ || {};
    if (cfg.enabled !== true) {
      return { ok: true, enabled: false, skipped: true, version: SCRIPT_VERSION };
    }
    const key = SCRIPT_VERSION + ":on";
    if (window.__chatgptToolsPluginUnlockInstalled === key) {
      return { ok: true, enabled: true, skipped: true, version: SCRIPT_VERSION };
    }
    window.__chatgptToolsPluginUnlockInstalled = key;
    try { installPluginBuildFlavorFilterPatch(); } catch (e) {
      sendCodexPlusDiagnostic("filter_patch_failed", { error: String(e && e.message || e) });
    }
    try { installPluginMarketplaceWindowEventPatchOnly(); } catch (e) {
      sendCodexPlusDiagnostic("window_patch_failed", { error: String(e && e.message || e) });
    }
    try { installPluginMarketplaceBridgePatch(); } catch (e) {
      sendCodexPlusDiagnostic("bridge_patch_failed", { error: String(e && e.message || e) });
    }
    try { installPluginMarketplaceRequestPatch(); } catch (e) {
      sendCodexPlusDiagnostic("request_patch_failed", { error: String(e && e.message || e) });
    }
    try { schedulePluginAutoExpand(true); } catch (e) {
      sendCodexPlusDiagnostic("auto_expand_failed", { error: String(e && e.message || e) });
    }
    let n = 0;
    const timer = setInterval(() => {
      n += 1;
      try { installPluginMarketplaceBridgePatch(); } catch (_) {}
      try { installPluginMarketplaceRequestPatch(); } catch (_) {}
      if (n >= 40) clearInterval(timer);
    }, 250);
    return { ok: true, enabled: true, version: SCRIPT_VERSION };
  }

  return installAll();
})()
`;

const out = preamble + body + outro;
fs.writeFileSync(outPath, out, "utf8");
console.log("wrote", outPath);
console.log("chars", out.length, "lines", out.split(/\n/).length);
