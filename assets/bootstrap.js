(() => {
  "use strict";

  const config = /*__CODEX_ADMINISTRATOR_CONFIG__*/;
  const rootId = "codex-administrator-root";
  const styleId = "codex-administrator-style";
  const modes = Object.freeze({
    grok: "grok_injected_main",
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
  let messageListenerInstalled = false;

  function installStyle() {
    if (document.getElementById(styleId)) return;
    const style = document.createElement("style");
    style.id = styleId;
    style.textContent = `
      #${rootId} { position: fixed; inset: 0; z-index: 2147483000; pointer-events: none; font-family: ui-sans-serif, "Segoe UI", sans-serif; }
      #${rootId} * { box-sizing: border-box; }
      #${rootId} .ca-switch { position: absolute; z-index: 2; top: 10px; right: 76px; display: grid; grid-template-columns: 1fr 1fr; width: 112px; height: 32px; padding: 3px; border: 1px solid color-mix(in srgb, CanvasText 18%, transparent); border-radius: 7px; background: color-mix(in srgb, Canvas 92%, transparent); box-shadow: 0 2px 10px rgba(0,0,0,.12); pointer-events: auto; color: CanvasText; }
      #${rootId} .ca-switch button { min-width: 0; border: 0; border-radius: 5px; background: transparent; color: inherit; font: 600 12px/1 ui-sans-serif, "Segoe UI", sans-serif; cursor: pointer; }
      #${rootId} .ca-switch button[aria-pressed="true"] { background: color-mix(in srgb, CanvasText 12%, Canvas); }
      #${rootId} .ca-workspace { position: absolute; inset: 0; background: Canvas; pointer-events: auto; }
      #${rootId} .ca-workspace[hidden] { display: none; }
      #${rootId} iframe { display: block; width: 100%; height: 100%; border: 0; background: Canvas; }
      @media (max-width: 720px) { #${rootId} .ca-switch { right: 12px; } }
    `;
    document.head.appendChild(style);
  }

  function buildRoot() {
    root = document.createElement("div");
    root.id = rootId;
    root.innerHTML = `
      <div class="ca-switch" role="group" aria-label="Main agent">
        <button type="button" data-mode="${modes.grok}" aria-label="Use Grok main agent">Grok</button>
        <button type="button" data-mode="${modes.gpt}" aria-label="Use native GPT main agent">GPT</button>
      </div>
      <main class="ca-workspace" data-workspace hidden></main>
    `;
    root.addEventListener("click", (event) => {
      const button = event.target.closest("button[data-mode]");
      if (button) void setMode(button.dataset.mode, true);
    });
    document.body.appendChild(root);
  }

  function updateView() {
    if (!root) return;
    for (const button of root.querySelectorAll("button[data-mode]")) {
      button.setAttribute("aria-pressed", String(button.dataset.mode === currentMode));
    }
    const workspace = root.querySelector("[data-workspace]");
    const grokActive = currentMode === modes.grok;
    workspace.hidden = !grokActive;
    if (!frame) {
      frame = document.createElement("iframe");
      frame.title = "Grok main agent workspace";
      frame.referrerPolicy = "no-referrer";
      frame.src = `${currentConfig.base_url}/ui/#capability=${encodeURIComponent(currentConfig.capability)}`;
      frame.addEventListener("load", () => {
        frameLoaded = true;
        sendToFrame({ type: "codex-administrator:state-request" });
      });
      workspace.appendChild(frame);
    }
  }

  function sendToFrame(message) {
    if (!frameLoaded || !frame?.contentWindow) return false;
    frame.contentWindow.postMessage(message, currentConfig.base_url);
    return true;
  }

  function handleMessage(event) {
    if (!frame?.contentWindow || event.source !== frame.contentWindow) return;
    if (event.origin !== currentConfig.base_url) return;
    if (event.data?.type !== "codex-administrator:state") return;
    if (event.data.state?.mode) void setMode(event.data.state.mode, false);
  }

  async function setMode(mode, persist = false) {
    if (mode !== modes.grok && mode !== modes.gpt) throw new Error(`Unsupported mode: ${mode}`);
    currentMode = mode;
    updateView();
    if (persist) {
      sendToFrame({ type: "codex-administrator:set-mode", mode });
    }
    return currentMode;
  }

  function hydrate() {
    sendToFrame({ type: "codex-administrator:state-request" });
  }

  function mount() {
    if (!document.body) {
      document.addEventListener("DOMContentLoaded", mount, { once: true });
      return false;
    }
    installStyle();
    if (!messageListenerInstalled) {
      window.addEventListener("message", handleMessage);
      messageListenerInstalled = true;
    }
    root = document.getElementById(rootId);
    if (!root) buildRoot();
    updateView();
    hydrate();
    return true;
  }

  function dispose() {
    if (messageListenerInstalled) {
      window.removeEventListener("message", handleMessage);
      messageListenerInstalled = false;
    }
    frameLoaded = false;
    frame = null;
    root?.remove();
    document.getElementById(styleId)?.remove();
    root = null;
    return true;
  }

  function health() {
    return {
      ok: Boolean(root && document.documentElement.contains(root)),
      mode: currentMode,
      version: currentConfig.version,
    };
  }

  function configure(nextConfig) {
    if (!nextConfig || !nextConfig.base_url || !nextConfig.capability) return false;
    currentConfig = nextConfig;
    if (frame) {
      frame.remove();
      frame = null;
      frameLoaded = false;
    }
    updateView();
    return true;
  }

  window.__codexAdministrator = Object.freeze({ configure, dispose, health, mount, setMode });
  mount();
})();
