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
  const want = ".jiuyi-home .group\\\\/home-suggestions button:not([class*=\\"home-suggestion-list-item\\"])";
  const found = [];
  for (const sheet of document.styleSheets) {
    let rules;
    try { rules = sheet.cssRules; } catch { continue; }
    if (!rules) continue;
    for (let i = 0; i < rules.length; i++) {
      const rule = rules[i];
      if (!rule.selectorText) continue;
      // exact-ish card shell (no child combinators beyond button)
      if (
        rule.selectorText.includes("home-suggestions") &&
        rule.selectorText.includes("button:not") &&
        !rule.selectorText.includes(" > ") &&
        !rule.selectorText.includes(" span") &&
        !rule.selectorText.includes("svg") &&
        !rule.selectorText.includes("::") &&
        !rule.selectorText.includes(":hover")
      ) {
        const props = {};
        for (let j = 0; j < rule.style.length; j++) {
          const n = rule.style[j];
          props[n] = rule.style.getPropertyValue(n) + (rule.style.getPropertyPriority(n) ? " !important" : "");
        }
        found.push({
          selector: rule.selectorText,
          cssText: rule.cssText.slice(0, 900),
          propCount: rule.style.length,
          props,
        });
      }
    }
  }
  // also check for any rule that sets background-image none on buttons
  const noneBg = [];
  for (const sheet of document.styleSheets) {
    let rules;
    try { rules = sheet.cssRules; } catch { continue; }
    if (!rules) continue;
    for (const rule of rules) {
      if (!rule.style) continue;
      const bi = rule.style.getPropertyValue("background-image");
      const b = rule.style.getPropertyValue("background");
      if (
        rule.selectorText &&
        /button|home-suggestion|main-surface/i.test(rule.selectorText) &&
        (/none/i.test(bi) || /none/i.test(b) || /transparent/i.test(b))
      ) {
        noneBg.push({
          selector: rule.selectorText.slice(0, 140),
          bi,
          b: b.slice(0, 120),
          biImp: rule.style.getPropertyPriority("background-image"),
          bImp: rule.style.getPropertyPriority("background"),
        });
      }
    }
  }
  return { found, noneBg: noneBg.slice(0, 20) };
})()`;

const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
