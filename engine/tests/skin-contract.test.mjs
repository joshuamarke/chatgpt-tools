/**
 * Framework + personalization smoke tests:
 * - immersive-skin.css is always merged as baseline capability
 * - framework first, skin last (engine does not override author CSS)
 * - baseline file declares full-window / native / suggestion support
 */
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  loadSkinBundle,
  mergeSkinCss,
  immersiveContractPath,
} from "../payload.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const dreamDir = path.join(root, "skins", "dream");

test("immersive baseline file exists and states framework capability", () => {
  const p = immersiveContractPath();
  assert.ok(fs.existsSync(p), `missing ${p}`);
  const text = fs.readFileSync(p, "utf8");
  assert.match(text, /FRAMEWORK baseline|adaptive full-window/i);
  assert.match(text, /dream-art-wide/);
  assert.match(text, /app-header-tint/);
  assert.match(text, /home-suggestions|home-suggestions/);
  assert.match(text, /composer-surface-chrome/);
  assert.match(text, /pointer-events:\s*auto/i);
});

test("mergeSkinCss puts framework baseline before skin personalization", () => {
  const merged = mergeSkinCss(
    "/* BASELINE */\nhtml.dream-art-wide body { background: red; }",
    "/* SKIN */\nhtml.x { color: blue; }"
  );
  const baseIdx = merged.indexOf("framework baseline");
  const skinIdx = merged.indexOf("skin personalization");
  const baseRule = merged.indexOf("/* BASELINE */");
  const skinRule = merged.indexOf("/* SKIN */");
  assert.ok(baseIdx >= 0 && skinIdx > baseIdx, "markers ordered: baseline then skin");
  assert.ok(baseRule >= 0 && skinRule > baseRule, "rules ordered: baseline then skin");
  assert.match(merged, /author-owned|does not restrict/i);
});

test("loadSkinBundle(dream) injects baseline before personalization", async () => {
  const bundle = await loadSkinBundle(dreamDir);
  assert.ok(bundle.css.includes("framework baseline"), "baseline banner present");
  assert.ok(bundle.css.includes("skin personalization"), "skin banner present");
  const base = bundle.css.indexOf("framework baseline");
  const pers = bundle.css.indexOf("skin personalization");
  assert.ok(pers > base, "personalization must follow framework baseline");
  // Framework full-window rules present
  assert.match(bundle.css, /background-attachment:\s*fixed/i);
  assert.match(bundle.css, /group\\\/home-suggestions|home-suggestions/);
  // Dream personalization tokens still present (author-owned)
  assert.match(bundle.css, /--dream-pink|--dream-violet|codex-dream-skin/);
});

test("dream personalization is free to refine on top of baseline", () => {
  const css = fs.readFileSync(
    path.join(dreamDir, "assets", "dream-skin.css"),
    "utf8"
  );
  assert.match(css, /--dream-text|--dream-ink/, "text tokens for suggestion readability");
  assert.match(css, /pointer-events:\s*none/i, "chrome decoration default");
  // Author may scope panel polish to standard mode by convention
  assert.match(
    css,
    /dream-art-standard/,
    "dream uses adaptive standard mode for panel personalization"
  );
});
