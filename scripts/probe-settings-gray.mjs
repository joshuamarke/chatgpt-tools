/**
 * Find what paints settings group cards rgb(35,35,35) and related controls.
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "probe-settings-gray-out.json.txt");
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

  // Settings group cards: class has after:bg-token-border
  const cards = qa("*").filter((el) => fullCls(el).includes("after:bg-token-border"));
  const cardInfo = cards.map((el) => {
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    // Get ALL class tokens
    const classes = fullCls(el).split(/\\s+/).filter(Boolean);
    // Find which single class contributes background by toggling
    const bgContrib = [];
    const original = el.className;
    for (const c of classes) {
      if (!/bg-|elevation|shadow|surface|card|token|rounded|border|ring/.test(c) && !c.includes("bg")) continue;
      // temporarily remove
      el.classList.remove(c);
      const after = getComputedStyle(el).backgroundColor;
      el.classList.add(c);
      if (after !== s.backgroundColor) {
        bgContrib.push({ cls: c, without: after, with: s.backgroundColor });
      }
    }
    el.className = original;

    // Also check if background comes from style attribute or inherited pseudo
    // matched rules with background (limit)
    const matched = [];
    for (const sheet of document.styleSheets) {
      let rules;
      try { rules = sheet.cssRules; } catch { continue; }
      if (!rules) continue;
      for (const rule of rules) {
        if (!rule.selectorText || !rule.style) continue;
        const bg = rule.style.getPropertyValue("background-color")
          || rule.style.getPropertyValue("background");
        if (!bg) continue;
        try {
          if (el.matches(rule.selectorText)) {
            matched.push({
              sel: rule.selectorText.slice(0, 200),
              bg: bg.slice(0, 100),
              text: (rule.cssText || "").slice(0, 250),
            });
            if (matched.length >= 15) break;
          }
        } catch (_) {}
      }
      if (matched.length >= 15) break;
    }

    // children rows
    const rows = [...el.children].slice(0, 6).map((c) => {
      const cs = getComputedStyle(c);
      return {
        cls: fullCls(c).slice(0, 220),
        bg: cs.backgroundColor,
        color: cs.color,
        text: (c.innerText || "").replace(/\\s+/g, " ").slice(0, 50),
      };
    });

    // controls in first card
    const controls = [...el.querySelectorAll("button, select, input, [role='combobox'], [role='switch']")]
      .slice(0, 8)
      .map((c) => {
        const cs = getComputedStyle(c);
        return {
          tag: c.tagName,
          role: c.getAttribute("role"),
          cls: fullCls(c).slice(0, 200),
          bg: cs.backgroundColor,
          border: cs.borderTopColor,
          color: cs.color,
          text: (c.innerText || c.getAttribute("aria-label") || "").replace(/\\s+/g, " ").slice(0, 40),
        };
      });

    return {
      bg: s.backgroundColor,
      color: s.color,
      borderRadius: s.borderRadius,
      boxShadow: (s.boxShadow || "").slice(0, 150),
      border: s.borderTopWidth + " " + s.borderTopColor,
      overflow: s.overflow,
      y: Math.round(r.top),
      w: Math.round(r.width),
      h: Math.round(r.height),
      cls: fullCls(el).slice(0, 500),
      bgContrib,
      matched,
      rows,
      controls,
    };
  });

  // Look for host rules: .bg-token-something that equals #232323 / rgb(35,35,35)
  const grayRules = [];
  const target = "rgb(35, 35, 35)";
  // resolve by probing each bg-token class found on page
  const bgTokenClasses = new Set();
  for (const el of qa("[class*='bg-token'], [class*='bg-'], [class*='elevation']").slice(0, 500)) {
    for (const c of fullCls(el).split(/\\s+/)) {
      if (/^bg-|^elevation/.test(c) || c.includes("bg-token") || c.includes("elevation")) {
        bgTokenClasses.add(c);
      }
    }
  }
  // also from cards
  for (const el of cards) {
    for (const c of fullCls(el).split(/\\s+/)) bgTokenClasses.add(c);
  }

  const tmp = document.createElement("div");
  document.body.appendChild(tmp);
  const classResolve = {};
  for (const c of [...bgTokenClasses].slice(0, 120)) {
    tmp.className = c;
    const bg = getComputedStyle(tmp).backgroundColor;
    if (bg && bg !== "rgba(0, 0, 0, 0)") {
      classResolve[c] = bg;
    }
  }
  // specific suspects for #232323
  const suspects = [
    "bg-token-bg-primary",
    "bg-token-bg-secondary",
    "bg-token-bg-tertiary",
    "bg-token-main-surface-primary",
    "bg-[#232323]",
    "bg-neutral-900",
    "bg-zinc-900",
    "bg-gray-900",
    "bg-token-surface-primary",
    "bg-token-card",
    "bg-token-elevated",
  ];
  for (const c of suspects) {
    tmp.className = c;
    classResolve["probe:" + c] = getComputedStyle(tmp).backgroundColor;
  }
  // style var probes for possible missing tokens
  const varProbe = [
    "--color-token-bg-primary",
    "--color-token-bg-secondary",
    "--color-token-bg-tertiary",
    "--color-token-card-background",
    "--color-token-elevated-background",
    "--color-token-surface-elevated",
    "--vscode-editor-background",
    "--vscode-sideBar-background",
    "--vscode-input-background",
    "--vscode-dropdown-background",
    "--vscode-list-inactiveSelectionBackground",
    "--color-token-list-inactive-selection-background",
    "--color-token-editor-background",
    "--codex-base-surface",
    "--color-background-surface",
  ];
  const vars = {};
  const rootCs = getComputedStyle(document.documentElement);
  for (const k of varProbe) {
    vars[k] = rootCs.getPropertyValue(k).trim().slice(0, 80);
    tmp.className = "";
    tmp.style.backgroundColor = "var(" + k + ")";
    vars[k + "=>resolved"] = getComputedStyle(tmp).backgroundColor;
  }
  tmp.remove();

  // Search CSS for rgb(35, 35, 35) or #232323
  const graySrc = [];
  for (const sheet of document.styleSheets) {
    let rules;
    try { rules = sheet.cssRules; } catch { continue; }
    if (!rules) continue;
    for (const rule of rules) {
      const t = rule.cssText || "";
      if (/35,\\s*35,\\s*35|#232323|#232323ff|rgb\\(35/i.test(t) && t.length < 400) {
        graySrc.push(t.slice(0, 350));
        if (graySrc.length >= 20) break;
      }
    }
    if (graySrc.length >= 20) break;
  }

  // Settings page nav items list
  const navLabels = qa("button, a").filter((b) => {
    const t = (b.getAttribute("aria-label") || b.innerText || "").trim();
    return /^(常规|外观|配置|账户|通知|高级|数据控制|通用|权限|模型|关于)$/.test(t);
  }).map((b) => ({
    label: b.getAttribute("aria-label") || (b.innerText || "").trim(),
    ariaCurrent: b.getAttribute("aria-current"),
    cls: fullCls(b).slice(0, 100),
  }));

  // Section headers near cards
  const sectionHeaders = qa("h1, h2, h3, div, span").filter((el) => {
    const t = (el.innerText || "").trim();
    return /^(权限|常规|外观|终端|通知|高级|键盘)$/.test(t) && el.children.length === 0;
  }).slice(0, 15).map((el) => {
    const s = getComputedStyle(el);
    return { text: el.innerText.trim(), color: s.color, cls: fullCls(el).slice(0, 120), bg: s.backgroundColor };
  });

  return {
    rootCls: [...document.documentElement.classList],
    cardCount: cards.length,
    cardInfo,
    classResolve,
    vars,
    graySrc,
    navLabels,
    sectionHeaders,
    target,
  };
})()`;

const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
const val = r.result?.result?.value ?? r.result;
writeFileSync(OUT, JSON.stringify(val, null, 2), "utf8");
if (val?.subtype === "error" || val?.description) {
  console.log("ERR", JSON.stringify(val).slice(0, 600));
  process.exit(1);
}
console.log("root", val.rootCls);
console.log("cards", val.cardCount);
(val.cardInfo || []).forEach((c, i) => {
  console.log("\n=== card", i, "===");
  console.log("bg", c.bg, "radius", c.borderRadius, "shadow", c.boxShadow);
  console.log("cls", c.cls);
  console.log("bgContrib", c.bgContrib);
  console.log("matched", c.matched?.length);
  (c.matched || []).forEach((m) => console.log(" *", m.sel.slice(0, 100), "=>", m.bg));
  console.log("rows", c.rows);
  console.log("controls", c.controls);
});
console.log("\nclassResolve (non-transparent):");
Object.entries(val.classResolve || {}).forEach(([k, v]) => console.log(" ", k, "=>", v));
console.log("\nvars:");
Object.entries(val.vars || {}).forEach(([k, v]) => console.log(" ", k, "=", v));
console.log("\ngraySrc:");
(val.graySrc || []).forEach((g) => console.log(g));
console.log("\nnav", val.navLabels);
console.log("headers", val.sectionHeaders);
console.log("wrote", OUT);
ws.close();
