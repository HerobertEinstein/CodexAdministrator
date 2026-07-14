(() => {
  const rendererApi = window.__codexAdministratorRendererApi;
  if (typeof rendererApi?.postMessage !== "function") {
    return Promise.resolve({ ok: false, error: "renderer API unavailable" });
  }

  const requestId = `codex-administrator-provider-ready-${Date.now()}-${Math.random()}`;
  return new Promise((resolve) => {
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      window.removeEventListener("message", onMessage, true);
      resolve(value);
    };
    const onMessage = (event) => {
      const envelope = event?.data;
      const message = envelope?.message;
      if (
        envelope?.type !== "mcp-response" ||
        envelope?.hostId !== "local" ||
        message?.id !== requestId
      ) {
        return;
      }
      if (message.error) {
        finish({
          ok: false,
          error: message.error.message || "config/read failed",
        });
        return;
      }
      const config = message.result?.config;
      const provider =
        config?.model_providers?.grok_native ??
        config?.modelProviders?.grok_native ??
        null;
      finish(
        provider
          ? { ok: true, provider: "grok_native" }
          : { ok: false, error: "model provider 'grok_native' not found" },
      );
    };
    const timer = setTimeout(
      () => finish({ ok: false, error: "config/read timed out" }),
      3000,
    );

    window.addEventListener("message", onMessage, true);
    Promise.resolve(
      rendererApi.postMessage({
        type: "mcp-request",
        hostId: "local",
        request: {
          id: requestId,
          method: "config/read",
          params: { includeLayers: false, cwd: null },
        },
      }),
    ).catch((error) => {
      finish({
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      });
    });
  });
})()
