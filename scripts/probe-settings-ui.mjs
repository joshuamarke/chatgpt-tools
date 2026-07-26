/**
 * Probe host Settings UI: open path, DOM tree, bg styles, token classes.
 * Usage: node scripts/probe-settings-ui.mjs
 * Requires host CDP on 127.0.0.1:9335
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "probe-settings-ui-out.json.txt");

const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find((p) => p.type === "page") || pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.log("no cdp page");
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
const send = (method, params = {}) =>
  new Promise((res, rej) => {
    const i = ++id;
    pending.set(i, { res, rej });
    ws.send(JSON.stringify({ id: i, method, params }));
    setTimeout(() => rej(new Error(`timeout ${method}`)), 25000);
  });
ws.on("message", (d) => {
  const m = JSON.parse(d);
  if (m.id && pending.has(m.id)) {
    pending.get(m.id).res(m);
    pending.delete(m.id);
  }
});
await new Promise((r) => ws.once("open", r));
await send("Runtime.enable");

const expr = `(() => {
  const q = (s) => document.querySelector(s);
  const qa = (s) => [...document.querySelectorAll(s)];
  const cls = (el) => (el ? String(el.className || "").slice(0, 280) : null);
  const textOf = (el, n = 80) => (el?.innerText || "").replace(/\\s+/g, " ").trim().slice(0, n);
  const pick = (el) => {
    if (!el) return null;
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      tag: el.tagName,
      cls: cls(el),
      role: el.getAttribute("role"),
      testid: el.getAttribute("data-testid"),
      aria: el.getAttribute("aria-label"),
      bg: cs.backgroundColor,
      bgImg: (cs.backgroundImage || "").slice(0, 120),
      color: cs.color,
      border: cs.borderColor + " / " + cs.borderWidth,
      y: Math.round(r.top),
      x: Math.round(r.left),
      w: Math.round(r.width),
      h: Math.round(r.height),
      text: textOf(el, 100),
    };
  };
  const ancestors = (el, depth = 8) => {
    const a = [];
    let n = el;
    for (let i = 0; i < depth && n; i++) {
      const cs = getComputedStyle(n);
      a.push({
        tag: n.tagName,
        cls: cls(n),
        bg: cs.backgroundColor,
        bgImg: (cs.backgroundImage || "").slice(0, 60),
        role: n.getAttribute("role"),
        testid: n.getAttribute("data-testid"),
      });
      n = n.parentElement;
    }
    return a;
  };

  const root = document.documentElement;
  const skin = {
    jiuyi: root.classList.contains("codex-jiuyi-skin"),
    dreamDark: root.classList.contains("dream-theme-dark"),
    classes: [...root.classList].filter((c) => /skin|theme|dark|light|jiuyi|dream/i.test(c)),
  };

  // Find settings entry points in UI
  const settingsTriggers = qa("button, a, [role='button'], [role='menuitem']")
    .filter((el) => {
      const t = textOf(el, 40);
      const al = el.getAttribute("aria-label") || "";
      return /设置|Settings|偏好|Preferences|选项|Options|配置/i.test(t + " " + al);
    })
    .slice(0, 15)
    .map((el) => ({
      ...pick(el),
      path: ancestors(el, 5).map((a) => a.tag + (a.cls ? "." + a.cls.split(" ").slice(0, 3).join(".") : "")),
    }));

  // Open settings-like surfaces currently visible
  const dialogs = qa('[role="dialog"], [aria-modal="true"], dialog, [data-state="open"]')
    .map(pick)
    .filter(Boolean);

  // Heuristic: settings panel / page by text
  const settingsNodes = qa("div, section, main, aside, nav, h1, h2, h3, span, button")
    .filter((el) => {
      const t = textOf(el, 30);
      return /^(设置|Settings|通用|General|外观|Appearance|账户|Account|模型|Model|关于|About|高级|Advanced|隐私|Privacy)$/i.test(t)
        || (t.includes("设置") && t.length < 12);
    })
    .slice(0, 40)
    .map((el) => ({
      ...pick(el),
      ancestors: ancestors(el, 6),
    }));

  // All elements with bg-token / surface-ish utility classes in main area
  const tokenBgEls = qa("[class*='bg-token'], [class*='bg-main'], [class*='surface'], [class*='panel']")
    .filter((el) => {
      const r = el.getBoundingClientRect();
      return r.width > 40 && r.height > 20 && r.top > 0;
    })
    .slice(0, 80)
    .map((el) => {
      const p = pick(el);
      const c = String(el.className || "");
      const tokenClasses = c.split(/\\s+/).filter((x) =>
        /bg-token|bg-main|surface|panel|card|modal|dialog|sheet|drawer|popover|menu|dropdown|list|input|border-token/i.test(x)
      );
      return { ...p, tokenClasses };
    });

  // Group opaque / non-transparent backgrounds that look "untokened" (default gray)
  const grayish = tokenBgEls.filter((e) => {
    const bg = e.bg || "";
    // rgb(24,24,24) rgb(45,45,45) white-ish defaults
    return /rgb\\(\\s*(24|32|37|45|48|64)\\s*,\\s*\\1\\s*,\\s*\\1\\s*\\)/.test(bg)
      || bg === "rgb(255, 255, 255)"
      || bg === "rgb(250, 250, 250)"
      || bg === "rgb(18, 18, 18)";
  });

  // Sidebar nav labels for mode
  const modeBtn = qa("button").find((b) => /切换模式|当前模式/.test(textOf(b, 40) + (b.getAttribute("aria-label") || "")));
  const modeInfo = modeBtn
    ? { text: textOf(modeBtn, 60), aria: modeBtn.getAttribute("aria-label") }
    : null;

  // Scan style rules related to settings
  const rules = [];
  for (const sheet of document.styleSheets) {
    let rs;
    try {
      rs = sheet.cssRules;
    } catch {
      continue;
    }
    if (!rs) continue;
    for (const r of rs) {
      const t = r.cssText || "";
      const sel = r.selectorText || "";
      if (/settings|Settings|preference|modal-content|dialog-content|bg-token-bg|token-settings|settings-row|setting-item/i.test(sel + t) && t.length < 600) {
        rules.push(t.slice(0, 400));
        if (rules.length >= 40) break;
      }
    }
    if (rules.length >= 40) break;
  }

  // Root CSS vars that settings may consume
  const cs = getComputedStyle(root);
  const varNames = [
    "--color-token-bg-primary",
    "--color-token-bg-secondary",
    "--color-token-bg-tertiary",
    "--color-token-main-surface-primary",
    "--color-token-dropdown-background",
    "--color-token-menu-background",
    "--color-token-input-background",
    "--color-token-side-bar-background",
    "--color-token-list-hover-background",
    "--vscode-settings-dropdownBackground",
    "--vscode-editor-background",
    "--vscode-sideBar-background",
    "--vscode-panel-background",
    "--main-surface-primary",
    "--surface-primary",
    "--codex-base-surface",
  ];
  const tokens = {};
  for (const k of varNames) tokens[k] = cs.getPropertyValue(k).trim().slice(0, 80);

  // All CSS custom props matching settings/panel/surface on :root
  const allVars = [...cs].filter((k) =>
    /settings|panel|surface|bg-primary|bg-secondary|bg-tertiary|card|modal|dialog|widget|input|list|menu|dropdown|sidebar|side-bar/i.test(k)
  );
  const extraTokens = {};
  for (const k of allVars.slice(0, 80)) {
    extraTokens[k] = cs.getPropertyValue(k).trim().slice(0, 60);
  }

  // Visible large panels with solid bg (likely settings content areas)
  const largePanels = qa("div, section, main, aside")
    .filter((el) => {
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      if (r.width < 200 || r.height < 100) return false;
      if (cs.backgroundColor === "rgba(0, 0, 0, 0)" || cs.backgroundColor === "transparent") return false;
      return true;
    })
    .slice(0, 40)
    .map(pick);

  return {
    skin,
    modeInfo,
    url: location.href,
    title: document.title,
    settingsTriggers,
    dialogs,
    settingsNodes: settingsNodes.slice(0, 25),
    tokenBgSample: tokenBgEls.slice(0, 40),
    grayish,
    largePanels,
    rules,
    tokens,
    extraTokensCount: allVars.length,
    extraTokens,
  };
})()`;

const r = await send("Runtime.evaluate", {
  expression: expr,
  returnByValue: true,
});
const val = r.result?.result?.value ?? r.result;
const text = JSON.stringify(val, null, 2);
writeFileSync(OUT, text, "utf8");
console.log(text.slice(0, 12000));
console.log("\n... wrote", OUT, "len", text.length);
ws.close();
