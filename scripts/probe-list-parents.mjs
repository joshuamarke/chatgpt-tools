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
  // open list if needed
  let item = document.querySelector(".group\\\\/home-suggestion-list-item");
  if (!item) {
    const btn = document.querySelector(".group\\\\/home-suggestions button:not(.group\\\\/home-suggestion-list-item)");
    if (btn) btn.click();
  }
  return { willRetry: !item };
})()`;
await send("Runtime.evaluate", { expression: expr, returnByValue: true });
await new Promise((r) => setTimeout(r, 500));

const expr2 = `(() => {
  const item = document.querySelector(".group\\\\/home-suggestion-list-item");
  if (!item) return { noItem: true };
  const parents = [];
  let el = item;
  for (let i = 0; i < 10 && el; i++) {
    const s = getComputedStyle(el);
    parents.push({
      tag: el.tagName,
      cls: String(el.className || "").slice(0, 260),
      bg: s.backgroundColor,
      color: s.color,
      pad: s.padding,
      radius: s.borderRadius,
      border: s.border,
      shadow: (s.boxShadow || "").slice(0, 100),
      display: s.display,
      gap: s.gap,
      w: Math.round(el.getBoundingClientRect().width),
      h: Math.round(el.getBoundingClientRect().height),
    });
    el = el.parentElement;
  }

  const probe = document.createElement("div");
  document.body.appendChild(probe);
  const one = (cls) => {
    probe.className = cls;
    const s = getComputedStyle(probe);
    return { bg: s.backgroundColor, color: s.color, border: s.borderColor };
  };
  const classes = {
    "bg-token-main-surface-primary": one("bg-token-main-surface-primary"),
    "bg-token-list-hover-background": one("bg-token-list-hover-background"),
    "text-token-description-foreground": one("text-token-description-foreground"),
    "text-token-text-tertiary": one("text-token-text-tertiary"),
    "text-token-text-primary": one("text-token-text-primary"),
    "text-token-foreground": one("text-token-foreground"),
    "border-token-input-border": one("border-token-input-border"),
  };
  probe.remove();

  const icon = item.querySelector("span.flex");
  const svg = item.querySelector("svg");
  const textSpan = item.querySelector("span.text-token-text-tertiary, span[class*=text-token]");

  // sibling structure of item
  const kids = [...item.children].map((c) => ({
    tag: c.tagName,
    cls: String(c.className || "").slice(0, 180),
    text: (c.innerText || "").replace(/\\s+/g, " ").slice(0, 60),
  }));

  return {
    parents,
    kids,
    itemHtml: item.outerHTML.slice(0, 900),
    icon: icon
      ? {
          cls: String(icon.className).slice(0, 180),
          size: getComputedStyle(icon).width,
          bg: getComputedStyle(icon).backgroundColor,
          color: getComputedStyle(icon).color,
        }
      : null,
    svg: svg
      ? {
          w: getComputedStyle(svg).width,
          color: getComputedStyle(svg).color,
          opacity: getComputedStyle(svg).opacity,
        }
      : null,
    textSpan: textSpan
      ? {
          cls: String(textSpan.className).slice(0, 180),
          color: getComputedStyle(textSpan).color,
        }
      : null,
    classes,
    itemCs: {
      bg: getComputedStyle(item).backgroundColor,
      color: getComputedStyle(item).color,
      minH: getComputedStyle(item).minHeight,
      gap: getComputedStyle(item).gap,
      pad: getComputedStyle(item).padding,
      radius: getComputedStyle(item).borderRadius,
      fontSize: getComputedStyle(item).fontSize,
    },
  };
})()`;

const r = await send("Runtime.evaluate", { expression: expr2, returnByValue: true });
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
