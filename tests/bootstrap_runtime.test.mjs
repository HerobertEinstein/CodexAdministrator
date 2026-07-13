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

async function boot({ bridgeInitiallyAvailable = true } = {}) {
  const [core, template] = await Promise.all([
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
    window.electronBridge = { sendMessageFromView: originalSend };
  }
  const context = vm.createContext({
    clearInterval(id) {
      intervals.delete(id);
    },
    console,
    Set,
    setInterval(callback) {
      const id = nextIntervalId++;
      intervals.set(id, callback);
      return id;
    },
    window,
  });
  const config = JSON.stringify({ version: 2, provider_id: "grok_native", models: [grokModel] });
  vm.runInContext(`${core}\n${template.replace("/*__CODEX_ADMINISTRATOR_CONFIG__*/", config)}`, context);
  return { context, intervals, listeners, originalSend, sent, window };
}

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

test("bootstrap stops retrying when the native bridge never appears", async () => {
  const { intervals, window } = await boot({ bridgeInitiallyAvailable: false });
  const retry = intervals.values().next().value;

  for (let attempt = 0; attempt < 300; attempt += 1) retry();

  assert.equal(intervals.size, 0);
  assert.equal(window.__codexAdministrator.health().ok, false);
});

test("bootstrap patches only Grok new-thread traffic and preserves GPT", async () => {
  const { sent, window } = await boot();
  const gpt = { type: "mcp-request", request: { id: 1, method: "thread/start", params: { model: "gpt-5.4" } } };
  const grok = { type: "mcp-request", request: { id: 2, method: "thread/start", params: { model: "grok-4" } } };

  await window.electronBridge.sendMessageFromView(gpt);
  await window.electronBridge.sendMessageFromView(grok);

  assert.deepEqual(sent[0], gpt);
  assert.equal(sent[1].request.params.model, "grok-4");
  assert.equal(sent[1].request.params.modelProvider, "grok_native");
  assert.equal("modelProvider" in grok.request.params, false);
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
