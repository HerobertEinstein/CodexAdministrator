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

const modelPickerConfig = {
  actionPath: "/responses",
  actionPathAuto: true,
  baseUrl: "https://ai.hebox.net/v1",
  controlNonce: "test-control-nonce-0123456789abcdef",
  credentialPresent: true,
  syncNativeAuth: true,
  syncNativeSessions: false,
};

async function boot({
  autoDiscoverRendererApi = false,
  bridgeInitiallyAvailable = true,
  frozenBridge = false,
  modelPickerHarness = false,
  models = [grokModel],
  rendererApiAvailable = false,
  rendererAddonRegistry = null,
} = {}) {
  const [discovery, core, template] = await Promise.all([
    readFile(new URL("renderer-api-discovery.js", assets), "utf8"),
    readFile(new URL("model-injection-core.js", assets), "utf8"),
    readFile(new URL("bootstrap.js", assets), "utf8"),
  ]);
  const sent = [];
  const listeners = new Map();
  const intervals = new Map();
  const timeouts = new Map();
  let nextIntervalId = 1;
  let nextTimeoutId = 1;
  let openModelPicker = null;
  let modelPickerRequest = null;
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
  if (rendererAddonRegistry) window.__codexAdministratorRendererAddons = rendererAddonRegistry;
  const discoveryImports = [];
  if (autoDiscoverRendererApi) {
    window.__codexAdministratorImportRendererModule = async (url) => {
      discoveryImports.push(url);
      return { rendererApi };
    };
  }
  const modelPickerMount = modelPickerHarness
    ? {
        closeManagerDialog() {},
        createController({ onOpen }) {
          openModelPicker = onOpen;
          return {
            dispose() {},
            health() { return { mounted: true }; },
            reconcile() {},
          };
        },
        openManagerDialog({ request }) {
          modelPickerRequest = request;
          return {};
        },
      }
    : undefined;
  const context = vm.createContext({
    __codexAdministratorModelPickerMount: modelPickerMount,
    clearInterval(id) {
      intervals.delete(id);
    },
    clearTimeout(id) {
      timeouts.delete(id);
    },
    console,
    document: autoDiscoverRendererApi || modelPickerHarness
      ? {
          body: modelPickerHarness ? {} : undefined,
          scripts: autoDiscoverRendererApi
            ? [{ src: "app://-/assets/index-build.js", type: "module" }]
            : [],
        }
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
    setTimeout(callback) {
      const id = nextTimeoutId++;
      timeouts.set(id, callback);
      return id;
    },
    URL,
    window,
  });
  const config = JSON.stringify({
    version: 2,
    provider_id: "grok_native",
    models,
    model_picker: modelPickerConfig,
  });
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
    requestControl(operation, payload) {
      openModelPicker?.();
      if (typeof modelPickerRequest !== "function") {
        throw new Error("model picker request bridge was not captured");
      }
      return modelPickerRequest(operation, payload);
    },
    rendererApi,
    sent,
    timeouts,
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

test("management-only bootstrap stays healthy and never routes Grok", async () => {
  const { sent, window } = await boot({ models: [] });
  const request = {
    type: "mcp-request",
    request: { id: "management-grok", method: "thread/start", params: { model: "grok-4" } },
  };

  assert.equal(window.__codexAdministrator.health().ok, true);
  assert.deepEqual(Array.from(window.__codexAdministrator.health().models), []);
  assert.equal(window.__codexAdministrator.configure({
    version: 2,
    provider_id: "grok_native",
    models: [],
    model_picker: modelPickerConfig,
  }), true);

  await window.electronBridge.sendMessageFromView(request);
  assert.deepEqual(sent[0], request);
  assert.equal("modelProvider" in sent[0].request.params, false);
});

test("renderer configure rejects any descriptor change without a fresh bootstrap", async () => {
  const runtime = await boot({ models: [] });
  const { sent, window } = runtime;
  const fabricated = {
    ...grokModel,
    id: "gpt-not-native",
    model: "gpt-not-native",
    displayName: "Fabricated GPT",
  };

  assert.equal(window.__codexAdministrator.configure({
    version: 2,
    provider_id: "grok_native",
    models: [fabricated],
    model_picker: modelPickerConfig,
  }), false);

  await window.electronBridge.sendMessageFromView({
    type: "mcp-request",
    request: { id: "fabricated-list", method: "model/list", params: { cursor: null } },
  });
  const response = {
    type: "mcp-response",
    message: {
      id: "fabricated-list",
      result: { data: [{ id: "gpt-5.4", model: "gpt-5.4", displayName: "GPT-5.4" }] },
    },
  };
  runtime.listeners.get("message")[0].listener({ data: response });
  assert.deepEqual(response.message.result.data.map((entry) => entry.model), ["gpt-5.4"]);

  sent.length = 0;
  const request = {
    type: "mcp-request",
    request: { id: "fabricated-start", method: "thread/start", params: { model: "gpt-not-native" } },
  };
  await window.electronBridge.sendMessageFromView(request);
  assert.deepEqual(sent[0], request);
  assert.equal("modelProvider" in sent[0].request.params, false);
});

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
  assert.equal(intervals.size, 2);

  window.electronBridge = { sendMessageFromView: originalSend };
  const bridgeRetry = [...intervals.values()].find((callback) => callback.name === "retryBridgePatch");
  assert.equal(typeof bridgeRetry, "function");
  bridgeRetry();

  assert.equal(window.__codexAdministrator.health().ok, true);
  assert.notEqual(window.electronBridge.sendMessageFromView, originalSend);
  assert.equal(intervals.size, 1);
  assert.equal([...intervals.values()][0].name, "maintainModelPickerMount");
});

