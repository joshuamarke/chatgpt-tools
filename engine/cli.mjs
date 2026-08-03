#!/usr/bin/env node
/**
 * ChatGPT Tools Engine CLI
 *
 * Stable JSON contract for Tauri / other frontends.
 * Usage:
 *   node engine/cli.mjs <command> [--json] [args...]
 *
 * Commands mirror engine/manager.js and are the extension point
 * for future native (Rust) reimplementation without UI changes.
 */
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import fs from "node:fs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");

// Ensure manager resolves bundled resources from repo / install root.
if (!process.env.CODEX_SKIN_ROOT) {
  process.env.CODEX_SKIN_ROOT = ROOT;
}

const require = createRequire(import.meta.url);
const manager = require("./manager.js");

function parseArgs(argv) {
  const args = [...argv];
  const flags = { json: true };
  const positional = [];
  const kv = {};
  while (args.length) {
    const a = args.shift();
    if (a === "--json") flags.json = true;
    else if (a === "--no-json") flags.json = false;
    else if (a === "--help" || a === "-h") flags.help = true;
    else if (a.startsWith("--")) {
      const key = a.slice(2);
      const next = args[0] && !args[0].startsWith("--") ? args.shift() : "true";
      kv[key] = next;
    } else {
      positional.push(a);
    }
  }
  return { command: positional[0] || "help", positional: positional.slice(1), flags, kv };
}

function printHelp() {
  const text = `ChatGPT Tools engine CLI

Commands:
  status
  host-status [--force true|false]
  detect
  list-skins
  apply --skin-id <id> [--restart true|false]   (default: false; true forces client relaunch)
  restore [--restore-theme true|false]
  pause
  resume [--restart true|false]
  verify --skin-id <id>
  check-payload --skin-id <id>
  export-skin --skin-id <id> --output <path>
  import-skin --path <package> [--overwrite true|false]
  inspect-skin --path <package>
  delete-skin --skin-id <id>
  design-wallpaper --payload <json-file|json-string>
  set-app-path --path <exe-or-app> | clear-app-path
  resolve-asset --skin-id <id> --kind art|screenshot|preview
  paths
  version
`;
  process.stdout.write(text);
}

function boolFlag(v, fallback = true) {
  if (v === undefined || v === null) return fallback;
  if (typeof v === "boolean") return v;
  const s = String(v).toLowerCase();
  if (["0", "false", "no", "off"].includes(s)) return false;
  if (["1", "true", "yes", "on"].includes(s)) return true;
  return fallback;
}

function parsePayload(raw) {
  if (!raw) return {};
  const s = String(raw);
  if (fs.existsSync(s) && fs.statSync(s).isFile()) {
    return JSON.parse(fs.readFileSync(s, "utf8"));
  }
  return JSON.parse(s);
}

async function dispatch(command, positional, kv) {
  switch (command) {
    case "help":
      printHelp();
      return { ok: true, help: true };
    case "version":
      return {
        ok: true,
        name: manager.ENGINE_NAME || "chatgpt-tools-engine",
        version: manager.ENGINE_VERSION || "2.2.0",
        protocol: manager.ENGINE_PROTOCOL || 2,
        root: ROOT,
      };
    case "paths":
      return {
        ok: true,
        root: manager.ROOT,
        stateRoot: manager.STATE_ROOT,
        bundledSkins: manager.BUNDLED_SKINS_DIR,
        librarySkins: manager.LIBRARY_DIR || manager.USER_SKINS_DIR,
        userSkins: manager.LIBRARY_DIR || manager.USER_SKINS_DIR,
      };
    case "status":
      return await manager.getStatus();
    case "host-status":
    case "host_status":
      return await manager.getHostStatus({ force: boolFlag(kv.force, false) });
    case "detect":
      return await manager.detectCodex();
    case "list-skins":
      return { skins: manager.listSkins().map((s) => ({ id: s.id, name: s.name, dir: s.dir, source: s.source })) };
    case "apply": {
      const skinId = kv["skin-id"] || positional[0];
      if (!skinId) throw new Error("apply requires --skin-id");
      return await manager.applySkin(skinId, { restart: boolFlag(kv.restart, false) });
    }
    case "restore":
      return await manager.restoreSkin({ restoreTheme: boolFlag(kv["restore-theme"], true) });
    case "pause":
      return await manager.pauseSkin();
    case "resume":
      return await manager.resumeSkin({ restart: boolFlag(kv.restart, false) });
    case "verify": {
      const skinId = kv["skin-id"] || positional[0];
      if (!skinId) throw new Error("verify requires --skin-id");
      return await manager.hardVerifySkin(skinId);
    }
    case "check-payload": {
      const skinId = kv["skin-id"] || positional[0];
      if (!skinId) throw new Error("check-payload requires --skin-id");
      return await manager.checkSkinPayload(skinId);
    }
    case "export-skin": {
      const skinId = kv["skin-id"] || positional[0];
      const output = kv.output || positional[1];
      if (!skinId || !output) throw new Error("export-skin requires --skin-id and --output");
      return manager.exportSkin(skinId, output);
    }
    case "import-skin": {
      const packagePath = kv.path || positional[0];
      if (!packagePath) throw new Error("import-skin requires --path");
      return manager.importSkin(packagePath, { overwrite: boolFlag(kv.overwrite, true) });
    }
    case "inspect-skin": {
      const packagePath = kv.path || positional[0];
      if (!packagePath) throw new Error("inspect-skin requires --path");
      return manager.inspectSkinPackage(packagePath);
    }
    case "delete-skin": {
      const skinId = kv["skin-id"] || positional[0];
      if (!skinId) throw new Error("delete-skin requires --skin-id");
      return manager.deleteUserSkin(skinId);
    }
    case "design-wallpaper": {
      const payload = parsePayload(kv.payload || positional[0]);
      return manager.createWallpaperSkin(payload);
    }
    case "set-app-path": {
      const appPath = kv.path || positional[0];
      if (!appPath) throw new Error("set-app-path requires --path");
      return manager.setConfiguredAppPath(appPath);
    }
    case "clear-app-path":
      return manager.setConfiguredAppPath(null);
    case "resolve-asset": {
      const skinId = kv["skin-id"] || positional[0];
      const kind = kv.kind || positional[1] || "art";
      if (!skinId) throw new Error("resolve-asset requires --skin-id");
      const filePath = manager.resolveSkinAsset(skinId, kind);
      return { ok: true, path: filePath, skinId, kind };
    }
    default:
      throw new Error(`Unknown command: ${command}`);
  }
}

const { command, positional, flags, kv } = parseArgs(process.argv.slice(2));

try {
  if (flags.help || command === "help") {
    printHelp();
    process.exit(0);
  }
  const result = await dispatch(command, positional, kv);
  if (flags.json) {
    process.stdout.write(JSON.stringify(result ?? { ok: true }) + "\n");
  } else {
    process.stdout.write(String(result) + "\n");
  }
  process.exit(0);
} catch (error) {
  const payload = {
    ok: false,
    error: error?.message || String(error),
    code: error?.code || "ENGINE_ERROR",
  };
  process.stderr.write(JSON.stringify(payload) + "\n");
  process.exit(1);
}
