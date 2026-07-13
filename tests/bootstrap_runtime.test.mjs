import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const assets = new URL("../assets/", import.meta.url);

const grokModel = {
  id: "grok-4",
  model: "grok-4",
  upgrade: null,
  upgradeInfo: null,
  availabilityNux: null,
  displayName: "Grok 4",
  description: "Configured xAI model; new tasks only",
  hidden: false,
  supportedReasoningEfforts: [{ reasoningEffort: "medium", description: "Balanced reasoning" }],
  defaultReasoningEffort: "medium",
  inputModalities: ["text"],
  supportsPersonality: false,
  additionalSpeedTiers: [],
  serviceTiers: [],
  defaultServiceTier: null,
  isDefault: false,
};

async function boot({
  autoDiscoverRendererApi = false,
  bridgeInitiallyAvailable = true,
  frozenBridge = false,
  models = [grokModel],
  rendererApiAvailable = false,
} = {}) {
  const [discovery, core, template] = await Promise.all([
    readFile(new URL("renderer-api-discovery.js", assets), "utf8"),
    readFile(new URL("model-injection-core.js", assets), "utf8"),
    readFile(new URL("bootstrap.js", assets), "utf8"),
  ]);
  const sent = [];
  const listeners = new Map();
  const intervals = new Map();
  let nextIntervalId = 1;
  const originalSend = async (message) => {
    sent.push(message);
  };
  const rendererApi = rendererApiAvailable || autoDiscoverRendererApi
    ? { getState() {}, postMessage: originalSend, setState() {} }
    : null;
  const window = {
    addEventListener(type, listener, options) {
      const entries = listeners.get(type) || [];
      entries.push({ listener, options });
      listeners.set(type, entries);
    },
    removeEventListener(type, listener, options) {
      const entries = listeners.get(type) || [];
      listeners.set(type, entries.filter((entry) => entry.listener !== listener || entry.options !== options));
    },
  };
  if (bridgeInitiallyAvailable) {
    const bridge = { sendMessageFromView: originalSend };
    window.electronBridge = frozenBridge ? Object.freeze(bridge) : bridge;
  }
  if (rendererApiAvailable && rendererApi) window.__codexAdministratorRendererApi = rendererApi;
  const discoveryImports = [];
  if (autoDiscoverRendererApi) {
    window.__codexAdministratorImportRendererModule = async (url) => {
      discoveryImports.push(url);
      return { rendererApi };
    };
  }
  const context = vm.createContext({
    clearInterval(id) {
      intervals.delete(id);
    },
    console,
    document: autoDiscoverRendererApi
      ? { scripts: [{ src: "app://-/assets/index-build.js", type: "module" }] }
      : undefined,
    fetch: autoDiscoverRendererApi
      ? async () => ({ ok: true, text: async () => 'import "./vscode-api-review123.js";' })
      : undefined,
    Set,
    setInterval(callback) {
      const id = nextIntervalId++;
      intervals.set(id, callback);
      return id;
    },
    URL,
    window,
  });
  const config = JSON.stringify({ version: 2, provider_id: "grok_native", models });
  vm.runInContext(
    `${discovery}\n${core}\n${template.replace("/*__CODEX_ADMINISTRATOR_CONFIG__*/", config)}`,
    context,
  );
  await Promise.resolve();
  await Promise.resolve();
  if (autoDiscoverRendererApi) {
    for (let attempt = 0; attempt < 5 && discoveryImports.length === 0; attempt += 1) {
      await new Promise((resolve) => setImmediate(resolve));
    }
  }
  return {
    context,
    discoveryImports,
    intervals,
    listeners,
    originalSend,
    rendererApi,
    sent,
    window,
  };
}

async function activateInjectedModels(runtime) {
  const transport = runtime.rendererApi?.postMessage
    ?? runtime.window.electronBridge?.sendMessageFromView;
  await transport({
    type: "mcp-request",
    request: { id: "activate-models", method: "model/list", params: { cursor: null } },
  });
  const listener = runtime.listeners.get("message")[0].listener;
  listener({
    data: {
      type: "mcp-response",
      message: {
        id: "activate-models",
        result: { data: [{ id: "gpt-5.4", model: "gpt-5.4", displayName: "GPT-5.4" }] },
      },
    },
  });
  runtime.sent.length = 0;
}

test("bootstrap patches the renderer API without modifying a frozen native bridge", async () => {
  const runtime = await boot({
    frozenBridge: true,
    rendererApiAvailable: true,
  });
  const { originalSend, rendererApi, sent, window } = runtime;
  await activateInjectedModels(runtime);
  const grok = {
    type: "mcp-request",
    request: { id: 2, method: "thread/start", params: { model: "grok-4" } },
  };

  assert.equal(window.electronBridge.sendMessageFromView, originalSend);
  assert.notEqual(rendererApi.postMessage, originalSend);
  await rendererApi.postMessage(grok);

  assert.equal(sent[0].request.params.modelProvider, "grok_native");
  assert.equal(window.electronBridge.sendMessageFromView, originalSend);
});

