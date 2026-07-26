import WebSocket from "ws";
import fs from "fs";

const css = fs.readFileSync("E:/demo/chatgpt-tools/skins/jiuyi/assets/jiuyi-skin.css", "utf8");
console.log("file checks:", {
  dropdown: css.includes("--color-token-dropdown-background: #1a2330"),
  menu: css.includes("--color-token-menu-background: rgba(26, 35, 48, 0.97)"),
  vscode: css.includes("--vscode-menu-background: rgba(26, 35, 48, 0.97)"),
  noDefault1818: !css.includes("--main-surface-primary: #181818"),
  rootMain: css.includes("--color-token-main-surface-primary: #101820"),
});

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
    setTimeout(() => rej(new Error("timeout")), 15000);
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
  const s = getComputedStyle(document.documentElement);
  const keys = [
    "--main-surface-primary",
    "--color-token-dropdown-background",
    "--color-token-menu-background",
    "--color-token-main-surface-primary",
    "--color-token-bg-primary",
    "--codex-base-surface",
    "--vscode-menu-background",
    "--vscode-dropdown-background",
  ];
  const out = { hasJiuyi: document.documentElement.classList.contains("codex-jiuyi-skin") };
  for (const k of keys) out[k] = s.getPropertyValue(k).trim();
  return out;
})()`;

const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
console.log("live page (re-apply skin to pick up CSS):");
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
