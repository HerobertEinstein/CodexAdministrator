(() => {
  "use strict";

  const config = /*__CODEX_ADMINISTRATOR_CONFIG__*/;
  const core = globalThis.__codexAdministratorModelInjectionCore;
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
  let activeBridge = null;
  let originalSendMessageFromView = null;
  let patchedSendMessageFromView = null;
  let messageListenerInstalled = false;
  let bridgeRetryTimer = null;
  let bridgeRetryAttempts = 0;

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
    if (bridgeRetryAttempts >= 300) stopBridgeRetry();
  }

  function installBridgePatch() {
    const bridge = window.electronBridge;
    if (!bridge || typeof bridge.sendMessageFromView !== "function") return false;
    if (activeBridge === bridge && bridge.sendMessageFromView === patchedSendMessageFromView) {
      return true;
    }
    restoreBridgePatch();
    activeBridge = bridge;
    originalSendMessageFromView = bridge.sendMessageFromView;
    patchedSendMessageFromView = function codexAdministratorSendMessageFromView(message) {
      core.trackModelListRequest(message, pendingModelListRequests);
      const routed = core.routeProvider(
        message,
        grokModels,
        currentConfig.provider_id,
        grokThreadIds,
      );
      return originalSendMessageFromView.call(bridge, routed);
    };
    bridge.sendMessageFromView = patchedSendMessageFromView;
    const installed = bridge.sendMessageFromView === patchedSendMessageFromView;
    if (installed) stopBridgeRetry();
    return installed;
  }

  function restoreBridgePatch() {
    if (
      activeBridge
      && originalSendMessageFromView
      && activeBridge.sendMessageFromView === patchedSendMessageFromView
    ) {
      activeBridge.sendMessageFromView = originalSendMessageFromView;
    }
    activeBridge = null;
    originalSendMessageFromView = null;
    patchedSendMessageFromView = null;
  }

  function handleMessage(event) {
    core.learnGrokThreads(event?.data, grokThreadIds, currentConfig.provider_id);
    core.patchModelListMessage(event?.data, pendingModelListRequests, currentConfig.models);
  }

  function mount() {
    if (!messageListenerInstalled) {
      window.addEventListener("message", handleMessage, true);
      messageListenerInstalled = true;
    }
    if (installBridgePatch()) return true;
    if (bridgeRetryTimer === null) {
      bridgeRetryAttempts = 0;
      bridgeRetryTimer = setInterval(retryBridgePatch, 100);
    }
    return false;
  }

  function dispose() {
    stopBridgeRetry();
    restoreBridgePatch();
    if (messageListenerInstalled) {
      window.removeEventListener("message", handleMessage, true);
      messageListenerInstalled = false;
    }
    pendingModelListRequests.clear();
    grokThreadIds.clear();
    return true;
  }

  function health() {
    return {
      ok: Boolean(
        messageListenerInstalled
        && activeBridge
        && activeBridge.sendMessageFromView === patchedSendMessageFromView
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
