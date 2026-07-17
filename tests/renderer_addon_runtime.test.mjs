import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const runtimeUrl = new URL("../assets/renderer-addon-runtime.js", import.meta.url);

async function loadRuntime(contextValues = {}) {
  const source = await readFile(runtimeUrl, "utf8");
  const context = vm.createContext({ ...contextValues });
  vm.runInContext(source, context);
  return { context, runtime: context.__codexAdministratorRendererAddons };
}

test("renderer addon runtime isolates failures and disposes successful addons in reverse order", async () => {
  const events = [];
  const { context, runtime } = await loadRuntime();

  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "first",
    revision: "one",
    stateKey: "__FIRST__",
  }, () => {
    context.__FIRST__ = { cleanup() { events.push("dispose:first"); } };
    events.push("apply:first");
  }), true);
  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "broken",
    revision: "bad",
    stateKey: "__BROKEN__",
  }, () => {
    events.push("apply:broken");
    throw new Error("broken addon");
  }), false);
  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "second",
    revision: "two",
    stateKey: "__SECOND__",
  }, () => {
    context.__SECOND__ = { cleanup() { events.push("dispose:second"); } };
    events.push("apply:second");
  }), true);

  assert.deepEqual(Array.from(runtime.health().active, (entry) => entry.id), ["first", "second"]);
  assert.deepEqual(Array.from(runtime.health().failed, (entry) => entry.id), ["broken"]);
  runtime.disposeAll();
  assert.deepEqual(events, [
    "apply:first",
    "apply:broken",
    "apply:second",
    "dispose:second",
    "dispose:first",
  ]);
});

test("loading a new runtime generation cleans the previous generation first", async () => {
  let disposed = 0;
  const first = await loadRuntime();
  first.runtime.apply({
    disposeMethod: "cleanup",
    id: "skin",
    revision: "one",
    stateKey: "__SKIN__",
  }, () => {
    first.context.__SKIN__ = { cleanup() { disposed += 1; } };
  });

  const source = await readFile(runtimeUrl, "utf8");
  vm.runInContext(source, first.context);

  assert.equal(disposed, 1);
  assert.equal(first.context.__codexAdministratorRendererAddons.health().active.length, 0);
});

test("failed cleanup remains retryable and blocks a replacement runtime generation", async () => {
  let cleanupAttempts = 0;
  const first = await loadRuntime();
  const firstRuntime = first.runtime;
  first.runtime.apply({
    disposeMethod: "cleanup",
    id: "sticky-skin",
    revision: "one",
    stateKey: "__STICKY__",
  }, () => {
    first.context.__STICKY__ = {
      cleanup() {
        cleanupAttempts += 1;
        if (cleanupAttempts <= 2) throw new Error("still attached");
      },
    };
  });

  assert.equal(first.runtime.disposeAll(), false);
  assert.equal(first.context.__STICKY__ !== undefined, true);
  assert.equal(first.runtime.health().active[0].ok, false);
  assert.equal(first.runtime.health().failed[0].reason, "dispose_failed");

  const source = await readFile(runtimeUrl, "utf8");
  vm.runInContext(source, first.context);
  assert.equal(first.context.__codexAdministratorRendererAddons, firstRuntime);
  assert.equal(first.context.__codexAdministratorRendererAddons.apply({
    disposeMethod: "cleanup",
    id: "sticky-skin",
    revision: "one",
    stateKey: "__STICKY__",
  }, () => {}), false);
  assert.equal(first.context.__codexAdministratorRendererAddons.apply({
    disposeMethod: "cleanup",
    id: "other-skin",
    revision: "two",
    stateKey: "__OTHER__",
  }, () => {
    first.context.__OTHER__ = { cleanup() {} };
  }), false);
  assert.equal(first.context.__OTHER__, undefined);

  assert.equal(first.runtime.disposeAll(), true);
  assert.equal(first.context.__STICKY__, undefined);
  vm.runInContext(source, first.context);
  assert.notEqual(first.context.__codexAdministratorRendererAddons, firstRuntime);
});

