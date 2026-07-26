import fs from "fs";

const path = "E:/demo/chatgpt-tools/skins/jiuyi/assets/jiuyi-skin.css";
let css = fs.readFileSync(path, "utf8");

function mustReplace(label, oldStr, newStr) {
  if (!css.includes(oldStr)) {
    console.error(`FAIL: ${label} not found`);
    process.exit(1);
  }
  css = css.replace(oldStr, newStr);
  console.log(`OK: ${label}`);
}

// 1) Restore suggestion card background
mustReplace(
  "suggestion card bg",
  `  border: 1px solid var(--jiuyi-glass-border) !important;
  border-radius: 16px !important;
  /* background: */
    /* radial-gradient(circle at 50% 18%, rgba(255, 255, 255, 0.08), transparent 42%), */
    /* linear-gradient(160deg, rgba(36, 48, 64, 0.78), rgba(22, 30, 42, 0.84)) !important; */
  /* color: #e6e2d8 !important; */
  font-weight: 600 !important;`,
  `  border: 1px solid var(--jiuyi-glass-border) !important;
  border-radius: 16px !important;
  background:
    radial-gradient(circle at 50% 18%, rgba(255, 255, 255, 0.08), transparent 42%),
    linear-gradient(160deg, rgba(36, 48, 64, 0.78), rgba(22, 30, 42, 0.84)) !important;
  color: #e6e2d8 !important;
  font-weight: 600 !important;`
);

// 2) Insert native list-item styles before composer fade comment
mustReplace(
  "list-item styles",
  `.jiuyi-home .group\\/home-suggestions button > span:last-child {
  flex-direction: column !important;
  align-items: center !important;
  justify-content: center !important;
  text-align: center !important;
  color: #d8d4c8 !important;
  font-size: 13px !important;
}


/*
 * 宿主输入区底部遮罩：`,
  `.jiuyi-home .group\\/home-suggestions button > span:last-child {
  flex-direction: column !important;
  align-items: center !important;
  justify-content: center !important;
  text-align: center !important;
  color: #d8d4c8 !important;
  font-size: 13px !important;
}

/*
 * 建议展开列表（home-suggestion-list-item）：对齐宿主原生 electron-dark
 * 深色实心底 + 行图标 + 浅色正文，无毛玻璃卡片壳。
 */
html.codex-jiuyi-skin [class*="home-suggestion-list"],
html.codex-jiuyi-skin [class*="HomeSuggestionList"],
html.codex-jiuyi-skin [data-testid*="home-suggestion-list"],
html.codex-jiuyi-skin [role="listbox"]:has([class*="home-suggestion"]),
html.codex-jiuyi-skin [role="menu"]:has([class*="home-suggestion"]) {
  color-scheme: dark !important;
  background: #141414 !important;
  background-color: #141414 !important;
  border: 1px solid rgba(255, 255, 255, 0.08) !important;
  border-radius: 12px !important;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.55) !important;
  color: #ececec !important;
  backdrop-filter: none !important;
  -webkit-backdrop-filter: none !important;
}

html.codex-jiuyi-skin [class*="home-suggestion-list-item"],
html.codex-jiuyi-skin [class*="HomeSuggestionListItem"],
html.codex-jiuyi-skin button[class*="home-suggestion-list"],
html.codex-jiuyi-skin [role="option"][class*="suggestion"],
html.codex-jiuyi-skin [role="menuitem"]:has(svg):has(span) {
  display: flex !important;
  align-items: center !important;
  gap: 12px !important;
  width: 100% !important;
  min-height: 44px !important;
  padding: 10px 14px !important;
  margin: 0 !important;
  border: 0 !important;
  border-radius: 0 !important;
  background: transparent !important;
  background-image: none !important;
  box-shadow: none !important;
  backdrop-filter: none !important;
  -webkit-backdrop-filter: none !important;
  color: #e8e8e8 !important;
  font-weight: 450 !important;
  font-size: 14px !important;
  text-align: left !important;
  justify-content: flex-start !important;
  transform: none !important;
}

html.codex-jiuyi-skin [class*="home-suggestion-list-item"]:hover,
html.codex-jiuyi-skin [class*="HomeSuggestionListItem"]:hover,
html.codex-jiuyi-skin button[class*="home-suggestion-list"]:hover,
html.codex-jiuyi-skin [role="option"][class*="suggestion"]:hover {
  background: rgba(255, 255, 255, 0.06) !important;
  transform: none !important;
  box-shadow: none !important;
  border-color: transparent !important;
}

/* 列表行：取消卡片式圆形图标底与朱砂 ♡ */
html.codex-jiuyi-skin [class*="home-suggestion-list-item"]::after,
html.codex-jiuyi-skin [class*="HomeSuggestionListItem"]::after,
html.codex-jiuyi-skin button[class*="home-suggestion-list"]::after {
  content: none !important;
  display: none !important;
}

html.codex-jiuyi-skin [class*="home-suggestion-list-item"] span:has(> svg),
html.codex-jiuyi-skin [class*="home-suggestion-list-item"] > span:first-child,
html.codex-jiuyi-skin [class*="HomeSuggestionListItem"] span:has(> svg) {
  width: 20px !important;
  height: 20px !important;
  min-width: 20px !important;
  min-height: 20px !important;
  margin: 0 !important;
  border: 0 !important;
  border-radius: 0 !important;
  background: transparent !important;
  box-shadow: none !important;
  flex: 0 0 auto !important;
}

html.codex-jiuyi-skin [class*="home-suggestion-list-item"] svg,
html.codex-jiuyi-skin [class*="HomeSuggestionListItem"] svg,
html.codex-jiuyi-skin button[class*="home-suggestion-list"] svg {
  width: 18px !important;
  height: 18px !important;
  color: #bdbdbd !important;
  opacity: 0.95 !important;
}

html.codex-jiuyi-skin [class*="home-suggestion-list-item"] span,
html.codex-jiuyi-skin [class*="HomeSuggestionListItem"] span {
  justify-content: flex-start !important;
  text-align: left !important;
  color: #e8e8e8 !important;
  font-size: 14px !important;
  flex-direction: row !important;
}


/*
 * 宿主输入区底部遮罩：`
);

