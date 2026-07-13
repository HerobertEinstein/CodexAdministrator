(() => {
  "use strict";

  const config = /*__CODEX_ADMINISTRATOR_CONFIG__*/;
  const core = globalThis.__codexAdministratorModelInjectionCore;
  const rendererApiDiscovery = globalThis.__codexAdministratorRendererApiDiscovery;
  if (!core) return;

  const prior = window.__codexAdministrator;
  if (prior && typeof prior.configure === "function") {
    prior.configure(config);
    prior.mount();
    return;
  }

  let currentConfig = config;
  let grokModels = new Set(config.models.map((model) => model.model));
  const grokThreadIds = new Set();
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
    if (bridgeRetryAttempts >= 300) stopBridgeRetry();
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
    core.learnGrokThreads(event?.data, grokThreadIds, currentConfig.provider_id);
    core.patchModelListMessage(event?.data, pendingModelListRequests, currentConfig.models);
  }

  function mount() {
    mounted = true;
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
    stopBridgeRetry();
    restoreBridgePatch();
    if (messageListenerInstalled) {
      window.removeEventListener("message", handleMessage, true);
      messageListenerInstalled = false;
    }
    pendingModelListRequests.clear();
    grokThreadIds.clear();
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
    return true;
  }

  function health() {
    return {
      ok: Boolean(
        messageListenerInstalled
        && activeTransport
        && activeTransportMethod
        && activeTransport[activeTransportMethod] === patchedPostMessage
      ),
      provider: currentConfig.provider_id,
      models: currentConfig.models.map((model) => model.model),
      grok_threads: grokThreadIds.size,
      version: currentConfig.version,
    };
  }

  function configure(nextConfig) {
    if (
      !nextConfig
      || nextConfig.provider_id !== "grok_native"
      || !Array.isArray(nextConfig.models)
      || nextConfig.models.length === 0
      || !nextConfig.models.every((model) => model && typeof model.model === "string")
    ) return false;
    currentConfig = nextConfig;
    grokModels = new Set(nextConfig.models.map((model) => model.model));
    pendingModelListRequests.clear();
    return true;
  }

  window.__codexAdministrator = Object.freeze({ configure, dispose, health, mount });
  mount();
})();
