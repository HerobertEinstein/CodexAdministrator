(() => {
  "use strict";

  const globalKey = "__codexAdministratorRendererAddons";
  const previous = globalThis[globalKey];
  if (previous && typeof previous.disposeAll === "function") {
    let disposed = false;
    try {
      disposed = previous.disposeAll() !== false;
    } catch {
      disposed = false;
    }
    if (!disposed) return;
  }

  const active = [];
  const failed = [];
  const pending = [];
  const ids = new Set();
  const blockingFailures = new Set();
  const disposalFailures = new Set();
  const maxAddons = 16;
  let retryTimer = null;
  let accepting = true;

  function validIdentifier(value) {
    return typeof value === "string"
      && value.length > 0
      && value.length <= 128
      && /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(value);
  }

  function validDescriptor(descriptor) {
    return descriptor
      && typeof descriptor.id === "string"
      && /^[a-z0-9-]{1,64}$/.test(descriptor.id)
      && typeof descriptor.revision === "string"
      && descriptor.revision.length > 0
      && descriptor.revision.length <= 128
      && validIdentifier(descriptor.stateKey)
      && validIdentifier(descriptor.disposeMethod);
  }

  function observeState(stateKey) {
    let descriptor;
    try {
      descriptor = Object.getOwnPropertyDescriptor(globalThis, stateKey);
    } catch {
      return { accessorIdentity: null, exists: true, readable: false, state: undefined };
    }
    if (!descriptor) {
      try {
        if (!(stateKey in globalThis)) {
          return { accessorIdentity: null, exists: false, readable: true, state: undefined };
        }
      } catch {
        // An inherited or exotic property cannot provide a stable retry identity.
      }
      return { accessorIdentity: null, exists: true, readable: false, state: undefined };
    }
    if (Object.prototype.hasOwnProperty.call(descriptor, "value")) {
      return {
        accessorIdentity: null,
        exists: true,
        readable: true,
        state: descriptor.value,
      };
    }
    const accessorIdentity = Object.freeze({
      configurable: descriptor.configurable === true,
      enumerable: descriptor.enumerable === true,
      get: descriptor.get,
      set: descriptor.set,
    });
    try {
      return { accessorIdentity, exists: true, readable: true, state: globalThis[stateKey] };
    } catch {
      return { accessorIdentity, exists: true, readable: false, state: undefined };
    }
  }

  function sameAccessorIdentity(expected, observed) {
    return Boolean(
      expected
      && observed
      && expected.configurable === observed.configurable
      && expected.enumerable === observed.enumerable
      && expected.get === observed.get
      && expected.set === observed.set
      && expected.set === undefined
    );
  }

  function lifecycleMethod(state, disposeMethod) {
    try {
      const lifecycle = state?.[disposeMethod];
      return typeof lifecycle === "function" ? lifecycle : null;
    } catch {
      return null;
    }
  }

  function capturableState(state) {
    return (typeof state === "object" && state !== null) || typeof state === "function";
  }

  function disposeRecord(record) {
    let state = record.state;
    if (record.stateCaptured && !observeState(record.stateKey).exists) return true;
    if (!record.stateCaptured) {
      const recovered = observeState(record.stateKey);
      if (!recovered.exists) return true;
      if (
        !recovered.readable
        || !capturableState(recovered.state)
        || !sameAccessorIdentity(record.stateAccessorIdentity, recovered.accessorIdentity)
      ) return false;
      state = recovered.state;
    }
    const lifecycle = lifecycleMethod(state, record.disposeMethod);
    if (!lifecycle) return !observeState(record.stateKey).exists;
    try {
      lifecycle.call(state);
    } catch {
      return !observeState(record.stateKey).exists;
    }
    const observed = observeState(record.stateKey);
    if (observed.readable && observed.state === state) {
      try {
        delete globalThis[record.stateKey];
      } catch {
        return false;
      }
    }
    return !observeState(record.stateKey).exists;
  }

  function recordFailure(descriptor, reason, blocking = false) {
    const entry = Object.freeze({
      id: descriptor.id,
      reason,
      revision: descriptor.revision,
    });
    const index = failed.findIndex((candidate) => candidate.id === descriptor.id);
    if (index >= 0) failed[index] = entry;
    else failed.push(entry);
    if (blocking) blockingFailures.add(descriptor.id);
  }

  function clearFailure(id) {
    for (let index = failed.length - 1; index >= 0; index -= 1) {
      if (failed[index].id === id) failed.splice(index, 1);
    }
    blockingFailures.delete(id);
    disposalFailures.delete(id);
  }

  function rendererReady() {
    return typeof document === "undefined"
      || Boolean(document.documentElement && document.body);
  }

  function install(entry) {
    const { descriptor, installer } = entry;
    try {
      installer();
    } catch {
      const partial = observeState(descriptor.stateKey);
      const record = Object.freeze({
        disposeMethod: descriptor.disposeMethod,
        id: descriptor.id,
        revision: descriptor.revision,
        state: partial.state,
        stateAccessorIdentity: partial.accessorIdentity,
        stateCaptured: partial.readable && capturableState(partial.state),
        stateKey: descriptor.stateKey,
      });
      if (partial.exists) {
        if (!disposeRecord(record)) {
          accepting = false;
          active.push(record);
          disposalFailures.add(descriptor.id);
          recordFailure(descriptor, "install_cleanup_failed", true);
          return false;
        }
        recordFailure(descriptor, "install_failed");
        return false;
      }
      recordFailure(descriptor, "install_failed", true);
      return false;
    }
    const observed = observeState(descriptor.stateKey);
    const record = Object.freeze({
      disposeMethod: descriptor.disposeMethod,
      id: descriptor.id,
      revision: descriptor.revision,
      state: observed.state,
      stateAccessorIdentity: observed.accessorIdentity,
      stateCaptured: observed.readable && capturableState(observed.state),
      stateKey: descriptor.stateKey,
    });
    if (!observed.exists || !observed.readable
      || !lifecycleMethod(observed.state, descriptor.disposeMethod)) {
      accepting = false;
      if (observed.exists) {
        active.push(record);
        disposalFailures.add(descriptor.id);
      }
      recordFailure(descriptor, "lifecycle_unavailable", true);
      return false;
    }
    active.push(record);
    return true;
  }

  function schedulePending() {
    if (retryTimer !== null || pending.length === 0) return;
    if (typeof setTimeout !== "function") {
      while (pending.length > 0) {
        const entry = pending.shift();
        failed.push(Object.freeze({
          id: entry.descriptor.id,
          reason: "dom_unavailable",
          revision: entry.descriptor.revision,
        }));
      }
      return;
    }
    retryTimer = setTimeout(() => {
      retryTimer = null;
      if (!rendererReady()) {
        schedulePending();
        return;
      }
      const ready = pending.splice(0);
      for (let index = 0; index < ready.length; index += 1) {
        if (!accepting) {
          pending.push(...ready.slice(index));
          break;
        }
        install(ready[index]);
      }
    }, 50);
  }

  function apply(descriptor, installer) {
    if (
      !accepting
      || active.length + pending.length >= maxAddons
      || !validDescriptor(descriptor)
      || typeof installer !== "function"
      || ids.has(descriptor.id)
    ) {
      return false;
    }
    ids.add(descriptor.id);
    const entry = Object.freeze({
      descriptor: Object.freeze({ ...descriptor }),
      installer,
    });
    if (!rendererReady()) {
      pending.push(entry);
      schedulePending();
      return true;
    }
    return install(entry);
  }

  function disposeAll() {
    let ok = true;
    accepting = false;
    if (retryTimer !== null && typeof clearTimeout === "function") {
      clearTimeout(retryTimer);
    }
    retryTimer = null;
    for (const entry of pending.splice(0)) ids.delete(entry.descriptor.id);
    for (let index = active.length - 1; index >= 0; index -= 1) {
      const record = active[index];
      if (disposeRecord(record)) {
        active.splice(index, 1);
        ids.delete(record.id);
        clearFailure(record.id);
      } else {
        ok = false;
        disposalFailures.add(record.id);
        recordFailure(record, "dispose_failed", true);
      }
    }
    if (blockingFailures.size > 0) ok = false;
    if (ok) {
      failed.splice(0);
      ids.clear();
      blockingFailures.clear();
      disposalFailures.clear();
    }
    return ok;
  }

  function health() {
    return Object.freeze({
      active: Object.freeze(active.map((record) => Object.freeze({
        id: record.id,
        ok: (() => {
          const observed = observeState(record.stateKey);
          return Boolean(
            !disposalFailures.has(record.id)
            && observed.readable
            && observed.state === record.state
            && lifecycleMethod(record.state, record.disposeMethod)
          );
        })(),
        revision: record.revision,
      }))),
      failed: Object.freeze([...failed]),
      pending: Object.freeze(pending.map((entry) => Object.freeze({
        id: entry.descriptor.id,
        revision: entry.descriptor.revision,
      }))),
    });
  }

  globalThis[globalKey] = Object.freeze({ apply, disposeAll, health });
})();