test("cleanup retry keeps the original lifecycle handle when the global is replaced", async () => {
  let originalCleanupAttempts = 0;
  let replacementCleanupAttempts = 0;
  const { context, runtime } = await loadRuntime();
  runtime.apply({
    disposeMethod: "cleanup",
    id: "swapping-skin",
    revision: "one",
    stateKey: "__SWAPPING__",
  }, () => {
    context.__SWAPPING__ = {
      cleanup() {
        originalCleanupAttempts += 1;
        if (originalCleanupAttempts === 1) {
          context.__SWAPPING__ = {
            cleanup() {
              replacementCleanupAttempts += 1;
            },
          };
          throw new Error("cleanup replaced its state before failing");
        }
        delete context.__SWAPPING__;
      },
    };
  });

  assert.equal(runtime.disposeAll(), false);
  assert.equal(runtime.health().active[0].ok, false);
  assert.equal(runtime.health().failed[0].reason, "dispose_failed");
  assert.equal(runtime.disposeAll(), true);
  assert.equal(originalCleanupAttempts, 2);
  assert.equal(replacementCleanupAttempts, 0);
  assert.equal(context.__SWAPPING__, undefined);
});

test("cleanup exception succeeds only when the namespaced global is already absent", async () => {
  const { context, runtime } = await loadRuntime();
  runtime.apply({
    disposeMethod: "cleanup",
    id: "delete-then-throw",
    revision: "one",
    stateKey: "__DELETE_THEN_THROW__",
  }, () => {
    context.__DELETE_THEN_THROW__ = {
      cleanup() {
        delete context.__DELETE_THEN_THROW__;
        throw new Error("cleanup threw after removing its state");
      },
    };
  });

  assert.equal(runtime.disposeAll(), true);
  assert.equal(runtime.health().active.length, 0);
  assert.equal("__DELETE_THEN_THROW__" in context, false);
});

test("installer failure seals the registry when reading partial lifecycle throws", async () => {
  let lifecycleReadable = false;
  let cleanupAttempts = 0;
  const { context, runtime } = await loadRuntime();
  const state = Object.defineProperty({}, "cleanup", {
    configurable: true,
    get() {
      if (!lifecycleReadable) throw new Error("lifecycle getter failed");
      return function cleanup() {
        cleanupAttempts += 1;
        delete context.__GETTER_PARTIAL__;
      };
    },
  });

  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "getter-partial",
    revision: "one",
    stateKey: "__GETTER_PARTIAL__",
  }, () => {
    context.__GETTER_PARTIAL__ = state;
    throw new Error("installer failed after publishing state");
  }), false);
  assert.equal(runtime.health().active[0].ok, false);
  assert.equal(runtime.health().failed[0].reason, "install_cleanup_failed");
  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "later-skin",
    revision: "two",
    stateKey: "__LATER_SKIN__",
  }, () => {
    context.__LATER_SKIN__ = { cleanup() {} };
  }), false);
  assert.equal(context.__LATER_SKIN__, undefined);

  lifecycleReadable = true;
  assert.equal(runtime.disposeAll(), true);
  assert.equal(cleanupAttempts, 1);
  assert.equal(context.__GETTER_PARTIAL__, undefined);
});

test("successful installer seals the registry when lifecycle inspection throws", async () => {
  let lifecycleReadable = false;
  const { context, runtime } = await loadRuntime();
  const state = Object.defineProperty({}, "cleanup", {
    configurable: true,
    get() {
      if (!lifecycleReadable) throw new Error("lifecycle getter failed");
      return function cleanup() {
        delete context.__GETTER_INSTALLED__;
      };
    },
  });

  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "getter-installed",
    revision: "one",
    stateKey: "__GETTER_INSTALLED__",
  }, () => {
    context.__GETTER_INSTALLED__ = state;
  }), false);
  assert.equal(runtime.health().active[0].ok, false);
  assert.equal(runtime.health().failed[0].reason, "lifecycle_unavailable");
  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "later-skin",
    revision: "two",
    stateKey: "__LATER_SKIN__",
  }, () => {
    context.__LATER_SKIN__ = { cleanup() {} };
  }), false);
  assert.equal(context.__LATER_SKIN__, undefined);

  lifecycleReadable = true;
  assert.equal(runtime.disposeAll(), true);
  assert.equal(context.__GETTER_INSTALLED__, undefined);
});