// 3) Widen fade layer vs composer track
mustReplace(
  "fade widen",
  `  --tw-gradient-from: transparent !important;
  --tw-gradient-to: transparent !important;
  --tw-gradient-stops: transparent !important;
}
/* ── 输入框：雨夜毛玻璃 ── */`,
  `  --tw-gradient-from: transparent !important;
  --tw-gradient-to: transparent !important;
  --tw-gradient-stops: transparent !important;
}

/*
 * 遮罩层适当宽于输入轨（宿主 fade 与 composer 同 max-w 时两侧会漏透）。
 * 在 --thread-content-max-width 基础上外扩 ~72px，并去掉过窄的 px 约束。
 */
html.codex-jiuyi-skin div.pointer-events-none.absolute.inset-x-0.bottom-0 > div[class*="bg-gradient-to-t"][class*="from-token-main-surface-primary"],
html.codex-jiuyi-skin.dream-art-wide main.main-surface .thread-scroll-container .bg-gradient-to-t.from-token-main-surface-primary,
html.codex-jiuyi-skin main.main-surface div.pointer-events-none.absolute.inset-x-0.bottom-0 > div.mx-auto[class*="bg-gradient-to-t"] {
  max-width: min(calc(var(--thread-content-max-width, 48rem) + 72px), 100%) !important;
  width: 100% !important;
  box-sizing: border-box !important;
  padding-inline: 0 !important;
  margin-inline: auto !important;
}

/* ── 输入框：雨夜毛玻璃 ── */`
);

