(() => {
  "use strict";

  const config = /*__CODEX_ADMINISTRATOR_CONFIG__*/;
  const rootId = "codex-administrator-root";
  const modes = Object.freeze({
    grok: "grok_native_model",
    gpt: "native_gpt_main",
  });

  const prior = window.__codexAdministrator;
  if (prior && typeof prior.configure === "function") {
    prior.configure(config);
    prior.mount();
    return;
  }

  let currentConfig = config;
  let currentMode = modes.gpt;
  let root = null;
  let frame = null;
  let frameLoaded = false;
  let bridgeAvailable = false;
  let messageListenerInstalled = false;

  function createBridge() {
    frame = document.createElement("iframe");
    frame.title = "Provider authentication bridge";
    frame.hidden = true;
    frame.tabIndex = -1;
    frame.referrerPolicy = "no-referrer";
    frame.src = `${currentConfig.base_url}/ui/#capability=${encodeURIComponent(currentConfig.capability)}`;
    frame.addEventListener("load", () => {
      frameLoaded = true;
      sendToFrame({ type: "codex-administrator:state-request" });
    });
    root.appendChild(frame);
  }

  function buildRoot() {
    root = document.createElement("div");
    root.id = rootId;
    root.hidden = true;
    root.setAttribute("aria-hidden", "true");
    document.body.appendChild(root);
    createBridge();
  }

  function sendToFrame(message) {
    if (!frameLoaded || !frame?.contentWindow) return false;
    frame.contentWindow.postMessage(message, currentConfig.base_url);
    return true;
  }

  function applyState(state) {
    if (state?.unavailable || (state?.mode !== modes.grok && state?.mode !== modes.gpt)) {
      bridgeAvailable = false;
      currentMode = modes.gpt;
    } else {
      bridgeAvailable = true;
      currentMode = state.mode;
    }
  }

  function handleMessage(event) {
    if (!frame?.contentWindow || event.source !== frame.contentWindow) return;
    if (event.origin !== currentConfig.base_url) return;
    if (event.data?.type !== "codex-administrator:state") return;
    applyState(event.data.state);
  }

  function hydrate() {
    if (!sendToFrame({ type: "codex-administrator:state-request" })) {
      applyState({ mode: modes.gpt, unavailable: true });
    }
  }

  function mount() {
    if (!document.body) {
      document.addEventListener("DOMContentLoaded", mount, { once: true });
      return false;
    }
    if (!messageListenerInstalled) {
      window.addEventListener("message", handleMessage);
      messageListenerInstalled = true;
    }
    root = document.getElementById(rootId);
    if (!root) buildRoot();
    if (frameLoaded) hydrate();
    return true;
  }

  function dispose() {
    if (messageListenerInstalled) {
      window.removeEventListener("message", handleMessage);
      messageListenerInstalled = false;
    }
    frameLoaded = false;
    bridgeAvailable = false;
    frame = null;
    root?.remove();
    root = null;
    return true;
  }

  function health() {
    return {
      ok: Boolean(root && document.documentElement.contains(root)),
      bridge_available: bridgeAvailable,
      mode: currentMode,
      version: currentConfig.version,
    };
  }

  function configure(nextConfig) {
    if (!nextConfig || !nextConfig.base_url || !nextConfig.capability) return false;
    currentConfig = nextConfig;
    currentMode = modes.gpt;
    bridgeAvailable = false;
    frameLoaded = false;
    frame?.remove();
    frame = null;
    if (root) createBridge();
    return true;
  }

  window.__codexAdministrator = Object.freeze({ configure, dispose, health, mount });
  mount();
})();
