(() => {
  "use strict";

  function hasModel(models, model) {
    return typeof model === "string"
      && (typeof models?.has === "function" ? models.has(model) : Array.isArray(models) && models.includes(model));
  }

  function cloneModel(model) {
    return {
      ...model,
      supportedReasoningEfforts: Array.isArray(model.supportedReasoningEfforts)
        ? model.supportedReasoningEfforts.map((effort) => ({ ...effort }))
        : [],
      inputModalities: Array.isArray(model.inputModalities) ? [...model.inputModalities] : ["text"],
      additionalSpeedTiers: Array.isArray(model.additionalSpeedTiers)
        ? [...model.additionalSpeedTiers]
        : [],
      serviceTiers: Array.isArray(model.serviceTiers)
        ? model.serviceTiers.map((tier) => ({ ...tier }))
        : [],
    };
  }

  function appendModels(response, injectedModels) {
    if (!response || !Array.isArray(response.data) || !Array.isArray(injectedModels)) return false;
    if (!response.data.every((entry) => entry && typeof entry === "object" && typeof entry.model === "string")) {
      return false;
    }

    const existing = new Set(response.data.map((entry) => entry.model));
    let changed = false;
    for (const model of injectedModels) {
      if (!model || typeof model.model !== "string" || existing.has(model.model)) continue;
      response.data.push(cloneModel(model));
      existing.add(model.model);
      changed = true;
    }
    return changed;
  }

  function routeParams(message, read, write, method, grokModels, providerId, grokThreadIds) {
    const params = read(message);
    if (!params) return message;
    const selectedGrok = hasModel(grokModels, params.model);
    const resumingKnownGrok = method === "thread/resume"
      && typeof params.threadId === "string"
      && typeof grokThreadIds?.has === "function"
      && grokThreadIds.has(params.threadId);
    if (!selectedGrok && !resumingKnownGrok) return message;
    return write(message, { ...params, modelProvider: providerId });
  }

  function routeProvider(message, grokModels, providerId, grokThreadIds) {
    if (!message || typeof message !== "object" || typeof providerId !== "string") return message;

    if (message.type === "send-cli-request-for-host") {
      if (message.method !== "thread/start" && message.method !== "thread/resume") return message;
      return routeParams(
        message,
        (value) => value.params,
        (value, params) => ({ ...value, params }),
        message.method,
        grokModels,
        providerId,
        grokThreadIds,
      );
    }

    if (message.type === "mcp-request" || message.type === "worker-request") {
      const method = message.request?.method;
      if (method !== "thread/start" && method !== "thread/resume") return message;
      return routeParams(
        message,
        (value) => value.request?.params,
        (value, params) => ({ ...value, request: { ...value.request, params } }),
        method,
        grokModels,
        providerId,
        grokThreadIds,
      );
    }

    if (message.type === "thread-prewarm-start") {
      return routeParams(
        message,
        (value) => value.request?.params,
        (value, params) => ({ ...value, request: { ...value.request, params } }),
        "thread/start",
        grokModels,
        providerId,
        grokThreadIds,
      );
    }

    if (message.type === "prewarm-thread-start-for-host") {
      return routeParams(
        message,
        (value) => value.params,
        (value, params) => ({ ...value, params }),
        "thread/start",
        grokModels,
        providerId,
        grokThreadIds,
      );
    }

    if (message.type === "start-conversation" || message.type === "start-thread-for-host") {
      return routeParams(
        message,
        (value) => value,
        (_value, params) => params,
        "thread/start",
        grokModels,
        providerId,
        grokThreadIds,
      );
    }

    return message;
  }

  function trackModelListRequest(message, pendingRequestIds) {
    if (typeof pendingRequestIds?.add !== "function") return false;
    const request = message?.type === "mcp-request" ? message.request : null;
    if (request?.method !== "model/list" || request.id == null) return false;
    pendingRequestIds.add(String(request.id));
    return true;
  }

  function patchModelListMessage(data, pendingRequestIds, injectedModels) {
    if (
      data?.type !== "mcp-response"
      || typeof pendingRequestIds?.has !== "function"
      || typeof pendingRequestIds?.delete !== "function"
    ) return false;
    const message = data.message || data.response;
    const requestId = message?.id == null ? "" : String(message.id);
    if (!pendingRequestIds.has(requestId)) return false;
    pendingRequestIds.delete(requestId);
    return appendModels(message?.result, injectedModels);
  }

  function learnGrokThreads(data, grokThreadIds, providerId) {
    if (typeof grokThreadIds?.add !== "function" || typeof providerId !== "string") return false;
    const before = typeof grokThreadIds.size === "number" ? grokThreadIds.size : null;
    const message = data?.message || data?.response || data;
    const roots = [message?.result, message?.params, message];

    function learn(value) {
      if (!value || typeof value !== "object") return;
      const provider = value.modelProvider ?? value.model_provider;
      const id = value.id ?? value.threadId ?? value.thread_id;
      if (provider === providerId && typeof id === "string" && id) grokThreadIds.add(id);
      if (provider === providerId && value.thread && typeof value.thread.id === "string") {
        grokThreadIds.add(value.thread.id);
      }
      if (value.thread && typeof value.thread === "object") learn(value.thread);
      if (Array.isArray(value.data)) value.data.forEach(learn);
    }

    roots.forEach(learn);
    return before == null ? true : grokThreadIds.size !== before;
  }

  globalThis.__codexAdministratorModelInjectionCore = Object.freeze({
    appendModels,
    learnGrokThreads,
    patchModelListMessage,
    routeProvider,
    trackModelListRequest,
  });
})();
