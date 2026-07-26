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
  const pick = (el, depth = 0) => {
    if (!el) return null;
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      tag: el.tagName,
      role: el.getAttribute("role"),
      testid: el.getAttribute("data-testid"),
      feature: el.getAttribute("data-feature"),
      cls: String(el.className || "").slice(0, 280),
      bg: cs.backgroundColor,
      bgImg: (cs.backgroundImage || "").slice(0, 100),
      color: cs.color,
      border: cs.border,
      backdrop: cs.backdropFilter || cs.webkitBackdropFilter,
      filter: cs.filter,
      pos: cs.position,
      display: cs.display,
      opacity: cs.opacity,
      w: Math.round(r.width),
      h: Math.round(r.height),
      y: Math.round(r.top),
      x: Math.round(r.left),
      text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 80),
    };
  };

  const chain = (el, n = 8) => {
    const out = [];
    let cur = el;
    for (let i = 0; i < n && cur; i++) {
      out.push(pick(cur));
      cur = cur.parentElement;
    }
    return out;
  };

  const root = document.documentElement;
  const main = document.querySelector("main.main-surface") || document.querySelector("main");
  const roleMain = document.querySelector('[role="main"]');
  const home = document.querySelector(".jiuyi-home") || document.querySelector(".dream-home") || roleMain;

  // mode switch labels
  const modeBtns = [...document.querySelectorAll("button")].filter((b) =>
    /Codex|Worker|切换模式|模式/.test(b.getAttribute("aria-label") || b.innerText || "")
  ).slice(0, 12).map((b) => ({
    label: (b.getAttribute("aria-label") || b.innerText || "").replace(/\\s+/g, " ").slice(0, 80),
    pressed: b.getAttribute("aria-pressed"),
    state: b.getAttribute("data-state"),
    cls: String(b.className || "").slice(0, 120),
  }));

  const hero =
    document.querySelector('[data-feature="game-source"]') ||
    document.querySelector('[data-testid="home-icon"]')?.parentElement ||
    document.querySelector("h1") ||
    document.querySelector('[class*="empty-state"]');

  const sug =
    document.querySelector(".group\\\\/home-suggestions") ||
    document.querySelector('[class*="home-suggestions"]');

  const project =
    document.querySelector(".group\\\\/project-selector") ||
    document.querySelector('[class*="project-selector"]') ||
    document.querySelector('[aria-label*="项目"]');

  const fadeMask = document.querySelector(".horizontal-scroll-fade-mask");
  const utility =
    document.querySelector('[class*="homeUtilityBar"]') ||
    document.querySelector(".jiuyi-home-utility") ||
    document.querySelector(".dream-home-utility");

  const composer = document.querySelector(".composer-surface-chrome");

  // Find all elements with project-related classes near composer
  const nearComposer = composer
    ? [...composer.parentElement?.querySelectorAll("div, section, button") || []].slice(0, 40).map((el) => ({
        tag: el.tagName,
        cls: String(el.className || "").slice(0, 160),
        text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 40),
        bg: getComputedStyle(el).backgroundColor,
      }))
    : [];

  // Any missing class markers
  const markers = {
    hasJiuyiRoot: root.classList.contains("codex-jiuyi-skin"),
    rootClass: root.className,
    mainClass: main ? String(main.className).slice(0, 200) : null,
    roleMainClass: roleMain ? String(roleMain.className).slice(0, 200) : null,
    hasJiuyiHome: !!document.querySelector(".jiuyi-home"),
    hasJiuyiHomeShell: !!document.querySelector(".jiuyi-home-shell") || main?.classList.contains("jiuyi-home-shell"),
    hasDreamHome: !!document.querySelector(".dream-home"),
  };

  // Host native styles on cards without skin (computed now with skin)
  const sugBtns = sug
    ? [...sug.querySelectorAll("button")].slice(0, 4).map((b) => ({
        ...pick(b),
        matchesJiuyiCard: b.matches?.('.jiuyi-home .group\\\\/home-suggestions button') || false,
        classList: String(b.className).slice(0, 220),
      }))
    : [];

  // project selector parent for :has rule
  const projectParent = project?.parentElement;
  const projectGrand = projectParent?.parentElement;
  const projectHasMask = !!(
    project?.closest(".horizontal-scroll-fade-mask") ||
    projectParent?.querySelector?.(".horizontal-scroll-fade-mask") ||
    projectGrand?.querySelector?.(".horizontal-scroll-fade-mask")
  );

  return {
    markers,
    modeBtns,
    hero: pick(hero),
    heroChain: chain(hero, 6),
    sug: pick(sug),
    sugBtns,
    sugChain: chain(sug, 6),
    project: pick(project),
    projectHtml: project ? project.outerHTML.slice(0, 500) : null,
    projectChain: chain(project, 8),
    projectHasMask,
    projectParent: pick(projectParent),
    projectGrand: pick(projectGrand),
    fadeMask: pick(fadeMask),
    utility: pick(utility),
    composer: pick(composer),
    nearComposer: nearComposer.filter((n) => /project|selector|scroll|utility|home/i.test(n.cls + n.text)).slice(0, 20),
    // body text sample to detect mode copy
    bodySample: (document.body?.innerText || "").replace(/\\s+/g, " ").slice(0, 200),
  };
})()`;

const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
