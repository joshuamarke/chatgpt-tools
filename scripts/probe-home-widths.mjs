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
  const root = document.documentElement;
  const roleMain = document.querySelector('[role="main"]');
  const home =
    document.querySelector('[role="main"].dream-home') ||
    document.querySelector('[role="main"].mortal-home') ||
    document.querySelector('[role="main"].qingkong-home') ||
    document.querySelector('[role="main"].jiuyi-home') ||
    document.querySelector('[role="main"][class*="-home"]');
  const homeShell = document.querySelector("main.main-surface");
  const hero = document.querySelector('[data-feature="game-source"]');

  let track = hero;
  while (track && track !== document.body) {
    const c = String(track.className || "");
    if (
      c.includes("thread-content-max-width") ||
      (c.includes("mx-auto") && c.includes("min-w-0") && c.includes("px-panel"))
    ) {
      break;
    }
    track = track.parentElement;
  }

  const sug =
    document.querySelector(".group\\\\/home-suggestions") ||
    document.querySelector('[class*="home-suggestions"]');
  const sugWrap = sug?.parentElement;
  const composer = document.querySelector(".composer-surface-chrome");

  const box = (el) => {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return {
      tag: el.tagName,
      cls: String(el.className || "").slice(0, 240),
      w: Math.round(r.width),
      h: Math.round(r.height),
      x: Math.round(r.x),
      maxW: cs.maxWidth,
      width: cs.width,
      threadVar: cs.getPropertyValue("--thread-content-max-width").trim(),
    };
  };

  const mainCs = roleMain ? getComputedStyle(roleMain) : null;
  const homeCs = home ? getComputedStyle(home) : null;
  const bodyCs = getComputedStyle(document.body);

  const composerChain = [];
  let n = composer;
  for (let i = 0; n && i < 14; i++) {
    const cs = getComputedStyle(n);
    const r = n.getBoundingClientRect();
    composerChain.push({
      cls: String(n.className || "").slice(0, 180),
      w: Math.round(r.width),
      maxW: cs.maxWidth,
      width: cs.width,
      threadVar: cs.getPropertyValue("--thread-content-max-width").trim() || undefined,
    });
    n = n.parentElement;
  }

  const sugBtns = [
    ...document.querySelectorAll(
      '.group\\\\/home-suggestions button, [class*="home-suggestions"] button'
    ),
  ]
    .slice(0, 8)
    .map((b) => {
      const r = b.getBoundingClientRect();
      return {
        w: Math.round(r.width),
        h: Math.round(r.height),
        cls: String(b.className).slice(0, 140),
      };
    });

  // Host native value on body / :root if skin not applied
  const hostNative = bodyCs.getPropertyValue("--thread-content-max-width").trim();

  // Find any element that sets max-w-(--thread-content-max-width)
  const maxWTracks = [...document.querySelectorAll("div")]
    .filter((el) => String(el.className || "").includes("thread-content-max-width"))
    .slice(0, 8)
    .map(box);

  return {
    rootClass: root.className,
    roleMainClass: roleMain ? String(roleMain.className).slice(0, 220) : null,
    homeClass: home ? String(home.className).slice(0, 220) : null,
    shellClass: homeShell ? String(homeShell.className).slice(0, 220) : null,
    vars: {
      body: hostNative,
      roleMain: mainCs?.getPropertyValue("--thread-content-max-width").trim(),
      home: homeCs?.getPropertyValue("--thread-content-max-width").trim(),
      container: roleMain
        ? {
            type: getComputedStyle(roleMain).containerType,
            name: getComputedStyle(roleMain).containerName,
            w: Math.round(roleMain.getBoundingClientRect().width),
          }
        : null,
    },
    hero: box(hero),
    track: box(track),
    sug: box(sug),
    sugWrap: box(sugWrap),
    composer: box(composer),
    composerChain,
    maxWTracks,
    sugBtns,
    viewport: { w: innerWidth, h: innerHeight },
  };
})()`;

const r = await send("Runtime.evaluate", {
  expression: expr,
  returnByValue: true,
});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
