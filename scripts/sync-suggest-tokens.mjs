/**
 * Sync skins/* suggestion CARD/LIST styles to engine contract:
 *  - CARD: keep skin art; require :not(list-item); set --skins-suggest-card-* where useful
 *  - LIST: remove per-skin layout/paint blocks; set --skins-suggest-list-* tokens only
 *    (engine applies when defined; unset = host native)
 *
 * Usage: node scripts/sync-suggest-tokens.mjs
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const skinsDir = path.join(root, "skins");

/** Per-skin list theme tokens (only skins that previously themed the list). */
const LIST_TOKENS = {
  bengong: `
  /* 推荐列表 opt-in（引擎 --skins-suggest-list-*；未设则宿主原生） */
  --skins-suggest-list-color: rgba(74, 56, 52, 0.58);
  --skins-suggest-list-color-hover: #4a3834;
  --skins-suggest-list-bg: transparent;
  --skins-suggest-list-bg-image: none;
  --skins-suggest-list-bg-hover: rgba(232, 208, 200, 0.28);
  --skins-suggest-list-radius: 12px;
  --skins-suggest-list-border: 0;
  --skins-suggest-list-shadow: none;
  --skins-suggest-list-panel-bg: linear-gradient(165deg, rgba(255, 251, 249, 0.96) 0%, rgba(243, 235, 230, 0.97) 100%);
  --skins-suggest-list-panel-border: 1px solid rgba(196, 120, 110, 0.16);
  --skins-suggest-list-panel-radius: 14px;
  --skins-suggest-list-panel-padding: 8px 10px 10px;
  --skins-suggest-list-panel-shadow: 0 12px 28px rgba(74, 56, 52, 0.08), inset 0 1px 0 rgba(255, 255, 255, 0.85);
  /* 推荐卡片 token */
  --skins-suggest-card-color: var(--bengong-ink, #4a3834);
  --skins-suggest-card-bg: rgba(255, 251, 249, 0.92);
  --skins-suggest-card-radius: 16px;
  --skins-suggest-card-border: rgba(196, 120, 110, 0.12);
  --skins-suggest-card-shadow: 0 8px 18px rgba(74, 56, 52, 0.04);
`,
  jiuyi: `
  /* 推荐列表 opt-in（引擎 --skins-suggest-list-*；未设则宿主原生） */
  --skins-suggest-list-color: rgba(255, 255, 255, 0.5);
  --skins-suggest-list-color-hover: rgb(255, 255, 255);
  --skins-suggest-list-bg: transparent;
  --skins-suggest-list-bg-image: none;
  --skins-suggest-list-bg-hover: transparent;
  --skins-suggest-list-radius: 12.5px;
  --skins-suggest-list-border: 0;
  --skins-suggest-list-shadow: none;
  --skins-suggest-list-panel-bg: linear-gradient(165deg, rgba(32, 44, 60, 0.88) 0%, rgba(22, 32, 46, 0.9) 55%, rgba(16, 24, 34, 0.92) 100%);
  --skins-suggest-list-panel-border: 1px solid var(--jiuyi-glass-border, rgba(180, 200, 220, 0.22));
  --skins-suggest-list-panel-radius: 14px;
  --skins-suggest-list-panel-padding: 8px 10px 10px;
  --skins-suggest-list-panel-shadow: 0 14px 36px rgba(0, 0, 0, 0.36), inset 0 1px 0 rgba(255, 255, 255, 0.08);
  /* 推荐卡片 token */
  --skins-suggest-card-color: #ebe7df;
  --skins-suggest-card-bg: rgba(28, 40, 54, 0.52);
  --skins-suggest-card-radius: 14px;
  --skins-suggest-card-border: rgba(200, 220, 240, 0.28);
  --skins-suggest-card-shadow: 0 10px 28px rgba(0, 0, 0, 0.22), inset 0 1px 0 rgba(255, 255, 255, 0.12);
`,
  qingkong: `
  /* 推荐列表 opt-in（引擎 --skins-suggest-list-*；未设则宿主原生） */
  --skins-suggest-list-color: rgba(30, 58, 95, 0.55);
  --skins-suggest-list-color-hover: #1e3a5f;
  --skins-suggest-list-bg: transparent;
  --skins-suggest-list-bg-image: none;
  --skins-suggest-list-bg-hover: transparent;
  --skins-suggest-list-radius: 12px;
  --skins-suggest-list-border: 0;
  --skins-suggest-list-shadow: none;
  --skins-suggest-list-panel-bg: linear-gradient(165deg, rgba(255, 255, 255, 0.72) 0%, rgba(220, 238, 250, 0.78) 100%);
  --skins-suggest-list-panel-border: 1px solid rgba(255, 255, 255, 0.55);
  --skins-suggest-list-panel-radius: 14px;
  --skins-suggest-list-panel-padding: 8px 10px 10px;
  --skins-suggest-list-panel-shadow: 0 14px 36px rgba(30, 80, 130, 0.12), inset 0 1px 0 rgba(255, 255, 255, 0.65);
  /* 推荐卡片 token */
  --skins-suggest-card-color: #1e3a5f;
  --skins-suggest-card-bg: rgba(255, 255, 255, 0.28);
  --skins-suggest-card-radius: 14px;
  --skins-suggest-card-border: rgba(255, 255, 255, 0.55);
  --skins-suggest-card-shadow: 0 8px 24px rgba(30, 80, 130, 0.1), inset 0 1px 0 rgba(255, 255, 255, 0.55);
`,
  mortal: `
  /* 推荐列表 opt-in（引擎 --skins-suggest-list-*；未设则宿主原生） */
  --skins-suggest-list-color: rgba(42, 32, 64, 0.55);
  --skins-suggest-list-color-hover: #2a2040;
  --skins-suggest-list-bg: transparent;
  --skins-suggest-list-bg-image: none;
  --skins-suggest-list-bg-hover: transparent;
  --skins-suggest-list-radius: 12px;
  --skins-suggest-list-border: 0;
  --skins-suggest-list-shadow: none;
  --skins-suggest-list-panel-bg: linear-gradient(165deg, rgba(255, 255, 255, 0.72) 0%, rgba(240, 232, 250, 0.78) 100%);
  --skins-suggest-list-panel-border: 1px solid rgba(255, 255, 255, 0.55);
  --skins-suggest-list-panel-radius: 14px;
  --skins-suggest-list-panel-padding: 8px 10px 10px;
  --skins-suggest-list-panel-shadow: 0 14px 36px rgba(42, 32, 64, 0.12), inset 0 1px 0 rgba(255, 255, 255, 0.65);
  /* 推荐卡片 token */
  --skins-suggest-card-color: #2a2040;
  --skins-suggest-card-bg: rgba(255, 255, 255, 0.28);
  --skins-suggest-card-radius: 14px;
  --skins-suggest-card-border: rgba(255, 255, 255, 0.55);
  --skins-suggest-card-shadow: 0 8px 24px rgba(42, 32, 64, 0.1), inset 0 1px 0 rgba(255, 255, 255, 0.55);
`,
  dream: `
  /* 推荐列表 opt-in（引擎 --skins-suggest-list-*；未设则宿主原生） */
  --skins-suggest-list-color: color-mix(in srgb, var(--skins-text, #202536) 55%, transparent);
  --skins-suggest-list-color-hover: var(--skins-text, #202536);
  --skins-suggest-list-bg: transparent;
  --skins-suggest-list-bg-image: none;
  --skins-suggest-list-bg-hover: transparent;
  --skins-suggest-list-radius: 12px;
  --skins-suggest-list-border: 0;
  --skins-suggest-list-shadow: none;
  --skins-suggest-list-panel-bg: color-mix(in srgb, var(--skins-surface-raised, #fff) 78%, transparent);
  --skins-suggest-list-panel-border: 1px solid color-mix(in srgb, var(--skins-line, #c8cad4) 70%, transparent);
  --skins-suggest-list-panel-radius: 14px;
  --skins-suggest-list-panel-padding: 8px 10px 10px;
  --skins-suggest-list-panel-shadow: 0 14px 36px color-mix(in srgb, var(--skins-canvas, #f7f8fc) 16%, transparent);
  /* 推荐卡片 token */
  --skins-suggest-card-color: var(--skins-text, #202536);
  --skins-suggest-card-bg: color-mix(in srgb, var(--skins-surface-raised, #fff) 40%, transparent);
  --skins-suggest-card-radius: 14px;
`,
};

