(() => {
  "use strict";

  const config = /*__CODEX_ADMINISTRATOR_CONFIG__*/;
  const core = globalThis.__codexAdministratorModelInjectionCore;
  const modelPickerMount = globalThis.__codexAdministratorModelPickerMount;
  const rendererApiDiscovery = globalThis.__codexAdministratorRendererApiDiscovery;
  if (!core) return;

  const prior = window.__codexAdministrator;
  if (prior && typeof prior.configure === "function") {
    prior.configure(config);
    prior.mount();
    return;
  }

  let currentConfig = config;
  let currentConfigSignature = JSON.stringify(config);
  const allowedControlOperations = new Set([
    "config.apply",
    "credential.clear",
    "models.discover",
    "state.read",
  ]);
  let controlRequestSequence = 0;
  const controlQueue = [];
  const pendingControlRequests = new Map();
  let grokModels = new Set();
  const grokModelConflicts = new Set();
  const grokThreadIds = new Set();
  const grokThreadModels = new Map();
  const pendingModelListRequests = new Set();
  let activeTransport = null;
  let activeTransportMethod = null;
  let originalPostMessage = null;
  let patchedPostMessage = null;
  let messageListenerInstalled = false;
  let bridgeRetryTimer = null;
  let bridgeRetryAttempts = 0;
  let rendererDiscoveryPromise = null;
  let discoveredRendererApi = null;
  let mounted = false;
  let modelPickerController = null;
  let modelPickerObserver = null;
  let modelPickerFrame = null;
  let modelPickerRetryTimer = null;
  const maxBridgeRetryAttempts = 1800;

  function controlNonce() {
    return typeof currentConfig.model_picker?.controlNonce === "string"
      ? currentConfig.model_picker.controlNonce
      : "";
  }

  function rejectPendingControlRequests(message) {
    controlQueue.splice(0);
    for (const pending of pendingControlRequests.values()) {
      if (pending.timer !== null && typeof clearTimeout === "function") {
        clearTimeout(pending.timer);
      }
      pending.reject(new Error(message));
    }
    pendingControlRequests.clear();
  }

  function removeQueuedControlRequest(id) {
    const index = controlQueue.findIndex((request) => request.id === id);
    if (index >= 0) controlQueue.splice(index, 1);
  }

  function requestControl(operation, payload = {}) {
    if (!allowedControlOperations.has(operation)) {
      return Promise.reject(new Error("unsupported control operation"));
    }
    const nonce = controlNonce();
    if (!nonce) return Promise.reject(new Error("secure broker is unavailable"));
    let payloadSize;
    try {
      payloadSize = JSON.stringify(payload).length;
    } catch {
      return Promise.reject(new Error("control payload is not serializable"));
    }
    if (payloadSize > 64 * 1024 || controlQueue.length >= 32) {
      return Promise.reject(new Error("secure broker queue is full"));
    }
    controlRequestSequence += 1;
    const id = `ca-${controlRequestSequence.toString(36)}`;
    controlQueue.push({ id, nonce, operation, payload, version: 1 });
    return new Promise((resolve, reject) => {
      const timer = typeof setTimeout === "function"
        ? setTimeout(() => {
          removeQueuedControlRequest(id);
          pendingControlRequests.delete(id);
          reject(new Error("secure broker request timed out"));
        }, 30_000)
        : null;
      pendingControlRequests.set(id, { reject, resolve, timer });
    });
  }

  function drainControlRequests(expectedNonce) {
    if (!expectedNonce || expectedNonce !== controlNonce()) return [];
    return controlQueue
      .splice(0, 32)
      .filter((request) => pendingControlRequests.has(request.id));
  }

  function deliverControlResponse(response) {
    if (
      !response
      || response.version !== 1
      || response.nonce !== controlNonce()
      || typeof response.id !== "string"
    ) return false;
    const pending = pendingControlRequests.get(response.id);
    if (!pending) return false;
    pendingControlRequests.delete(response.id);
    if (pending.timer !== null && typeof clearTimeout === "function") clearTimeout(pending.timer);
    if (response.ok === true) {
      pending.resolve(response.result ?? null);
    } else {
      pending.reject(new Error(
        typeof response.error === "string" && response.error
          ? response.error
          : "secure broker request failed",
      ));
    }
    return true;
  }

  const internalControlApi = Object.freeze({
    deliver: deliverControlResponse,
    drain: drainControlRequests,
  });
  window.__codexAdministratorControlInternal = internalControlApi;

  function stopBridgeRetry() {
    if (bridgeRetryTimer !== null) {
      clearInterval(bridgeRetryTimer);
      bridgeRetryTimer = null;
    }
    bridgeRetryAttempts = 0;
  }

  function retryBridgePatch() {
    bridgeRetryAttempts += 1;
    if (installBridgePatch()) return;
    requestRendererApiDiscovery();
    if (bridgeRetryAttempts >= maxBridgeRetryAttempts) stopBridgeRetry();
  }

  function requestRendererApiDiscovery() {
    if (
      !mounted
      || !rendererApiDiscovery
      || rendererDiscoveryPromise
      || window.__codexAdministratorRendererApi
      || typeof document === "undefined"
      || typeof fetch !== "function"
    ) {
      return;
    }
    const importModule = typeof window.__codexAdministratorImportRendererModule === "function"
      ? window.__codexAdministratorImportRendererModule
      : (url) => import(url);
    rendererDiscoveryPromise = rendererApiDiscovery.discoverRendererApi({
      documentRef: document,
      fetchFn: (url) => fetch(url),
      importModule,
    }).then((rendererApi) => {
      if (!mounted || !rendererApi) return;
      discoveredRendererApi = rendererApi;
      window.__codexAdministratorRendererApi = rendererApi;
      installBridgePatch();
    }).catch(() => {
      // An unknown host stays native until an exact renderer API is found.
    }).finally(() => {
      rendererDiscoveryPromise = null;
    });
  }

  function findWritableTransport() {
    const candidates = [
      [window.__codexAdministratorRendererApi, "postMessage"],
      [window.electronBridge, "sendMessageFromView"],
    ];
    for (const [target, method] of candidates) {
      if (!target || typeof target[method] !== "function") continue;
      const descriptor = Object.getOwnPropertyDescriptor(target, method);
      if (descriptor && descriptor.writable === false && typeof descriptor.set !== "function") {
        continue;
      }
      return { method, target };
    }
    return null;
  }

  function installBridgePatch() {
    const candidate = findWritableTransport();
    if (!candidate) return false;
    const { method, target } = candidate;
    if (
      activeTransport === target
      && activeTransportMethod === method
      && target[method] === patchedPostMessage
    ) {
      return true;
    }
    restoreBridgePatch();
    const original = target[method];
    const patched = function codexAdministratorPostMessage(message) {
      core.trackModelListRequest(message, pendingModelListRequests);
      const routed = core.routeProvider(
        message,
        grokModels,
        currentConfig.provider_id,
        grokThreadIds,
        grokThreadModels,
      );
      return original.call(target, routed);
    };
    try {
      target[method] = patched;
    } catch {
      return false;
    }
    const installed = target[method] === patched;
    if (installed) {
      activeTransport = target;
      activeTransportMethod = method;
      originalPostMessage = original;
      patchedPostMessage = patched;
    }
    if (installed) stopBridgeRetry();
    return installed;
  }

  function restoreBridgePatch() {
    if (
      activeTransport
      && activeTransportMethod
      && originalPostMessage
      && activeTransport[activeTransportMethod] === patchedPostMessage
    ) {
      try {
        activeTransport[activeTransportMethod] = originalPostMessage;
      } catch {
        // A host update may freeze the transport after mounting; fail closed.
      }
    }
    activeTransport = null;
    activeTransportMethod = null;
    originalPostMessage = null;
    patchedPostMessage = null;
  }

  function handleMessage(event) {
    core.learnGrokThreads(
      event?.data,
      grokThreadIds,
      currentConfig.provider_id,
      grokThreadModels,
      grokModels,
    );
    core.patchModelListMessage(
      event?.data,
      pendingModelListRequests,
      currentConfig.models,
      grokModels,
      grokModelConflicts,
    );
  }

  function openModelManager() {
    if (!modelPickerMount || typeof document === "undefined") return false;
    return Boolean(modelPickerMount.openManagerDialog({
      documentRef: document,
      injectedModels: currentConfig.models,
      modelPicker: {
        ...currentConfig.model_picker,
        codexPlusDetected: currentConfig.model_picker?.hostAdapter === "codexplusplus",
        modelConflicts: [...grokModelConflicts],
      },
      request(operation, payload) {
        return requestControl(operation, payload);
      },
    }));
  }

  function scheduleModelPickerReconcile() {
    if (!mounted || !modelPickerController || modelPickerFrame !== null) return;
    const run = () => {
      modelPickerController?.reconcile();
      modelPickerController?.reconcile();
      modelPickerFrame = null;
    };
    if (typeof window.requestAnimationFrame === "function") {
      modelPickerFrame = window.requestAnimationFrame(run);
    } else {
      run();
    }
  }

  function startModelPickerMount() {
    if (modelPickerController) return true;
    if (
      !modelPickerMount
      || typeof document === "undefined"
      || !document.body
    ) return false;
    modelPickerController = modelPickerMount.createController({
      documentRef: document,
      label: "Manage Grok models",
      onOpen: openModelManager,
    });
    if (typeof window.MutationObserver === "function") {
      modelPickerObserver = new window.MutationObserver(scheduleModelPickerReconcile);
      modelPickerObserver.observe(document.body, {
        attributeFilter: ["aria-controls", "aria-expanded", "aria-hidden", "data-state"],
        attributes: true,
        childList: true,
        subtree: true,
      });
    }
    scheduleModelPickerReconcile();
    return true;
  }

  function maintainModelPickerMount() {
    if (!mounted) return;
    if (!modelPickerController) startModelPickerMount();
    scheduleModelPickerReconcile();
  }

  function stopModelPickerMount() {
    modelPickerObserver?.disconnect?.();
    modelPickerObserver = null;
    if (modelPickerRetryTimer !== null) clearInterval(modelPickerRetryTimer);
    modelPickerRetryTimer = null;
    if (modelPickerFrame !== null && typeof window.cancelAnimationFrame === "function") {
      window.cancelAnimationFrame(modelPickerFrame);
    }
    modelPickerFrame = null;
    modelPickerController?.dispose?.();
    modelPickerController = null;
    if (typeof document !== "undefined") modelPickerMount?.closeManagerDialog?.(document);
  }

  function mount() {
    mounted = true;
    startModelPickerMount();
    if (modelPickerRetryTimer === null) {
      modelPickerRetryTimer = setInterval(maintainModelPickerMount, 500);
    }
    if (!messageListenerInstalled) {
      window.addEventListener("message", handleMessage, true);
      messageListenerInstalled = true;
    }
    if (installBridgePatch()) return true;
    requestRendererApiDiscovery();
    if (bridgeRetryTimer === null) {
      bridgeRetryAttempts = 0;
      bridgeRetryTimer = setInterval(retryBridgePatch, 100);
    }
    return false;
  }

  function dispose() {
    mounted = false;
    rejectPendingControlRequests("secure broker was disposed");
    const rendererAddons = window.__codexAdministratorRendererAddons;
    let rendererAddonsDisposed = true;
    try {
      rendererAddonsDisposed = rendererAddons?.disposeAll?.() !== false;
    } catch {
      rendererAddonsDisposed = false;
    }
    if (
      rendererAddonsDisposed
      && window.__codexAdministratorRendererAddons === rendererAddons
    ) {
      try {
        delete window.__codexAdministratorRendererAddons;
      } catch {
        // A foreign non-configurable value is left untouched.
      }
    }
    stopModelPickerMount();
    stopBridgeRetry();
    restoreBridgePatch();
    if (messageListenerInstalled) {
      window.removeEventListener("message", handleMessage, true);
      messageListenerInstalled = false;
    }
    pendingModelListRequests.clear();
    grokModelConflicts.clear();
    grokThreadIds.clear();
    grokThreadModels.clear();
    if (
      discoveredRendererApi
      && window.__codexAdministratorRendererApi === discoveredRendererApi
    ) {
      try {
        delete window.__codexAdministratorRendererApi;
      } catch {
        // The namespaced discovery handle is optional cleanup only.
      }
    }
    discoveredRendererApi = null;
    if (window.__codexAdministratorControlInternal === internalControlApi) {
      delete window.__codexAdministratorControlInternal;
    }
    return true;
  }

  function health() {
    let rendererAddons = { active: [], failed: [] };
    try {
      const observed = window.__codexAdministratorRendererAddons?.health?.();
      if (observed && Array.isArray(observed.active) && Array.isArray(observed.failed)) {
        rendererAddons = observed;
      }
    } catch {
      rendererAddons = { active: [], failed: [{ id: "registry", reason: "health_failed" }] };
    }
    return {
      ok: Boolean(
        messageListenerInstalled
        && activeTransport
        && activeTransportMethod
        && activeTransport[activeTransportMethod] === patchedPostMessage
      ),
      provider: currentConfig.provider_id,
      models: currentConfig.models.map((model) => model.model),
      model_conflicts: [...grokModelConflicts],
      codex_plus_detected: currentConfig.model_picker?.hostAdapter === "codexplusplus",
      grok_threads: grokThreadIds.size,
      grok_thread_models: grokThreadModels.size,
      version: currentConfig.version,
      model_picker_mounted: modelPickerController?.health?.().mounted === true,
      renderer_addons: rendererAddons,
    };
  }

  function configure(nextConfig) {
    if (
      !nextConfig
      || nextConfig.provider_id !== "grok_native"
      || !Array.isArray(nextConfig.models)
      || !nextConfig.models.every((model) => model && typeof model.model === "string")
      || typeof nextConfig.model_picker?.controlNonce !== "string"
      || nextConfig.model_picker.controlNonce !== controlNonce()
    ) return false;
    let nextConfigSignature;
    try {
      nextConfigSignature = JSON.stringify(nextConfig);
    } catch {
      return false;
    }
    return nextConfigSignature === currentConfigSignature;
  }

  window.__codexAdministrator = Object.freeze({ configure, dispose, health, mount });
  mount();
})();
