/**
 * Single source of truth for engine version / protocol.
 * CJS for manager.js; ESM consumers use createRequire or version.mjs.
 */
const ENGINE_NAME = "chatgpt-tools-engine";
const ENGINE_VERSION = "2.3.0";
const ENGINE_PROTOCOL = 2;

module.exports = {
  ENGINE_NAME,
  ENGINE_VERSION,
  ENGINE_PROTOCOL,
};
