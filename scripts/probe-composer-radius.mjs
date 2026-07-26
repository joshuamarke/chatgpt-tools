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

const expr = `(() => {
  const box = (el) => {
    if (!el) return null;
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      cls: String(el.className || "").slice(0, 220),
      radius: cs.borderRadius,
      tl: cs.borderTopLeftRadius,
      tr: cs.borderTopRightRadius,
      bl: cs.borderBottomLeftRadius,
      br: cs.borderBottomRightRadius,
      border: cs.border,
      borderTop: cs.borderTop,
      bg: cs.backgroundColor,
      backdrop: cs.backdropFilter || cs.webkitBackdropFilter,
      y: Math.round(r.y),
      h: Math.round(r.height),
      w: Math.round(r.width),
    };
  };

  const composer = document.querySelector(".composer-surface-chrome");
  const util = [
    ...document.querySelectorAll(
      '[class*="homeUtilityBar"], [class*="_homeUtilityBar_"], .qingkong-home-utility, .mortal-home-utility, .dream-home-utility'
    ),
  ];
  const projectBars = [
    ...document.querySelectorAll(
      ".group\\\\/project-selector, [class*='project-selector']"
    ),
  ]
    .map((el) => el.closest("div.select-none") || el.parentElement)
    .filter(Boolean);

  // siblings just above composer
  let above = null;
  if (composer) {
    let p = composer.parentElement;
    for (let i = 0; p && i < 6; i++) {
      const kids = [...p.children];
      const idx = kids.indexOf(
        kids.find((k) => k.contains(composer) || k === composer) || composer
      );
      if (idx > 0) {
        above = kids[idx - 1];
        break;
      }
      // if composer is deep, look for previous sibling of intermediate
      const chain = [];
      let n = composer;
      while (n && n.parentElement === p) {
        chain.unshift(n);
        n = null;
      }
      p = p.parentElement;
    }
    // walk up until we find a previous element sibling in the flex col
    let node = composer;
    while (node && !above) {
      if (node.previousElementSibling) {
        above = node.previousElementSibling;
        break;
      }
      node = node.parentElement;
      if (node?.classList?.contains("flex") && node.children.length > 1) {
        const kids = [...node.children];
        const mine = kids.findIndex((k) => k.contains(composer));
        if (mine > 0) above = kids[mine - 1];
      }
    }
  }

  const cards = [
    ...document.querySelectorAll(
      '.group\\\\/home-suggestions button, [class*="home-suggestions"] button'
    ),
  ]
    .filter((b) => !String(b.className).includes("list-item"))
    .slice(0, 2)
    .map(box);

  // host token radius if any
  const body = getComputedStyle(document.body);
  return {
    root: document.documentElement.className,
    composer: box(composer),
    above: box(above),
    util: util.map(box),
    projectBars: [...new Set(projectBars)].slice(0, 3).map(box),
    cards,
    tokens: {
      radius: body.getPropertyValue("--radius").trim(),
      radiusLg: body.getPropertyValue("--radius-lg").trim(),
    },
  };
})()`;

const r = await send("Runtime.evaluate", {
  expression: expr,
  returnByValue: true,
});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
