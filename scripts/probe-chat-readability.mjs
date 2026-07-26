/**
 * Probe host chat/thread readability: message DOM, colors, opacity over art bg.
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "probe-chat-readability-out.json.txt");
const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find((p) => p.type === "page") || pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.log("no cdp");
  process.exit(1);
}
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

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `(() => {
    const qa = (s) => [...document.querySelectorAll(s)];
    const fullCls = (el) => String(el.className || "");
    const textOf = (el, n = 80) => (el?.innerText || "").replace(/\\s+/g, " ").trim().slice(0, n);
    const pick = (el) => {
      if (!el) return null;
      const cs = getComputedStyle(el);
      const r = el.getBoundingClientRect();
      return {
        tag: el.tagName,
        role: el.getAttribute("role"),
        testid: el.getAttribute("data-testid"),
        author: el.getAttribute("data-message-author-role"),
        cls: fullCls(el).slice(0, 220),
        color: cs.color,
        bg: cs.backgroundColor,
        bgImg: (cs.backgroundImage || "").slice(0, 100),
        opacity: cs.opacity,
        textShadow: (cs.textShadow || "").slice(0, 100),
        fontSize: cs.fontSize,
        fontWeight: cs.fontWeight,
        lineHeight: cs.lineHeight,
        backdrop: cs.backdropFilter || cs.webkitBackdropFilter,
        border: cs.borderTopWidth + " " + cs.borderTopColor,
        padding: cs.padding,
        y: Math.round(r.top),
        h: Math.round(r.height),
        w: Math.round(r.width),
        text: textOf(el, 70),
      };
    };

    const root = document.documentElement;
    const main = document.querySelector("main.main-surface") || document.querySelector("main");
    const mainCs = main ? getComputedStyle(main) : null;

    const messages = qa("[data-message-author-role], [data-testid*='message'], article").slice(0, 20).map(pick);
    const byRole = {
      user: qa('[data-message-author-role="user"]').length,
      assistant: qa('[data-message-author-role="assistant"]').length,
      system: qa('[data-message-author-role="system"]').length,
      anyAttr: qa("[data-message-author-role]").length,
      article: qa("article").length,
    };

    // Thread containers
    const thread = document.querySelector(".thread-scroll-container")
      || document.querySelector("[class*='thread-scroll']")
      || document.querySelector("[data-testid*='conversation']");
    const threadPick = pick(thread);

    // Markdown / prose bodies
    const md = qa(".markdown, .prose, [class*='markdown'], [class*='ProseMirror'], .agent-turn, [data-message-author-role] .whitespace-pre-wrap, [data-message-author-role] p")
      .filter((el) => {
        const r = el.getBoundingClientRect();
        return r.width > 80 && r.height > 12 && textOf(el, 20).length > 5;
      })
      .slice(0, 15)
      .map(pick);

    // Ancestors of first message for scrim structure
    const firstMsg = qa("[data-message-author-role]").find((el) => el.getBoundingClientRect().height > 20)
      || qa("article").find((el) => el.getBoundingClientRect().height > 40);
    const chain = [];
    if (firstMsg) {
      let n = firstMsg;
      for (let i = 0; i < 10 && n; i++) {
        const cs = getComputedStyle(n);
        chain.push({
          tag: n.tagName,
          cls: fullCls(n).slice(0, 180),
          bg: cs.backgroundColor,
          bgImg: (cs.backgroundImage || "").slice(0, 60),
          color: cs.color,
          opacity: cs.opacity,
          author: n.getAttribute("data-message-author-role"),
        });
        n = n.parentElement;
      }
    }

    // Text tokens related to chat
    const cs = getComputedStyle(root);
    const tokenKeys = [
      "--color-token-text-primary",
      "--color-token-text-secondary",
      "--color-token-text-tertiary",
      "--color-token-foreground",
      "--text-primary",
      "--text-secondary",
      "--dream-ink",
      "--dream-text",
      "--color-token-main-surface-primary",
      "--color-background-surface",
      "--jiuyi-art",
    ];
    const tokens = {};
    for (const k of tokenKeys) tokens[k] = cs.getPropertyValue(k).trim().slice(0, 80);

    // Contrast helpers: sample actual message text vs bg
    const contrastSamples = (firstMsg ? [firstMsg] : []).concat(
      qa("[data-message-author-role] p, [data-message-author-role] li, .markdown p").slice(0, 5)
    ).map((el) => {
      const s = getComputedStyle(el);
      return {
        color: s.color,
        bg: s.backgroundColor,
        opacity: s.opacity,
        textShadow: s.textShadow,
        fontSize: s.fontSize,
        text: textOf(el, 40),
        author: el.closest("[data-message-author-role]")?.getAttribute("data-message-author-role"),
      };
    });

    // Is art showing on main?
    const artOnMain = mainCs
      ? {
          bgImg: (mainCs.backgroundImage || "").slice(0, 150),
          hasArtVar: (mainCs.backgroundImage || "").includes("url(") || (cs.getPropertyValue("--jiuyi-art") || "").length > 10,
          bgColor: mainCs.backgroundColor,
        }
      : null;

    // Host default-looking surfaces still opaque?
    const opaqueInThread = qa("main div, main section, main article").filter((el) => {
      const r = el.getBoundingClientRect();
      if (r.width < 200 || r.height < 40) return false;
      if (r.top > innerHeight || r.bottom < 0) return false;
      const s = getComputedStyle(el);
      const bg = s.backgroundColor;
      if (bg === "rgba(0, 0, 0, 0)" || bg === "transparent") return false;
      // has alpha < 1 or solid
      return true;
    }).slice(0, 25).map(pick);

    // Find turn / row wrappers with testids
    const testids = qa("[data-testid]").map((el) => el.getAttribute("data-testid"))
      .filter((t) => /message|turn|thread|conversation|composer|markdown|response/i.test(t || ""))
      .filter((v, i, a) => a.indexOf(v) === i)
      .slice(0, 40);

    const hasHome = root.classList.contains("jiuyi-home") || !!document.querySelector(".jiuyi-home");
    const hasThread = !!document.querySelector(".thread-scroll-container")
      || byRole.anyAttr > 0;

    return {
      rootCls: [...root.classList],
      hasHome,
      hasThread,
      byRole,
      testids,
      mainArt: artOnMain,
      thread: threadPick,
      messages: messages.slice(0, 12),
      md: md.slice(0, 12),
      chain,
      tokens,
      contrastSamples,
      opaqueInThread: opaqueInThread.slice(0, 15),
      bodyTextSample: textOf(document.body, 200),
    };
  })()`,
});

const val = r.result?.result?.value ?? r.result;
writeFileSync(OUT, JSON.stringify(val, null, 2), "utf8");
console.log(JSON.stringify(val, null, 2).slice(0, 14000));
console.log("\nwrote", OUT);
ws.close();
