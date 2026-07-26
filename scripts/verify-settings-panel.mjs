/**
 * Hot-patch jiuyi CSS into host, then verify settings panel tokens resolve to rain-night slate.
 */
import WebSocket from "ws";
import fs from "fs";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const rootDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const cssPath = join(rootDir, "skins/jiuyi/assets/jiuyi-skin.css");
const css = fs.readFileSync(cssPath, "utf8");

const fileOk = {
  hasPanel: css.includes("--color-background-panel: #1a2330"),
  hasElevated: css.includes("--color-background-elevated-primary:"),
  hasEditor: css.includes("--vscode-editor-background: #141c28"),
  hasCardSelector: css.includes("color-background-panel"),
  hasControl: css.includes("--color-background-control:"),
};
console.log("file checks:", fileOk);
if (!Object.values(fileOk).every(Boolean)) {
  console.error("CSS file missing expected tokens");
  process.exit(1);
}

const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find((p) => p.type === "page") || pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.log("no cdp — file checks only");
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

const injectExpr = `(() => {
  const css = ${JSON.stringify(css)};
  let el = document.getElementById("codex-jiuyi-skin-style");
  if (!el) {
    el = document.createElement("style");
    el.id = "codex-jiuyi-skin-style";
    document.documentElement.appendChild(el);
  }
  el.textContent = css;
  // clear any non-important inline override of panel token if present without !important conflict
  document.documentElement.classList.add("codex-jiuyi-skin", "dream-theme-dark");
  // open settings: click 配置
  const cfg = [...document.querySelectorAll("button")].find(
    (b) => (b.getAttribute("aria-label") || "").trim() === "配置"
      || (b.innerText || "").trim() === "配置"
  );
  if (cfg) cfg.click();
  return new Promise((resolve) => {
    setTimeout(() => {
      const s = getComputedStyle(document.documentElement);
      const panel = s.getPropertyValue("--color-background-panel").trim();
      const elevated = s.getPropertyValue("--color-background-elevated-primary").trim();
      const control = s.getPropertyValue("--color-background-control").trim();
      const editor = s.getPropertyValue("--vscode-editor-background").trim();
      const cards = [...document.querySelectorAll("div")].filter((el) => {
        const st = el.getAttribute("style") || "";
        const c = String(el.className || "");
        return st.includes("color-background-panel")
          || (c.includes("rounded-2xl") && c.includes("border-token-border") && c.includes("overflow-hidden"));
      });
      const cardBgs = cards.slice(0, 8).map((el) => ({
        bg: getComputedStyle(el).backgroundColor,
        style: (el.getAttribute("style") || "").slice(0, 100),
        y: Math.round(el.getBoundingClientRect().top),
        text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 40),
      }));
      const stillGray = cardBgs.filter((c) =>
        c.bg === "rgb(35, 35, 35)" || c.bg === "rgb(40, 40, 40)" || c.bg === "rgb(45, 45, 45)"
      );
      const panelThemed = panel === "#1a2330" || /26,\\s*35,\\s*48/.test(
        (() => {
          const t = document.createElement("div");
          t.style.backgroundColor = "var(--color-background-panel)";
          document.body.appendChild(t);
          const v = getComputedStyle(t).backgroundColor;
          t.remove();
          return v;
        })()
      );
      resolve({
        panel,
        elevated,
        control,
        editor,
        cardCount: cards.length,
        cardBgs,
        stillGrayCount: stillGray.length,
        panelThemed,
        ok: panelThemed && (cards.length === 0 || stillGray.length === 0),
      });
    }, 500);
  });
})()`;

const v = await send("Runtime.evaluate", {
  expression: injectExpr,
  returnByValue: true,
  awaitPromise: true,
});
const val = v.result?.result?.value ?? v.result;
console.log(JSON.stringify(val, null, 2));
if (!val?.ok) {
  console.error("VERIFY FAILED");
  process.exitCode = 1;
} else {
  console.log("VERIFY OK — settings panel themed");
}
ws.close();
