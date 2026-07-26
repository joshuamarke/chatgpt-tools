/**
 * Use CDP DOM+CSS.getMatchedStylesForNode on settings group cards.
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "probe-settings-matched-out.json.txt");
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
    setTimeout(() => rej(new Error("timeout " + method)), 20000);
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
await send("DOM.enable");
await send("CSS.enable");

// Find node via JS
const find = await send("Runtime.evaluate", {
  expression: `(() => {
    const cards = [...document.querySelectorAll('*')].filter(el =>
      String(el.className||'').includes('after:bg-token-border') &&
      String(el.className||'').includes('rounded-2xl')
    );
    // also find any rounded-2xl border-token-border
    const cards2 = [...document.querySelectorAll('*')].filter(el => {
      const c = String(el.className||'');
      return c.includes('rounded-2xl') && c.includes('border-token-border') && c.includes('overflow-hidden');
    });
    const all = cards.length ? cards : cards2;
    return all.slice(0, 4).map((el, i) => {
      el.setAttribute('data-jiuyi-probe-card', String(i));
      const s = getComputedStyle(el);
      return {
        i,
        bg: s.backgroundColor,
        cls: String(el.className).slice(0, 300),
        text: (el.innerText||'').replace(/\\s+/g,' ').slice(0, 60),
      };
    });
  })()`,
  returnByValue: true,
});
const cards = find.result?.result?.value || [];
console.log("found cards", cards);

const doc = await send("DOM.getDocument", { depth: 0 });
const rootId = doc.result.root.nodeId;

const results = [];
for (const card of cards) {
  const q = await send("DOM.querySelector", {
    nodeId: rootId,
    selector: `[data-jiuyi-probe-card="${card.i}"]`,
  });
  const nodeId = q.result?.nodeId;
  if (!nodeId) {
    results.push({ card, error: "no node" });
    continue;
  }
  const matched = await send("CSS.getMatchedStylesForNode", { nodeId });
  const inline = matched.result?.inlineStyle;
  const attributes = matched.result?.attributesStyle;
  const rules = (matched.result?.matchedCSSRules || [])
    .map((entry) => {
      const rule = entry.rule;
      const style = rule?.style;
      const props = (style?.cssProperties || [])
        .filter((p) => /background|border|color|box-shadow|opacity/i.test(p.name) && p.value)
        .map((p) => ({
          name: p.name,
          value: p.value,
          important: p.important,
          disabled: p.disabled,
        }));
      if (!props.length) return null;
      return {
        origin: rule.origin,
        selector: (rule.selectorList?.text || "").slice(0, 250),
        props,
        source: (rule.sourceURL || rule.styleSheetId || "").toString().slice(-80),
      };
    })
    .filter(Boolean);

  // Also inherited
  const inherited = (matched.result?.inherited || []).slice(0, 5).map((inh) => {
    const props = [];
    for (const entry of inh.matchedCSSRules || []) {
      for (const p of entry.rule?.style?.cssProperties || []) {
        if (/background/i.test(p.name) && p.value) {
          props.push({
            sel: entry.rule.selectorList?.text?.slice(0, 100),
            name: p.name,
            value: p.value,
          });
        }
      }
    }
    return props;
  }).filter((x) => x.length);

  // Computed style for background
  const computed = await send("CSS.getComputedStyleForNode", { nodeId });
  const comp = {};
  for (const p of computed.result?.computedStyle || []) {
    if (/background|border-radius|box-shadow|color|border-top-color/.test(p.name)) {
      comp[p.name] = p.value;
    }
  }

  results.push({
    card,
    inline: inline?.cssText,
    attributes: attributes?.cssText,
    bgRules: rules,
    inheritedBg: inherited,
    computed: comp,
  });
}

// Also probe root tokens that might feed card surfaces in electron-dark
const tokenEval = await send("Runtime.evaluate", {
  expression: `(() => {
    const cs = getComputedStyle(document.documentElement);
    const keys = [...cs].filter(k =>
      /bg-fog|bg-primary|bg-secondary|bg-tertiary|editor-background|input-background|panel-background|dropdown|surface|elevation|card|fog|settings/i.test(k)
    );
    const out = {};
    for (const k of keys) out[k] = cs.getPropertyValue(k).trim().slice(0, 90);
    // Check if any rule sets rounded-2xl + border with background via attribute selector
    // Dump parent of first card
    const el = document.querySelector('[data-jiuyi-probe-card="0"]');
    const chain = [];
    let n = el;
    for (let i = 0; i < 8 && n; i++) {
      const s = getComputedStyle(n);
      chain.push({
        tag: n.tagName,
        cls: String(n.className||'').slice(0, 200),
        bg: s.backgroundColor,
        bgClip: s.backgroundClip,
        isolation: s.isolation,
      });
      n = n.parentElement;
    }
    // Does card have any pseudo with bg?
    const before = el ? getComputedStyle(el, '::before') : null;
    const after = el ? getComputedStyle(el, '::after') : null;
    return {
      outCount: keys.length,
      sample: Object.fromEntries(Object.entries(out).slice(0, 60)),
      allFog: Object.fromEntries(Object.entries(out).filter(([k]) => /fog|card|elevat|surface|editor-background|input-background|panel/i.test(k))),
      chain,
      beforeBg: before?.backgroundColor,
      beforeContent: before?.content,
      afterBg: after?.backgroundColor,
    };
  })()`,
  returnByValue: true,
});

const payload = {
  results,
  tokens: tokenEval.result?.result?.value,
};
writeFileSync(OUT, JSON.stringify(payload, null, 2), "utf8");

for (const r of results) {
  console.log("\n==== card", r.card.i, r.card.bg, r.card.text);
  console.log("inline:", r.inline);
  console.log("computed bg:", r.computed?.["background-color"], "image:", r.computed?.["background-image"]?.slice(0, 80));
  console.log("bg-related rules:");
  for (const rule of r.bgRules || []) {
    console.log(" [", rule.origin, "]", rule.selector);
    for (const p of rule.props) console.log("   ", p.name, ":", p.value, p.important ? "!important" : "");
  }
  if (r.inheritedBg?.length) console.log("inherited", JSON.stringify(r.inheritedBg).slice(0, 400));
}
console.log("\nchain:", JSON.stringify(payload.tokens?.chain, null, 2));
console.log("beforeBg", payload.tokens?.beforeBg, "afterBg", payload.tokens?.afterBg);
console.log("allFog tokens:");
Object.entries(payload.tokens?.allFog || {}).forEach(([k, v]) => console.log(" ", k, "=", v));
console.log("wrote", OUT);
ws.close();
