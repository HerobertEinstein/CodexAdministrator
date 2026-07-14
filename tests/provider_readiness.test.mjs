import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const readinessUrl = new URL("../assets/provider-readiness.js", import.meta.url);

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

async function startReadiness({ postMessage } = {}) {
  const source = await readFile(readinessUrl, "utf8");
  const listeners = [];
  const sent = [];
  const timers = new Map();
  let nextTimerId = 1;
  const window = {
    addEventListener(type, listener, capture) {
      listeners.push({ type, listener, capture });
    },
    removeEventListener(type, listener, capture) {
      const index = listeners.findIndex(
        (entry) =>
          entry.type === type &&
          entry.listener === listener &&
          entry.capture === capture,
      );
      if (index !== -1) listeners.splice(index, 1);
    },
  };
  if (postMessage !== null) {
    window.__codexAdministratorRendererApi = {
      postMessage(message) {
        sent.push(message);
        return postMessage?.(message);
      },
    };
  }
  const context = vm.createContext({
    clearTimeout(id) {
      timers.delete(id);
    },
    Date,
    Error,
    Math,
    Promise,
    setTimeout(callback) {
      const id = nextTimerId++;
      timers.set(id, callback);
      return id;
    },
    String,
    window,
  });

  return {
    listeners,
    result: vm.runInContext(source, context, { filename: readinessUrl.pathname }),
    sent,
    timers,
  };
}

function respond(runtime, message) {
  assert.equal(runtime.listeners.length, 1);
  runtime.listeners[0].listener({
    data: {
      type: "mcp-response",
      hostId: "local",
      message,
    },
  });
}

test("provider readiness fails closed without the official renderer API", async () => {
  const runtime = await startReadiness({ postMessage: null });

  assert.deepEqual(plain(await runtime.result), {
    ok: false,
    error: "renderer API unavailable",
  });
  assert.deepEqual(runtime.sent, []);
  assert.deepEqual(runtime.listeners, []);
});

test("provider readiness accepts only config/read with grok_native loaded", async () => {
  const runtime = await startReadiness();
  const request = runtime.sent[0];

  assert.equal(request.type, "mcp-request");
  assert.equal(request.hostId, "local");
  assert.equal(request.request.method, "config/read");
  assert.deepEqual(plain(request.request.params), { includeLayers: false, cwd: null });

  respond(runtime, {
    id: request.request.id,
    result: {
      config: {
        model_providers: {
          grok_native: { wire_api: "responses" },
        },
      },
    },
  });

  assert.deepEqual(plain(await runtime.result), {
    ok: true,
    provider: "grok_native",
  });
  assert.deepEqual(runtime.listeners, []);
  assert.equal(runtime.timers.size, 0);
});

test("provider readiness rejects a successful config/read without grok_native", async () => {
  const runtime = await startReadiness();
  const requestId = runtime.sent[0].request.id;

  respond(runtime, {
    id: requestId,
    result: { config: { model_providers: {} } },
  });

  assert.deepEqual(plain(await runtime.result), {
    ok: false,
    error: "model provider 'grok_native' not found",
  });
  assert.deepEqual(runtime.listeners, []);
  assert.equal(runtime.timers.size, 0);
});

test("provider readiness fails closed when config/read cannot be sent", async () => {
  const runtime = await startReadiness({
    postMessage: async () => {
      throw new Error("renderer transport closed");
    },
  });

  assert.deepEqual(plain(await runtime.result), {
    ok: false,
    error: "renderer transport closed",
  });
  assert.deepEqual(runtime.listeners, []);
  assert.equal(runtime.timers.size, 0);
});

test("provider readiness times out and removes its capture listener", async () => {
  const runtime = await startReadiness();
  const timeout = runtime.timers.values().next().value;

  timeout();

  assert.deepEqual(plain(await runtime.result), {
    ok: false,
    error: "config/read timed out",
  });
  assert.deepEqual(runtime.listeners, []);
  assert.equal(runtime.timers.size, 0);
});
