/**
 * Live host contract probe: selectors.json anchors + dream tokens + skin markers.
 * Requires Codex with CDP on 127.0.0.1:9335.
 * Usage: node scripts/probe-contract-live.mjs
 */
import WebSocket from "ws";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..");
const contractPath = path.join(root, "engine", "runtime", "selectors.json");
const contract = JSON.parse(fs.readFileSync(contractPath, "utf8"));

const port = Number(process.env.CODEX_DEBUG_PORT || 9335);
const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = pages.find((p) => p.type === "page") || pages[0];
if (!page?.webSocketDebuggerUrl) {
  console.log(JSON.stringify({ ok: false, reason: "no-cdp-page", port }, null, 2));
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
    setTimeout(() => rej(new Error("timeout " + method)), 30000);
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
  const q = (sel) => {
    try { return document.querySelector(sel); } catch { return null; }
  };
  const qa = (sel) => {
    try { return [...document.querySelectorAll(sel)]; } catch { return []; }
  };
  const box = (el) => {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return {
      tag: el.tagName,
      id: el.id || null,
      role: el.getAttribute("role"),
      testid: el.getAttribute("data-testid"),
      feature: el.getAttribute("data-feature"),
      cls: String(el.className || "").slice(0, 240),
      text: (el.innerText || "").replace(/\\s+/g, " ").trim().slice(0, 90),
      w: Math.round(r.width),
      h: Math.round(r.height),
      visible: el.getClientRects().length > 0 && r.width > 0 && r.height > 0,
      color: cs.color,
      bg: cs.backgroundColor,
      bgImg: (cs.backgroundImage || "").slice(0, 100),
    };
  };

  const contract = ${JSON.stringify(contract.selectors)};
  const appearance = ${JSON.stringify(contract.appearanceSignal)};
  const stableTestids = ${JSON.stringify(contract.stableTestids || [])};

  const results = contract.map((entry) => {
    let el = q(entry.selector);
    let via = el ? "primary" : null;
    if (!el && Array.isArray(entry.fallback)) {
      for (const fb of entry.fallback) {
        el = q(fb);
        if (el) {
          via = "fallback:" + fb;
          break;
        }
      }
    }
    // home-suggestions special: also try unescaped class*
    if (!el && entry.key === "home-suggestions") {
      el = q('[class*="home-suggestions"]');
      if (el) via = "alt:class*=home-suggestions";
    }
    return {
      key: entry.key,
      tier: entry.tier,
      required: !!entry.required,
      scope: entry.scope,
      selector: entry.selector,
      found: Boolean(el),
      via,
      count: el ? qa(entry.selector).length || 1 : 0,
      sample: box(el),
    };
  });

  const root = document.documentElement;
  const cs = getComputedStyle(root);
  const dreamVars = [
    "--dream-art",
    "--dream-art-position",
    "--dream-focus-x",
    "--dream-focus-y",
    "--dream-accent",
    "--dream-accent-ink",
    "--dream-image-luma",
    "--dream-canvas",
    "--dream-sidebar",
    "--dream-surface-raised",
    "--dream-text",
    "--dream-line",
    "--dream-ink",
    "--cg-text",
    "--cg-immersive-composer",
    "--cg-surface-raised",
    "--cg-primary-button-ink",
    "--cg-immersive-edge",
    "--cg-immersive-mid",
    "--cg-immersive-far",
    "--cg-immersive-sidebar",
    "--cg-immersive-line",
  ];
  const vars = {};
  for (const v of dreamVars) {
    const val = cs.getPropertyValue(v).trim();
    vars[v] = val
      ? val.startsWith("url(")
        ? "url(...)"
        : val.slice(0, 100)
      : null;
  }

  const styleArt = {};
  for (const p of String(root.getAttribute("style") || "").split(";")) {
    const m = p.trim().match(/^(--[\\w-]+)\\s*:\\s*(.+)$/);
    if (m && /(art|dream|cg-|skins-|bengong|jiuyi|mortal|qingkong|linglong)/i.test(m[1])) {
      styleArt[m[1]] = m[2].startsWith("url(") ? "url(...)" : m[2].slice(0, 80);
    }
  }

  const classes = [...root.classList];
  const markerClasses = classes.filter(
    (c) =>
      c.startsWith("dream-") ||
      c.startsWith("skins-") ||
      c.startsWith("codex-") ||
      /skin/i.test(c)
  );

  const attrs = {
    dataChatgptToolsSkin: root.getAttribute("data-chatgpt-tools-skin"),
    dataDreamShell: root.getAttribute("data-dream-shell"),
    dataSkinContract: root.getAttribute("data-skin-contract"),
    electronDark: root.classList.contains("electron-dark"),
    electronLight: root.classList.contains("electron-light"),
    appearanceDarkHit: !!q(appearance.dark),
    appearanceLightHit: !!q(appearance.light),
  };

  const host = window.__CHATGPT_TOOLS_SKIN_HOST__;
  let hostInfo = null;
  if (host) {
    hostInfo = {
      keys: Object.keys(host).slice(0, 50),
      hasEnsure: typeof host.ensure === "function",
      hasApplyArt: typeof host.applyArt === "function",
      hasApplySkin: typeof host.applySkin === "function",
    };
    try {
      host.ensure?.({ root: true, route: true, layout: true });
    } catch {
      /* ignore */
    }
  }

  // re-check home-shell after ensure
  const homeShellAfter = results.find((r) => r.key === "home-shell");
  if (homeShellAfter) {
    const el = q('main.main-surface[class*="home-shell"]');
    homeShellAfter.foundAfterEnsure = Boolean(el);
    homeShellAfter.sampleAfterEnsure = box(el);
  }

  const chrome = qa('[id$="-skin-chrome"], [id*="-skin-chrome"]').map((el) => ({
    id: el.id,
    parent: el.parentElement?.tagName,
    cls: String(el.className).slice(0, 160),
    childCount: el.children.length,
    sample: box(el),
  }));
  const styleTags = qa('style[id*="skin"], style[id*="codex"]').map((el) => ({
    id: el.id,
    rev: el.dataset.skinRevision,
    ver: el.dataset.skinVersion,
    len: (el.textContent || "").length,
  }));

  const presentTestids = [
    ...new Set(qa("[data-testid]").map((el) => el.getAttribute("data-testid"))),
  ].sort();
  const presentFeatures = [
    ...new Set(qa("[data-feature]").map((el) => el.getAttribute("data-feature"))),
  ].sort();

  const stableTestidHits = Object.fromEntries(
    stableTestids.map((t) => [t, presentTestids.includes(t)])
  );

  const altSignals = {
    homeMainContent: box(q('[class*="home-main-content"]')),
    threadScroll: box(q(".thread-scroll-container")),
    proseMirror: box(q(".ProseMirror")),
    appShellLeftAny: box(q('[class*="app-shell-left"]')),
    applicationMenu: box(q('[class~="group/application-menu-top-bar"]')),
    mainSurfacePrimaryCount: qa('[class*="bg-token-main-surface"]').length,
    tokenForegroundBtns: qa('button[class~="bg-token-foreground"]').length,
    composerAny: box(q('[class*="composer"]')),
    stickySearch: box(q('div.sticky:has(input[type="text"])')),
  };

  // candidate new anchors (class tokens containing shell/header/composer/home)
  const interestingClassTokens = new Map();
  for (const el of qa("[class]").slice(0, 4000)) {
    for (const c of String(el.className).split(/\\s+/)) {
      if (
        /main-surface|app-header|app-shell|composer|home-|thread-|utility|suggestion|ProseMirror/i.test(
          c
        )
      ) {
        interestingClassTokens.set(c, (interestingClassTokens.get(c) || 0) + 1);
      }
    }
  }
  const topInteresting = [...interestingClassTokens.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 80)
    .map(([c, n]) => ({ class: c, count: n }));

  const bodyCs = getComputedStyle(document.body);
  const shell = q("main.main-surface");
  const shellCs = shell ? getComputedStyle(shell) : null;

  // engine marker classes on body tree
  const engineMarks = {
    homeClassNodes: qa("[class*='-home']")
      .filter((el) =>
        [...el.classList].some((c) => /^(?:[\\w-]+-)?home(?:-shell|-utility)?$/.test(c) || c.endsWith("-home") || c.endsWith("-home-shell") || c.endsWith("-home-utility"))
      )
      .slice(0, 20)
      .map((el) => ({
        tag: el.tagName,
        role: el.getAttribute("role"),
        marks: [...el.classList].filter(
          (c) => /home|shell|utility/.test(c) && !c.includes("_")
        ),
      })),
  };

  return {
    ok: true,
    title: document.title,
    href: location.href,
    userAgent: navigator.userAgent.slice(0, 120),
    attrs,
    markerClasses,
    vars,
    styleArt,
    hostInfo,
    chrome,
    styleTags,
    contract: results,
    stableTestidHits,
    presentTestids: presentTestids.slice(0, 100),
    presentFeatures,
    altSignals,
    topInteresting,
    engineMarks,
    bodyBg: {
      color: bodyCs.backgroundColor,
      image: (bodyCs.backgroundImage || "").slice(0, 140),
      size: bodyCs.backgroundSize,
      position: bodyCs.backgroundPosition,
      attachment: bodyCs.backgroundAttachment,
    },
    shellBg: shellCs
      ? {
          color: shellCs.backgroundColor,
          image: (shellCs.backgroundImage || "").slice(0, 140),
        }
      : null,
    windowKeys: Object.keys(window)
      .filter((k) => /SKIN|CODEX|CHATGPT_TOOLS|DREAM/i.test(k))
      .slice(0, 50),
  };
})()`;

const r = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: expr,
});
const value = r.result?.result?.value;
if (!value) {
  console.log(JSON.stringify({ ok: false, raw: r }, null, 2));
  process.exit(1);
}

// summarize
const missingL1 = (value.contract || []).filter(
  (x) => x.tier === "L1" && x.required && !x.found
);
const missingL2 = (value.contract || []).filter((x) => x.tier === "L2" && !x.found);
const present = (value.contract || []).filter((x) => x.found);

const summary = {
  ok: true,
  packageHint: "live-probe",
  route: {
    title: value.title,
    href: value.href,
    homeish:
      value.contract?.find((c) => c.key === "home-icon")?.found ||
      value.contract?.find((c) => c.key === "game-source")?.found,
  },
  skin: {
    dataSkin: value.attrs?.dataChatgptToolsSkin,
    rootMarkers: value.markerClasses,
    chrome: value.chrome,
    styleTags: value.styleTags,
    host: value.hostInfo,
  },
  contractSummary: {
    total: value.contract?.length || 0,
    present: present.map((p) => p.key),
    missingRequiredL1: missingL1.map((p) => p.key),
    missingL2: missingL2.map((p) => p.key),
    details: value.contract,
  },
  tokens: {
    vars: value.vars,
    styleArt: value.styleArt,
    bodyBg: value.bodyBg,
    shellBg: value.shellBg,
  },
  appearance: {
    electronDark: value.attrs?.electronDark,
    electronLight: value.attrs?.electronLight,
    darkSignal: value.attrs?.appearanceDarkHit,
    lightSignal: value.attrs?.appearanceLightHit,
  },
  hostInventory: {
    stableTestidHits: value.stableTestidHits,
    presentTestids: value.presentTestids,
    presentFeatures: value.presentFeatures,
    altSignals: value.altSignals,
    topInteresting: value.topInteresting,
    engineMarks: value.engineMarks,
  },
};

const outPath = path.join(here, "probe-contract-live-out.json.txt");
fs.writeFileSync(outPath, JSON.stringify(summary, null, 2), "utf8");
console.log(JSON.stringify(summary, null, 2));
console.error("wrote " + outPath);
ws.close();