// 4) Restore portal/dialog dark surface tokens (layout-safe)
mustReplace(
  "portal dialog tokens",
  `/*
 * 建议卡点击后的弹出列表 / 菜单 / listbox：不改表面与布局，
 * 交给宿主原始样式（electron-dark 壳自带）。
 * 仅对设置类 dialog 补 token，避免亮色表单；不碰 menu/listbox 外观。
 */
/* html.codex-jiuyi-skin [role="dialog"],
html.codex-jiuyi-skin [role="alertdialog"],
html.codex-jiuyi-skin [class*="Modal"],
html.codex-jiuyi-skin [class*="Dialog"],
html.codex-jiuyi-skin.dream-theme-dark [role="dialog"],
html.codex-jiuyi-skin.dream-theme-dark [class*="Dialog"],
html.codex-jiuyi-skin.dream-theme-dark [class*="Modal"] {
  color-scheme: dark !important;
  --main-surface-primary: #141c28 !important;
  --main-surface-secondary: #1a2330 !important;
  --surface-primary: #1a2330 !important;
  --surface-secondary: #141c28 !important;
  --text-primary: #e8e4dc !important;
  --text-secondary: #b8b2a6 !important;
  --text-tertiary: #9aa8b8 !important;
  --icon-primary: #d8d4c8 !important;
  --icon-secondary: #a8a296 !important;
  --border-primary: rgba(140, 160, 185, 0.22) !important;
  --border-secondary: rgba(140, 160, 185, 0.14) !important;
}

html.codex-jiuyi-skin [role="dialog"] input,
html.codex-jiuyi-skin [role="dialog"] textarea,
html.codex-jiuyi-skin [role="dialog"] select,
html.codex-jiuyi-skin [role="alertdialog"] input,
html.codex-jiuyi-skin [role="alertdialog"] textarea,
html.codex-jiuyi-skin [class*="Dialog"] input,
html.codex-jiuyi-skin [class*="Modal"] input {
  color: #e8e4dc !important;
  background-color: rgba(14, 20, 28, 0.92) !important;
  border-color: rgba(140, 160, 185, 0.28) !important;
  caret-color: var(--jiuyi-cinnabar) !important;
} */`,
  `/*
 * 门户弹层（menu / listbox / dialog）：只补 dark 表面 token，
 * 不改布局/圆角/阴影，避免盖掉宿主 electron-dark 原生外观。
 * 解决 body 上 portal 吃不到 :root 浅色默认、或透明主区透出亮底的问题。
 */
html.codex-jiuyi-skin [role="menu"],
html.codex-jiuyi-skin [role="listbox"],
html.codex-jiuyi-skin [role="dialog"],
html.codex-jiuyi-skin [role="alertdialog"],
html.codex-jiuyi-skin [data-radix-popper-content-wrapper],
html.codex-jiuyi-skin [data-radix-menu-content],
html.codex-jiuyi-skin [data-radix-select-content],
html.codex-jiuyi-skin [class*="Popover"],
html.codex-jiuyi-skin [class*="Dropdown"],
html.codex-jiuyi-skin [class*="Modal"],
html.codex-jiuyi-skin [class*="Dialog"] {
  color-scheme: dark !important;
  --main-surface-primary: #141414 !important;
  --main-surface-secondary: #1a1a1a !important;
  --surface-primary: #1a1a1a !important;
  --surface-secondary: #141414 !important;
  --text-primary: #ececec !important;
  --text-secondary: #bdbdbd !important;
  --text-tertiary: #9a9a9a !important;
  --icon-primary: #e0e0e0 !important;
  --icon-secondary: #a8a8a8 !important;
  --border-primary: rgba(255, 255, 255, 0.1) !important;
  --border-secondary: rgba(255, 255, 255, 0.06) !important;
  --composer-background: #141414 !important;
}

/* 设置类 dialog 表单可读性（不碰 menu 布局） */
html.codex-jiuyi-skin [role="dialog"] input,
html.codex-jiuyi-skin [role="dialog"] textarea,
html.codex-jiuyi-skin [role="dialog"] select,
html.codex-jiuyi-skin [role="alertdialog"] input,
html.codex-jiuyi-skin [role="alertdialog"] textarea,
html.codex-jiuyi-skin [class*="Dialog"] input,
html.codex-jiuyi-skin [class*="Modal"] input {
  color: #e8e4dc !important;
  background-color: rgba(20, 20, 20, 0.96) !important;
  border-color: rgba(255, 255, 255, 0.12) !important;
  caret-color: var(--jiuyi-cinnabar) !important;
}`
);

const dataUrlCount = (css.match(/data:image\/png;base64,/g) || []).length;
console.log("data URLs:", dataUrlCount);
if (dataUrlCount < 1) {
  console.error("FAIL: lost composer-bg data URL");
  process.exit(1);
}

fs.writeFileSync(path, css, "utf8");
console.log("Wrote", path, "bytes", Buffer.byteLength(css, "utf8"));
