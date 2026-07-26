/**
 * Inspect skin host/state and home class health; optional route click.
 * Usage: node scripts/probe-skin-state.mjs [stay|new-task|scheduled|plugins]
 */
import WebSocket from "ws";

const mode = process.argv[2] || "stay";
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

const clickLabel =
  mode === "new-task"
    ? "新建任务"
    : mode === "scheduled"
      ? "已安排"
      : mode === "plugins"
        ? "插件"
        : null;

if (clickLabel) {
  const click = await send("Runtime.evaluate", {
    returnByValue: true,
    expression: `(() => {
      const items = [...document.querySelectorAll("aside a, aside button, nav a, nav button")];
      const target = items.find((el) => {
        const t = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || ""))
          .replace(/\\s+/g, " ")
          .trim();
        return t === ${JSON.stringify(clickLabel)} || t.startsWith(${JSON.stringify(clickLabel)});
      });
      if (!target) return { ok: false, label: ${JSON.stringify(clickLabel)} };
      target.click();
      return { ok: true, text: (target.innerText || "").slice(0, 40) };
    })()`,
  });
  console.log("CLICK", JSON.stringify(click.result?.result?.value));
  await new Promise((r) => setTimeout(r, 2000));
}

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `(() => {
    const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
    const keys = Object.keys(window).filter((k) => /SKIN|STATE|DISABLED/i.test(k));
    const states = {};
    for (const k of keys) {
      const v = window[k];
      if (v && typeof v === "object" && (v.lifeMode || v.markers || v.installToken)) {
        states[k] = {
          lifeMode: v.lifeMode,
          homeClass: v.markers?.homeClass,
          homeShellClass: v.markers?.homeShellClass,
          rootClass: v.markers?.rootClass,
          metrics: v.metrics,
        };
      }
    }

    // force ensure if available
    let ensureErr = null;
    try {
      host?.ensure?.({ root: true, route: true, layout: true });
    } catch (e) {
      ensureErr = String(e && e.message ? e.message : e);
    }

    const roleMain = document.querySelector('[role="main"]');
    const shell = document.querySelector("main.main-surface");
    const homeIcon = document.querySelector('[data-testid="home-icon"]');
    const gameSource = document.querySelector('[data-feature="game-source"]');
    const suggestions = document.querySelector('[class*="home-suggestions"]');
    const header = document.querySelector("header.app-header-tint");

    const homeEls = [...document.querySelectorAll("[class]")]
      .filter((el) =>
        /(?:^|\\s)[\\w-]*(?:-home(?:-shell|-utility)?|home-shell|home-utility)(?:\\s|$)/.test(
          String(el.className)
        )
      )
      .slice(0, 30)
      .map((el) => ({
        tag: el.tagName,
        role: el.getAttribute("role"),
        cls: [...el.classList].filter((c) => /home|shell|utility/.test(c)).join(" "),
      }));

    // create / 创建 buttons top-right
    const createBtns = [...document.querySelectorAll("button, a, [role='button']")]
      .filter((el) => {
        const t = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || "")).trim();
        return /创建|Create|新建自动化|新建插件|新建/.test(t) && el.getClientRects().length > 0;
      })
      .map((el) => {
        const cs = getComputedStyle(el);
        const r = el.getBoundingClientRect();
        return {
          text: (el.innerText || el.getAttribute("aria-label") || "")
            .replace(/\\s+/g, " ")
            .trim()
            .slice(0, 60),
          aria: el.getAttribute("aria-label"),
          color: cs.color,
          bg: cs.backgroundColor,
          border: cs.borderColor + " / " + cs.borderWidth,
          opacity: cs.opacity,
          x: Math.round(r.x),
          y: Math.round(r.y),
          w: Math.round(r.width),
          h: Math.round(r.height),
          inHeader: Boolean(header && header.contains(el)),
          cls: String(el.className || "").slice(0, 160),
        };
      })
      .filter((b) => b.y < 120 || b.inHeader)
      .slice(0, 12);

    // sticky bars text
    const stickies = [...document.querySelectorAll("div.sticky, header")]
      .filter((el) => el.getClientRects().length > 0)
      .slice(0, 6)
      .map((el) => {
        const cs = getComputedStyle(el);
        const r = el.getBoundingClientRect();
        return {
          text: (el.innerText || "").replace(/\\s+/g, " ").trim().slice(0, 100),
          color: cs.color,
          bg: cs.backgroundColor,
          y: Math.round(r.y),
          h: Math.round(r.height),
          cls: String(el.className || "").slice(0, 140),
        };
      });

    const sugBtn = suggestions?.querySelector("button");
    const sugStyle = sugBtn
      ? {
          color: getComputedStyle(sugBtn).color,
          bg: getComputedStyle(sugBtn).backgroundColor,
          shadow: getComputedStyle(sugBtn).boxShadow.slice(0, 100),
        }
      : null;

    return {
      root: document.documentElement.className,
      keys,
      states,
      active: host?.getActive?.() || null,
      ensureErr,
      hasHomeIcon: Boolean(homeIcon),
      hasGameSource: Boolean(gameSource),
      hasSuggestions: Boolean(suggestions),
      roleMainHomeCls: roleMain
        ? [...roleMain.classList].filter((c) => /home/.test(c))
        : null,
      shellHomeCls: shell ? [...shell.classList].filter((c) => /home/.test(c)) : null,
      homeEls,
      sugStyle,
      heroColor: gameSource ? getComputedStyle(gameSource).color : null,
      createBtns,
      stickies,
      headerColor: header ? getComputedStyle(header).color : null,
      headerBg: header ? getComputedStyle(header).backgroundColor : null,
    };
  })()`,
});

console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
