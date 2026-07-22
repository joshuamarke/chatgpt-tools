import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const injector = path.join(root, "engine", "injector.mjs");

test("injector --self-test passes loopback validation", () => {
  const r = spawnSync(process.execPath, [injector, "--self-test"], {
    encoding: "utf8",
    cwd: root,
  });
  assert.equal(r.status, 0, r.stderr || r.stdout);
  const json = JSON.parse(r.stdout.trim());
  assert.equal(json.pass, true);
});

test("injector --check-payload for all bundled skins", () => {
  const skins = ["dream", "cyberpunk", "eva", "guofeng", "jianlai", "jiuyi", "miku", "mortal"];
  for (const dirName of skins) {
    const skinDir = path.join(root, "skins", dirName);
    const r = spawnSync(
      process.execPath,
      [injector, "--check-payload", "--skin-dir", skinDir],
      { encoding: "utf8", cwd: root, maxBuffer: 8 * 1024 * 1024 }
    );
    assert.equal(r.status, 0, `${dirName}: ${r.stderr || r.stdout}`);
    const json = JSON.parse(r.stdout.trim());
    assert.equal(json.pass, true, dirName);
    assert.ok(json.skinId, dirName);
    assert.ok(json.payloadBytes > 0, dirName);
    assert.equal(json.deferredArt, true, dirName);
    assert.equal(json.supportsDelta, true, dirName);
    assert.ok(json.shellBytes > 0, dirName);
    assert.ok(json.deltaShellBytes > 0, dirName);
    assert.ok(json.artPayloadBytes > 0, dirName);
    assert.ok(
      json.shellBytes < json.payloadBytes,
      `${dirName}: shell should be smaller than full payload`
    );
    assert.ok(
      json.deltaShellBytes < json.shellBytes,
      `${dirName}: delta shell should be smaller than full shell (no core)`
    );
  }
});
