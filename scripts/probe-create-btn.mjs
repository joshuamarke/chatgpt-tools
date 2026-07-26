/**
 * Probe header Create button styles/classes on current route.
 */
import WebSocket from "ws";

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
    setTimeout(() => rej(new Error("timeout")), 15000);
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

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `(() => {
    const header = document.querySelector("header.app-header-tint");
    const btns = [...document.querySelectorAll("button, a, [role='button']")].filter((el) => {
      const t = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || "")).trim();
      return /^(创建|Create)/.test(t) || t === "创建" || (el.getAttribute("aria-label") || "") === "创建";
    });
    return {
      root: document.documentElement.className,
      shellCls: document.querySelector("main.main-surface")?.className?.slice?.(0, 200),
      headerBg: header ? getComputedStyle(header).backgroundColor : null,
      headerBgImg: header ? getComputedStyle(header).backgroundImage.slice(0, 160) : null,
      headerColor: header ? getComputedStyle(header).color : null,
      btns: btns.map((el) => {
        const cs = getComputedStyle(el);
        const r = el.getBoundingClientRect();
        // matched rules with color/bg
        const matched = [];
        for (const sheet of document.styleSheets) {
          let rules;
          try { rules = sheet.cssRules; } catch { continue; }
          if (!rules) continue;
          for (const rule of rules) {
            if (!rule.selectorText || !rule.style) continue;
            try {
              if (!el.matches(rule.selectorText)) continue;
            } catch { continue; }
            const color = rule.style.color || rule.style.getPropertyValue("color");
            const bg = rule.style.backgroundColor || rule.style.background || rule.style.getPropertyValue("background");
            if (!color && !bg) continue;
            matched.push({
              sel: rule.selectorText.slice(0, 200),
              color: color || null,
              bg: (bg || "").slice(0, 120) || null,
              importantColor: rule.style.getPropertyPriority("color"),
              sheet: (sheet.ownerNode?.id || "").slice(0, 40),
            });
            if (matched.length >= 25) break;
          }
          if (matched.length >= 25) break;
        }
        return {
          text: (el.innerText || el.getAttribute("aria-label") || "").replace(/\\s+/g, " ").trim().slice(0, 40),
          aria: el.getAttribute("aria-label"),
          cls: String(el.className || ""),
          color: cs.color,
          bg: cs.backgroundColor,
          bgImg: cs.backgroundImage.slice(0, 120),
          border: cs.border,
          font: cs.fontSize + " " + cs.fontWeight,
          x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.w || r.width), h: Math.round(r.height),
          inHeader: Boolean(header && header.contains(el)),
          parentCls: el.parentElement ? String(el.parentElement.className).slice(0, 200) : null,
          matched: matched.slice(0, 20),
        };
      }),
    };
  })()`,
});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
