/**
 * Hot-inject jiuyi CSS; verify home header transparent, signature gone;
 * optionally click 配置 and verify settings header solid #101820.
 */
import WebSocket from "ws";
import fs from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const css = fs.readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../skins/jiuyi/assets/jiuyi-skin.css"),
  "utf8"
);
const plugin = JSON.parse(
  fs.readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../skins/jiuyi/assets/plugin.json"),
    "utf8"
  )
);

const fileOk = {
  noSignatureInChrome: !String(plugin.chromeHtml || "").includes("jiuyi-signature"),
  hasHomeTransparent: css.includes("jiuyi-home-shell > header.app-header-tint"),
  hasSettingsSolid: css.includes("not(.jiuyi-home-shell):not(:has(.thread-scroll-container))"),
  signatureHidden: css.includes(".jiuyi-signature") && css.includes("display: none !important"),
  noOldGradientBlock:
    !css.includes("linear-gradient(90deg, rgba(12, 18, 28, 0.72), rgba(22, 32, 46, 0.45), rgba(168, 58, 46, 0.06))"),
};
console.log("file checks:", fileOk);
if (!Object.values(fileOk).every(Boolean)) {
  console.error("FILE CHECK FAILED");
  process.exit(1);
}

const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find((p) => p.type === "page") || pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.log("no cdp — file only");
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

    // Remove signature node if still in chrome from old inject
    document.querySelectorAll(".jiuyi-signature").forEach((n) => n.remove());
    // Also hide via class if recreated
    const chrome = document.getElementById("codex-jiuyi-skin-chrome");
    if (chrome && chrome.innerHTML.includes("jiuyi-signature")) {
      chrome.querySelectorAll(".jiuyi-signature").forEach((n) => n.remove());
    }

    const headerStyle = () => {
      const h = document.querySelector("main.main-surface > header.app-header-tint");
      if (!h) return null;
      const s = getComputedStyle(h);
      return {
        bg: s.backgroundColor,
        bgImg: (s.backgroundImage || "").slice(0, 80),
        borderBottom: s.borderBottomWidth + " " + s.borderBottomColor,
        backdrop: s.backdropFilter,
        textShadow: s.textShadow.slice(0, 60),
      };
    };
    const mainStyle = () => {
      const m = document.querySelector("main.main-surface");
      if (!m) return null;
      const s = getComputedStyle(m);
      return {
        bg: s.backgroundColor,
        hasArt: (s.backgroundImage || "").includes("url(") || (s.backgroundImage || "").includes("blob:"),
        homeShell: m.classList.contains("jiuyi-home-shell"),
        hasThread: !!m.querySelector(".thread-scroll-container"),
        classes: [...m.classList].filter((c) => /home|shell|main|surface/i.test(c)),
      };
    };
    const sig = () => {
      const n = document.querySelector(".jiuyi-signature");
      if (!n) return { present: false };
      const s = getComputedStyle(n);
      return { present: true, display: s.display, visibility: s.visibility, text: n.textContent };
    };

    const homeSnap = {
      header: headerStyle(),
      main: mainStyle(),
      signature: sig(),
    };

    // Try open settings
    const cfg = [...document.querySelectorAll("button")].find(
      (b) => (b.getAttribute("aria-label") || "").trim() === "配置"
        || (b.innerText || "").trim() === "配置"
    );
    if (cfg) cfg.click();

    return new Promise((resolve) => {
      setTimeout(() => {
        const settingsSnap = {
          header: headerStyle(),
          main: mainStyle(),
          signature: sig(),
          settingsNav: [...document.querySelectorAll("button")].some((b) =>
            /^(常规|外观|配置)$/.test((b.getAttribute("aria-label") || "").trim())
          ),
        };

        const homeOk =
          homeSnap.header
          && (homeSnap.header.bg === "rgba(0, 0, 0, 0)" || homeSnap.header.bgImg === "none")
          && (!homeSnap.signature.present || homeSnap.signature.display === "none");

        const settingsHeaderSolid =
          !settingsSnap.header
          || settingsSnap.header.bg === "rgb(16, 24, 32)"
          || settingsSnap.main?.homeShell
          || settingsSnap.main?.hasThread;

        const settingsMainNoArt =
          !settingsSnap.main
          || settingsSnap.main.homeShell
          || settingsSnap.main.hasThread
          || !settingsSnap.main.hasArt;

        resolve({
          homeSnap,
          settingsSnap,
          homeOk,
          settingsHeaderSolid,
          settingsMainNoArt,
          ok: homeOk && settingsHeaderSolid && settingsMainNoArt
            && (!settingsSnap.signature.present || settingsSnap.signature.display === "none"),
        });
      }, 600);
    });
  })()`,
});

const val = r.result?.result?.value ?? r.result;
console.log(JSON.stringify(val, null, 2));
if (!val?.ok) {
  console.error("VERIFY FAILED");
  process.exitCode = 1;
} else {
  console.log("VERIFY OK");
}
ws.close();
