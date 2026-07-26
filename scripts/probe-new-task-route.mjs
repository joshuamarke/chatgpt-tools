/**
 * Navigate to 新建任务 (or inspect current home-like route) and dump DOM markers.
 * Usage: node scripts/probe-new-task-route.mjs [click|stay]
 *   click (default) — click sidebar 新建任务 then probe
 *   stay — probe current page only
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const mode = process.argv[2] || "click";
const OUT = join(dirname(fileURLToPath(import.meta.url)), "probe-new-task-route-out.json.txt");

const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find((p) => p.type === "page") || pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.log("no cdp");
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

if (mode === "click") {
  const click = await send("Runtime.evaluate", {
    returnByValue: true,
    expression: `(() => {
      const items = [...document.querySelectorAll(
        'aside a, aside button, nav a, nav button, [class*="sidebar"] a, [class*="sidebar"] button'
      )];
      const target = items.find((el) => {
        const t = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || ""))
          .replace(/\\s+/g, " ")
          .trim();
        return t === "新建任务" || t.startsWith("新建任务");
      });
      if (!target) {
        return {
          ok: false,
          candidates: items.slice(0, 20).map((el) => ({
            t: ((el.innerText || el.getAttribute("aria-label") || "") + "").slice(0, 40),
            tag: el.tagName,
          })),
        };
      }
      target.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window }));
      if (typeof target.click === "function") target.click();
      return {
        ok: true,
        text: (target.innerText || target.getAttribute("aria-label") || "").slice(0, 40),
        cls: String(target.className).slice(0, 140),
      };
    })()`,
  });
  console.log("CLICK", JSON.stringify(click.result?.result?.value, null, 2));
  await new Promise((r) => setTimeout(r, 1800));
}

const probeExpr = `(() => {
  const box = (el) => {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return {
      tag: el.tagName,
      role: el.getAttribute("role"),
      testid: el.getAttribute("data-testid"),
      feature: el.getAttribute("data-feature"),
      cls: String(el.className || "").slice(0, 220),
      text: (el.innerText || "").replace(/\\s+/g, " ").trim().slice(0, 80),
      w: Math.round(r.width),
      h: Math.round(r.height),
      x: Math.round(r.x),
      y: Math.round(r.y),
      color: cs.color,
      bg: cs.backgroundColor,
      bgImg: (cs.backgroundImage || "").slice(0, 100),
      radius: cs.borderRadius,
    };
  };

  const shell = document.querySelector("main.main-surface");
  const homeIcon = document.querySelector('[data-testid="home-icon"]');
  const gameSource = document.querySelector('[data-feature="game-source"]');
  const gameSurface = document.querySelector('[data-feature="game-surface"]');
  const suggestions =
    document.querySelector(".group\\\\/home-suggestions") ||
    document.querySelector('[class*="home-suggestions"]');
  const roleMain = document.querySelector('[role="main"]');
  const homeMainContent = document.querySelector('[class*="home-main-content"]');
  const composer = document.querySelector(".composer-surface-chrome");
  const header = document.querySelector("header.app-header-tint");

  // How would current renderer-core resolve home?
  const queryHomeAnchor = () =>
    document.querySelector('[data-testid="home-icon"]') ||
    document.querySelector('[data-feature="game-source"]') ||
    document.querySelector(".group\\\\/home-suggestions") ||
    document.querySelector('[class*="home-suggestions"]') ||
    null;
  const queryHomeRoute = (homeAnchor) =>
    homeAnchor?.closest('[role="main"]') ||
    document.querySelector('[role="main"]:has([data-testid="home-icon"])') ||
    document.querySelector('[role="main"]:has([data-feature="game-source"])') ||
    document.querySelector('[role="main"]:has([class*="home-suggestions"])') ||
    document.querySelector('[role="main"][class*="home-main-content"]') ||
    null;
  const anchor = queryHomeAnchor();
  const resolvedHome = queryHomeRoute(anchor);

  // Proposed broader resolvers
  const closestMainish = (el) => {
    if (!el) return null;
    return (
      el.closest('[role="main"]') ||
      el.closest('[class*="home-main-content"]') ||
      el.closest(".app-shell-main-content-frame") ||
      el.closest("main.main-surface > div") ||
      el.closest("main") ||
      null
    );
  };

  const homeClasses = [...document.querySelectorAll("[class]")]
    .filter((el) =>
      /(?:^|\\s)(?:dream|mortal|qingkong|jiuyi|linglong|cyberpunk|eva|bengong|miku|cn|skin)-home(?:-shell|-utility)?(?:\\s|$)/.test(
        String(el.className)
      )
    )
    .map((el) => ({
      tag: el.tagName,
      cls: [...el.classList].filter((c) => /home/.test(c)).join(" "),
      role: el.getAttribute("role"),
    }));

  // shallow tree under shell
  const tree = [];
  const walk = (el, depth) => {
    if (!el || depth > 4) return;
    const r = el.getBoundingClientRect();
    if (depth > 0 && (r.width < 40 || r.height < 16)) return;
    tree.push({
      depth,
      tag: el.tagName,
      role: el.getAttribute("role"),
      testid: el.getAttribute("data-testid"),
      feature: el.getAttribute("data-feature"),
      cls: String(el.className || "").slice(0, 180),
      w: Math.round(r.width),
      h: Math.round(r.height),
      kids: el.children.length,
    });
    for (const c of [...el.children].slice(0, 10)) walk(c, depth + 1);
  };
  if (shell) walk(shell, 0);

  const chainOf = (el) => {
    if (!el) return null;
    const chain = [];
    let n = el;
    for (let i = 0; n && i < 10; i++) {
      chain.push({
        tag: n.tagName,
        role: n.getAttribute("role"),
        testid: n.getAttribute("data-testid"),
        feature: n.getAttribute("data-feature"),
        cls: String(n.className || "").slice(0, 160),
      });
      n = n.parentElement;
    }
    return chain;
  };

  // suggestion button styles if present
  const sugBtns = suggestions
    ? [...suggestions.querySelectorAll("button")].slice(0, 3).map(box)
    : [];

  // header create-like buttons
  const headerBtns = header
    ? [...header.querySelectorAll("button, a, [role='button']")].slice(0, 15).map((el) => {
        const cs = getComputedStyle(el);
        return {
          text: (el.innerText || el.getAttribute("aria-label") || "")
            .replace(/\\s+/g, " ")
            .trim()
            .slice(0, 60),
          aria: el.getAttribute("aria-label"),
          color: cs.color,
          bg: cs.backgroundColor,
          cls: String(el.className || "").slice(0, 160),
        };
      })
    : [];

  // create buttons anywhere (top-right area)
  const createBtns = [...document.querySelectorAll("button, a, [role='button']")]
    .filter((el) => {
      const t = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || "")).trim();
      return /创建|Create|新建/.test(t) && el.getClientRects().length > 0;
    })
    .slice(0, 16)
    .map((el) => {
      const cs = getComputedStyle(el);
      const r = el.getBoundingClientRect();
      return {
        text: (el.innerText || el.getAttribute("aria-label") || "")
          .replace(/\\s+/g, " ")
          .trim()
          .slice(0, 80),
        aria: el.getAttribute("aria-label"),
        color: cs.color,
        bg: cs.backgroundColor,
        border: cs.borderColor,
        x: Math.round(r.x),
        y: Math.round(r.y),
        w: Math.round(r.width),
        h: Math.round(r.height),
        inHeader: Boolean(header && header.contains(el)),
        cls: String(el.className || "").slice(0, 180),
      };
    });

  const st = window.__CODEX_SKIN_STATE__;
  const host = window.__CHATGPT_TOOLS_SKIN_HOST__;

  // Does shell itself contain home anchors?
  const shellContainsHome = shell
    ? Boolean(
        shell.querySelector('[data-testid="home-icon"]') ||
          shell.querySelector('[data-feature="game-source"]') ||
          shell.querySelector('[class*="home-suggestions"]')
      )
    : false;

  return {
    root: document.documentElement.className,
    title: document.title,
    shell: box(shell),
    shellHomeClasses: shell ? [...shell.classList].filter((c) => /home/.test(c)) : [],
    shellContainsHome,
    anchors: {
      homeIcon: box(homeIcon),
      gameSource: box(gameSource),
      gameSurface: box(gameSurface),
      suggestions: box(suggestions),
      roleMain: box(roleMain),
      homeMainContent: box(homeMainContent),
      composer: box(composer),
      header: box(header),
    },
    resolver: {
      hasAnchor: Boolean(anchor),
      resolvedHomeTag: resolvedHome?.tagName || null,
      resolvedHomeRole: resolvedHome?.getAttribute?.("role") || null,
      resolvedHomeCls: resolvedHome ? String(resolvedHome.className).slice(0, 160) : null,
      proposedClosest: box(closestMainish(anchor)),
      // if no role=main, would shell work as home container?
      shellAsHome: shellContainsHome,
    },
    homeClasses,
    tree: tree.slice(0, 50),
    chains: {
      homeIcon: chainOf(homeIcon),
      gameSource: chainOf(gameSource),
      suggestions: chainOf(suggestions),
    },
    sugBtns,
    headerBtns,
    createBtns,
    life: st
      ? {
          lifeMode: st.lifeMode,
          metrics: st.metrics,
          markers: st.markers
            ? {
                homeClass: st.markers.homeClass,
                homeShellClass: st.markers.homeShellClass,
                rootClass: st.markers.rootClass,
              }
            : null,
        }
      : null,
    hostKeys: host ? Object.keys(host).slice(0, 40) : [],
  };
})()`;

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: probeExpr,
});
const value = r.result?.result?.value ?? r.result;
const text = JSON.stringify(value, null, 2);
writeFileSync(OUT, text, "utf8");
console.log(text);
ws.close();