test("bootstrap keeps retrying through slow official cold starts and still stops", async () => {
  const { intervals, window } = await boot({ bridgeInitiallyAvailable: false });
  const retry = [...intervals.values()].find((callback) => callback.name === "retryBridgePatch");
  assert.equal(typeof retry, "function");

  for (let attempt = 0; attempt < 300; attempt += 1) retry();

  assert.equal(intervals.size, 2);
  assert.equal(window.__codexAdministrator.health().ok, false);

  for (let attempt = 300; attempt < 1800; attempt += 1) retry();

  assert.equal(intervals.size, 1);
  assert.equal([...intervals.values()][0].name, "maintainModelPickerMount");
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
  const runtime = await boot({ models: [collision] });
  const { sent, window } = runtime;
  const request = {
    type: "mcp-request",
    request: { id: 11, method: "thread/start", params: { model: "gpt-5.4" } },
  };

  await window.electronBridge.sendMessageFromView(request);
  assert.deepEqual(sent[0], request);
  assert.equal("modelProvider" in sent[0].request.params, false);

  await activateInjectedModels(runtime);
  assert.deepEqual(
    Array.from(window.__codexAdministrator.health().model_conflicts),
    ["gpt-5.4"],
  );

  assert.equal(window.__codexAdministrator.configure({
    version: 2,
    provider_id: "grok_native",
    models: [collision],
    model_picker: modelPickerConfig,
  }), true);
  await window.electronBridge.sendMessageFromView(request);
  assert.deepEqual(sent[0], request);
  assert.equal("modelProvider" in sent[0].request.params, false);
});

test("reapplying an identical bootstrap keeps activated Grok routing", async () => {
  const runtime = await boot();
  const { sent, window } = runtime;
  await activateInjectedModels(runtime);

  assert.equal(window.__codexAdministrator.configure({
    version: 2,
    provider_id: "grok_native",
    models: [grokModel],
    model_picker: modelPickerConfig,
  }), true);
  await window.electronBridge.sendMessageFromView({
    type: "mcp-request",
    request: { id: 12, method: "thread/start", params: { model: "grok-4" } },
  });

  assert.equal(sent[0].request.params.modelProvider, "grok_native");
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
  let addonDisposals = 0;
  const rendererAddonRegistry = {
    disposeAll() {
      addonDisposals += 1;
      return true;
    },
    health() {
      return { active: [{ id: "reviewed-skin", ok: true }], failed: [] };
    },
  };
  const { listeners, originalSend, window } = await boot({ rendererAddonRegistry });

  assert.notEqual(window.electronBridge.sendMessageFromView, originalSend);
  assert.deepEqual(
    Array.from(window.__codexAdministrator.health().renderer_addons.active, (entry) => entry.id),
    ["reviewed-skin"],
  );
  assert.equal(window.__codexAdministrator.dispose(), true);
  assert.equal(addonDisposals, 1);
  assert.equal("__codexAdministratorRendererAddons" in window, false);
  assert.equal(window.electronBridge.sendMessageFromView, originalSend);
  assert.deepEqual(listeners.get("message"), []);
});

