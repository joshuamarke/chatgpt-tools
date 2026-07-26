/**
 * Lighter settings probe: current page, white surfaces, card classes, elevation tokens.
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "probe-settings-v2-out.json.txt");

const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find((p) => p.type === "page") || pages[0];
const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
const send = (method, params = {}) =>
  new Promise((res, rej) => {
    const i = ++id;
    pending.set(i, { res, rej });
    ws.send(JSON.stringify({ id: i, method, params }));
    setTimeout(() => rej(new Error("timeout")), 20000);
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
  const qa = (s) => [...document.querySelectorAll(s)];
  const fullCls = (el) => String(el.className || "");
  const textOf = (el, n = 60) => (el?.innerText || "").replace(/\\s+/g, " ").trim().slice(0, n);

  const root = document.documentElement;
  const rootCls = [...root.classList];
  const cs = getComputedStyle(root);

  // All solid non-transparent large surfaces
  const surfaces = qa("div, section, main, aside, form, ul, li").map((el) => {
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    if (r.width < 120 || r.height < 24) return null;
    if (r.bottom < 0 || r.top > innerHeight + 200) return null;
    const bg = s.backgroundColor;
    if (!bg || bg === "rgba(0, 0, 0, 0)" || bg === "transparent") return null;
    return {
      bg,
      y: Math.round(r.top),
      x: Math.round(r.left),
      w: Math.round(r.width),
      h: Math.round(r.height),
      cls: fullCls(el).slice(0, 350),
      text: textOf(el, 70),
      radius: s.borderRadius,
      shadow: (s.boxShadow || "").slice(0, 100),
    };
  }).filter(Boolean);

  // Group by bg
  const byBg = {};
  for (const s of surfaces) {
    byBg[s.bg] = (byBg[s.bg] || 0) + 1;
  }

  // Settings-looking text
  const hasSettings = /常规|权限|外观|设置|General|Settings|默认权限/.test(document.body.innerText || "");

  // Find elements whose class contains "after:bg-token-border" (from first probe)
  const afterBorder = qa("*").filter((el) => fullCls(el).includes("after:bg-token-border")).slice(0, 10).map((el) => {
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      bg: s.backgroundColor,
      bgImg: (s.backgroundImage || "").slice(0, 80),
      y: Math.round(r.top),
      w: Math.round(r.width),
      h: Math.round(r.height),
      cls: fullCls(el).slice(0, 400),
      text: textOf(el, 50),
    };
  });

  // Elevation / card related vars
  const elev = {};
  for (const k of [...cs]) {
    if (/elevation|card|surface|bg-primary|bg-secondary|bg-tertiary|bg-elevated|token-bg|settings-row|settings-header|panel-background|editor-background/i.test(k)) {
      elev[k] = cs.getPropertyValue(k).trim().slice(0, 100);
    }
  }

  // Resolve common bg utilities via temp element
  const probeClasses = [
    "bg-token-bg-primary",
    "bg-token-bg-secondary",
    "bg-token-bg-tertiary",
    "bg-token-main-surface-primary",
    "bg-token-main-surface-secondary",
    "bg-token-side-bar-background",
    "bg-white",
    "bg-token-dropdown-background",
    "bg-token-editor-widget-background",
    "bg-token-surface-elevated",
    "bg-token-bg-elevated",
  ];
  const resolved = {};
  const tmp = document.createElement("div");
  document.body.appendChild(tmp);
  for (const c of probeClasses) {
    tmp.className = c;
    resolved[c] = getComputedStyle(tmp).backgroundColor;
  }
  // also try data attributes / style vars
  tmp.className = "";
  tmp.style.backgroundColor = "var(--color-token-bg-primary)";
  resolved["var(--color-token-bg-primary)"] = getComputedStyle(tmp).backgroundColor;
  tmp.style.backgroundColor = "var(--color-token-bg-secondary)";
  resolved["var(--color-token-bg-secondary)"] = getComputedStyle(tmp).backgroundColor;
  tmp.style.backgroundColor = "var(--color-token-bg-tertiary)";
  resolved["var(--color-token-bg-tertiary)"] = getComputedStyle(tmp).backgroundColor;
  tmp.style.backgroundColor = "var(--vscode-editor-background)";
  resolved["var(--vscode-editor-background)"] = getComputedStyle(tmp).backgroundColor;
  tmp.style.backgroundColor = "var(--vscode-settings-rowHoverBackground)";
  resolved["var(--vscode-settings-rowHoverBackground)"] = getComputedStyle(tmp).backgroundColor;
  tmp.style.backgroundColor = "var(--vscode-panel-background)";
  resolved["var(--vscode-panel-background)"] = getComputedStyle(tmp).backgroundColor;
  tmp.remove();

  // CSS rules that set white background with short selectors
  const whiteRules = [];
  for (const sheet of document.styleSheets) {
    let rules;
    try { rules = sheet.cssRules; } catch { continue; }
    if (!rules) continue;
    for (const rule of rules) {
      if (!rule.style) continue;
      const bg = rule.style.getPropertyValue("background-color") || rule.style.getPropertyValue("background") || "";
      if (!bg) continue;
      if (/#fff|#ffffff|white|255,\\s*255,\\s*255|rgb\\(255/i.test(bg) || bg === "#fff" || bg === "white") {
        const sel = rule.selectorText || "";
        if (sel.length < 300) {
          whiteRules.push({ sel: sel.slice(0, 220), bg: bg.slice(0, 80), text: (rule.cssText || "").slice(0, 220) });
          if (whiteRules.length >= 25) break;
        }
      }
    }
    if (whiteRules.length >= 25) break;
  }

  // sidebar "配置" nav - click target for settings
  const configBtn = qa("button").find((b) => (b.getAttribute("aria-label") || textOf(b, 20)) === "配置"
    || textOf(b, 10) === "配置");
  const appearanceBtn = qa("button").find((b) => (b.getAttribute("aria-label") || "") === "外观");

  // main surface children summary
  const main = document.querySelector("main.main-surface") || document.querySelector("main");
  let mainKids = [];
  if (main) {
    mainKids = [...main.querySelectorAll(":scope div")].slice(0, 0); // skip heavy
    // walk first few levels
    const walk = (el, depth, acc) => {
      if (depth > 4 || acc.length > 40) return;
      for (const c of el.children) {
        const s = getComputedStyle(c);
        const r = c.getBoundingClientRect();
        if (r.width < 50 || r.height < 20) continue;
        acc.push({
          depth,
          tag: c.tagName,
          bg: s.backgroundColor,
          cls: fullCls(c).slice(0, 200),
          text: textOf(c, 40),
          y: Math.round(r.top),
          h: Math.round(r.height),
        });
        walk(c, depth + 1, acc);
      }
    };
    walk(main, 0, mainKids);
  }

  // Bright surfaces (white / near white / light gray that break dark theme)
  const bright = surfaces.filter((s) => {
    const m = s.bg.match(/rgba?\\((\\d+),\\s*(\\d+),\\s*(\\d+)/);
    if (!m) return false;
    const [_, r, g, b] = m.map(Number);
    return r > 200 && g > 200 && b > 200;
  });

  return {
    rootCls,
    hasSettings,
    configPresent: !!configBtn,
    appearancePresent: !!appearanceBtn,
    byBg,
    bright,
    afterBorder,
    elevKeys: Object.keys(elev).length,
    elev,
    resolved,
    whiteRules,
    mainKids: mainKids.slice(0, 50),
    surfacesSample: surfaces.slice(0, 30),
  };
})()`;

const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
const val = r.result?.result?.value ?? r.result;
writeFileSync(OUT, JSON.stringify(val, null, 2), "utf8");

if (val?.subtype === "error" || val?.description) {
  console.log("ERR", JSON.stringify(val).slice(0, 500));
  process.exit(1);
}

console.log("rootCls", val.rootCls);
console.log("hasSettings", val.hasSettings, "config", val.configPresent, "appearance", val.appearancePresent);
console.log("byBg", val.byBg);
console.log("bright count", val.bright?.length);
(val.bright || []).forEach((b) => console.log(" BRIGHT", b.bg, b.y, b.w + "x" + b.h, b.cls?.slice(0, 120), "|", b.text));
console.log("\nafterBorder", val.afterBorder?.length);
(val.afterBorder || []).forEach((b) => console.log(" ", b.bg, b.cls?.slice(0, 150), b.text));
console.log("\nresolved utilities:");
Object.entries(val.resolved || {}).forEach(([k, v]) => console.log(" ", k, "=>", v));
console.log("\nwhiteRules:");
(val.whiteRules || []).slice(0, 15).forEach((w) => console.log(" ", w.sel, "=>", w.bg));
console.log("\nelev (selected):");
Object.entries(val.elev || {}).filter(([k]) => /settings|elevation|card|bg-primary|bg-secondary|panel|editor-background|surface/i.test(k)).slice(0, 40).forEach(([k, v]) => console.log(" ", k, "=", v));
console.log("\nmainKids with solid bg:");
(val.mainKids || []).filter((k) => k.bg && k.bg !== "rgba(0, 0, 0, 0)").slice(0, 25).forEach((k) => console.log(" d"+k.depth, k.bg, k.cls?.slice(0, 100), k.text));
console.log("wrote", OUT);
ws.close();
