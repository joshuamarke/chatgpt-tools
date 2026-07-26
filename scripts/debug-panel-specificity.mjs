/**
 * After inject: which rule owns --color-background-panel, and does card selector match?
 */
import WebSocket from "ws";
import fs from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const css = fs.readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../skins/jiuyi/assets/jiuyi-skin.css"),
  "utf8"
);

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
await send("DOM.enable");
await send("CSS.enable");

// inject
await send("Runtime.evaluate", {
  expression: `(() => {
    const css = ${JSON.stringify(css)};
    let el = document.getElementById("codex-jiuyi-skin-style");
    if (!el) { el = document.createElement("style"); el.id = "codex-jiuyi-skin-style"; document.documentElement.appendChild(el); }
    el.textContent = css;
    return el.textContent.includes("--color-background-panel: #1a2330");
  })()`,
  returnByValue: true,
});

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `(() => {
    const root = document.documentElement;
    const cs = getComputedStyle(root);
    const panel = cs.getPropertyValue("--color-background-panel").trim();
    const styleEl = document.getElementById("codex-jiuyi-skin-style");
    // Find rules in styleEl that mention color-background-panel
    const sheet = [...document.styleSheets].find((s) => s.ownerNode === styleEl);
    const ourRules = [];
    if (sheet) {
      try {
        for (const rule of sheet.cssRules) {
          if ((rule.cssText || "").includes("color-background-panel")) {
            ourRules.push({
              sel: rule.selectorText,
              text: (rule.cssText || "").slice(0, 200),
            });
          }
        }
      } catch (e) {
        ourRules.push({ err: String(e) });
      }
    }

    // All sheets defining --color-background-panel
    const allDefs = [];
    for (const s of document.styleSheets) {
      let rules;
      try { rules = s.cssRules; } catch { continue; }
      if (!rules) continue;
      for (const rule of rules) {
        if (!rule.style) continue;
        const v = rule.style.getPropertyValue("--color-background-panel");
        if (v) {
          allDefs.push({
            sel: (rule.selectorText || "").slice(0, 150),
            value: v,
            priority: rule.style.getPropertyPriority("--color-background-panel"),
            href: (s.href || (s.ownerNode && s.ownerNode.id) || "inline").toString().slice(-60),
          });
        }
      }
    }

    // Card match test
    const cards = [...document.querySelectorAll("div.rounded-2xl.border.border-token-border.overflow-hidden")];
    const cards2 = [...document.querySelectorAll("div.rounded-2xl")].filter((el) => {
      const c = String(el.className);
      return c.includes("border-token-border") && c.includes("overflow-hidden");
    });
    const inMain = cards2.map((el) => ({
      matchesMain: !!el.closest("main.main-surface"),
      matchesSel: el.matches("div.rounded-2xl.border.border-token-border.overflow-hidden"),
      bg: getComputedStyle(el).backgroundColor,
      cls: String(el.className).slice(0, 200),
      inMainTag: el.closest("main")?.className?.toString?.()?.slice(0, 80),
    }));

    // Force set on root via JS to see if cascade works
    root.style.setProperty("--color-background-panel", "#1a2330", "important");
    const afterForce = getComputedStyle(root).getPropertyValue("--color-background-panel").trim();
    const cardAfter = cards2[0] ? getComputedStyle(cards2[0]).backgroundColor : null;

    return {
      panelComputed: panel,
      afterForce,
      cardAfter,
      ourRules,
      allDefs,
      cardsQuery: cards.length,
      cards2: cards2.length,
      inMain,
      styleLen: styleEl?.textContent?.length,
      sheetFound: !!sheet,
      sheetRules: sheet ? sheet.cssRules.length : 0,
    };
  })()`,
});

console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
