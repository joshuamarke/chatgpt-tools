/**
 * Probe bengong chrome / brand positioning vs host shell after inject.
 * Usage: node scripts/probe-bengong-chrome.mjs [port=9335]
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";

const port = process.argv[2] || "9335";
const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page =
  pages.find((p) => p.type === "page" && !String(p.url || "").includes("avatar")) ||
  pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.log("no cdp page");
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

await send("Runtime.evaluate", {
  expression: `(() => {
    try {
      window.__CHATGPT_TOOLS_SKIN_HOST__?.ensure?.({ root: true, route: true, layout: true });
    } catch (e) {}
    return true;
  })()`,
});
await new Promise((r) => setTimeout(r, 500));

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `(() => {
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return {
        tag: el.tagName,
        id: el.id || null,
        cls:
          typeof el.className === "string"
            ? el.className.slice(0, 180)
            : [...(el.classList || [])].join(" ").slice(0, 180),
        parent: el.parentElement
          ? el.parentElement.id ||
            el.parentElement.tagName +
              "." +
              [...el.parentElement.classList].slice(0, 4).join(".")
          : null,
        rect: {
          x: Math.round(r.x),
          y: Math.round(r.y),
          w: Math.round(r.width),
          h: Math.round(r.height),
        },
        pos: cs.position,
        display: cs.display,
        visibility: cs.visibility,
        opacity: cs.opacity,
        z: cs.zIndex,
        top: cs.top,
        left: cs.left,
        right: cs.right,
        bottom: cs.bottom,
        width: cs.width,
        height: cs.height,
        overflow: cs.overflow,
        bgImage: (cs.backgroundImage || "").slice(0, 90),
        transform: cs.transform,
        pointerEvents: cs.pointerEvents,
      };
    };

    const root = document.documentElement;
    const shell = document.querySelector("main.main-surface");
    const roleMain = document.querySelector('[role="main"]');
    const header = document.querySelector("header.app-header-tint");
    const aside = document.querySelector("aside.app-shell-left-panel, aside");
    const chrome = document.getElementById("codex-bengong-skin-chrome");
    const brand = document.querySelector(".bengong-brand");
    const blossoms = document.querySelector(".bengong-blossoms");
    const ornament = document.querySelector(".bengong-ornament");
    const game = document.querySelector('[data-feature="game-source"]');
    const home = document.querySelector(".bengong-home");
    const homeIcon = document.querySelector('[data-testid="home-icon"]');
    const suggestions = document.querySelector('[class*="home-suggestions"]');

    const chain = [];
    let n = brand;
    for (let i = 0; n && i < 8; i++) {
      const cs = getComputedStyle(n);
      const rr = n.getBoundingClientRect();
      chain.push({
        tag: n.tagName,
        id: n.id || null,
        cls:
          [...(n.classList || [])]
            .filter((c) => /bengong|chrome|home|shell|main|surface/i.test(c))
            .join(" ") || String(n.className || "").slice(0, 100),
        pos: cs.position,
        display: cs.display,
        rect: {
          x: Math.round(rr.x),
          y: Math.round(rr.y),
          w: Math.round(rr.width),
          h: Math.round(rr.height),
        },
        left: cs.left,
        top: cs.top,
      });
      n = n.parentElement;
    }

    let homeTree = null;
    if (home) {
      const walk = (el, depth = 0) => {
        if (!el || depth > 4) return null;
        const kids = [...el.children]
          .slice(0, 6)
          .map((c) => walk(c, depth + 1))
          .filter(Boolean);
        const rr = el.getBoundingClientRect();
        return {
          tag: el.tagName,
          cls: [...el.classList].slice(0, 8).join(" "),
          testid: el.getAttribute("data-testid"),
          feature: el.getAttribute("data-feature"),
          rect: {
            x: Math.round(rr.x),
            y: Math.round(rr.y),
            w: Math.round(rr.width),
            h: Math.round(rr.height),
          },
          kids,
        };
      };
      homeTree = walk(home);
    }

    const chromeInline = chrome
      ? {
          styleAttr: chrome.getAttribute("style"),
          left: chrome.style.left,
          top: chrome.style.top,
          width: chrome.style.width,
          height: chrome.style.height,
          className: chrome.className,
          childCount: chrome.children.length,
          childClasses: [...chrome.children].map((c) => c.className),
          htmlHead: chrome.innerHTML.slice(0, 360),
        }
      : null;

    let chromeBefore = null;
    if (chrome) {
      const ps = getComputedStyle(chrome, "::before");
      chromeBefore = {
        content: ps.content,
        display: ps.display,
        pos: ps.position,
        w: ps.width,
        h: ps.height,
        bgImage: (ps.backgroundImage || "").slice(0, 120),
        bgPos: ps.backgroundPosition,
        bgSize: ps.backgroundSize,
        opacity: ps.opacity,
        z: ps.zIndex,
      };
    }

    const chromeCs = chrome ? getComputedStyle(chrome) : null;

    const art = {
      mode: root.getAttribute("data-skins-art-mode"),
      paint: root.getAttribute("data-skins-art-paint"),
      contract: root.getAttribute("data-skin-contract"),
      skin: root.getAttribute("data-chatgpt-tools-skin"),
      rootClasses: [...root.classList].filter((c) =>
        /skin|bengong|art|theme|safe|task|focus/i.test(c)
      ),
      bengongArt: getComputedStyle(root).getPropertyValue("--bengong-art").slice(0, 80),
      skinsArt: getComputedStyle(root).getPropertyValue("--skins-art").slice(0, 80),
      artPos: getComputedStyle(root).getPropertyValue("--skins-art-position"),
    };

    const align = {
      brandLeftVsShell:
        brand && shell
          ? Math.round(brand.getBoundingClientRect().left - shell.getBoundingClientRect().left)
          : null,
      brandTopVsShell:
        brand && shell
          ? Math.round(brand.getBoundingClientRect().top - shell.getBoundingClientRect().top)
          : null,
      brandLeftVsChrome:
        brand && chrome
          ? Math.round(brand.getBoundingClientRect().left - chrome.getBoundingClientRect().left)
          : null,
      brandTopVsChrome:
        brand && chrome
          ? Math.round(brand.getBoundingClientRect().top - chrome.getBoundingClientRect().top)
          : null,
      brandViewport: brand
        ? {
            x: Math.round(brand.getBoundingClientRect().left),
            y: Math.round(brand.getBoundingClientRect().top),
          }
        : null,
      chromeEqualsShell:
        chrome && shell
          ? (() => {
              const a = chrome.getBoundingClientRect();
              const b = shell.getBoundingClientRect();
              return {
                leftDiff: Math.round(a.left - b.left),
                topDiff: Math.round(a.top - b.top),
                wDiff: Math.round(a.width - b.width),
                hDiff: Math.round(a.height - b.height),
                chrome: {
                  x: Math.round(a.left),
                  y: Math.round(a.top),
                  w: Math.round(a.width),
                  h: Math.round(a.height),
                },
                shell: {
                  x: Math.round(b.left),
                  y: Math.round(b.top),
                  w: Math.round(b.width),
                  h: Math.round(b.height),
                },
              };
            })()
          : null,
      diagnosis: null,
    };

    // Detect inset:0 vs engine geometry conflict
    if (chrome && chromeCs) {
      const hasInlineBox =
        Boolean(chrome.style.left) &&
        Boolean(chrome.style.top) &&
        Boolean(chrome.style.width) &&
        Boolean(chrome.style.height);
      const computedFullViewport =
        Math.round(chrome.getBoundingClientRect().width) >= innerWidth - 2 &&
        Math.round(chrome.getBoundingClientRect().left) <= 1;
      align.diagnosis = {
        hasInlineBox,
        computedFullViewport,
        conflictLikely:
          hasInlineBox &&
          computedFullViewport &&
          shell &&
          Math.round(shell.getBoundingClientRect().left) > 20,
        note: hasInlineBox
          ? "Engine sets left/top/width/height to main.main-surface box; CSS inset:0 can force full-viewport containing block and break brand absolute offsets."
          : "No engine inline geometry yet — chrome may not be synced to shell.",
      };
    }

    const gameText = game
      ? (game.innerText || "").replace(/\\s+/g, " ").trim().slice(0, 140)
      : null;

    const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
    const stateKey = Object.keys(window).find(
      (k) => k.includes("BENGONG") && k.includes("STATE")
    );
    const state = stateKey ? window[stateKey] : null;

    return {
      viewport: { w: innerWidth, h: innerHeight },
      art,
      host: host
        ? {
            hasEnsure: typeof host.ensure === "function",
            lifeMode: state?.lifeMode,
            markers: state?.markers || host.markers || null,
          }
        : null,
      boxes: {
        shell: box(shell),
        roleMain: box(roleMain),
        header: box(header),
        aside: box(aside),
        chrome: box(chrome),
        brand: box(brand),
        blossoms: box(blossoms),
        ornament: box(ornament),
        game: box(game),
        home: box(home),
        homeIcon: box(homeIcon),
        suggestions: box(suggestions),
      },
      chromeInline,
      chromeBefore,
      chromeComputed: chromeCs
        ? {
            position: chromeCs.position,
            top: chromeCs.top,
            left: chromeCs.left,
            right: chromeCs.right,
            bottom: chromeCs.bottom,
            width: chromeCs.width,
            height: chromeCs.height,
          }
        : null,
      brandChain: chain,
      align,
      gameText,
      homeTree,
    };
  })()`,
});

const val = r.result?.result?.value ?? r.result;
const out = "scripts/probe-bengong-chrome-out.json.txt";
writeFileSync(out, JSON.stringify(val, null, 2), "utf8");
console.log(JSON.stringify(val, null, 2));
console.log("wrote", out);
ws.close();
