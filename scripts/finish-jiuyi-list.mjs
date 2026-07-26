import fs from "fs";

const path = "E:/demo/chatgpt-tools/skins/jiuyi/assets/jiuyi-skin.css";
let css = fs.readFileSync(path, "utf8");

// Media query may still have unscoped card button rule
const mediaOld = `  .jiuyi-home .group\\/home-suggestions button {`;
const mediaNew = `  .jiuyi-home .group\\/home-suggestions button:not([class*="home-suggestion-list-item"]) {`;
if (css.includes(mediaOld)) {
  css = css.split(mediaOld).join(mediaNew);
  console.log("OK: scoped media query button rule");
} else {
  console.log("skip: media button already scoped or missing");
}

// If list block still old (heavy #141414), replace again
const listStart = css.indexOf("/*\n * 建议展开列表");
const listEnd = css.indexOf("/*\n * 宿主输入区底部遮罩：");
if (listStart < 0 || listEnd < 0) {
  console.error("FAIL bounds", listStart, listEnd);
  process.exit(1);
}

const nativeList = `/*
 * 建议展开列表 — 对齐宿主原生 DOM（CDP 实测 electron-dark）：
 *   button.group/home-suggestion-list-item
 *     flex min-h-10 items-center gap-1.5 rounded-lg pr-1 text-sm
 *     text-token-description-foreground (~ rgba(255,255,255,.5))
 *     行内 icon size-4；无卡片壳 / 无圆形图标底 / 无朱砂 ♡
 * 列表与卡片同属 section.group/home-suggestions，靠 :not(list-item) 隔离卡片样式。
 */
.jiuyi-home .group\\/home-suggestions:has(.group\\/home-suggestion-list-item),
.jiuyi-home .group\\/home-suggestions:has([class*="home-suggestion-list-item"]) {
  gap: 0 !important;
  overflow: visible !important;
  background: transparent !important;
  border: 0 !important;
  box-shadow: none !important;
  backdrop-filter: none !important;
  -webkit-backdrop-filter: none !important;
}

.jiuyi-home .group\\/home-suggestions .flex.min-h-32,
.jiuyi-home .group\\/home-suggestions div:has(> .group\\/home-suggestion-list-item),
.jiuyi-home .group\\/home-suggestions div:has(> [class*="home-suggestion-list-item"]) {
  background: transparent !important;
  border: 0 !important;
  box-shadow: none !important;
  backdrop-filter: none !important;
}

html.codex-jiuyi-skin .group\\/home-suggestion-list-item,
html.codex-jiuyi-skin button.group\\/home-suggestion-list-item,
html.codex-jiuyi-skin .jiuyi-home button[class*="home-suggestion-list-item"] {
  position: relative !important;
  display: flex !important;
  flex-direction: row !important;
  align-items: center !important;
  justify-content: flex-start !important;
  gap: 6px !important;
  width: 100% !important;
  min-height: 40px !important;
  height: auto !important;
  padding: 0 4px 0 0 !important;
  margin: 0 !important;
  border: 0 !important;
  border-radius: 12.5px !important;
  background: transparent !important;
  background-image: none !important;
  box-shadow: none !important;
  backdrop-filter: none !important;
  -webkit-backdrop-filter: none !important;
  color: rgba(255, 255, 255, 0.5) !important;
  font-size: 13px !important;
  font-weight: 445 !important;
  line-height: 1.4 !important;
  text-align: left !important;
  transform: none !important;
  transition: color 0.12s ease, transform 0.12s ease !important;
}

html.codex-jiuyi-skin .group\\/home-suggestion-list-item:hover,
html.codex-jiuyi-skin button.group\\/home-suggestion-list-item:hover,
html.codex-jiuyi-skin .jiuyi-home button[class*="home-suggestion-list-item"]:hover {
  background: transparent !important;
  background-image: none !important;
  border: 0 !important;
  box-shadow: none !important;
  transform: none !important;
  color: rgb(255, 255, 255) !important;
}

html.codex-jiuyi-skin .group\\/home-suggestion-list-item::after,
html.codex-jiuyi-skin button.group\\/home-suggestion-list-item::after,
html.codex-jiuyi-skin .jiuyi-home button[class*="home-suggestion-list-item"]::after {
  content: none !important;
  display: none !important;
}

html.codex-jiuyi-skin .group\\/home-suggestion-list-item > span:first-child,
html.codex-jiuyi-skin button.group\\/home-suggestion-list-item > span.flex,
html.codex-jiuyi-skin .jiuyi-home button[class*="home-suggestion-list-item"] > span:first-child {
  width: 16px !important;
  height: 16px !important;
  min-width: 16px !important;
  min-height: 16px !important;
  margin: 0 !important;
  padding: 0 !important;
  border: 0 !important;
  border-radius: 0 !important;
  background: transparent !important;
  background-image: none !important;
  box-shadow: none !important;
  display: flex !important;
  align-items: center !important;
  justify-content: center !important;
  flex: 0 0 auto !important;
  color: rgba(255, 255, 255, 0.5) !important;
}

html.codex-jiuyi-skin .group\\/home-suggestion-list-item svg,
html.codex-jiuyi-skin button.group\\/home-suggestion-list-item svg,
html.codex-jiuyi-skin .jiuyi-home button[class*="home-suggestion-list-item"] svg {
  width: 16px !important;
  height: 16px !important;
  margin: 0 !important;
  color: rgba(255, 255, 255, 0.5) !important;
  opacity: 1 !important;
  flex-shrink: 0 !important;
}

html.codex-jiuyi-skin .group\\/home-suggestion-list-item:hover svg,
html.codex-jiuyi-skin button.group\\/home-suggestion-list-item:hover svg {
  color: rgb(255, 255, 255) !important;
}

html.codex-jiuyi-skin .group\\/home-suggestion-list-item > span:last-child,
html.codex-jiuyi-skin .group\\/home-suggestion-list-item span.min-w-0,
html.codex-jiuyi-skin .jiuyi-home button[class*="home-suggestion-list-item"] > span:last-child {
  display: block !important;
  flex: 1 1 auto !important;
  min-width: 0 !important;
  width: auto !important;
  margin: 0 !important;
  padding: 0 !important;
  flex-direction: row !important;
  align-items: center !important;
  justify-content: flex-start !important;
  text-align: left !important;
  color: rgba(255, 255, 255, 0.5) !important;
  font-size: 13px !important;
  font-weight: 445 !important;
  white-space: nowrap !important;
  overflow: hidden !important;
  text-overflow: ellipsis !important;
}

html.codex-jiuyi-skin .group\\/home-suggestion-list-item:hover > span:last-child,
html.codex-jiuyi-skin .group\\/home-suggestion-list-item:hover span.min-w-0,
html.codex-jiuyi-skin .group\\/home-suggestion-list-item:hover .text-token-text-tertiary {
  color: rgb(255, 255, 255) !important;
}

html.codex-jiuyi-skin .group\\/home-suggestion-list-item .text-token-text-tertiary {
  color: rgba(255, 255, 255, 0.5) !important;
}

`;

