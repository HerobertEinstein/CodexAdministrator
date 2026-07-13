import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const discoveryUrl = new URL("../assets/renderer-api-discovery.js", import.meta.url);

async function loadDiscovery() {
  const source = await readFile(discoveryUrl, "utf8");
  const context = vm.createContext({ globalThis: {}, URL });
  vm.runInContext(source, context, { filename: discoveryUrl.pathname });
  return context.globalThis.__codexAdministratorRendererApiDiscovery;
}

test("finds the same-origin renderer API module from the official entry bundle", async () => {
  const discovery = await loadDiscovery();
  const entryUrl = "app://-/assets/index-build.js";
  const source = 'const deps=["./chunk.js","./vscode-api-review123.js"];';

  assert.equal(
    discovery.findRendererApiModuleUrl(entryUrl, source),
    "app://-/assets/vscode-api-review123.js",
  );
});

test("rejects missing, malformed, and cross-origin renderer module references", async () => {
  const discovery = await loadDiscovery();

  assert.equal(discovery.findRendererApiModuleUrl("app://-/assets/index.js", "no module"), null);
  assert.equal(
    discovery.findRendererApiModuleUrl(
      "app://-/assets/index.js",
      'const value="https://evil.example/vscode-api-bad.js";',
    ),
    null,
  );
});

test("selects only a writable renderer message API export", async () => {
  const discovery = await loadDiscovery();
  const rendererApi = { getState() {}, postMessage() {}, setState() {} };
  const frozen = Object.freeze({ getState() {}, postMessage() {}, setState() {} });

  assert.equal(discovery.findRendererApiExport({ a: frozen, b: rendererApi }), rendererApi);
  assert.equal(discovery.findRendererApiExport({ a: frozen }), null);
  assert.equal(discovery.findRendererApiExport({ a: { postMessage() {} } }), null);
});

test("discovers the renderer API through injected fetch and import boundaries", async () => {
  const discovery = await loadDiscovery();
  const rendererApi = { getState() {}, postMessage() {}, setState() {} };
  const imported = [];

  const result = await discovery.discoverRendererApi({
    documentRef: {
      scripts: [{ src: "app://-/assets/index-build.js", type: "module" }],
    },
    fetchFn: async (url) => {
      assert.equal(url, "app://-/assets/index-build.js");
      return { ok: true, text: async () => 'import "./vscode-api-review123.js";' };
    },
    importModule: async (url) => {
      imported.push(url);
      return { api: rendererApi };
    },
  });

  assert.equal(result, rendererApi);
  assert.deepEqual(imported, ["app://-/assets/vscode-api-review123.js"]);
});
