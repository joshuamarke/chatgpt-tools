/**
 * Deep-probe settings item cards: which class/token paints white bg.
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "probe-settings-cards-out.json.txt");

const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find((p) => p.type === "page") || pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.log("no cdp");
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
    setTimeout(() => rej(new Error("timeout " + method)), 25000);
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

  const whiteCards = qa("div, section").filter((el) => {
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    if (r.width < 200 || r.height < 40) return false;
    return cs.backgroundColor === "rgb(255, 255, 255)"
      || cs.backgroundColor === "rgb(250, 250, 250)"
      || cs.backgroundColor === "rgba(255, 255, 255, 0.96)";
  });

  // Only fully inspect first 3 cards; avoid full stylesheet walk per element
  const detail = whiteCards.slice(0, 3).map((el) => {
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();

    // Matched rules: only check sheets that mention bg / elevation / token
    const matched = [];
    for (const sheet of document.styleSheets) {
      let rules;
      try { rules = sheet.cssRules; } catch { continue; }
      if (!rules) continue;
      for (const rule of rules) {
        if (!rule.selectorText || !rule.style) continue;
        const bgProp = rule.style.getPropertyValue("background-color")
          || rule.style.getPropertyValue("background");
        if (!bgProp) continue;
        try {
          if (el.matches(rule.selectorText)) {
            matched.push({
              sel: rule.selectorText.slice(0, 220),
              bgProp: bgProp.slice(0, 120),
              snippet: (rule.cssText || "").slice(0, 280),
            });
            if (matched.length >= 20) break;
          }
        } catch (_) {}
      }
      if (matched.length >= 20) break;
    }

    const parents = [];
    let n = el;
    for (let i = 0; i < 8 && n; i++) {
      const pcs = getComputedStyle(n);
      parents.push({
        tag: n.tagName,
        cls: fullCls(n).slice(0, 240),
        bg: pcs.backgroundColor,
      });
      n = n.parentElement;
    }

    const kids = [...el.children].slice(0, 6).map((c) => {
      const kcs = getComputedStyle(c);
      return {
        tag: c.tagName,
        cls: fullCls(c).slice(0, 200),
        bg: kcs.backgroundColor,
        color: kcs.color,
        text: (c.innerText || "").replace(/\\s+/g, " ").slice(0, 50),
      };
    });

    const controls = [...el.querySelectorAll("button, select, input, [role='switch'], [role='combobox']")]
      .slice(0, 8)
      .map((c) => {
        const kcs = getComputedStyle(c);
        return {
          tag: c.tagName,
          role: c.getAttribute("role"),
          cls: fullCls(c).slice(0, 180),
          bg: kcs.backgroundColor,
          border: kcs.borderColor,
          color: kcs.color,
          text: (c.innerText || c.getAttribute("aria-label") || "").replace(/\\s+/g, " ").slice(0, 40),
        };
      });

    // Inline style / dataset
    return {
      bg: cs.backgroundColor,
      color: cs.color,
      border: (cs.borderTopWidth + " " + cs.borderTopStyle + " " + cs.borderTopColor),
      borderRadius: cs.borderRadius,
      boxShadow: (cs.boxShadow || "").slice(0, 160),
      y: Math.round(r.top),
      w: Math.round(r.width),
      h: Math.round(r.height),
      cls: fullCls(el).slice(0, 500),
      attrStyle: el.getAttribute("style"),
      text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 100),
      matchedBgRules: matched,
      parents,
      kids,
      controls,
    };
  });

  // Search host CSS for common white surface utilities
  const utilityHits = [];
  for (const sheet of document.styleSheets) {
    let rules;
    try { rules = sheet.cssRules; } catch { continue; }
    if (!rules) continue;
    for (const rule of rules) {
      const sel = rule.selectorText || "";
      const t = rule.cssText || "";
      if (t.length > 500) continue;
      if (
        /bg-white|bg-token-bg-primary|bg-token-main-surface|elevation|settings-group|card-surface|bg-token-surface|bg-token-card/i.test(sel)
        || (/background-color:\\s*(#fff|#ffffff|white|rgb\\(255,\\s*255,\\s*255\\))/i.test(t)
          && /\\.(bg-|card|surface|elevation|token)/i.test(sel))
      ) {
        utilityHits.push(t.slice(0, 320));
        if (utilityHits.length >= 30) break;
      }
    }
    if (utilityHits.length >= 30) break;
  }

  // Find label "默认权限" chain
  const rowSample = (() => {
    const label = qa("div, span, label, p").find((el) => (el.innerText || "").trim() === "默认权限");
    if (!label) return null;
    let n = label;
    const chain = [];
    for (let i = 0; i < 12 && n; i++) {
      const cs = getComputedStyle(n);
      chain.push({
        tag: n.tagName,
        cls: fullCls(n).slice(0, 320),
        bg: cs.backgroundColor,
        display: cs.display,
      });
      n = n.parentElement;
    }
    return chain;
  })();

  // Token dump for settings-related vscode vars that still look light
  const cs = getComputedStyle(document.documentElement);
  const lightish = {};
  for (const k of [...cs]) {
    if (!/vscode|color-token|elevation|surface|settings|input|panel|editor/i.test(k)) continue;
    const v = cs.getPropertyValue(k).trim();
    if (/255,\\s*255,\\s*255|#fff|#ffffff|white|1a1c1f|#f[0-9a-f]{5}/i.test(v)) {
      lightish[k] = v.slice(0, 100);
    }
  }

  // Does host use class bg-token-* or style elevation?
  const classFreq = {};
  for (const el of whiteCards) {
    for (const c of fullCls(el).split(/\\s+/)) {
      if (!c) continue;
      if (/bg-|border-|elevation|shadow|surface|card|token|rounded|ring/.test(c)) {
        classFreq[c] = (classFreq[c] || 0) + 1;
      }
    }
  }

  return {
    whiteCardCount: whiteCards.length,
    detail,
    utilityHits,
    rowSample,
    lightish,
    classFreq,
    rootCls: [...document.documentElement.classList],
  };
})()`;

const r = await send("Runtime.evaluate", {
  expression: expr,
  returnByValue: true,
});
const val = r.result?.result?.value ?? r.result;
const text = JSON.stringify(val, null, 2);
writeFileSync(OUT, text, "utf8");

if (val?.exceptionDetails || val?.subtype === "error") {
  console.log("EVAL ERROR", text.slice(0, 800));
  process.exit(1);
}

console.log("whiteCards", val.whiteCardCount);
console.log("classFreq", JSON.stringify(val.classFreq, null, 2));
console.log("\n---detail[0]---");
if (val.detail?.[0]) {
  const d = val.detail[0];
  console.log("cls:", d.cls);
  console.log("bg:", d.bg, "radius:", d.borderRadius, "shadow:", d.boxShadow);
  console.log("matched:", d.matchedBgRules?.length);
  (d.matchedBgRules || []).forEach((m) => console.log(" *", m.sel.slice(0, 120), "=>", m.bgProp));
  console.log("parents:");
  (d.parents || []).forEach((p) => console.log(" ", p.bg, "|", p.cls?.slice(0, 120)));
  console.log("kids:");
  (d.kids || []).forEach((k) => console.log(" ", k.bg, k.text, "|", k.cls?.slice(0, 100)));
  console.log("controls:");
  (d.controls || []).forEach((c) => console.log(" ", c.bg, c.border, c.text, "|", c.cls?.slice(0, 100)));
}
console.log("\n---rowSample---");
console.log(JSON.stringify(val.rowSample, null, 2)?.slice(0, 5000));
console.log("\n---utilityHits---");
(val.utilityHits || []).slice(0, 15).forEach((u) => console.log(u));
console.log("\n---lightish tokens (first 40)---");
Object.entries(val.lightish || {}).slice(0, 40).forEach(([k, v]) => console.log(k, "=", v));
console.log("\nwrote", OUT);
ws.close();
