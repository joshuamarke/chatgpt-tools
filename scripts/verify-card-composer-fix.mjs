import WebSocket from "ws";

const override = `
html.codex-qingkong-skin.dream-art-wide main.main-surface.qingkong-home-shell .qingkong-home .group\\/home-suggestions button:not([class*="home-suggestion-list-item"]),
html.codex-qingkong-skin main.main-surface .qingkong-home .group\\/home-suggestions button:not([class*="home-suggestion-list-item"]) {
  backdrop-filter: none !important;
  -webkit-backdrop-filter: none !important;
  background: rgba(255, 255, 255, 0.72) !important;
}
html.codex-qingkong-skin.dream-art-wide main.main-surface .composer-surface-chrome,
html.codex-qingkong-skin main.main-surface .composer-surface-chrome,
html.codex-qingkong-skin .qingkong-home:has(.qingkong-home-utility) .composer-surface-chrome,
html.codex-qingkong-skin .qingkong-home:has([class*="homeUtilityBar"]) .composer-surface-chrome {
  border-radius: var(--composer-border-radius, 1.25rem) !important;
  border-top-left-radius: var(--composer-border-radius, 1.25rem) !important;
  border-top-right-radius: var(--composer-border-radius, 1.25rem) !important;
  border-bottom-left-radius: var(--composer-border-radius, 1.25rem) !important;
  border-bottom-right-radius: var(--composer-border-radius, 1.25rem) !important;
}
`;

const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find((p) => p.type === "page") || pages[0];
const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
const send = (method, params = {}) =>
  new Promise((res, rej) => {
    const i = ++id;
    pending.set(i, { res, rej });
    ws.send(JSON.stringify({ id: i, method, params }));
    setTimeout(() => rej(new Error("timeout " + method)), 12000);
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

const inject = `(() => {
  const id = "cg-tools-temp-card-composer-fix";
  document.getElementById(id)?.remove();
  const s = document.createElement("style");
  s.id = id;
  s.textContent = ${JSON.stringify(override)};
  (document.head || document.documentElement).appendChild(s);
  void document.body.offsetHeight;
  const c = document.querySelector(".composer-surface-chrome");
  const b =
    document.querySelector('.group\\\\/home-suggestions button') ||
    document.querySelector('[class*="home-suggestions"] button');
  const cs = getComputedStyle(c);
  const bs = getComputedStyle(b);
  return {
    composer: {
      radius: cs.borderRadius,
      tl: cs.borderTopLeftRadius,
      tr: cs.borderTopRightRadius,
      hostVar: cs.getPropertyValue("--composer-border-radius").trim(),
    },
    card: {
      backdrop: bs.backdropFilter || bs.webkitBackdropFilter,
      bg: bs.backgroundColor,
    },
  };
})()`;

const r = await send("Runtime.evaluate", {
  expression: inject,
  returnByValue: true,
});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
