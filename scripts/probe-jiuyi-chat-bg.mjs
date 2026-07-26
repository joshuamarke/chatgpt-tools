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

const evalJson = async (expression) => {
  const r = await send("Runtime.evaluate", { returnByValue: true, expression });
  return r.result?.result?.value;
};

const artProbe = `(() => {
  const body = getComputedStyle(document.body);
  const mainEl = document.querySelector("main.main-surface");
  const main = mainEl ? getComputedStyle(mainEl) : null;
  const countUrl = (img) => (String(img || "").match(/url\\(/g) || []).length;
  return {
    hasThread: !!document.querySelector(".thread-scroll-container"),
    hasHome: !!document.querySelector('[data-testid="home-icon"]'),
    bodyUrl: countUrl(body.backgroundImage),
    mainUrl: main ? countUrl(main.backgroundImage) : null,
    mainHasGradient: main ? /gradient/i.test(main.backgroundImage) : null,
    mainImg: main ? main.backgroundImage.slice(0, 240) : null,
    mainBg: main ? main.backgroundColor : null,
    shellHome: mainEl ? [...mainEl.classList].filter((c) => /home/.test(c)) : [],
  };
})()`;

// click a thread-like row
const click = await evalJson(`(() => {
  const items = [...document.querySelectorAll("aside a, aside button, aside [role=button]")];
  const skip = /新建任务|已安排|插件|搜索|设置|工作区|项目|模式|切换|收起|展开/;
  const t = items.find((el) => {
    const s = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || ""))
      .replace(/\\s+/g, " ")
      .trim();
    if (!s || s.length < 2 || skip.test(s)) return false;
    const r = el.getBoundingClientRect();
    return r.y > 180 && r.height >= 22 && r.height < 100;
  });
  if (t) t.click();
  return t
    ? {
        text: (t.innerText || t.getAttribute("aria-label") || "").slice(0, 60),
        y: Math.round(t.getBoundingClientRect().y),
      }
    : null;
})()`);
console.log("CLICK", JSON.stringify(click));
await new Promise((r) => setTimeout(r, 2000));
console.log("CHAT", JSON.stringify(await evalJson(artProbe), null, 2));

// also force ensure route after new-task
await evalJson(`(() => {
  const items = [...document.querySelectorAll("aside a, aside button")];
  const t = items.find((el) => /新建任务/.test(el.innerText || ""));
  t?.click();
  return !!t;
})()`);
await new Promise((r) => setTimeout(r, 1500));
console.log("HOME", JSON.stringify(await evalJson(artProbe), null, 2));
ws.close();
