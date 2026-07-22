/**
 * @deprecated Use migrate-skins-v2.mjs
 * Kept as a thin redirect for old docs / muscle memory.
 */
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const target = path.join(here, "migrate-skins-v2.mjs");
const r = spawnSync(process.execPath, [target], { stdio: "inherit" });
process.exit(r.status ?? 1);
