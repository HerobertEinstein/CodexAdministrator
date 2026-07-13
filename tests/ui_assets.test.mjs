import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const assets = new URL("../assets/", import.meta.url);

async function readAsset(name) {
  return readFile(new URL(name, assets), "utf8");
}

test("bootstrap preserves the host UI and mounts only a hidden bridge", async () => {
  const script = await readAsset("bootstrap.js");

  assert.match(script, /grok_native_model/);
  assert.doesNotMatch(script, /grok_injected_main/);
  assert.match(script, /root\.hidden = true/);
  assert.match(script, /frame\.hidden = true/);
  assert.doesNotMatch(script, /<select|ca-provider|aria-label="Model provider"/);
  assert.doesNotMatch(script, /ca-workspace|Grok main agent workspace/);
  assert.doesNotMatch(script, /position:\s*fixed;\s*inset:\s*0/);
});

test("authenticated iframe is a bridge rather than a replacement chat UI", async () => {
  const [html, script] = await Promise.all([
    readAsset("ui.html"),
    readAsset("ui-app.js"),
  ]);

  for (const forbidden of [
    "Grok Workspace",
    "textarea",
    "composer",
    "Parallel sessions",
    "Memory",
    "Evolution",
    "main agent",
  ]) {
    assert.doesNotMatch(`${html}\n${script}`, new RegExp(forbidden, "i"));
  }
  assert.match(script, /codex-administrator:state-request/);
  assert.match(script, /codex-administrator:set-mode/);
  assert.match(script, /native_gpt_main/);
});
