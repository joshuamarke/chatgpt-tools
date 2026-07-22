/**
 * Smoke-test engine CLI without Tauri.
 */
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const cli = path.join(root, "engine", "cli.mjs");

function run(args) {
  const r = spawnSync(process.execPath, [cli, ...args], {
    cwd: root,
    encoding: "utf8",
    env: { ...process.env, CODEX_SKIN_ROOT: root, CODEX_SKIN_STATE_NAME: "ChatGPTTools" },
  });
  return r;
}

const checks = [
  ["version"],
  ["paths"],
  ["list-skins"],
  ["status"],
  ["detect"],
  ["check-payload", "--skin-id", "dream"],
];

let failed = 0;
for (const args of checks) {
  const r = run(args);
  const out = (r.stdout || "").trim();
  const err = (r.stderr || "").trim();
  if (r.status !== 0) {
    console.error(`FAIL ${args.join(" ")}: ${err || out || r.status}`);
    failed += 1;
    continue;
  }
  try {
    const json = JSON.parse(out);
    console.log(`OK   ${args.join(" ").padEnd(14)} keys=${Object.keys(json).slice(0, 8).join(",")}`);
  } catch (e) {
    console.error(`FAIL ${args.join(" ")}: invalid JSON: ${out.slice(0, 200)}`);
    failed += 1;
  }
}

if (failed) {
  console.error(`\n${failed} check(s) failed`);
  process.exit(1);
}
console.log("\nengine CLI smoke tests passed");
