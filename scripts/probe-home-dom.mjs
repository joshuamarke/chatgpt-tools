/**
 * One-shot CDP probe of Codex home layout (hero / suggestions / composer / send).
 */
import WebSocket from "ws";

const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page =
  pages.find((p) => p.type === "page" && String(p.url || "").includes("app://")) || pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.error("No CDP page");
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();

function send(method, params = {}) {
  const i = ++id;
  return new Promise((res, rej) => {
    pending.set(i, { res, rej });
    ws.send(JSON.stringify({ id: i, method, params }));
    setTimeout(() => rej(new Error("timeout " + method)), 20000);
  });
}

ws.on("message", (d) => {
  const m = JSON.parse(d.toString());
  if (m.id && pending.has(m.id)) {
    const { res } = pending.get(m.id);
    pending.delete(m.id);
    res(m);
  }
});

await new Promise((r) => ws.once("open", r));
await send("Runtime.enable");

const expr = `(() => {
  const pick = (el) => {
    if (!el) return null;
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      tag: el.tagName,
      class: (el.className && String(el.className).slice(0, 200)) || "",
      testid: el.getAttribute("data-testid"),
      feature: el.getAttribute("data-feature"),
      role: el.getAttribute("role"),
      aria: el.getAttribute("aria-label"),
      pos: cs.position,
      top: cs.top,
      left: cs.left,
      right: cs.right,
      display: cs.display,
      flex: cs.flex,
      flexBasis: cs.flexBasis,
      alignItems: cs.alignItems,
      justifyContent: cs.justifyContent,
      mt: cs.marginTop,
      pt: cs.paddingTop,
      minH: cs.minHeight,
      h: Math.round(r.height),
      y: Math.round(r.top),
      x: Math.round(r.left),
      w: Math.round(r.width),
      bottom: Math.round(r.bottom),
    };
  };

  const home =
    document.querySelector(".jiuyi-home") ||
    document.querySelector("[class$='-home']") ||
    document.querySelector("main [role='main']") ||
    document.querySelector("main");

  const hero = document.querySelector('[data-feature="game-source"]');
  const sug =
    document.querySelector(".group\\\\/home-suggestions") ||
    document.querySelector('[class*="home-suggestions"]');

  let banner = hero;
  for (let i = 0; i < 8 && banner && banner.parentElement; i++) {
    const p = banner.parentElement;
    if (p.querySelector('[class*="home-suggestions"]') || p.querySelector(".group\\\\/home-suggestions")) {
      banner = p;
      break;
    }
    banner = p;
  }

  const chain = [];
  let cur = home;
  for (let i = 0; i < 6 && cur; i++) {
    chain.push(pick(cur));
    cur = cur.children && cur.children[0];
  }

  const composer = document.querySelector(".composer-surface-chrome");
  const composerBtns = composer
    ? Array.from(composer.querySelectorAll("button")).map((b) => ({
        ...pick(b),
        bg: getComputedStyle(b).backgroundColor,
        color: getComputedStyle(b).color,
        borderRadius: getComputedStyle(b).borderRadius,
        html: b.outerHTML.slice(0, 280),
      }))
    : [];

  const menus = Array.from(
    document.querySelectorAll('[role="menu"],[role="listbox"],[data-radix-popper-content-wrapper]')
  )
    .slice(0, 20)
    .map((el) => ({
      ...pick(el),
      text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 160),
    }));

  const root = getComputedStyle(document.documentElement);
  const tokens = {
    mainSurface: root.getPropertyValue("--main-surface-primary").trim(),
    composerBg: root.getPropertyValue("--composer-background").trim(),
    textPrimary: root.getPropertyValue("--text-primary").trim(),
  };

  return {
    rootClasses: document.documentElement.className,
    vw: window.innerWidth,
    vh: window.innerHeight,
    chain,
    home: pick(home),
    banner: pick(banner),
    hero: pick(hero),
    sug: pick(sug),
    sugParent: pick(sug && sug.parentElement),
    sugGrand: pick(sug && sug.parentElement && sug.parentElement.parentElement),
    sugButtons: sug ? Array.from(sug.querySelectorAll("button")).slice(0, 4).map(pick) : [],
    composer: pick(composer),
    composerBtns,
    menus,
    tokens,
  };
})()`;

const r = await send("Runtime.evaluate", {
  expression: expr,
  returnByValue: true,
});

if (r.result?.exceptionDetails) {
  console.error(JSON.stringify(r.result.exceptionDetails, null, 2));
} else {
  console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
}
ws.close();
process.exit(0);