test("bootstrap discovers the official renderer API without modifying a frozen bridge", async () => {
  const { discoveryImports, originalSend, rendererApi, window } = await boot({
    autoDiscoverRendererApi: true,
    frozenBridge: true,
  });

  assert.deepEqual(discoveryImports, ["app://-/assets/vscode-api-review123.js"]);
  assert.equal(window.electronBridge.sendMessageFromView, originalSend);
  assert.notEqual(rendererApi.postMessage, originalSend);
  assert.equal(window.__codexAdministrator.health().ok, true);
});

test("bootstrap installs after the native bridge becomes available", async () => {
  const { intervals, originalSend, window } = await boot({ bridgeInitiallyAvailable: false });

  assert.equal(window.__codexAdministrator.health().ok, false);
  assert.equal(intervals.size, 1);

  window.electronBridge = { sendMessageFromView: originalSend };
  intervals.values().next().value();

  assert.equal(window.__codexAdministrator.health().ok, true);
  assert.notEqual(window.electronBridge.sendMessageFromView, originalSend);
  assert.equal(intervals.size, 0);
});

test("bootstrap keeps retrying through slow official cold starts and still stops", async () => {
  const { intervals, window } = await boot({ bridgeInitiallyAvailable: false });
  const retry = intervals.values().next().value;

  for (let attempt = 0; attempt < 300; attempt += 1) retry();

  assert.equal(intervals.size, 1);
  assert.equal(window.__codexAdministrator.health().ok, false);

  for (let attempt = 300; attempt < 1800; attempt += 1) retry();

  assert.equal(intervals.size, 0);
  assert.equal(window.__codexAdministrator.health().ok, false);
});

test("bootstrap patches only Grok new-thread traffic and preserves GPT", async () => {
  const runtime = await boot();
  const { sent, window } = runtime;
  await activateInjectedModels(runtime);
  const gpt = { type: "mcp-request", request: { id: 1, method: "thread/start", params: { model: "gpt-5.4" } } };
  const grok = { type: "mcp-request", request: { id: 2, method: "thread/start", params: { model: "grok-4" } } };

  await window.electronBridge.sendMessageFromView(gpt);
  await window.electronBridge.sendMessageFromView(grok);

  assert.deepEqual(sent[0], gpt);
  assert.equal(sent[1].request.params.model, "grok-4");
  assert.equal(sent[1].request.params.modelProvider, "grok_native");
  assert.equal("modelProvider" in grok.request.params, false);
});

test("native model-id collisions fail closed before model/list and after reconfigure", async () => {
  const collision = { ...grokModel, id: "gpt-5.4", model: "gpt-5.4" };
  const { sent, window } = await boot({ models: [collision] });
  const request = {
    type: "mcp-request",
    request: { id: 11, method: "thread/start", params: { model: "gpt-5.4" } },
  };

  await window.electronBridge.sendMessageFromView(request);
  assert.deepEqual(sent[0], request);
  assert.equal("modelProvider" in sent[0].request.params, false);

  assert.equal(window.__codexAdministrator.configure({
    version: 2,
    provider_id: "grok_native",
    models: [collision],
  }), true);
  await window.electronBridge.sendMessageFromView(request);
  assert.deepEqual(sent[1], request);
  assert.equal("modelProvider" in sent[1].request.params, false);
});

test("bootstrap appends Grok to the matching native model/list response", async () => {
  const { listeners, window } = await boot();
  await window.electronBridge.sendMessageFromView({
    type: "mcp-request",
    request: { id: "models-2", method: "model/list", params: { cursor: null } },
  });
  const response = {
    type: "mcp-response",
    message: {
      id: "models-2",
      result: { data: [{ id: "gpt-5.4", model: "gpt-5.4", displayName: "GPT-5.4" }] },
    },
  };

  const messageListeners = listeners.get("message") || [];
  assert.equal(messageListeners.length, 1);
  assert.equal(messageListeners[0].options, true);
  messageListeners[0].listener({ data: response });

  assert.deepEqual(response.message.result.data.map((entry) => entry.model), ["gpt-5.4", "grok-4"]);
});

test("dispose restores the official bridge and removes the capture listener", async () => {
  const { listeners, originalSend, window } = await boot();

  assert.notEqual(window.electronBridge.sendMessageFromView, originalSend);
  assert.equal(window.__codexAdministrator.dispose(), true);
  assert.equal(window.electronBridge.sendMessageFromView, originalSend);
  assert.deepEqual(listeners.get("message"), []);
});

test("bootstrap learns a Grok thread before routing its model-less resume", async () => {
  const { listeners, sent, window } = await boot();
  const messageListener = listeners.get("message")[0].listener;
  messageListener({
    data: {
      type: "mcp-response",
      message: {
        id: "read-1",
        result: { thread: { id: "thread-grok", modelProvider: "grok_native" } },
      },
    },
  });

  await window.electronBridge.sendMessageFromView({
    type: "mcp-request",
    request: {
      id: "resume-1",
      method: "thread/resume",
      params: { threadId: "thread-grok", model: null },
    },
  });

  assert.equal(sent[0].request.params.modelProvider, "grok_native");
});
