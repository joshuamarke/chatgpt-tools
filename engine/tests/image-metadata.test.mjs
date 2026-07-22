import test from "node:test";
import assert from "node:assert/strict";
import {
  classifyImageDimensions,
  readImageMetadata,
  assertArtBytes,
  MAX_ART_BYTES,
} from "../image-metadata.mjs";

test("classifyImageDimensions rejects bombs", () => {
  assert.equal(classifyImageDimensions({ width: 20000, height: 20000 }), null);
  assert.equal(classifyImageDimensions({ width: 8000, height: 8000 }), null); // >50MP
  const ok = classifyImageDimensions({ width: 2560, height: 1440 });
  assert.equal(ok.wide, true);
  assert.equal(ok.taskMode, "ambient");
});

test("readImageMetadata parses minimal PNG IHDR", () => {
  // 1x1 PNG
  const png = Buffer.from(
    "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000a49444154789c63000100000500010d0a2db40000000049454e44ae426082",
    "hex"
  );
  const meta = readImageMetadata(png, ".png");
  assert.equal(meta.width, 1);
  assert.equal(meta.height, 1);
});

test("assertArtBytes enforces limits", () => {
  assert.throws(() => assertArtBytes(0), /empty/);
  assert.throws(() => assertArtBytes(MAX_ART_BYTES + 1), /exceeds/);
  assert.doesNotThrow(() => assertArtBytes(1024));
});
