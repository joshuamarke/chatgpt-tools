/**
 * Verifies buildDesktopThemeSettings emits dark chrome keys for dark skins.
 * Loads the function source from manager.js (CommonJS, not exported).
 */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const managerSrc = fs.readFileSync(path.join(__dirname, "../manager.js"), "utf8");
const start = managerSrc.indexOf("function buildDesktopThemeSettings");
const end = managerSrc.indexOf("function readConfigStrict");
assert.ok(start >= 0 && end > start, "locate buildDesktopThemeSettings in manager.js");
// eslint-disable-next-line no-new-func
const buildDesktopThemeSettings = new Function(
  `${managerSrc.slice(start, end)}\nreturn buildDesktopThemeSettings;`
)();

test("dark skin emits appearanceDark* chrome keys", () => {
  const lines = buildDesktopThemeSettings({
    appearanceTheme: "dark",
    appearanceDarkCodeThemeId: "codex",
    appearanceDarkChromeTheme:
      '{ accent = "#A83A2E", ink = "#E8E4DC", surface = "#141A24", opaqueWindows = true }',
  });
  const text = lines.join("\n");
  assert.match(text, /appearanceTheme = "dark"/);
  assert.match(text, /appearanceDarkCodeThemeId = "codex"/);
  assert.match(text, /appearanceDarkChromeTheme = \{ accent/);
  assert.match(text, /appearanceLightChromeTheme = \{ accent/);
  assert.match(text, /surface = "#141A24"/);
});

test("light skin keeps light-only keys (no forced dark pair)", () => {
  const lines = buildDesktopThemeSettings({
    appearanceTheme: "light",
    appearanceLightCodeThemeId: "codex",
    appearanceLightChromeTheme: '{ accent = "#B65CFF" }',
  });
  const text = lines.join("\n");
  assert.match(text, /appearanceTheme = "light"/);
  assert.match(text, /appearanceLightChromeTheme = \{ accent/);
  assert.doesNotMatch(text, /appearanceDarkCodeThemeId/);
  assert.doesNotMatch(text, /appearanceDarkChromeTheme/);
});

test("dark-only chrome is mirrored into light pair", () => {
  const lines = buildDesktopThemeSettings({
    appearanceTheme: "dark",
    appearanceDarkChromeTheme: '{ surface = "#141A24" }',
  });
  const text = lines.join("\n");
  assert.match(text, /appearanceLightChromeTheme = \{ surface = "#141A24" \}/);
  assert.match(text, /appearanceDarkChromeTheme = \{ surface = "#141A24" \}/);
});
