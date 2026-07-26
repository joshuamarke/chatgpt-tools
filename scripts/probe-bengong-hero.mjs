/**
 * Probe bengong hero title shell + suggestion rail geometry.
 * Usage: node scripts/probe-bengong-hero.mjs [port=9335]
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";

const port = process.argv[2] || "9335";
const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page =
  pages.find((p) => p.type === "page" && !String(p.url || "").includes("avatar")) ||
  pages[0];
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
await send("Runtime.evaluate", {
  expression: `(()=>{try{window.__CHATGPT_TOOLS_SKIN_HOST__?.ensure?.({root:true,route:true,layout:true})}catch(e){}return 1})()`,
});
await new Promise((r) => setTimeout(r, 400));

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `(() => {
    const rect = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return {
        tag: el.tagName,
        cls: [...el.classList].slice(0, 8).join(" "),
        rect: { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height) },
        pos: cs.position,
        overflow: cs.overflow,
        overflowX: cs.overflowX,
        overflowY: cs.overflowY,
        display: cs.display,
        height: cs.height,
        minHeight: cs.minHeight,
        maxHeight: cs.maxHeight,
        bgImage: (cs.backgroundImage || "").slice(0, 80),
        z: cs.zIndex,
      };
    };

    const home = document.querySelector(".bengong-home");
    const game = document.querySelector('[data-feature="game-source"]');
    const suggestions = document.querySelector(".group\\\\/home-suggestions, [class*='home-suggestions']");
    const shell =
      (game && game.closest("div.relative")) ||
      null;
    const rail = shell?.querySelector(":scope > div.absolute") || null;
    const cards = suggestions
      ? [...suggestions.querySelectorAll("button")].slice(0, 6).map((b) => {
          const r = b.getBoundingClientRect();
          return {
            text: (b.innerText || "").replace(/\\s+/g, " ").trim().slice(0, 40),
            listItem: /home-suggestion-list-item/.test(b.className),
            rect: { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height) },
            display: getComputedStyle(b).display,
            visibility: getComputedStyle(b).visibility,
            opacity: getComputedStyle(b).opacity,
          };
        })
      : [];

    // chain from game-source up
    const chain = [];
    let n = game;
    for (let i = 0; n && i < 10; i++) {
      chain.push(rect(n));
      n = n.parentElement;
    }

    const chrome = document.getElementById("codex-bengong-skin-chrome");
    const brand = document.querySelector(".bengong-brand");
    const main = document.querySelector("main.main-surface");

    // computed pseudo on shell
    let before = null;
    let after = null;
    if (shell) {
      const b = getComputedStyle(shell, "::before");
      const a = getComputedStyle(shell, "::after");
      before = { content: b.content, pos: b.position, w: b.width, h: b.height, bg: (b.backgroundImage||"").slice(0,80), z: b.zIndex };
      after = { content: a.content, pos: a.position, w: a.width, h: a.height, bg: (a.backgroundImage||"").slice(0,80), z: a.zIndex };
    }

    const gap =
      shell && suggestions
        ? Math.round(suggestions.getBoundingClientRect().top - shell.getBoundingClientRect().bottom)
        : null;

    return {
      root: {
        mode: document.documentElement.getAttribute("data-skins-art-mode"),
        paint: document.documentElement.getAttribute("data-skins-art-paint"),
        classes: [...document.documentElement.classList].filter((c) => /bengong|art|skin/i.test(c)),
        art: getComputedStyle(document.documentElement).getPropertyValue("--bengong-art").slice(0, 60),
      },
      main: rect(main),
      home: rect(home),
      shell: rect(shell),
      rail: rect(rail),
      suggestions: rect(suggestions),
      game: rect(game),
      cards,
      gapShellToCards: gap,
      chrome: rect(chrome),
      chromeInline: chrome
        ? { style: chrome.getAttribute("style"), left: chrome.style.left, top: chrome.style.top, w: chrome.style.width, h: chrome.style.height }
        : null,
      brand: rect(brand),
      before,
      after,
      chain,
      gameText: game ? (game.innerText || "").replace(/\\s+/g, " ").trim().slice(0, 100) : null,
    };
  })()`,
});

const val = r.result?.result?.value ?? r.result;
writeFileSync("scripts/probe-bengong-hero-out.json.txt", JSON.stringify(val, null, 2));
console.log(JSON.stringify(val, null, 2));
ws.close();
