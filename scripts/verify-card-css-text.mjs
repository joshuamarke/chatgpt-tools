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

const expr = `(() => {
  const styleEl = document.getElementById("codex-jiuyi-skin-style");
  const css = styleEl?.textContent || "";
  const idx = css.indexOf("button:not([class*=\\"home-suggestion-list-item\\"])");
  const block = idx >= 0 ? css.slice(idx, idx + 900) : null;
  // parse sheets for the rule
  let sheetRule = null;
  for (const sheet of document.styleSheets) {
    let rules;
    try { rules = sheet.cssRules; } catch { continue; }
    if (!rules) continue;
    for (const rule of rules) {
      if (rule.selectorText && rule.selectorText.includes("home-suggestions") && rule.selectorText.includes("button") && rule.selectorText.includes("not")) {
        const t = rule.cssText || "";
        if (t.includes("backdrop-filter") || t.includes("background")) {
          sheetRule = {
            selector: rule.selectorText.slice(0, 160),
            cssText: t.slice(0, 700),
            bg: rule.style?.background,
            bgImg: rule.style?.backgroundImage,
            bgColor: rule.style?.backgroundColor,
            backdrop: rule.style?.backdropFilter,
          };
          // keep last matching (most relevant)
        }
      }
    }
  }
  // force apply test
  const btn = document.querySelector('.group\\\\/home-suggestions button');
  let afterForce = null;
  if (btn) {
    btn.style.setProperty("background-image", "linear-gradient(red, blue)", "important");
    const cs = getComputedStyle(btn);
    afterForce = { bgImg: cs.backgroundImage, bg: cs.backgroundColor };
    btn.style.removeProperty("background-image");
  }
  return { block, sheetRule, afterForce, styleLen: css.length };
})()`;

const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
