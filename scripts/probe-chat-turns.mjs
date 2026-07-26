/**
 * Deep probe visible chat turns: user bubble, assistant markdown, code blocks.
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "probe-chat-turns-out.json.txt");
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

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `(() => {
    const fullCls = (el) => String(el.className || "");
    const textOf = (el, n = 60) => (el?.innerText || "").replace(/\\s+/g, " ").trim().slice(0, n);
    const vis = (el) => {
      const r = el.getBoundingClientRect();
      return r.width > 40 && r.height > 12 && r.bottom > 0 && r.top < innerHeight + 100;
    };
    const styleOf = (el) => {
      const cs = getComputedStyle(el);
      const r = el.getBoundingClientRect();
      return {
        tag: el.tagName,
        cls: fullCls(el).slice(0, 260),
        color: cs.color,
        bg: cs.backgroundColor,
        bgImg: (cs.backgroundImage || "").slice(0, 80),
        opacity: cs.opacity,
        textShadow: (cs.textShadow || "").slice(0, 120),
        fontSize: cs.fontSize,
        fontWeight: cs.fontWeight,
        backdrop: (cs.backdropFilter || "none").slice(0, 60),
        border: cs.borderTopWidth + " " + cs.borderTopColor,
        radius: cs.borderRadius,
        padding: cs.padding,
        y: Math.round(r.top),
        h: Math.round(r.height),
        w: Math.round(r.width),
        text: textOf(el, 50),
      };
    };

    // User bubbles: bg-token-foreground/5 rounded-2xl
    const userBubbles = [...document.querySelectorAll("div")].filter((el) => {
      const c = fullCls(el);
      return c.includes("bg-token-foreground/5") && c.includes("rounded-2xl") && vis(el);
    }).map(styleOf);

    // Markdown content blocks visible
    const mdBlocks = [...document.querySelectorAll("[class*='markdownContent'], [class*='_markdownContent_']")]
      .filter(vis)
      .slice(0, 8)
      .map((el) => {
        const s = styleOf(el);
        // walk up for background surface
        const chain = [];
        let n = el;
        for (let i = 0; i < 8 && n; i++) {
          const cs = getComputedStyle(n);
          chain.push({
            tag: n.tagName,
            cls: fullCls(n).slice(0, 140),
            bg: cs.backgroundColor,
            color: cs.color,
          });
          n = n.parentElement;
        }
        return { ...s, chain };
      });

    // Secondary / muted text in thread
    const muted = [...document.querySelectorAll(".thread-scroll-container span, .thread-scroll-container div, .thread-scroll-container p")]
      .filter(vis)
      .filter((el) => {
        const c = fullCls(el);
        const col = getComputedStyle(el).color;
        return /text-token-text-secondary|text-token-text-tertiary|text-secondary|opacity-|text-muted/i.test(c)
          || col === "rgb(168, 162, 150)" || col === "rgb(138, 155, 176)" || col === "rgb(184, 178, 166)";
      })
      .slice(0, 12)
      .map(styleOf);

    // Code blocks
    const codes = [...document.querySelectorAll("pre, code, [class*='code-block'], [class*='CodeBlock']")]
      .filter(vis)
      .slice(0, 8)
      .map(styleOf);

    // Turn row wrappers - common patterns
    const turnRows = [...document.querySelectorAll(".thread-scroll-container > div > div, .thread-scroll-container div[class*='group']")]
      .filter((el) => {
        if (!vis(el)) return false;
        const r = el.getBoundingClientRect();
        return r.width > 400 && r.height > 40 && r.height < 800;
      })
      .slice(0, 12)
      .map(styleOf);

    // Content column max width track
    const contentCols = [...document.querySelectorAll(".thread-scroll-container div")]
      .filter((el) => {
        const c = fullCls(el);
        return /thread-content|max-w|mx-auto/.test(c) && vis(el);
      })
      .slice(0, 8)
      .map(styleOf);

    // Secondary text token on root
    const root = getComputedStyle(document.documentElement);
    const textTokens = {};
    for (const k of [...root]) {
      if (/text-primary|text-secondary|text-tertiary|foreground|description|cg-text|dream-text|dream-ink/i.test(k)) {
        textTokens[k] = root.getPropertyValue(k).trim().slice(0, 60);
      }
    }

    // Sample colors of ALL visible text nodes' parents in thread (top of viewport)
    const thread = document.querySelector(".thread-scroll-container");
    const colorBuckets = {};
    if (thread) {
      const walker = document.createTreeWalker(thread, NodeFilter.SHOW_ELEMENT);
      let count = 0;
      while (walker.nextNode() && count < 400) {
        const el = walker.currentNode;
        if (!vis(el)) continue;
        if ((el.innerText || "").trim().length < 2) continue;
        if (el.children.length > 3 && el.innerText.length > 200) continue; // skip huge containers
        const col = getComputedStyle(el).color;
        const bg = getComputedStyle(el).backgroundColor;
        const key = col + " | " + bg;
        colorBuckets[key] = (colorBuckets[key] || 0) + 1;
        count++;
      }
    }

    // Main gradient opacity at center of thread content
    const main = document.querySelector("main.main-surface");
    const mainBg = main ? getComputedStyle(main).backgroundImage.slice(0, 400) : null;

    // Host native vs skin: does message use only transparent bg?
    const nearTransparent = userBubbles.filter((b) =>
      /0\\.0[0-9]|\\/ 0\\.0|rgba\\(0, 0, 0, 0\\)|oklab\\([^)]+\\/ 0\\.0/i.test(b.bg)
    );

    return {
      userBubbles,
      mdBlocks,
      muted,
      codes,
      turnRows: turnRows.slice(0, 8),
      contentCols,
      textTokens,
      colorBuckets,
      mainBg,
      nearTransparentCount: nearTransparent.length,
      viewport: { w: innerWidth, h: innerHeight },
    };
  })()`,
});

const val = r.result?.result?.value ?? r.result;
writeFileSync(OUT, JSON.stringify(val, null, 2), "utf8");

console.log("userBubbles", val.userBubbles?.length);
(val.userBubbles || []).forEach((b) => console.log(" USER", b.bg, b.color, b.cls?.slice(0, 100), b.text));
console.log("\nmdBlocks", val.mdBlocks?.length);
(val.mdBlocks || []).forEach((b) => {
  console.log(" MD", b.bg, b.color, b.y, b.text);
  console.log("  chain:", b.chain?.map((c) => c.bg + " " + c.cls?.slice(0, 50)).join(" <- "));
});
console.log("\nmuted:");
(val.muted || []).forEach((m) => console.log(" ", m.color, m.bg, m.text, m.cls?.slice(0, 80)));
console.log("\ncodes:");
(val.codes || []).forEach((c) => console.log(" ", c.bg, c.color, c.cls?.slice(0, 80), c.text));
console.log("\ncolorBuckets top:");
Object.entries(val.colorBuckets || {})
  .sort((a, b) => b[1] - a[1])
  .slice(0, 20)
  .forEach(([k, n]) => console.log(n, k));
console.log("\nmainBg", val.mainBg);
console.log("wrote", OUT);
ws.close();
