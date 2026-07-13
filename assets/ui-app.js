(() => {
  "use strict";

  const fragment = new URLSearchParams(window.location.hash.slice(1));
  const capability = fragment.get("capability") || "";
  window.history.replaceState(null, "", "/ui/");

  async function request(path, init = {}) {
    if (!capability) throw new Error("Missing launch capability");
    const headers = new Headers(init.headers || {});
    headers.set("Accept", "application/json");
    headers.set("Authorization", `Bearer ${capability}`);
    if (init.body) headers.set("Content-Type", "application/json");
    const response = await fetch(path, {
      ...init,
      headers,
      credentials: "same-origin",
      cache: "no-store",
    });
    if (!response.ok) throw new Error(`Companion request failed: ${response.status}`);
    return response.json();
  }

  function reply(event, state) {
    const targetOrigin = event.origin === "null" ? "*" : event.origin;
    event.source?.postMessage({ type: "codex-administrator:state", state }, targetOrigin);
  }

  window.addEventListener("message", async (event) => {
    if (event.source !== window.parent) return;
    const type = event.data?.type;
    if (type !== "codex-administrator:state-request" && type !== "codex-administrator:set-mode") return;
    try {
      const state = type === "codex-administrator:set-mode"
        ? await request("/api/state/mode", {
            method: "PUT",
            body: JSON.stringify({ mode: event.data.mode }),
          })
        : await request("/api/state");
      reply(event, state);
    } catch {
      reply(event, { mode: "native_gpt_main", unavailable: true });
    }
  });

  document.querySelector("[data-composer]")?.addEventListener("submit", (event) => {
    event.preventDefault();
  });
})();