test("dispose preserves an addon registry whose cleanup needs a retry", async () => {
  const rendererAddonRegistry = {
    disposeAll() {
      return false;
    },
    health() {
      return {
        active: [{ id: "sticky-skin", ok: false }],
        failed: [{ id: "sticky-skin", reason: "dispose_failed" }],
      };
    },
  };
  const { window } = await boot({ rendererAddonRegistry });

  assert.equal(window.__codexAdministrator.dispose(), true);
  assert.equal(window.__codexAdministratorRendererAddons, rendererAddonRegistry);
});

test("bootstrap keeps a learned Grok thread fail-closed until a model is activated", async () => {
  const runtime = await boot();
  const { listeners, sent, window } = runtime;
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

  const resume = {
    type: "mcp-request",
    request: {
      id: "resume-1",
      method: "thread/resume",
      params: { threadId: "thread-grok", model: null },
    },
  };

  await window.electronBridge.sendMessageFromView(resume);
  assert.deepEqual(sent[0], resume);
  assert.equal("modelProvider" in sent[0].request.params, false);

  await activateInjectedModels(runtime);
  await window.electronBridge.sendMessageFromView(resume);

  assert.equal(sent[0].request.params.modelProvider, "grok_native");
  assert.equal(sent[0].request.params.model, "grok-4");
});

test("control queue drains only with the configured nonce and resolves by request id", async () => {
  const runtime = await boot({ modelPickerHarness: true });
  const { window } = runtime;
  const pending = runtime.requestControl("state.read", {});

  assert.equal("__codexAdministratorControl" in window, false);

  assert.equal(
    Array.from(window.__codexAdministratorControlInternal.drain("wrong-nonce")).length,
    0,
  );
  const [request] = window.__codexAdministratorControlInternal.drain(
    modelPickerConfig.controlNonce,
  );
  assert.equal(request.operation, "state.read");
  assert.equal(request.nonce, modelPickerConfig.controlNonce);
  assert.equal(
    Array.from(
      window.__codexAdministratorControlInternal.drain(modelPickerConfig.controlNonce),
    ).length,
    0,
  );

  assert.equal(window.__codexAdministratorControlInternal.deliver({
    id: request.id,
    nonce: modelPickerConfig.controlNonce,
    ok: true,
    result: { credential_present: true },
    version: 1,
  }), true);
  assert.deepEqual(await pending, { credential_present: true });
});

test("control queue rejects unknown operations without retaining their payload", async () => {
  const runtime = await boot({ modelPickerHarness: true });
  const { window } = runtime;

  await assert.rejects(
    runtime.requestControl("credential.get", { credential: "must-not-stay" }),
    /unsupported control operation/,
  );
  assert.equal(
    Array.from(
      window.__codexAdministratorControlInternal.drain(modelPickerConfig.controlNonce),
    ).length,
    0,
  );
});

test("timed out control requests cannot be drained and executed later", async () => {
  const runtime = await boot({ modelPickerHarness: true });
  const pending = runtime.requestControl("config.apply", { selected_models: ["grok-4"] });
  const [timeout] = runtime.timeouts.values();
  timeout();

  await assert.rejects(pending, /timed out/);
  assert.equal(
    Array.from(
      runtime.window.__codexAdministratorControlInternal.drain(modelPickerConfig.controlNonce),
    ).length,
    0,
  );
});
