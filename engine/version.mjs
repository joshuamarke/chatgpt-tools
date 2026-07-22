/**
 * ESM re-export of engine version (injector / tests).
 */
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const v = require("./version.js");

export const ENGINE_NAME = v.ENGINE_NAME;
export const ENGINE_VERSION = v.ENGINE_VERSION;
export const ENGINE_PROTOCOL = v.ENGINE_PROTOCOL;
