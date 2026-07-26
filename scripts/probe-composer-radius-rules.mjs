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
await send("DOM.enable");
await send("CSS.enable");
await send("Runtime.enable");

const { result: doc } = await send("DOM.getDocument", { depth: 0 });
const { result: nodeIdRes } = await send("DOM.querySelector", {
  nodeId: doc.root.nodeId,
  selector: ".composer-surface-chrome",
});
const nodeId = nodeIdRes.nodeId;
const matched = await send("CSS.getMatchedStylesForNode", { nodeId });
const props = ["border-radius", "border-top-left-radius", "border-top-right-radius", "border-start-start-radius", "border-start-end-radius"];
const hits = [];
for (const rule of matched.result?.matchedCSSRules || []) {
  const style = rule.rule?.style;
  if (!style?.cssProperties) continue;
  const relevant = style.cssProperties.filter((p) => props.includes(p.name) && p.value);
  if (!relevant.length) continue;
  hits.push({
    selector: rule.rule.selectorList?.text?.slice(0, 200),
    origin: rule.rule.origin,
    source: rule.rule.styleSheetId,
    props: relevant.map((p) => `${p.name}:${p.value}${p.important ? " !important" : ""}`),
  });
}
const inline = matched.result?.inlineStyle?.cssProperties?.filter((p) => props.includes(p.name) && p.value) || [];
const attributes = matched.result?.attributesStyle?.cssProperties?.filter((p) => props.includes(p.name) && p.value) || [];

// also check computed
const computed = await send("CSS.getComputedStyleForNode", { nodeId });
const comp = Object.fromEntries(
  (computed.result?.computedStyle || [])
    .filter((p) => props.includes(p.name) || p.name.includes("radius"))
    .map((p) => [p.name, p.value])
);

console.log(
  JSON.stringify(
    {
      hits: hits.slice(-30),
      inline,
      attributes,
      computed: comp,
    },
    null,
    2
  )
);
ws.close();
