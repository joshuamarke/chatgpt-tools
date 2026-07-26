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
  const q = (s) => document.querySelector(s);
  const btn = q('.group\\\\/home-suggestions')?.querySelector(
    'button:not([class*="home-suggestion-list-item"])'
  );
  const maskParent = q(".horizontal-scroll-fade-mask")?.parentElement;
  const home = q(".jiuyi-home");
  const hero = q('[data-feature="game-source"]');
  const pick = (el) => {
    if (!el) return null;
    const cs = getComputedStyle(el);
    return {
      cls: String(el.className || "").slice(0, 180),
      bg: cs.backgroundColor,
      bgImg: (cs.backgroundImage || "").slice(0, 180),
      backdrop: cs.backdropFilter || cs.webkitBackdropFilter,
      border: cs.border,
      boxShadow: (cs.boxShadow || "").slice(0, 120),
      color: cs.color,
      paddingTop: cs.paddingTop,
      hasJiuyiHomeAnc: !!el.closest(".jiuyi-home"),
    };
  };
  const styleEl = document.getElementById("codex-jiuyi-skin-style");
  const cssText = styleEl?.textContent || "";
  return {
    hasHome: !!home,
    homeCls: home ? String(home.className).slice(0, 140) : null,
    cssVer: styleEl?.dataset?.skinVersion || styleEl?.dataset?.skinRevision,
    cssHasSideBar: cssText.includes("bg-token-side-bar"),
    cssHasCardNot: cssText.includes("home-suggestion-list-item"),
    cssGlassSnippet: (() => {
      const i = cssText.indexOf("button:not([class*=\\"home-suggestion-list-item\\"])");
      return i >= 0 ? cssText.slice(i, i + 400) : null;
    })(),
    btn: pick(btn),
    hero: pick(hero),
    heroBefore: hero ? getComputedStyle(hero, "::before").content : null,
    heroAfter: hero ? getComputedStyle(hero, "::after").content : null,
    maskParent: pick(maskParent),
    maskParentMatches: maskParent
      ? {
          hasFadeChild: !!maskParent.querySelector(":scope > .horizontal-scroll-fade-mask"),
          hasSideBarCls: /bg-token-side-bar/.test(maskParent.className),
          hasSelectNone: maskParent.classList.contains("select-none"),
          className: String(maskParent.className).slice(0, 220),
        }
      : null,
  };
})()`;

const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
