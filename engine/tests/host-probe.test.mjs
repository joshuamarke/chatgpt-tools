import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const hostProbe = require("../host-probe.js");
const version = require("../version.js");

test("version module is consistent", () => {
  assert.equal(typeof version.ENGINE_VERSION, "string");
  assert.match(version.ENGINE_VERSION, /^\d+\.\d+\.\d+/);
  assert.equal(version.ENGINE_PROTOCOL, 2);
});

test("resolveTimingBudget stretches on starting lifecycle", () => {
  const normal = hostProbe.resolveTimingBudget({ lifecycle: "ready" });
  const slow = hostProbe.resolveTimingBudget({ lifecycle: "starting" });
  assert.ok(slow.scale >= normal.scale);
  assert.ok(slow.waitRendererMs >= normal.waitRendererMs);
  assert.ok(slow.softOnceTimeoutMs >= 2500);
});

test("resolveTimingBudget respects CODEX_SKIN_SLOW_SCALE", () => {
  const prev = process.env.CODEX_SKIN_SLOW_SCALE;
  process.env.CODEX_SKIN_SLOW_SCALE = "2";
  try {
    const budget = hostProbe.resolveTimingBudget({ lifecycle: "offline" });
    assert.equal(budget.scale, 2);
    assert.ok(budget.waitDebugPortMs >= 50000);
  } finally {
    if (prev === undefined) delete process.env.CODEX_SKIN_SLOW_SCALE;
    else process.env.CODEX_SKIN_SLOW_SCALE = prev;
  }
});

test("probeHostLifecycle returns shape without live host", async () => {
  hostProbe._resetProbeStateForTests();
  // Unlikely debug port — should be offline (or sticky if process open), not throw.
  const snap = await hostProbe.probeHostLifecycle(59999, {
    fetchTimeoutMs: 400,
    force: true,
  });
  assert.equal(typeof snap.lifecycle, "string");
  assert.ok(["offline", "starting", "ready"].includes(snap.lifecycle));
  assert.equal(typeof snap.codexRunning, "boolean");
  assert.equal(typeof snap.debugPortOpen, "boolean");
  assert.equal(typeof snap.rendererReady, "boolean");
  assert.equal(typeof snap.processRunning, "boolean");
  assert.equal(typeof snap.canHotApply, "boolean");
  assert.equal(typeof snap.needsRestartForInject, "boolean");
  assert.ok(snap.lifecycleRaw);
  assert.ok(Array.isArray(snap.pids));
  if (!snap.processRunning && !snap.debugPortOpen && snap.lifecycleRaw === "offline") {
    // May still be probing if first hit; force enough offline confirms
    hostProbe._resetProbeStateForTests();
    for (let i = 0; i < 4; i++) {
      await hostProbe.probeHostLifecycle(59999, { fetchTimeoutMs: 200, force: true });
    }
    const last = await hostProbe.probeHostLifecycle(59999, {
      fetchTimeoutMs: 200,
      force: true,
    });
    if (!last.processRunning && !last.debugPortOpen) {
      assert.equal(last.lifecycle, "offline");
      assert.equal(last.codexRunning, false);
    }
  }
});

test("hysteresis sticky ready survives brief offline raw", () => {
  hostProbe._resetProbeStateForTests();
  hostProbe._setHystForTests({
    stable: "ready",
    lastReadyAt: Date.now(),
    offlineSince: null,
    offlineHits: 0,
  });
  const once = hostProbe._applyHysteresisForTests("offline", false);
  assert.equal(once.lifecycle, "ready");
  assert.equal(once.confidence, "probing");
});

test("hysteresis eventually accepts offline after confirms", () => {
  hostProbe._resetProbeStateForTests();
  hostProbe._setHystForTests({
    stable: "ready",
    lastReadyAt: Date.now() - 10_000,
    offlineSince: Date.now() - 10_000,
    offlineHits: 0,
  });
  let last = null;
  for (let i = 0; i < 4; i++) {
    last = hostProbe._applyHysteresisForTests("offline", false);
  }
  assert.equal(last.lifecycle, "offline");
});

test("getHostStatus returns compact ok shape", async () => {
  hostProbe._resetProbeStateForTests();
  const h = await hostProbe.getHostStatus(59999, { force: true });
  assert.equal(h.ok, true);
  assert.ok(["offline", "starting", "ready"].includes(h.lifecycle));
});
