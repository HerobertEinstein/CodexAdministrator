import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const assets = new URL("../assets/", import.meta.url);

test("injection assets preserve the native UI and add only a namespaced model-picker surface", async () => {
  const [discovery, bootstrap, core, picker, addonRuntime] = await Promise.all([
    readFile(new URL("renderer-api-discovery.js", assets), "utf8"),
    readFile(new URL("bootstrap.js", assets), "utf8"),
    readFile(new URL("model-injection-core.js", assets), "utf8"),
    readFile(new URL("model-picker-mount.js", assets), "utf8"),
    readFile(new URL("renderer-addon-runtime.js", assets), "utf8"),
  ]);
  const source = `${discovery}\n${bootstrap}\n${core}\n${picker}\n${addonRuntime}`;

  assert.match(source, /sendMessageFromView/);
  assert.match(source, /vscode-api-/);
  assert.match(source, /postMessage/);
  assert.match(source, /patchModelListMessage/);
  assert.match(source, /data-codex-administrator-model-manager/);
  assert.match(source, /data-codex-intelligence-trigger/);
  assert.match(source, /Compatible injectors/);
  assert.match(source, /rendererAddonCatalog/);
  assert.match(source, /data-codex-administrator-renderer-addon/);
  assert.match(source, /__codexAdministratorRendererAddons/);
  assert.match(source, /renderer_addons/);
  assert.match(source, /modelWhitelistUnlock/);
  assert.doesNotMatch(source, /install-dream-skin|start-dream-skin|restore-dream-skin/i);
  assert.doesNotMatch(source, /Codex Dream Skin/);
  assert.doesNotMatch(source, /createElement\(["']iframe|<iframe|createRoot\(|ReactDOM|innerHTML/i);
  assert.doesNotMatch(source, /https?:\/\//i);
});
