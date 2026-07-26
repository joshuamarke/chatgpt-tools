/**
 * Simulate chat → 新建任务 SPA switch and verify home markers re-apply.
 * 1) Click a recent thread in sidebar (if any)
 * 2) Wait
 * 3) Click 新建任务
 * 4) Probe home markers + suggestion styles over a few seconds
 */
import WebSocket from "ws";

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
    setTimeout(() => rej(new Error("timeout " + method)), 25000);
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

const clickText = async (label, opts = {}) => {
  const r = await send("Runtime.evaluate", {
    returnByValue: true,
    expression: `(() => {
      const label = ${JSON.stringify(label)};
      const exact = ${opts.exact === false ? "false" : "true"};
      const items = [...document.querySelectorAll("aside a, aside button, nav a, nav button, [class*='sidebar'] a, [class*='sidebar'] button")];
      let target = items.find((el) => {
        const t = ((el.innerText || "") + " " + (el.getAttribute("aria-label") || ""))
          .replace(/\\s+/g, " ")
          .trim();
        return exact ? t === label || t.startsWith(label) : t.includes(label);
      });
      // fallback: first thread-like row under sidebar if label is __thread__
      if (!target && label === "__thread__") {
        target = items.find((el) => {
          const t = ((el.innerText || "") + "").replace(/\\s+/g, " ").trim();
          if (!t || t.length < 4) return false;
          if (/新建任务|已安排|插件|搜索|设置|工作区|项目|模式/.test(t)) return false;
          const r = el.getBoundingClientRect();
          return r.y > 120 && r.height > 20 && r.height < 80;
        });
      }
      if (!target) return { ok: false, label };
      target.click();
      return {
        ok: true,
        text: (target.innerText || target.getAttribute("aria-label") || "").slice(0, 60),
      };
    })()`,
  });
  return r.result?.result?.value;
};

const probe = async (tag) => {
  const r = await send("Runtime.evaluate", {
    returnByValue: true,
    expression: `(() => {
      const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
      const st = window.__CODEX_MORTAL_SKIN_STATE__ || window.__CODEX_DREAM_SKIN_STATE__ || window.__CODEX_EVA_SKIN_STATE__;
      const homeIcon = document.querySelector('[data-testid="home-icon"]');
      const gameSource = document.querySelector('[data-feature="game-source"]');
      const suggestions = document.querySelector('[class*="home-suggestions"]');
      const roleMain = document.querySelector('[role="main"]');
      const shell = document.querySelector("main.main-surface");
      const sugBtn = suggestions?.querySelector("button");
      const cs = sugBtn ? getComputedStyle(sugBtn) : null;
      return {
        tag: ${JSON.stringify(tag)},
        lifeMode: st?.lifeMode || host?.getActive?.()?.lifeMode || null,
        metrics: st?.metrics || host?.getActive?.()?.metrics || null,
        hasHomeIcon: !!homeIcon,
        hasGameSource: !!gameSource,
        hasSuggestions: !!suggestions,
        roleHome: roleMain ? [...roleMain.classList].filter((c) => /home/.test(c)) : null,
        shellHome: shell ? [...shell.classList].filter((c) => /home/.test(c)) : null,
        homeEls: [...document.querySelectorAll("[class]")]
          .filter((el) => /(?:^|\\s)[\\w-]*-home(?:-shell|-utility)?(?:\\s|$)/.test(String(el.className)))
          .slice(0, 10)
          .map((el) => ({
            tag: el.tagName,
            role: el.getAttribute("role"),
            cls: [...el.classList].filter((c) => /home/.test(c)).join(" "),
          })),
        sug: cs
          ? {
              color: cs.color,
              bg: cs.backgroundColor,
              shadow: cs.boxShadow.slice(0, 80),
              radius: cs.borderRadius,
            }
          : null,
        heroColor: gameSource ? getComputedStyle(gameSource).color : null,
        root: document.documentElement.className.slice(0, 160),
      };
    })()`,
  });
  return r.result?.result?.value;
};

console.log("STEP click thread", await clickText("__thread__"));
await new Promise((r) => setTimeout(r, 1800));
console.log("AFTER_THREAD", JSON.stringify(await probe("after-thread"), null, 2));

console.log("STEP click 新建任务", await clickText("新建任务"));
const snaps = [];
for (const ms of [200, 600, 1200, 2500, 4000]) {
  await new Promise((r) => setTimeout(r, ms === 200 ? 200 : ms - (snaps.length ? [200, 600, 1200, 2500][snaps.length - 1] : 0)));
  snaps.push(await probe(`t+${ms}ms`));
}
console.log("SNAPS", JSON.stringify(snaps, null, 2));
ws.close();