test("cleanup retry recaptures a namespaced global after its getter recovers", async () => {
  let globalReadable = false;
  let cleanupAttempts = 0;
  const { context, runtime } = await loadRuntime();
  const state = {
    cleanup() {
      cleanupAttempts += 1;
      delete context.__GLOBAL_GETTER__;
    },
  };

  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "global-getter",
    revision: "one",
    stateKey: "__GLOBAL_GETTER__",
  }, () => {
    Object.defineProperty(context, "__GLOBAL_GETTER__", {
      configurable: true,
      get() {
        if (!globalReadable) throw new Error("global getter failed");
        return state;
      },
    });
  }), false);
  assert.equal(runtime.health().active[0].ok, false);
  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "later-skin",
    revision: "two",
    stateKey: "__LATER_SKIN__",
  }, () => {
    context.__LATER_SKIN__ = { cleanup() {} };
  }), false);

  globalReadable = true;
  assert.equal(runtime.disposeAll(), true);
  assert.equal(cleanupAttempts, 1);
  assert.equal("__GLOBAL_GETTER__" in context, false);
});

test("cleanup retry rejects primitive and foreign replacements before accessor recovery", async () => {
  let globalReadable = false;
  let currentState;
  let originalCleanupAttempts = 0;
  let foreignCleanupAttempts = 0;
  const { context, runtime } = await loadRuntime();
  const originalState = {
    cleanup() {
      originalCleanupAttempts += 1;
      delete context.__IDENTITY_GETTER__;
    },
  };
  const originalGetter = function originalGetter() {
    if (!globalReadable) throw new Error("global getter failed");
    return currentState;
  };
  currentState = originalState;
  context.__PRIMITIVE_CLEANUP_COUNT__ = 0;
  vm.runInContext(`
    Object.defineProperty(String.prototype, "cleanup", {
      configurable: true,
      value() { globalThis.__PRIMITIVE_CLEANUP_COUNT__ += 1; },
    });
  `, context);

  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "identity-getter",
    revision: "one",
    stateKey: "__IDENTITY_GETTER__",
  }, () => {
    Object.defineProperty(context, "__IDENTITY_GETTER__", {
      configurable: true,
      get: originalGetter,
    });
  }), false);

  globalReadable = true;
  currentState = "foreign primitive";
  assert.equal(runtime.disposeAll(), false);
  assert.equal(context.__PRIMITIVE_CLEANUP_COUNT__, 0);

  Object.defineProperty(context, "__IDENTITY_GETTER__", {
    configurable: true,
    value: {
      cleanup() {
        foreignCleanupAttempts += 1;
        delete context.__IDENTITY_GETTER__;
      },
    },
    writable: true,
  });
  assert.equal(runtime.disposeAll(), false);
  assert.equal(foreignCleanupAttempts, 0);
  assert.equal(runtime.health().active.length, 1);

  currentState = originalState;
  Object.defineProperty(context, "__IDENTITY_GETTER__", {
    configurable: true,
    get: originalGetter,
  });
  assert.equal(runtime.disposeAll(), true);
  assert.equal(originalCleanupAttempts, 1);
  assert.equal(foreignCleanupAttempts, 0);
  assert.equal("__IDENTITY_GETTER__" in context, false);
});

