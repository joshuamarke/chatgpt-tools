import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  assembleArtPayload,
  assembleDeltaShellPayload,
  assemblePayload,
  buildDeltaShellPayload,
  buildPayload,
  buildShellPayload,
  buildStagedPayload,
  checkSkinPayload,
  clearPayloadCache,
} from "../payload.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const dreamDir = path.join(root, "skins", "dream");

test("assemblePayload fails if a required token is missing from template", () => {
  assert.throws(
    () =>
      assemblePayload("((x)=>x)(__SKIN_CSS_JSON__)", {
        __SKIN_CSS_JSON__: "1",
        __SKIN_ART_JSON__: "2",
        __SKIN_THEME_JSON__: "3",
        __SKIN_MARKERS_JSON__: "4",
        __SKIN_PLUGIN_JSON__: "5",
        __SKIN_REVISION_JSON__: "6",
      }),
    /missing placeholder/
  );
});

test("assemblePayload replaces all duplicates (comment + IIFE)", () => {
  // Reproduces the production bug: String.replace only swapped the first token
  // (often a header comment), leaving the IIFE args unresolved.
  const template = `
/* also __SKIN_CSS_JSON__ in comment */
((css, art, theme, markers, plugin, rev) => ({ css, art, theme, markers, plugin, rev }))(
  __SKIN_CSS_JSON__,
  __SKIN_ART_JSON__,
  __SKIN_THEME_JSON__,
  __SKIN_MARKERS_JSON__,
  __SKIN_PLUGIN_JSON__,
  __SKIN_REVISION_JSON__
);
`;
  const out = assemblePayload(template, {
    __SKIN_CSS_JSON__: JSON.stringify("body{}"),
    __SKIN_ART_JSON__: JSON.stringify("data:image/png;base64,aa"),
    __SKIN_THEME_JSON__: JSON.stringify({ appearance: "auto" }),
    __SKIN_MARKERS_JSON__: JSON.stringify({ rootClass: "x", styleId: "s", stateKey: "k" }),
    __SKIN_PLUGIN_JSON__: JSON.stringify({ chromeHtml: "<div/>" }),
    __SKIN_REVISION_JSON__: JSON.stringify("abc"),
  });
  assert.equal(out.includes("__SKIN_"), false);
  // CSS token appeared twice (comment + arg) — both must become the JSON string
  assert.equal((out.match(/"body\{\}"/g) || []).length, 2);
  // eslint-disable-next-line no-new-func
  new Function(out); // must be valid JS
  // eslint-disable-next-line no-new-func
  const result = new Function(`return (${out.trim().replace(/;\s*$/, "")})`)();
  assert.equal(result.css, "body{}");
  assert.equal(result.rev, "abc");
});

test("check-payload dream reports staged shell + art sizes", async () => {
  clearPayloadCache();
  const report = await checkSkinPayload(dreamDir);
  assert.equal(report.pass, true);
  assert.equal(report.skinId, "dream");
  assert.equal(report.deferredArt, true);
  assert.equal(report.phase, "staged");
  assert.ok(report.payloadBytes > 1000);
  assert.ok(report.shellBytes > 1000);
  assert.ok(report.artPayloadBytes > 1000);
  // Shell must be smaller than monolithic full payload (no base64 art).
  assert.ok(
    report.shellBytes < report.payloadBytes,
    `shell ${report.shellBytes} should be < full ${report.payloadBytes}`
  );
  assert.ok(report.fingerprint);
  assert.equal(report.appearance, "auto");
});