/** list-item only inside :not(...) does NOT make a card rule a list rule */
function listItemOutsideNot(line) {
  const stripped = line.replace(/:not\(\[class\*="home-suggestion-list-item"\]\)/g, "");
  return /home-suggestion-list-item/.test(stripped);
}

/**
 * Remove CSS rules that style list-item / list panel layout.
 * Keeps card rules that use :not(list-item) — including multi-line selectors.
 * Never drop intermediate selector lines of a card rule (main.main-surface …).
 */
function stripListBlocks(css) {
  const lines = css.split(/\r?\n/);
  const result = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];

    // List-only documentation comments (not card isolation notes)
    const isListDocComment =
      line.trim().startsWith("/*") &&
      /建议展开列表|LIST rows|内层列表轨/.test(line) &&
      !/:not\(/.test(line) &&
      !/排除 list-item|仅卡片/.test(line);

    if (isListDocComment) {
      if (!line.includes("*/")) {
        i++;
        while (i < lines.length && !lines[i].includes("*/")) i++;
        i++;
        while (i < lines.length && lines[i].trim() === "") i++;
        continue;
      }
      // single-line list doc comment
      if (/建议展开列表|内层列表轨/.test(line) && line.includes("*/")) {
        i++;
        continue;
      }
    }

    // List rule: list-item outside :not, panel :has(list-item), or list flex rail
    const isListRule =
      listItemOutsideNot(line) ||
      (/home-suggestions:has\(/.test(line) && /list-item/.test(line)) ||
      (/\.flex\.min-h-32/.test(line) && /home-suggestions/.test(line));

    if (isListRule) {
      let depth = 0;
      let sawOpen = false;
      while (i < lines.length) {
        const l = lines[i];
        for (const ch of l) {
          if (ch === "{") {
            depth++;
            sawOpen = true;
          } else if (ch === "}") depth--;
        }
        i++;
        if (sawOpen && depth <= 0) break;
        // multi-line list selector: keep consuming while comma / list anchors
        if (
          !sawOpen &&
          lines[i] !== undefined &&
          !/[{,]/.test(lines[i]) &&
          lines[i].trim() !== "" &&
          !/home-suggestion|flex\.min-h|list-item|main\.|html\.|skins-art/.test(lines[i])
        ) {
          break;
        }
      }
      while (i < lines.length && lines[i].trim() === "") i++;
      continue;
    }

    result.push(line);
    i++;
  }
  return result.join("\n").replace(/\n{3,}/g, "\n\n");
}

/** Ensure card button selectors use :not(list-item) */
function fixBareCardButtons(css) {
  // .xxx-home .group\/home-suggestions button {  → with :not
  // Avoid double-not
  return css.replace(
    /(\.group\\\/home-suggestions\s+)button(?!:not)/g,
    `$1button:not([class*="home-suggestion-list-item"])`
  );
}

function injectTokens(css, skinId, tokenBlock) {
  if (!tokenBlock) return css;
  if (css.includes("--skins-suggest-list-color")) {
    // already migrated
    return css;
  }
  // Prefer first :root.codex-*-skin { block
  const re = new RegExp(`(:root\\.codex-${skinId}-skin\\s*\\{)`);
  if (re.test(css)) {
    return css.replace(re, `$1${tokenBlock}`);
  }
  // dream uses codex-dream-skin
  const re2 = /(:root\.codex-[\w-]+-skin\s*\{)/;
  if (re2.test(css)) {
    return css.replace(re2, `$1${tokenBlock}`);
  }
  // prepend
  return `/* suggest tokens */\n:root {\n${tokenBlock}}\n\n` + css;
}

function addCardOnlyComment(css) {
  if (css.includes("推荐列表 opt-in") || css.includes("--skins-suggest-list-color")) {
    return css;
  }
  // for skins without list theming — only fix buttons
  return css;
}

const CSS_FILES = [
  ["bengong", "assets/bengong-skin.css", true],
  ["jiuyi", "assets/jiuyi-skin.css", true],
  ["qingkong", "assets/qingkong-skin.css", true],
  ["mortal", "assets/mortal-skin.css", true],
  ["dream", "assets/dream-skin.css", true],
  ["miku", "assets/miku-skin.css", false],
  ["cyberpunk", "assets/cyberpunk-skin.css", false],
  ["jianlai", "assets/linglong-skin.css", false],
];

const report = [];

for (const [id, rel, hasListTheme] of CSS_FILES) {
  const file = path.join(skinsDir, id, rel);
  if (!fs.existsSync(file)) {
    report.push({ id, ok: false, reason: "missing " + rel });
    continue;
  }
  let css = fs.readFileSync(file, "utf8");
  const before = css.length;

  if (hasListTheme) {
    css = stripListBlocks(css);
    css = injectTokens(css, id, LIST_TOKENS[id] || "");
  }
  css = fixBareCardButtons(css);
  css = addCardOnlyComment(css);

  // jianlai root class is codex-linglong-skin
  if (id === "jianlai" && !css.includes("--skins-suggest-card-color")) {
    // card-only comment tokens optional — skip list entirely
    const cardTok = `
  /* 推荐卡片 token（列表保持宿主原生，不设 --skins-suggest-list-*） */
  --skins-suggest-card-color: inherit;
`;
    // try linglong root
    if (/:root\.codex-linglong-skin\s*\{/.test(css)) {
      css = css.replace(/(:root\.codex-linglong-skin\s*\{)/, `$1${cardTok}`);
    }
  }
  if ((id === "miku" || id === "cyberpunk") && !css.includes("--skins-suggest-list-color")) {
    const note = `
  /* 推荐列表：不设 --skins-suggest-list-* → 宿主原生；卡片选择器已排除 list-item */
`;
    const rootRe = new RegExp(`(:root\\.codex-${id}-skin\\s*\\{)`);
    if (rootRe.test(css) && !css.includes("推荐列表：不设")) {
      css = css.replace(rootRe, `$1${note}`);
    }
  }

  fs.writeFileSync(file, css, "utf8");
  const listRulesLeft = (css.match(/home-suggestion-list-item/g) || []).length;
  const hasListTok = css.includes("--skins-suggest-list-color");
  const bareButtons = (css.match(/home-suggestions\s+button\s*[{,:]/g) || []).length;
  report.push({
    id,
    ok: true,
    before,
    after: css.length,
    listRulesLeft,
    hasListTok,
    bareButtons,
  });
}

console.log(JSON.stringify(report, null, 2));
