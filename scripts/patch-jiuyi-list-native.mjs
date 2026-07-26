import fs from "fs";

const path = "E:/demo/chatgpt-tools/skins/jiuyi/assets/jiuyi-skin.css";
let css = fs.readFileSync(path, "utf8");

// Scope card chrome to cards only — list rows share the same section as buttons.
const cardOnly = `.jiuyi-home .group\\/home-suggestions button:not([class*="home-suggestion-list-item"])`;
const replacements = [
  [`.jiuyi-home .group\\/home-suggestions button::after`, `${cardOnly}::after`],
  [`.jiuyi-home .group\\/home-suggestions button:hover`, `${cardOnly}:hover`],
  [`.jiuyi-home .group\\/home-suggestions button > span:first-child > span:first-child,
.jiuyi-home .group\\/home-suggestions button span:has(> svg),
.jiuyi-home .group\\/home-suggestions button span:has(> span > svg)`, `${cardOnly} > span:first-child > span:first-child,
${cardOnly} span:has(> svg),
${cardOnly} span:has(> span > svg)`],
  [`.jiuyi-home .group\\/home-suggestions button svg`, `${cardOnly} svg`],
  [`.jiuyi-home .group\\/home-suggestions button > span:last-child`, `${cardOnly} > span:last-child`],
  [`.jiuyi-home .group\\/home-suggestions button > span`, `${cardOnly} > span`],
  // base button rule last so earlier partials don't double-match inside it
  [`.jiuyi-home .group\\/home-suggestions button {`, `${cardOnly} {`],
];

// media query at end
replacements.push([
  `  .jiuyi-home .group\\/home-suggestions button {`,
  `  ${cardOnly} {`,
]);

for (const [from, to] of replacements) {
  if (!css.includes(from)) {
    console.error("FAIL missing selector:", from.slice(0, 80));
    process.exit(1);
  }
  css = css.split(from).join(to);
  console.log("OK scoped:", from.split("\n")[0].slice(0, 70));
}

// Replace the previous heavy list block with host-native restore.
const listStart = css.indexOf("/*\n * 建议展开列表（home-suggestion-list-item）");
const listEnd = css.indexOf("/*\n * 宿主输入区底部遮罩：");
if (listStart < 0 || listEnd < 0 || listEnd <= listStart) {
  console.error("FAIL list block bounds", listStart, listEnd);
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

/* 列表容器（宿主: flex min-h-32 flex-col justify-end py-2 pl-6）保持原生 */
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
  /* 宿主几何：flex row / min-h-10 / gap-1.5 / rounded-lg / pr-1 / text-sm */
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

/* 取消卡片朱砂 ♡ */
html.codex-jiuyi-skin .group\\/home-suggestion-list-item::after,
html.codex-jiuyi-skin button.group\\/home-suggestion-list-item::after,
html.codex-jiuyi-skin .jiuyi-home button[class*="home-suggestion-list-item"]::after {
  content: none !important;
  display: none !important;
}

/* 图标槽：宿主 size-4，无圆形玻璃底 */
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

/* 文案：宿主 truncate + text-token-text-tertiary；hover 提亮 */
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
console.log("OK: native list restore");

// Soften portal tokens: keep dialog dark, don't force #141414 shell that fights host.
// Leave as-is if already patched — optional tighten of menuitem :has rule removed already.

const dataUrlCount = (css.match(/data:image\/png;base64,/g) || []).length;
if (dataUrlCount < 1) {
  console.error("FAIL lost data URL");
  process.exit(1);
}

// Ensure we didn't leave unscoped card button rules that still hit list items
const bad = css.match(/\.jiuyi-home \.group\\\/home-suggestions button(?!:not)/g);
if (bad && bad.length) {
  console.warn("WARN still unscoped button rules:", bad.length);
  // show contexts
  let from = 0;
  for (let i = 0; i < Math.min(bad.length, 8); i++) {
    const idx = css.indexOf(bad[i], from);
    console.warn(JSON.stringify(css.slice(idx, idx + 90)));
    from = idx + 1;
  }
}

fs.writeFileSync(path, css, "utf8");
console.log("Wrote", path, "bytes", Buffer.byteLength(css, "utf8"));
