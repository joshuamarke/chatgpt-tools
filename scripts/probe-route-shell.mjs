/**
 * Probe current Codex route shell: home markers, hero, suggestions, header create buttons.
 * Usage: node scripts/probe-route-shell.mjs
 */
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
    setTimeout(() => rej(new Error("timeout " + method)), 15000);
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
  const box = (el) => {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return {
      tag: el.tagName,
      role: el.getAttribute("role"),
      testid: el.getAttribute("data-testid"),
      feature: el.getAttribute("data-feature"),
      cls: String(el.className || "").slice(0, 260),
      text: (el.innerText || "").replace(/\\s+/g, " ").trim().slice(0, 120),
      aria: el.getAttribute("aria-label"),
      w: Math.round(r.width),
      h: Math.round(r.height),
      x: Math.round(r.x),
      y: Math.round(r.y),
      color: cs.color,
      bg: cs.backgroundColor,
      bgImg: cs.backgroundImage?.slice(0, 80),
      opacity: cs.opacity,
      display: cs.display,
      radius: cs.borderRadius,
    };
  };

  const root = document.documentElement;
  const shell = document.querySelector("main.main-surface");
  const roleMains = [...document.querySelectorAll('[role="main"]')].map(box);
  const homeIcon = document.querySelector('[data-testid="home-icon"]');
  const gameSource = document.querySelector('[data-feature="game-source"]');
  const gameSurface = document.querySelector('[data-feature="game-surface"]');
  const suggestions =
    document.querySelector(".group\\\\/home-suggestions") ||
    document.querySelector('[class*="home-suggestions"]');
  const composer = document.querySelector(".composer-surface-chrome");
  const header = document.querySelector("header.app-header-tint");

  // home-like classes currently applied
  const homeClasses = [...document.querySelectorAll("[class]")].filter((el) =>
    /(?:^|\\s)(?:dream|mortal|qingkong|jiuyi|linglong|cyberpunk|eva|bengong|miku|cn|skin)-home(?:-shell|-utility)?(?:\\s|$)/.test(
      String(el.className)
    )
  ).map((el) => ({
    tag: el.tagName,
    cls: String(el.className).split(/\\s+/).filter((c) => /home/.test(c)).join(" "),
    role: el.getAttribute("role"),
  }));

  // header buttons (create etc.)
  const headerBtns = header
    ? [...header.querySelectorAll("button, a, [role='button']")].slice(0, 20).map((el) => {
        const cs = getComputedStyle(el);
        return {
          text: (el.innerText || el.getAttribute("aria-label") || "").replace(/\\s+/g, " ").trim().slice(0, 60),
          aria: el.getAttribute("aria-label"),
          cls: String(el.className || "").slice(0, 180),
          color: cs.color,
          bg: cs.backgroundColor,
          opacity: cs.opacity,
          visible: el.getClientRects().length > 0,
        };
      })
    : [];

  // any "创建" text buttons in main/header
  const createBtns = [...document.querySelectorAll("button, a, [role='button']")]
    .filter((el) => {
      const t = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || "")).toLowerCase();
      return /创建|create|new|新建/.test(t) && el.getClientRects().length > 0;
    })
    .slice(0, 12)
    .map((el) => {
      const cs = getComputedStyle(el);
      const r = el.getBoundingClientRect();
      return {
        text: (el.innerText || el.getAttribute("aria-label") || "").replace(/\\s+/g, " ").trim().slice(0, 80),
        aria: el.getAttribute("aria-label"),
        cls: String(el.className || "").slice(0, 200),
        color: cs.color,
        bg: cs.backgroundColor,
        x: Math.round(r.x),
        y: Math.round(r.y),
        w: Math.round(r.width),
        h: Math.round(r.height),
        inHeader: Boolean(header && header.contains(el)),
      };
    });

  // hero chain
  const hero = gameSource;
  const heroChain = [];
  let n = hero;
  for (let i = 0; n && i < 10; i++) {
    heroChain.push(box(n));
    n = n.parentElement;
  }

  // location / title signals
  const pathish = {
    href: location.href,
    hash: location.hash,
    title: document.title,
  };

  // nav items pressed/active
  const nav = [...document.querySelectorAll("aside a, aside button, nav a, nav button")]
    .filter((el) => el.getClientRects().length > 0)
    .slice(0, 30)
    .map((el) => ({
      text: (el.innerText || el.getAttribute("aria-label") || "").replace(/\\s+/g, " ").trim().slice(0, 50),
      aria: el.getAttribute("aria-label"),
      current: el.getAttribute("aria-current"),
      pressed: el.getAttribute("aria-pressed"),
      cls: String(el.className || "").slice(0, 100),
    }));

  // sticky top bars / route chrome
  const stickies = [...document.querySelectorAll("div.sticky, header")]
    .slice(0, 8)
    .map(box);

  // suggestion buttons sample
  const sugBtns = suggestions
    ? [...suggestions.querySelectorAll("button")].slice(0, 4).map(box)
    : [];

  return {
    rootClass: root.className,
    pathish,
    shell: box(shell),
    roleMains,
    homeClasses,
    anchors: {
      homeIcon: box(homeIcon),
      gameSource: box(gameSource),
      gameSurface: box(gameSurface),
      suggestions: box(suggestions),
      composer: box(composer),
      header: box(header),
    },
    heroChain,
    sugBtns,
    headerBtns,
    createBtns,
    nav,
    stickies,
    host: window.__CHATGPT_TOOLS_SKIN_HOST__
      ? {
          hasHost: true,
          keys: Object.keys(window.__CHATGPT_TOOLS_SKIN_HOST__).slice(0, 30),
        }
      : { hasHost: false },
  };
})()`;

const r = await send("Runtime.evaluate", {
  expression: expr,
  returnByValue: true,
});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
