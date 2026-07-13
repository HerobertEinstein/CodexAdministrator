import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const assets = new URL("../assets/", import.meta.url);

test("injection assets preserve the native UI and contain no replacement interface", async () => {
  const [bootstrap, core] = await Promise.all([
    readFile(new URL("bootstrap.js", assets), "utf8"),
    readFile(new URL("model-injection-core.js", assets), "utf8"),
  ]);
  const source = `${bootstrap}\n${core}`;

  assert.match(source, /sendMessageFromView/);
  assert.match(source, /patchModelListMessage/);
  assert.doesNotMatch(source, /createElement|iframe|textarea|composer|postMessage|fetch\(/i);
});
