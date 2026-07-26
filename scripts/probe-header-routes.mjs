/**
 * Probe app-header-tint styles/DOM on current route (home vs settings).
 */
import WebSocket from "ws";
import { writeFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "probe-header-routes-out.json.txt");
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
    const fullCls = (el) => String(el?.className || "");
    const styleOf = (el) => {
      if (!el) return null;
      const cs = getComputedStyle(el);
      const rect = el.getBoundingClientRect();
      return {
        tag: el.tagName,
        cls: fullCls(el).slice(0, 220),
        bg: cs.backgroundColor,
        bgImg: (cs.backgroundImage || "").slice(0, 200),
        backdrop: (cs.backdropFilter || "none").slice(0, 80),
        borderBottom: cs.borderBottomWidth + " " + cs.borderBottomColor,
        color: cs.color,
        textShadow: (cs.textShadow || "").slice(0, 80),
        opacity: cs.opacity,
        position: cs.position,
        zIndex: cs.zIndex,
        y: Math.round(rect.top),
        h: Math.round(rect.height),
        w: Math.round(rect.width),
        text: (el.innerText || "").replace(/\\s+/g, " ").trim().slice(0, 80),
      };
    };

    const root = document.documentElement;
    const main = document.querySelector("main.main-surface");
    const header = document.querySelector("main.main-surface > header.app-header-tint")
      || document.querySelector("header.app-header-tint")
      || document.querySelector("main header");

    // Route signals
    const hasHome = !!document.querySelector(".jiuyi-home, [data-testid='home-icon'], [data-feature='game-source'], .group\\\\/home-suggestions");
    const hasThread = !!document.querySelector(".thread-scroll-container");
    const hasSettings = /常规|外观|配置|账户|权限|设置/.test(document.body.innerText || "")
      && !!document.querySelector("button[aria-label='常规'], button[aria-label='配置'], button[aria-label='外观']");
    const settingsNav = [...document.querySelectorAll("button")].filter((b) => {
      const t = (b.getAttribute("aria-label") || b.innerText || "").trim();
      return /^(常规|外观|配置|账户|语音|个性化|键盘快捷键|集成)$/.test(t);
    }).map((b) => b.getAttribute("aria-label") || b.innerText?.trim());

    // Main surface under header
    const mainStyle = main ? {
      bg: getComputedStyle(main).backgroundColor,
      bgImg: (getComputedStyle(main).backgroundImage || "").slice(0, 250),
      cls: fullCls(main).slice(0, 120),
      homeShell: main.classList.contains("jiuyi-home-shell") || main.classList.contains("dream-home-shell"),
      classes: [...main.classList].filter((c) => /home|shell|jiuyi|dream|main/i.test(c)),
    } : null;

    // First solid content under header in main
    const underHeader = [];
    if (main) {
      for (const el of main.querySelectorAll("div, section, form, nav")) {
        const r = el.getBoundingClientRect();
        if (r.top < 70 || r.top > 160) continue;
        if (r.width < 200 || r.height < 40) continue;
        const cs = getComputedStyle(el);
        if (cs.backgroundColor === "rgba(0, 0, 0, 0)" && cs.backgroundImage === "none") continue;
        underHeader.push({
          ...styleOf(el),
          top: Math.round(r.top),
        });
        if (underHeader.length >= 8) break;
      }
    }

    // Chrome nodes: signature / brand
    const chrome = document.getElementById("codex-jiuyi-skin-chrome");
    const signatures = [...document.querySelectorAll(".jiuyi-signature, .jiuyi-brand, [class*='signature']")]
      .map((el) => ({
        ...styleOf(el),
        display: getComputedStyle(el).display,
        visibility: getComputedStyle(el).visibility,
        html: el.outerHTML.slice(0, 200),
      }));

    // Host default header rules (matched via walk)
    const headerRules = [];
    if (header) {
      for (const sheet of document.styleSheets) {
        let rules;
        try { rules = sheet.cssRules; } catch { continue; }
        if (!rules) continue;
        for (const rule of rules) {
          if (!rule.selectorText || !rule.style) continue;
          if (!/app-header-tint|header/i.test(rule.selectorText)) continue;
          const bg = rule.style.getPropertyValue("background")
            || rule.style.getPropertyValue("background-color")
            || rule.style.getPropertyValue("background-image");
          if (!bg && !/background|backdrop|border/i.test(rule.cssText || "")) continue;
          try {
            if (header.matches(rule.selectorText.split(",")[0].trim()) || rule.selectorText.includes("app-header-tint")) {
              headerRules.push({
                sel: rule.selectorText.slice(0, 180),
                text: (rule.cssText || "").slice(0, 280),
                sheet: (sheet.ownerNode?.id || sheet.href || "x").toString().slice(-40),
              });
              if (headerRules.length >= 20) break;
            }
          } catch {}
        }
        if (headerRules.length >= 20) break;
      }
    }

    // Children of header
    const headerKids = header ? [...header.children].slice(0, 8).map(styleOf) : [];

    // Is dream-art-wide forcing transparent header?
    const artWideHeader = getComputedStyle(document.documentElement).getPropertyValue("--dream-art-position");

    return {
      rootCls: [...root.classList],
      hasHome,
      hasThread,
      hasSettings,
      settingsNav,
      header: styleOf(header),
      headerKids,
      mainStyle,
      underHeader,
      signatures,
      chromeHtml: chrome ? chrome.innerHTML.slice(0, 500) : null,
      chromeDisplay: chrome ? getComputedStyle(chrome).display : null,
      headerRules: headerRules.slice(0, 15),
      artWideHeader,
    };
  })()`,
});

const val = r.result?.result?.value ?? r.result;
writeFileSync(OUT, JSON.stringify(val, null, 2), "utf8");
console.log(JSON.stringify(val, null, 2).slice(0, 12000));
console.log("wrote", OUT);
ws.close();