test("cleanup retry rejects an uncaptured accessor whose setter can replace its state", async () => {
  let globalReadable = false;
  let backingState;
  let originalCleanupAttempts = 0;
  let foreignCleanupAttempts = 0;
  const { context, runtime } = await loadRuntime();
  const originalState = {
    cleanup() {
      originalCleanupAttempts += 1;
      delete context.__SETTER_GETTER__;
    },
  };
  backingState = originalState;

  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "setter-getter",
    revision: "one",
    stateKey: "__SETTER_GETTER__",
  }, () => {
    Object.defineProperty(context, "__SETTER_GETTER__", {
      configurable: true,
      get() {
        if (!globalReadable) throw new Error("global getter failed");
        return backingState;
      },
      set(value) {
        backingState = value;
      },
    });
  }), false);

  globalReadable = true;
  context.__SETTER_GETTER__ = {
    cleanup() {
      foreignCleanupAttempts += 1;
      delete context.__SETTER_GETTER__;
    },
  };
  assert.equal(runtime.disposeAll(), false);
  assert.equal(originalCleanupAttempts, 0);
  assert.equal(foreignCleanupAttempts, 0);
  assert.equal(runtime.health().active.length, 1);

  delete context.__SETTER_GETTER__;
  assert.equal(runtime.disposeAll(), true);
  assert.equal(runtime.health().active.length, 0);
});

test("blocking pending install keeps later addons pending after the registry seals", async () => {
  const timers = new Map();
  let nextTimer = 1;
  let cleanupFails = true;
  let secondInstalls = 0;
  const document = { body: null, documentElement: null };
  const { context, runtime } = await loadRuntime({
    clearTimeout(id) {
      timers.delete(id);
    },
    document,
    setTimeout(callback) {
      const id = nextTimer;
      nextTimer += 1;
      timers.set(id, callback);
      return id;
    },
  });

  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "blocking-first",
    revision: "one",
    stateKey: "__BLOCKING_FIRST__",
  }, () => {
    context.__BLOCKING_FIRST__ = {
      cleanup() {
        if (cleanupFails) throw new Error("cleanup still blocked");
        delete context.__BLOCKING_FIRST__;
      },
    };
    throw new Error("installer failed after publishing state");
  }), true);
  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "second-pending",
    revision: "two",
    stateKey: "__SECOND_PENDING__",
  }, () => {
    secondInstalls += 1;
    context.__SECOND_PENDING__ = { cleanup() {} };
  }), true);

  document.documentElement = {};
  document.body = {};
  const retry = [...timers.values()][0];
  timers.clear();
  retry();

  assert.equal(secondInstalls, 0);
  assert.deepEqual(
    Array.from(runtime.health().active, (entry) => entry.id),
    ["blocking-first"],
  );
  assert.deepEqual(
    Array.from(runtime.health().pending, (entry) => entry.id),
    ["second-pending"],
  );
  assert.equal(context.__SECOND_PENDING__, undefined);
  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "later-skin",
    revision: "three",
    stateKey: "__LATER_SKIN__",
  }, () => {}), false);

  cleanupFails = false;
  assert.equal(runtime.disposeAll(), true);
  assert.equal(runtime.health().pending.length, 0);
  assert.equal(secondInstalls, 0);
});

test("renderer payload installation waits for a usable DOM and remains disposable while pending", async () => {
  const timers = new Map();
  let nextTimer = 1;
  let installs = 0;
  const document = { body: null, documentElement: null };
  const { context, runtime } = await loadRuntime({
    clearTimeout(id) {
      timers.delete(id);
    },
    document,
    setTimeout(callback) {
      const id = nextTimer;
      nextTimer += 1;
      timers.set(id, callback);
      return id;
    },
  });

  assert.equal(runtime.apply({
    disposeMethod: "cleanup",
    id: "deferred-skin",
    revision: "one",
    stateKey: "__DEFERRED__",
  }, () => {
    installs += 1;
    context.__DEFERRED__ = { cleanup() {} };
  }), true);
  assert.equal(installs, 0);
  assert.deepEqual(Array.from(runtime.health().pending, (entry) => entry.id), ["deferred-skin"]);
  assert.equal(timers.size, 1);

  document.documentElement = {};
  document.body = {};
  const retry = [...timers.values()][0];
  timers.clear();
  retry();

  assert.equal(installs, 1);
  assert.deepEqual(Array.from(runtime.health().active, (entry) => entry.id), ["deferred-skin"]);
  assert.equal(runtime.health().pending.length, 0);
  runtime.disposeAll();
  assert.equal(timers.size, 0);
});
