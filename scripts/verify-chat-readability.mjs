/**
 * Hot-inject jiuyi CSS and verify chat readability over art background.
 */
import WebSocket from "ws";
import fs from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const css = fs.readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../skins/jiuyi/assets/jiuyi-skin.css"),
  "utf8"
);

const fileOk = {
  hasScrimMid: css.includes("rgba(10, 16, 26, 0.68)"),
  hasUserBubble: css.includes("bg-token-foreground/5"),
  hasMarkdownPad: css.includes("group.flex.min-w-0.flex-col"),
  hasTextShadow: css.includes("text-shadow:") && css.includes("thread-scroll-container"),
  noOldOnlyRole: css.includes("data-message-author-role") && css.includes("markdownContent"),
};
console.log("file checks:", fileOk);

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

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `(() => {
    const css = ${JSON.stringify(css)};
    let el = document.getElementById("codex-jiuyi-skin-style");
    if (!el) {
      el = document.createElement("style");
      el.id = "codex-jiuyi-skin-style";
      document.documentElement.appendChild(el);
    }
    el.textContent = css;

    const main = document.querySelector("main.main-surface");
    const mainBg = main ? getComputedStyle(main).backgroundImage : "";
    const scrimStronger = /0\\.68|0\\.78|0\\.88/.test(mainBg);

    const users = [...document.querySelectorAll(".thread-scroll-container div")].filter((n) =>
      String(n.className || "").includes("bg-token-foreground/5")
    );
    const userStyles = users.slice(0, 4).map((n) => {
      const s = getComputedStyle(n);
      return {
        bg: s.backgroundColor,
        border: s.borderTopColor,
        backdrop: s.backdropFilter,
        color: s.color,
        text: (n.innerText || "").replace(/\\s+/g, " ").slice(0, 40),
      };
    });

    // Prefer assistant shell (.group.flex.min-w-0.flex-col:has markdown)
    const asstShells = [...document.querySelectorAll(".thread-scroll-container .group.flex.min-w-0.flex-col")]
      .filter((n) => {
        if (n.closest("[class*='bg-token-foreground/5']")) return false;
        const hasMd = n.querySelector("[class*='markdownContent'], [class*='_markdownContent_']");
        if (!hasMd) return false;
        const r = n.getBoundingClientRect();
        return r.height > 20 && r.top < innerHeight && r.bottom > 0;
      });
    const asst = asstShells.slice(0, 4).map((n) => {
      const s = getComputedStyle(n);
      return {
        bg: s.backgroundColor,
        padding: s.padding,
        backdrop: s.backdropFilter,
        color: s.color,
        textShadow: s.textShadow.slice(0, 80),
        text: (n.innerText || "").replace(/\\s+/g, " ").slice(0, 40),
      };
    });

    const bodyText = [...document.querySelectorAll(".thread-scroll-container .text-size-chat, .thread-scroll-container [class*='markdownText']")]
      .filter((n) => n.getBoundingClientRect().height > 10)
      .slice(0, 3)
      .map((n) => {
        const s = getComputedStyle(n);
        return { color: s.color, textShadow: s.textShadow.slice(0, 60), text: (n.innerText||"").slice(0,30) };
      });

    // User bubble should no longer be ~5% white (very light)
    const userStillThin = userStyles.filter((u) =>
      /0\\.0[0-9]|\\/ 0\\.0[0-5]|oklab\\([^)]+\\/ 0\\.0[0-5]/i.test(u.bg)
      || u.bg === "rgba(0, 0, 0, 0)"
    );

    // Assistant should have non-transparent bg when present
    const asstTransparent = asst.filter((a) =>
      a.bg === "rgba(0, 0, 0, 0)" || a.bg === "transparent"
    );

    const ok =
      scrimStronger
      && (users.length === 0 || userStillThin.length === 0)
      && (asst.length === 0 || asstTransparent.length < asst.length);

    return {
      scrimStronger,
      mainBgHead: mainBg.slice(0, 220),
      userCount: users.length,
      userStyles,
      userStillThin: userStillThin.length,
      asst,
      asstTransparent: asstTransparent.length,
      bodyText,
      ok,
    };
  })()`,
});

const val = r.result?.result?.value ?? r.result;
console.log(JSON.stringify(val, null, 2));
if (!val?.ok) {
  console.error("VERIFY FAILED");
  process.exitCode = 1;
} else {
  console.log("VERIFY OK — chat readability improved");
}
ws.close();
