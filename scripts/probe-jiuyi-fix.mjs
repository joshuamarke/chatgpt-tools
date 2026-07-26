/**
 * Verify jiuyi Create button ink + no double art on chat/home.
 */
import WebSocket from "ws";

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

const click = async (label) => {
  await send("Runtime.evaluate", {
    returnByValue: true,
    expression: `(() => {
      const items = [...document.querySelectorAll("aside a, aside button, nav a, nav button")];
      const t = items.find((el) => {
        const s = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || "")).replace(/\\s+/g, " ").trim();
        return s === ${JSON.stringify(label)} || s.startsWith(${JSON.stringify(label)});
      });
      if (t) t.click();
      return !!t;
    })()`,
  });
  await new Promise((r) => setTimeout(r, 1500));
};

const probe = async (tag) => {
  const r = await send("Runtime.evaluate", {
    returnByValue: true,
    expression: `(() => {
      const body = document.body;
      const main = document.querySelector("main.main-surface");
      const header = document.querySelector("header.app-header-tint");
      const create = [...document.querySelectorAll("button")].find((el) => {
        const t = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || "")).trim();
        return t === "创建" || el.getAttribute("aria-label") === "创建";
      });
      const artOn = (el) => {
        if (!el) return null;
        const cs = getComputedStyle(el);
        const img = cs.backgroundImage || "";
        const hasUrl = /url\\(/i.test(img);
        const layerCount = (img.match(/url\\(/gi) || []).length;
        const hasGradient = /gradient/i.test(img);
        return {
          hasUrl,
          layerCount,
          hasGradient,
          bg: cs.backgroundColor,
          imgHead: img.slice(0, 180),
        };
      };
      const createStyle = create
        ? {
            text: (create.innerText || create.getAttribute("aria-label") || "").trim(),
            color: getComputedStyle(create).color,
            bg: getComputedStyle(create).backgroundColor,
            bgImg: getComputedStyle(create).backgroundImage.slice(0, 120),
            inHeader: Boolean(header && header.contains(create)),
          }
        : null;
      return {
        tag: ${JSON.stringify(tag)},
        root: document.documentElement.className,
        bodyArt: artOn(body),
        mainArt: artOn(main),
        shellHome: main ? [...main.classList].filter((c) => /home/.test(c)) : [],
        hasThread: !!document.querySelector(".thread-scroll-container"),
        createStyle,
        primaryInk: getComputedStyle(document.documentElement).getPropertyValue("--cg-primary-button-ink").trim(),
      };
    })()`,
  });
  return r.result?.result?.value;
};

await click("已安排");
console.log(JSON.stringify(await probe("scheduled"), null, 2));

// open a thread for chat double-bg check
await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `(() => {
    const items = [...document.querySelectorAll("aside a, aside button")];
    const t = items.find((el) => {
      const s = ((el.innerText || "") + "").replace(/\\s+/g, " ").trim();
      if (!s || s.length < 4) return false;
      if (/新建任务|已安排|插件|搜索|设置|工作区|项目|模式/.test(s)) return false;
      return el.getBoundingClientRect().y > 120;
    });
    if (t) t.click();
    return t ? (t.innerText || "").slice(0, 40) : null;
  })()`,
});
await new Promise((r) => setTimeout(r, 1800));
console.log(JSON.stringify(await probe("chat"), null, 2));

ws.close();