css = css.slice(0, listStart) + nativeList + css.slice(listEnd);
console.log("OK: native list block");

// Soften portal surface tokens to host dark surface (#181818) instead of pure #141414 chrome box
const portalOld = `--main-surface-primary: #141414 !important;
  --main-surface-secondary: #1a1a1a !important;
  --surface-primary: #1a1a1a !important;
  --surface-secondary: #141414 !important;`;
const portalNew = `--main-surface-primary: #181818 !important;
  --main-surface-secondary: #181818 !important;
  --surface-primary: #181818 !important;
  --surface-secondary: #181818 !important;`;
if (css.includes(portalOld)) {
  css = css.replace(portalOld, portalNew);
  console.log("OK: portal tokens match host #181818");
}

const dataUrlCount = (css.match(/data:image\/png;base64,/g) || []).length;
if (dataUrlCount < 1) {
  console.error("FAIL lost data URL");
  process.exit(1);
}

// Report remaining unscoped
const lines = css.split("\n");
const hits = [];
for (let i = 0; i < lines.length; i++) {
  if (
    lines[i].includes("home-suggestions button") &&
    !lines[i].includes(":not([class*=\"home-suggestion-list-item\"])")
  ) {
    hits.push(`${i + 1}: ${lines[i].trim().slice(0, 120)}`);
  }
}
console.log("unscoped hits:", hits.length);
hits.forEach((h) => console.log(" ", h));

fs.writeFileSync(path, css, "utf8");
console.log("Wrote", path, "bytes", Buffer.byteLength(css, "utf8"));
