/**
 * Resolve --color-background-panel and related panel/card tokens on settings.
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
    const cs = getComputedStyle(document.documentElement);
    const keys = [...cs].filter((k) =>
      /color-background|background-panel|bg-fog|bg-elevated|token-bg|panel|settings-row|settings-header|input-background|editor-background|dropdown-background|list-inactive|widget-background/i.test(k)
    );
    const tokens = {};
    for (const k of keys.sort()) tokens[k] = cs.getPropertyValue(k).trim().slice(0, 100);

    const tmp = document.createElement("div");
    document.body.appendChild(tmp);
    const resolve = (expr) => {
      tmp.style.backgroundColor = expr;
      return getComputedStyle(tmp).backgroundColor;
    };
    const resolved = {
      panel: resolve("var(--color-background-panel)"),
      panelFallback: resolve("var(--color-background-panel, var(--color-token-bg-fog))"),
      fog: resolve("var(--color-token-bg-fog)"),
      surface: resolve("var(--color-background-surface)"),
      surfaceUnder: resolve("var(--color-background-surface-under)"),
      bgPrimary: resolve("var(--color-token-bg-primary)"),
      bgSecondary: resolve("var(--color-token-bg-secondary)"),
      editor: resolve("var(--color-token-editor-background)"),
      vscodeEditor: resolve("var(--vscode-editor-background)"),
      vscodeInput: resolve("var(--vscode-input-background)"),
      vscodePanel: resolve("var(--vscode-panel-background)"),
    };

    // Find all elements using color-background-panel in inline style
    const users = [...document.querySelectorAll("[style]")].filter((el) =>
      (el.getAttribute("style") || "").includes("color-background-panel")
      || (el.getAttribute("style") || "").includes("bg-fog")
    ).slice(0, 20).map((el) => {
      const s = getComputedStyle(el);
      const r = el.getBoundingClientRect();
      return {
        style: el.getAttribute("style")?.slice(0, 200),
        bg: s.backgroundColor,
        y: Math.round(r.top),
        w: Math.round(r.width),
        h: Math.round(r.height),
        cls: String(el.className || "").slice(0, 160),
        text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 50),
      };
    });

    // Any CSS rules referencing color-background-panel
    const rules = [];
    for (const sheet of document.styleSheets) {
      let rs;
      try { rs = sheet.cssRules; } catch { continue; }
      if (!rs) continue;
      for (const rule of rs) {
        const t = rule.cssText || "";
        if (/color-background-panel|token-bg-fog|--color-background-/i.test(t) && t.length < 500) {
          rules.push(t.slice(0, 400));
          if (rules.length >= 30) break;
        }
      }
      if (rules.length >= 30) break;
    }

    // Controls that still look host-default gray
    const grayControls = [...document.querySelectorAll("button, input, select, [role='combobox'], [role='switch']")]
      .map((el) => {
        const s = getComputedStyle(el);
        const r = el.getBoundingClientRect();
        if (r.width < 20 || r.height < 16) return null;
        if (r.top < 0 || r.top > innerHeight) return null;
        const bg = s.backgroundColor;
        if (!/rgb\\((35|40|45|48|54)/.test(bg) && !/rgba\\((35|40|45|48|54)/.test(bg)) return null;
        return {
          tag: el.tagName,
          role: el.getAttribute("role"),
          bg,
          border: s.borderTopColor,
          color: s.color,
          cls: String(el.className || "").slice(0, 140),
          text: (el.innerText || el.getAttribute("aria-label") || "").replace(/\\s+/g, " ").slice(0, 40),
          style: (el.getAttribute("style") || "").slice(0, 120),
        };
      })
      .filter(Boolean)
      .slice(0, 25);

    tmp.remove();
    return {
      rootCls: [...document.documentElement.classList],
      tokens,
      resolved,
      users,
      rules,
      grayControls,
    };
  })()`,
});

const val = r.result?.result?.value ?? r.result;
console.log(JSON.stringify(val, null, 2));
ws.close();
