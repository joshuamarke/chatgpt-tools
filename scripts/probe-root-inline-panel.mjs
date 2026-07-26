import WebSocket from "ws";
import fs from "fs";

const css = fs.readFileSync("skins/jiuyi/assets/jiuyi-skin.css", "utf8");
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
    setTimeout(() => rej(new Error("to")), 15000);
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
  awaitPromise: true,
  expression: `(() => {
    const css = ${JSON.stringify(css)};
    let el = document.getElementById("codex-jiuyi-skin-style");
    if (!el) {
      el = document.createElement("style");
      el.id = "codex-jiuyi-skin-style";
      document.documentElement.appendChild(el);
    }
    el.textContent = css;

    const root = document.documentElement;
    const dump = (label) => {
      const s = getComputedStyle(root);
      return {
        label,
        panel: s.getPropertyValue("--color-background-panel").trim(),
        elevated: s.getPropertyValue("--color-background-elevated-primary").trim(),
        control: s.getPropertyValue("--color-background-control").trim(),
        editor: s.getPropertyValue("--vscode-editor-background").trim(),
        surface: s.getPropertyValue("--color-background-surface").trim(),
        inlinePanel: root.style.getPropertyValue("--color-background-panel"),
        inlinePri: root.style.getPropertyPriority("--color-background-panel"),
        hasClass: root.classList.contains("codex-jiuyi-skin"),
      };
    };
    const before = dump("after-inject");

    // Simulate host theme re-applying default (common path)
    // and see if our CSS !important wins
    root.style.setProperty("--color-background-panel", "#232323");
    const hostNoImp = dump("host-inline-no-important");
    root.style.removeProperty("--color-background-panel");

    root.style.setProperty("--color-background-panel", "#232323", "important");
    const hostImp = dump("host-inline-important");
    root.style.removeProperty("--color-background-panel");

    // Also inject a second style at end of head with higher cascade order
    let late = document.getElementById("jiuyi-panel-late");
    if (!late) {
      late = document.createElement("style");
      late.id = "jiuyi-panel-late";
      document.documentElement.appendChild(late);
    }
    late.textContent = \`
      html.codex-jiuyi-skin,
      html.codex-jiuyi-skin.electron-dark,
      html.codex-jiuyi-skin.electron-light,
      :root.codex-jiuyi-skin {
        --color-background-panel: #1a2330 !important;
        --color-background-elevated-primary: rgba(36, 48, 64, 0.96) !important;
        --color-background-elevated-primary-opaque: #243040 !important;
        --color-background-elevated-secondary: rgba(232, 228, 220, 0.04) !important;
        --color-background-elevated-secondary-opaque: #1a2330 !important;
        --color-background-control: rgba(26, 35, 48, 0.96) !important;
        --color-background-control-opaque: #1a2330 !important;
        --color-background-editor-opaque: #141c28 !important;
        --color-token-editor-background: #141c28 !important;
        --vscode-editor-background: #141c28 !important;
        --vscode-panel-background: rgba(26, 35, 48, 0.96) !important;
        --vscode-input-background: rgba(14, 20, 28, 0.92) !important;
      }
      html.codex-jiuyi-skin [style*="color-background-panel"],
      html.codex-jiuyi-skin div.rounded-2xl.border-token-border.overflow-hidden {
        background-color: #1a2330 !important;
      }
    \`;
    const afterLate = dump("after-late-style");

    // open settings
    const cfg = [...document.querySelectorAll("button")].find(
      (b) => (b.getAttribute("aria-label") || "").trim() === "配置"
    );
    if (cfg) cfg.click();

    return new Promise((resolve) => {
      setTimeout(() => {
        const cards = [...document.querySelectorAll("div")].filter((el) => {
          const st = el.getAttribute("style") || "";
          const c = String(el.className || "");
          return st.includes("color-background-panel")
            || (c.includes("rounded-2xl") && c.includes("border-token-border"));
        });
        resolve({
          before,
          hostNoImp,
          hostImp,
          afterLate,
          cardCount: cards.length,
          cardBgs: cards.slice(0, 6).map((el) => ({
            bg: getComputedStyle(el).backgroundColor,
            style: (el.getAttribute("style") || "").slice(0, 100),
            text: (el.innerText || "").replace(/\\s+/g, " ").slice(0, 40),
          })),
        });
      }, 500);
    });
  })()`,
});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
