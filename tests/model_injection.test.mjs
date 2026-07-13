import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const coreUrl = new URL("../assets/model-injection-core.js", import.meta.url);

async function loadCore() {
  const source = await readFile(coreUrl, "utf8");
  const context = vm.createContext({ globalThis: {} });
  vm.runInContext(source, context, { filename: coreUrl.pathname });
  return context.globalThis.__codexAdministratorModelInjectionCore;
}

const grokModel = Object.freeze({
  id: "grok-4",
  model: "grok-4",
  upgrade: null,
  upgradeInfo: null,
  availabilityNux: null,
  displayName: "Grok 4",
  description: "Configured xAI model; new tasks only",
  hidden: false,
  supportedReasoningEfforts: [
    { reasoningEffort: "medium", description: "Balanced reasoning" },
  ],
  defaultReasoningEffort: "medium",
  inputModalities: ["text"],
  supportsPersonality: false,
  additionalSpeedTiers: [],
  serviceTiers: [],
  defaultServiceTier: null,
  isDefault: false,
});

test("model/list keeps every native GPT entry and appends Grok once", async () => {
  const core = await loadCore();
  const gpt = {
    id: "gpt-5.4",
    model: "gpt-5.4",
    displayName: "GPT-5.4",
    description: "Native model",
    hidden: false,
    supportedReasoningEfforts: [],
    defaultReasoningEffort: "medium",
    inputModalities: ["text", "image"],
    supportsPersonality: true,
    additionalSpeedTiers: [],
    serviceTiers: [],
    defaultServiceTier: null,
    isDefault: true,
  };
  const response = { data: [gpt], nextCursor: null };

  assert.equal(core.appendModels(response, [grokModel]), true);
  assert.equal(response.data[0], gpt);
  assert.deepEqual(response.data.map((entry) => entry.model), ["gpt-5.4", "grok-4"]);
  assert.equal("modelProvider" in response.data[1], false);

  assert.equal(core.appendModels(response, [grokModel]), false);
  assert.deepEqual(response.data.map((entry) => entry.model), ["gpt-5.4", "grok-4"]);
});

test("only Grok thread/start and thread/resume messages receive grok_native", async () => {
  const core = await loadCore();
  const grokModels = new Set(["grok-4"]);
  const shapes = [
    {
      name: "send-cli-request-for-host",
      message: { type: "send-cli-request-for-host", method: "thread/start", params: { model: "MODEL" } },
      read: (message) => message.params,
    },
    {
      name: "mcp-request",
      message: { type: "mcp-request", request: { id: 7, method: "thread/start", params: { model: "MODEL" } } },
      read: (message) => message.request.params,
    },
    {
      name: "worker-request",
      message: { type: "worker-request", request: { id: 8, method: "thread/resume", params: { model: "MODEL" } } },
      read: (message) => message.request.params,
    },
    {
      name: "thread-prewarm-start",
      message: { type: "thread-prewarm-start", request: { id: 9, params: { model: "MODEL" } } },
      read: (message) => message.request.params,
    },
    {
      name: "start-conversation",
      message: { type: "start-conversation", model: "MODEL" },
      read: (message) => message,
    },
    {
      name: "prewarm-thread-start-for-host",
      message: { type: "prewarm-thread-start-for-host", params: { model: "MODEL" } },
      read: (message) => message.params,
    },
    {
      name: "start-thread-for-host",
      message: { type: "start-thread-for-host", model: "MODEL" },
      read: (message) => message,
    },
  ];

  for (const shape of shapes) {
    const gptMessage = structuredClone(shape.message);
    const gptParams = shape.read(gptMessage);
    gptParams.model = "gpt-5.4";
    const gptSnapshot = structuredClone(gptMessage);
    assert.equal(core.routeProvider(gptMessage, grokModels, "grok_native"), gptMessage, shape.name);
    assert.deepEqual(gptMessage, gptSnapshot, `${shape.name} changed a GPT request`);

    const grokMessage = structuredClone(shape.message);
    shape.read(grokMessage).model = "grok-4";
    const routed = core.routeProvider(grokMessage, grokModels, "grok_native");
    assert.notEqual(routed, grokMessage, shape.name);
    assert.equal(shape.read(routed).modelProvider, "grok_native", shape.name);
    assert.equal("modelProvider" in shape.read(grokMessage), false, `${shape.name} mutated its input`);
  }
});

test("turn/start and existing-thread model changes never fake a provider switch", async () => {
  const core = await loadCore();
  const grokModels = new Set(["grok-4"]);
  const messages = [
    { type: "send-cli-request-for-host", method: "turn/start", params: { model: "grok-4" } },
    { type: "mcp-request", request: { id: 10, method: "thread/settings/update", params: { model: "grok-4" } } },
  ];

  for (const message of messages) {
    const snapshot = structuredClone(message);
    assert.equal(core.routeProvider(message, grokModels, "grok_native"), message);
    assert.deepEqual(message, snapshot);
  }
});

test("Grok thread metadata routes only that thread's model-less resume", async () => {
  const core = await loadCore();
  const grokModels = new Set(["grok-4"]);
  const grokThreads = new Set();
  const metadata = {
    type: "mcp-response",
    message: {
      id: "thread-read-1",
      result: {
        thread: { id: "thread-grok", modelProvider: "grok_native" },
      },
    },
  };

  assert.equal(core.learnGrokThreads(metadata, grokThreads, "grok_native"), true);
  assert.deepEqual([...grokThreads], ["thread-grok"]);

  const grokResume = {
    type: "mcp-request",
    request: { id: 12, method: "thread/resume", params: { threadId: "thread-grok", model: null } },
  };
  const gptResume = {
    type: "mcp-request",
    request: { id: 13, method: "thread/resume", params: { threadId: "thread-gpt", model: null } },
  };
  const routed = core.routeProvider(grokResume, grokModels, "grok_native", grokThreads);

  assert.equal(routed.request.params.modelProvider, "grok_native");
  assert.equal(core.routeProvider(gptResume, grokModels, "grok_native", grokThreads), gptResume);
});

test("thread lists teach the router without treating native GPT threads as Grok", async () => {
  const core = await loadCore();
  const grokThreads = new Set();
  const response = {
    type: "mcp-response",
    message: {
      id: "thread-list-1",
      result: {
        data: [
          { id: "thread-grok", modelProvider: "grok_native" },
          { id: "thread-gpt", modelProvider: "openai" },
        ],
      },
    },
  };

  assert.equal(core.learnGrokThreads(response, grokThreads, "grok_native"), true);
  assert.deepEqual([...grokThreads], ["thread-grok"]);
});

test("only the matching model/list response is patched", async () => {
  const core = await loadCore();
  const request = {
    type: "mcp-request",
    request: { id: "models-1", method: "model/list", params: { cursor: null } },
  };
  const pending = new Set();

  core.trackModelListRequest(request, pending);
  assert.deepEqual([...pending], ["models-1"]);

  const unrelated = { type: "mcp-response", message: { id: "other", result: { data: [] } } };
  assert.equal(core.patchModelListMessage(unrelated, pending, [grokModel]), false);
  assert.equal(unrelated.message.result.data.length, 0);

  const response = { type: "mcp-response", message: { id: "models-1", result: { data: [] } } };
  assert.equal(core.patchModelListMessage(response, pending, [grokModel]), true);
  assert.deepEqual(response.message.result.data.map((entry) => entry.model), ["grok-4"]);
  assert.equal(pending.size, 0);
});