test("built full payload is valid JS without leftover placeholders", async () => {
  clearPayloadCache();
  const a = await buildPayload(dreamDir);
  assert.equal(a.payload.includes("__SKIN_"), false);
  assert.equal(a.payload.includes("__DREAM_"), false);
  // eslint-disable-next-line no-new-func
  new Function(a.payload);
  assert.match(a.payload, /__CHATGPT_TOOLS_SKIN_REGISTRY__/);
  assert.match(a.payload, /codex-dream-skin/);
  assert.match(a.payload, /data:image\//);
  assert.equal(a.deferredArt, false);
});

test("shell payload omits art data URL and exposes applyArt", async () => {
  clearPayloadCache();
  const shell = await buildShellPayload(dreamDir);
  assert.equal(shell.phase, "shell");
  assert.equal(shell.deferredArt, true);
  assert.equal(shell.payload.includes("data:image/"), false);
  assert.match(shell.payload, /applyArt/);
  // eslint-disable-next-line no-new-func
  new Function(shell.payload);
});

test("staged payload splits shell and art evaluates", async () => {
  clearPayloadCache();
  const staged = await buildStagedPayload(dreamDir);
  assert.equal(staged.phase, "staged");
  assert.equal(staged.deferredArt, true);
  assert.ok(staged.shellPayload);
  assert.ok(staged.artPayload);
  assert.equal(staged.shellPayload.includes("data:image/"), false);
  assert.match(staged.artPayload, /applyArt|data:image\//);
  assert.ok(staged.shellBytes < staged.totalBytes);
  // eslint-disable-next-line no-new-func
  new Function(staged.shellPayload);
  // eslint-disable-next-line no-new-func
  new Function(staged.artPayload);
});

test("assembleArtPayload is valid JS", () => {
  const script = assembleArtPayload(
    { stateKey: "__T_STATE__", disabledKey: "__T_DISABLED__" },
    "data:image/png;base64,aa",
    "rev1"
  );
  // eslint-disable-next-line no-new-func
  new Function(script);
  assert.match(script, /applyArt/);
});

test("payload cache hits on second staged build", async () => {
  clearPayloadCache();
  const a = await buildStagedPayload(dreamDir);
  const b = await buildStagedPayload(dreamDir);
  assert.equal(a.fingerprint, b.fingerprint);
  assert.equal(b.cacheHit, true);
});

test("full payload cache still works", async () => {
  clearPayloadCache();
  const a = await buildPayload(dreamDir);
  const b = await buildPayload(dreamDir);
  assert.equal(a.fingerprint, b.fingerprint);
  assert.equal(b.cacheHit, true);
});

test("delta shell omits renderer-core and is smaller than full shell", async () => {
  clearPayloadCache();
  const shell = await buildShellPayload(dreamDir);
  const delta = await buildDeltaShellPayload(dreamDir);
  assert.equal(delta.phase, "delta-shell");
  assert.match(delta.payload, /__CHATGPT_TOOLS_SKIN_HOST__/);
  assert.match(delta.payload, /applySkin/);
  // Full shell embeds core IIFE; delta must not ship the whole runtime body markers thrice.
  assert.ok(
    delta.payloadBytes < shell.payloadBytes,
    `delta ${delta.payloadBytes} should be < shell ${shell.payloadBytes}`
  );
  // eslint-disable-next-line no-new-func
  new Function(delta.payload);
});

test("assembleDeltaShellPayload is valid JS", () => {
  const script = assembleDeltaShellPayload({
    css: "body{color:red}",
    markers: { rootClass: "x", styleId: "s", stateKey: "k", chromeId: "c" },
    theme: { appearance: "auto" },
    plugin: { chromeHtml: "<div/>", version: "1" },
    revision: "r1",
  });
  // eslint-disable-next-line no-new-func
  new Function(script);
  assert.match(script, /applySkin/);
});

test("staged payload includes delta shell", async () => {
  clearPayloadCache();
  const staged = await buildStagedPayload(dreamDir);
  assert.equal(staged.supportsDelta, true);
  assert.ok(staged.deltaShellPayload);
  assert.ok(staged.deltaShellBytes > 0);
  assert.ok(staged.deltaShellBytes < staged.shellBytes);
  assert.ok(staged.coreRevision);
});

test("check-payload reports deltaShellBytes", async () => {
  clearPayloadCache();
  const report = await checkSkinPayload(dreamDir);
  assert.equal(report.supportsDelta, true);
  assert.ok(report.deltaShellBytes > 0);
  assert.ok(report.deltaShellBytes < report.shellBytes);
});
