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
await send("CSS.enable");
await send("DOM.enable");

const expr = `(() => {
  const btn = document.querySelector('.group\\\\/home-suggestions')?.querySelector(
    'button:not([class*="home-suggestion-list-item"])'
  );
  if (!btn) return { err: "no btn" };
  const cs = getComputedStyle(btn);
  return {
    background: cs.background,
    backgroundColor: cs.backgroundColor,
    backgroundImage: cs.backgroundImage,
    backdropFilter: cs.backdropFilter,
    // matched rules via CSSOM is hard; try el.style and class
    inline: btn.getAttribute("style"),
    className: String(btn.className).slice(0, 200),
  };
})()`;
const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
const basic = r.result?.result?.value;

// CSS matched styles via CDP
const doc = await send("DOM.getDocument", { depth: 0 });
const rootId = doc.result.root.nodeId;
const { result: q } = await send("DOM.querySelector", {
  nodeId: rootId,
  selector: '.group\\/home-suggestions button',
});
let matched = null;
if (q.nodeId) {
  const styles = await send("CSS.getMatchedStylesForNode", { nodeId: q.nodeId });
  const rules = (styles.result?.matchedCSSRules || [])
    .filter((x) => {
      const t = x.rule?.style?.cssText || "";
      return /background|backdrop/i.test(t) || /home-suggestion|jiuyi/i.test(x.rule?.selectorList?.text || "");
    })
    .slice(-12)
    .map((x) => ({
      selector: x.rule?.selectorList?.text?.slice(0, 120),
      origin: x.rule?.origin,
      cssText: (x.rule?.style?.cssText || "").slice(0, 280),
      important: x.rule?.style?.cssProperties
        ?.filter((p) => /background|backdrop/i.test(p.name))
        .map((p) => `${p.name}:${p.value}${p.important ? " !important" : ""}`),
    }));
  matched = rules;
}

console.log(JSON.stringify({ basic, matched }, null, 2));
ws.close();
