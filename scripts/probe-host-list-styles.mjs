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
    setTimeout(() => rej(new Error("timeout")), 25000);
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
  const out = {
    hasJiuyi: root.classList.contains("codex-jiuyi-skin"),
    rootClass: root.className,
    colorScheme: getComputedStyle(root).colorScheme,
    tokens: {},
    cssHits: [],
    openMenus: [],
    suggestionCards: null,
  };

  const cs = getComputedStyle(root);
  for (const k of [
    "--main-surface-primary",
    "--main-surface-secondary",
    "--surface-primary",
    "--surface-secondary",
    "--text-primary",
    "--text-secondary",
    "--icon-primary",
    "--border-primary",
    "--composer-background",
    "--dropdown-background",
    "--bg-primary",
    "--bg-secondary",
    "--bg-tertiary",
    "--interactive-bg-secondary-hover",
    "--interactive-label-primary-default",
  ]) {
    out.tokens[k] = cs.getPropertyValue(k).trim();
  }

  // Harvest host stylesheet rules for suggestion / dropdown / menu surfaces
  const re = /home-suggestion|suggestion-list|DropdownMenu|SelectContent|PopoverContent|menu-content|listbox|bg-token-main-surface|token-bg|popover/i;
  for (const sheet of document.styleSheets) {
    let rules;
    try { rules = sheet.cssRules; } catch { continue; }
    if (!rules) continue;
    for (const r of rules) {
      const text = r.cssText || "";
      const sel = r.selectorText || "";
      if (re.test(sel) || re.test(text)) {
        out.cssHits.push(text.slice(0, 420));
        if (out.cssHits.length >= 40) break;
      }
    }
    if (out.cssHits.length >= 40) break;
  }

  const pick = (el) => {
    if (!el) return null;
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      tag: el.tagName,
      role: el.getAttribute("role"),
      cls: String(el.className || "").slice(0, 260),
      bg: s.backgroundColor,
      bgImg: (s.backgroundImage || "").slice(0, 100),
      color: s.color,
      border: s.border,
      radius: s.borderRadius,
      pad: s.padding,
      gap: s.gap,
      shadow: (s.boxShadow || "").slice(0, 100),
      fontSize: s.fontSize,
      fontWeight: s.fontWeight,
      display: s.display,
      w: Math.round(r.width),
      h: Math.round(r.height),
      text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 80),
    };
  };

  out.openMenus = [...document.querySelectorAll(
    '[role="menu"],[role="listbox"],[data-radix-menu-content],[data-radix-select-content],[data-radix-popper-content-wrapper] > div, [class*="home-suggestion-list"]'
  )].slice(0, 12).map(pick);

  const sug = document.querySelector('.group\\\\/home-suggestions') || document.querySelector('[class*="home-suggestions"]');
  if (sug) {
    out.suggestionCards = {
      group: pick(sug),
      buttons: [...sug.querySelectorAll("button")].slice(0, 4).map(pick),
    };
  }

  // Click first suggestion card to open list if present and no menu open
  if (sug && out.openMenus.length === 0) {
    const btn = sug.querySelector("button");
    if (btn) {
      btn.click();
      // note: caller may re-probe; synchronous open may not finish
      out.clickedSuggestion = true;
    }
  }

  return out;
})()`;

const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));

// second pass after click
await new Promise((r) => setTimeout(r, 600));
const expr2 = `(() => {
  const pick = (el) => {
    if (!el) return null;
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      tag: el.tagName,
      role: el.getAttribute("role"),
      cls: String(el.className || "").slice(0, 300),
      bg: s.backgroundColor,
      color: s.color,
      border: s.border,
      radius: s.borderRadius,
      pad: s.padding,
      gap: s.gap,
      shadow: (s.boxShadow || "").slice(0, 120),
      fontSize: s.fontSize,
      display: s.display,
      flexDir: s.flexDirection,
      align: s.alignItems,
      w: Math.round(r.width),
      h: Math.round(r.height),
      text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 100),
      html: el.outerHTML.slice(0, 500),
    };
  };
  const menus = [...document.querySelectorAll(
    '[role="menu"],[role="listbox"],[class*="home-suggestion-list"],[data-radix-menu-content],[data-radix-select-content],[data-state="open"]'
  )].slice(0, 20).map(pick);
  const items = [...document.querySelectorAll(
    '[class*="home-suggestion-list-item"],[role="menuitem"],[role="option"]'
  )].slice(0, 10).map(pick);
  return { menus, items, hasJiuyi: document.documentElement.classList.contains("codex-jiuyi-skin") };
})()`;
const r2 = await send("Runtime.evaluate", { expression: expr2, returnByValue: true });
console.log("--- after click ---");
console.log(JSON.stringify(r2.result?.result?.value ?? r2.result, null, 2));
ws.close();
