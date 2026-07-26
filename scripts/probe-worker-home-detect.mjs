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
  const qa = (s) => [...document.querySelectorAll(s)];
  const cls = (el) => (el ? String(el.className || "").slice(0, 220) : null);
  const pick = (el) => {
    if (!el) return null;
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      tag: el.tagName,
      cls: cls(el),
      bg: cs.backgroundColor,
      bgImg: (cs.backgroundImage || "").slice(0, 80),
      y: Math.round(r.top),
      h: Math.round(r.height),
      w: Math.round(r.width),
      text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 60),
    };
  };

  const homeIcon = q('[data-testid="home-icon"]');
  const gameSource = q('[data-feature="game-source"]');
  const gameSurface = q('[data-feature="game-surface"]');
  const sug = q('.group\\\\/home-suggestions');
  const roleMain = q('[role="main"]');
  const composer = q(".composer-surface-chrome");

  // Engine home detection simulation
  const homeByIcon =
    homeIcon?.closest('[role="main"]') ||
    q('[role="main"]:has([data-testid="home-icon"])') ||
    null;
  const homeByGame =
    gameSource?.closest('[role="main"]') ||
    q('[role="main"]:has([data-feature="game-source"])') ||
    null;
  const homeBySug =
    sug?.closest('[role="main"]') ||
    q('[role="main"]:has(.group\\\\/home-suggestions)') ||
    null;

  // Find "选择项目" / project selector near composer (not sidebar)
  const allText = qa("div, section, span, button").filter((el) => {
    const t = (el.innerText || "").trim();
    return t === "选择项目" || t.startsWith("选择项目") || /选择项目/.test(t) && t.length < 20;
  }).slice(0, 10).map((el) => ({
    tag: el.tagName,
    cls: cls(el),
    text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 40),
    y: Math.round(el.getBoundingClientRect().top),
    inComposer: !!el.closest(".composer-surface-chrome") || !!el.closest('[class*="composer"]'),
    ancestors: (() => {
      const a = [];
      let n = el;
      for (let i = 0; i < 6 && n; i++) {
        a.push(n.tagName + "." + String(n.className || "").split(" ").slice(0, 4).join("."));
        n = n.parentElement;
      }
      return a;
    })(),
  }));

  // group/project-selector instances
  const projectGroups = qa('.group\\\\/project-selector, [class*="project-selector"]').map((el) => {
    const r = el.getBoundingClientRect();
    return {
      tag: el.tagName,
      cls: cls(el),
      y: Math.round(r.top),
      x: Math.round(r.left),
      w: Math.round(r.width),
      h: Math.round(r.height),
      text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 80),
      parentCls: cls(el.parentElement),
      grandCls: cls(el.parentElement?.parentElement),
      hasMaskParent: !!el.closest(".horizontal-scroll-fade-mask"),
      nearComposer: !!composer && Math.abs(r.top - composer.getBoundingClientRect().top) < 200,
      inAside: !!el.closest("aside"),
      html: el.outerHTML.slice(0, 300),
    };
  });

  // horizontal-scroll-fade-mask instances
  const masks = qa(".horizontal-scroll-fade-mask").map((el) => ({
    cls: cls(el),
    parentCls: cls(el.parentElement),
    y: Math.round(el.getBoundingClientRect().top),
    h: Math.round(el.getBoundingClientRect().height),
    hasProject: !!el.querySelector('.group\\\\/project-selector, [class*="project-selector"]'),
    text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 80),
    inAside: !!el.closest("aside"),
  }));

  // suggestion card computed with/without ancestor .jiuyi-home
  const btn = sug?.querySelector("button");
  const btnCs = btn ? getComputedStyle(btn) : null;

  // mode
  const modeLabel =
    qa("button")
      .map((b) => b.getAttribute("aria-label") || "")
      .find((t) => /切换模式|Codex|Work/.test(t)) || null;

  return {
    modeLabel,
    markers: {
      hasJiuyiRoot: document.documentElement.classList.contains("codex-jiuyi-skin"),
      hasJiuyiHome: !!q(".jiuyi-home"),
      hasJiuyiHomeShell: !!q(".jiuyi-home-shell") || document.querySelector("main")?.classList.contains("jiuyi-home-shell"),
      homeIcon: !!homeIcon,
      gameSource: !!gameSource,
      gameSurface: !!gameSurface,
      sug: !!sug,
      roleMain: !!roleMain,
    },
    engineDetect: {
      homeByIcon: !!homeByIcon,
      homeByGame: !!homeByGame,
      homeBySug: !!homeBySug,
    },
    homeIconHtml: homeIcon ? homeIcon.outerHTML.slice(0, 250) : null,
    gameSource: pick(gameSource),
    sug: pick(sug),
    sugBtn: btn
      ? {
          bg: btnCs.backgroundColor,
          bgImg: (btnCs.backgroundImage || "").slice(0, 120),
          backdrop: btnCs.backdropFilter,
          border: btnCs.border,
          cls: cls(btn),
        }
      : null,
    composer: pick(composer),
    projectGroups,
    masks,
    chooseProjectTexts: allText,
    roleMainCls: cls(roleMain),
  };
})()`;

const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
