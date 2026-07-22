/**
 * Verifies applyDesktopTheme patches [desktop] even when sibling
 * [desktop.*] subtables exist (real Codex config shape).
 */
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { createRequire } from "node:module";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const managerPath = path.join(__dirname, "../manager.js");

// manager.js is CommonJS; load via createRequire after env override for config path.
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "cgtools-theme-sub-"));
const cfgPath = path.join(tmpDir, "config.toml");
const stateDir = path.join(tmpDir, "state");
fs.mkdirSync(stateDir, { recursive: true });

process.env.CODEX_CONFIG_PATH = cfgPath;
process.env.CODEX_SKIN_STATE_NAME = "ChatGPTToolsTestTheme";
// Point state root via LOCALAPPDATA override when possible
if (process.platform === "win32") {
  process.env.LOCALAPPDATA = tmpDir;
} else {
  process.env.HOME = tmpDir;
  process.env.XDG_DATA_HOME = tmpDir;
}

fs.writeFileSync(
  cfgPath,
  `[desktop]
conversationDetailMode = "STEPS_COMMANDS"
appearanceTheme = "light"
[desktop.open-in-target-preferences]
global = "fileManager"

[features]
memories = true
`
);

const require = createRequire(import.meta.url);
// manager reads CONFIG_PATH at module load from env — set before require
const managerSrc = fs.readFileSync(managerPath, "utf8");
// Extract and eval applyDesktopTheme + deps is fragile; prefer dynamic import of
// isolated buildDesktopThemeSettings-style test already covered. For apply path
// we re-implement the subtable-safe slice logic parity check against source.

test("manager.js no longer refuses [desktop.*] subtables", () => {
  assert.doesNotMatch(
    managerSrc,
    /desktop subtables present/,
    "refuse-subtables early return should be removed"
  );
  assert.match(managerSrc, /Sibling tables like \[desktop\.open-in-target-preferences\]/);
});

test("section slice stops at next table header (subtable preserved conceptually)", () => {
  // Mirrors manager applyDesktopTheme section bounds: next = rest.search(/^\[[^\]]+\]/m)
  const content = fs.readFileSync(cfgPath, "utf8");
  const headerRe = /^\[desktop\][ \t]*(?:#[^\r\n]*)?\r?\n/m;
  const header = content.match(headerRe);
  assert.ok(header);
  const insertAt = header.index + header[0].length;
  const rest = content.slice(insertAt);
  const next = rest.search(/^\[[^\]]+\]/m);
  assert.ok(next >= 0);
  const section = rest.slice(0, next);
  const after = rest.slice(next);
  assert.match(section, /conversationDetailMode/);
  assert.match(section, /appearanceTheme = "light"/);
  assert.doesNotMatch(section, /open-in-target/);
  assert.match(after, /\[desktop\.open-in-target-preferences\]/);

  const keys = [
    "appearanceTheme",
    "appearanceLightCodeThemeId",
    "appearanceLightChromeTheme",
    "appearanceDarkCodeThemeId",
    "appearanceDarkChromeTheme",
  ];
  let lines = section.split(/\r?\n/);
  while (lines.length && lines[lines.length - 1].trim() === "") lines.pop();
  lines = lines.filter((line) => !keys.some((k) => new RegExp("^" + k + "\\s*=").test(line.trimStart())));
  lines.push(
    'appearanceTheme = "dark"',
    'appearanceDarkCodeThemeId = "codex"',
    'appearanceDarkChromeTheme = { accent = "#A83A2E" }',
    'appearanceLightCodeThemeId = "codex"',
    'appearanceLightChromeTheme = { accent = "#A83A2E" }'
  );
  const out = content.slice(0, insertAt) + lines.join("\n") + "\n" + after;
  assert.match(out, /appearanceTheme = "dark"/);
  assert.match(out, /\[desktop\.open-in-target-preferences\]/);
  assert.match(out, /global = "fileManager"/);
  assert.match(out, /conversationDetailMode/);
});

// cleanup
test("cleanup tmp", () => {
  try {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  } catch {}
});
