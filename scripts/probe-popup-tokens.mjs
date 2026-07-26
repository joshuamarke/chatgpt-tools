import WebSocket from "ws";

const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find((p) => p.type === "page") || pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.log("no cdp");
  process.exit(0);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
const send = (method, params = {}) =>
  new Promise((res, rej) => {
    const i = ++id;
    pending.set(i, { res, rej });
    ws.send(JSON.stringify({ id: i, method, params }));
    setTimeout(() => rej(new Error("timeout " + method)), 20000);
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
  const root = document.documentElement;
  const cs = getComputedStyle(root);

  // All --color-token* and related custom props
  const tokenKeys = [];
  for (const k of cs) {
    if (/^--/.test(k) && /token|surface|bg|dropdown|popover|menu|dialog|main-surface|color-text|color-bg/i.test(k)) {
      tokenKeys.push(k);
    }
  }
  const tokens = {};
  for (const k of tokenKeys.sort()) {
    const v = cs.getPropertyValue(k).trim();
    if (v) tokens[k] = v.slice(0, 100);
  }

  // Also try reading common aliases even if empty
  const aliases = [
    "--main-surface-primary",
    "--main-surface-secondary",
    "--surface-primary",
    "--surface-secondary",
    "--color-token-bg-primary",
    "--color-token-bg-secondary",
    "--color-token-bg-tertiary",
    "--color-token-bg-elevated",
    "--color-token-main-surface-primary",
    "--color-token-main-surface-secondary",
    "--color-main-surface-primary",
    "--token-main-surface-primary",
    "--bg-primary",
    "--bg-secondary",
    "--dropdown-background",
    "--popover-background",
  ];
  const aliasVals = {};
  for (const k of aliases) aliasVals[k] = cs.getPropertyValue(k).trim() || "(empty)";

  // Probe utility classes for actual resolved colors
  const probe = document.createElement("div");
  document.body.appendChild(probe);
  const one = (cls) => {
    probe.className = cls;
    const s = getComputedStyle(probe);
    return { bg: s.backgroundColor, color: s.color, border: s.borderColor };
  };
  const utilities = {
    "bg-token-main-surface-primary": one("bg-token-main-surface-primary"),
    "bg-token-main-surface-secondary": one("bg-token-main-surface-secondary"),
    "bg-token-bg-primary": one("bg-token-bg-primary"),
    "bg-token-bg-secondary": one("bg-token-bg-secondary"),
    "bg-token-bg-elevated": one("bg-token-bg-elevated"),
    "bg-token-surface-primary": one("bg-token-surface-primary"),
    "bg-token-dropdown": one("bg-token-dropdown"),
    "bg-token-popover": one("bg-token-popover"),
    "bg-token-sidebar-surface-primary": one("bg-token-sidebar-surface-primary"),
    "bg-token-composer": one("bg-token-composer"),
    "border-token-border-default": one("border-token-border-default"),
    "border-token-input-border": one("border-token-input-border"),
  };
  probe.remove();

  // Open any existing popovers
  const pick = (el) => {
    if (!el) return null;
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    // find which bg var might apply by walking style
    return {
      tag: el.tagName,
      role: el.getAttribute("role"),
      dataState: el.getAttribute("data-state"),
      cls: String(el.className || "").slice(0, 280),
      parentCls: el.parentElement ? String(el.parentElement.className || "").slice(0, 160) : null,
      inBody: el.parentElement === document.body || el.closest("body") === document.body,
      ancestors: (() => {
        const a = [];
        let n = el;
        for (let i = 0; i < 6 && n; i++) {
          a.push(n.tagName + (n.id ? "#" + n.id : "") + "." + String(n.className || "").split(" ").slice(0, 3).join("."));
          n = n.parentElement;
        }
        return a;
      })(),
      bg: s.backgroundColor,
      bgImg: (s.backgroundImage || "").slice(0, 80),
      color: s.color,
      border: s.border,
      radius: s.borderRadius,
      shadow: (s.boxShadow || "").slice(0, 100),
      w: Math.round(r.width),
      h: Math.round(r.height),
      // inherited custom props on the element itself
      localTokens: {
        mainSurf: s.getPropertyValue("--main-surface-primary").trim(),
        colorTokenBg: s.getPropertyValue("--color-token-bg-primary").trim(),
        surface: s.getPropertyValue("--surface-primary").trim(),
      },
      text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 80),
    };
  };

  // Try open a menu: project selector or model picker or any dropdown trigger
  const triggers = [
    document.querySelector('.group\\\\/project-selector button'),
    document.querySelector('[class*="ModelPicker"] button'),
    document.querySelector('button[aria-haspopup="menu"]'),
    document.querySelector('button[aria-haspopup="listbox"]'),
    document.querySelector('button[aria-haspopup="dialog"]'),
    ...document.querySelectorAll('[data-state="closed"][aria-haspopup]'),
  ].filter(Boolean);

  let clicked = null;
  for (const t of triggers.slice(0, 3)) {
    try {
      t.click();
      clicked = String(t.className || t.getAttribute("aria-label") || t.tagName).slice(0, 120);
      break;
    } catch {}
  }

  return {
    hasJiuyi: root.classList.contains("codex-jiuyi-skin"),
    rootClass: root.className,
    colorScheme: cs.colorScheme,
    aliasVals,
    tokenCount: Object.keys(tokens).length,
    tokensSample: Object.fromEntries(Object.entries(tokens).slice(0, 60)),
    allTokenKeys: Object.keys(tokens),
    utilities,
    clicked,
    styleIds: [...document.querySelectorAll("style[id]")].map((s) => s.id),
  };
})()`;

const r1 = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
console.log("=== PASS1 ===");
console.log(JSON.stringify(r1.result?.result?.value ?? r1.result, null, 2));

await new Promise((r) => setTimeout(r, 700));

const expr2 = `(() => {
  const pick = (el) => {
    if (!el) return null;
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      tag: el.tagName,
      role: el.getAttribute("role"),
      cls: String(el.className || "").slice(0, 300),
      parentTag: el.parentElement?.tagName,
      parentCls: String(el.parentElement?.className || "").slice(0, 200),
      bg: s.backgroundColor,
      color: s.color,
      border: s.border,
      radius: s.borderRadius,
      shadow: (s.boxShadow || "").slice(0, 120),
      pad: s.padding,
      w: Math.round(r.width),
      h: Math.round(r.height),
      localMain: s.getPropertyValue("--main-surface-primary").trim(),
      localColorBg: s.getPropertyValue("--color-token-bg-primary").trim(),
      localSurface: s.getPropertyValue("--surface-primary").trim(),
      // which stylesheet rules match bg?
      matchedBgRules: (() => {
        const hits = [];
        try {
          for (const sheet of document.styleSheets) {
            let rules; try { rules = sheet.cssRules; } catch { continue; }
            if (!rules) continue;
            for (const rule of rules) {
              if (!rule.selectorText || !rule.style) continue;
              const bg = rule.style.backgroundColor || rule.style.background || "";
              if (!bg && !/background/.test(rule.cssText)) continue;
              try {
                if (el.matches(rule.selectorText) && (bg || /background/.test(rule.cssText))) {
                  hits.push(rule.cssText.slice(0, 280));
                  if (hits.length >= 8) return hits;
                }
              } catch {}
            }
          }
        } catch {}
        return hits;
      })(),
      html: el.outerHTML.slice(0, 400),
      text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 100),
    };
  };

  const sels = [
    '[role="menu"]',
    '[role="listbox"]',
    '[role="dialog"]',
    '[data-radix-popper-content-wrapper]',
    '[data-radix-menu-content]',
    '[data-radix-select-content]',
    '[data-state="open"]',
    '[class*="DropdownMenu"]',
    '[class*="Popover"]',
    '[class*="SelectContent"]',
    '[class*="MenuContent"]',
  ];
  const nodes = [];
  for (const s of sels) {
    for (const el of document.querySelectorAll(s)) {
      if (!nodes.includes(el)) nodes.push(el);
    }
  }
  return {
    hasJiuyi: document.documentElement.classList.contains("codex-jiuyi-skin"),
    count: nodes.length,
    nodes: nodes.slice(0, 15).map(pick),
  };
})()`;

const r2 = await send("Runtime.evaluate", { expression: expr2, returnByValue: true });
console.log("=== PASS2 open layers ===");
console.log(JSON.stringify(r2.result?.result?.value ?? r2.result, null, 2));
ws.close();
