(() => {
  "use strict";

  const catalogOverlayMarker = "codex-administrator:grok-native-catalog-v1";

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

  function appendModels(response, injectedModels, routableModels, conflictingModels) {
    if (!response || !Array.isArray(response.data) || !Array.isArray(injectedModels)) return false;
    if (!response.data.every((entry) => entry && typeof entry === "object" && typeof entry.model === "string")) {
      return false;
    }

    const existing = new Map(response.data.map((entry, index) => [entry.model, { entry, index }]));
    let changed = false;
    for (const model of injectedModels) {
      if (!model || typeof model.model !== "string") continue;
      const match = existing.get(model.model);
      if (match) {
        const markedCatalogOverlay = match.entry.hidden === true
          && match.entry.availabilityNux?.message === catalogOverlayMarker;
        if (markedCatalogOverlay) {
          response.data[match.index] = cloneModel(model);
          routableModels?.add?.(model.model);
          conflictingModels?.delete?.(model.model);
          changed = true;
          continue;
        }
        routableModels?.delete?.(model.model);
        conflictingModels?.add?.(model.model);
        continue;
      }
      response.data.push(cloneModel(model));
      existing.set(model.model, { entry: model, index: response.data.length - 1 });
      routableModels?.add?.(model.model);
      conflictingModels?.delete?.(model.model);
      changed = true;
    }
    return changed;
  }

  function firstModel(models) {
    if (typeof models?.values === "function") return models.values().next().value;
    return Array.isArray(models) ? models[0] : undefined;
  }

  function routeParams(
    message,
    read,
    write,
    method,
    grokModels,
    providerId,
    grokThreadIds,
    grokThreadModels,
  ) {
    const params = read(message);
    if (!params) return message;
    const selectedGrok = hasModel(grokModels, params.model);
    const resumingKnownGrok = method === "thread/resume"
      && typeof params.threadId === "string"
      && typeof grokThreadIds?.has === "function"
      && grokThreadIds.has(params.threadId);
    if (!selectedGrok && !resumingKnownGrok) return message;
    const rememberedModel = resumingKnownGrok
      && typeof grokThreadModels?.get === "function"
      ? grokThreadModels.get(params.threadId)
      : undefined;
    const resumeModel = hasModel(grokModels, rememberedModel)
      ? rememberedModel
      : firstModel(grokModels);
    if (!selectedGrok && (!resumingKnownGrok || typeof resumeModel !== "string")) return message;
    return write(message, {
      ...params,
      ...(resumingKnownGrok && !selectedGrok && typeof resumeModel === "string"
        ? { model: resumeModel }
        : {}),
      modelProvider: providerId,
    });
  }

  function routeProvider(message, grokModels, providerId, grokThreadIds, grokThreadModels) {
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
        grokThreadModels,
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
        grokThreadModels,
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
        grokThreadModels,
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
        grokThreadModels,
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
        grokThreadModels,
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

  function patchModelListMessage(
    data,
    pendingRequestIds,
    injectedModels,
    routableModels,
    conflictingModels,
  ) {
    if (
      data?.type !== "mcp-response"
      || typeof pendingRequestIds?.has !== "function"
      || typeof pendingRequestIds?.delete !== "function"
    ) return false;
    const message = data.message || data.response;
    const requestId = message?.id == null ? "" : String(message.id);
    if (!pendingRequestIds.has(requestId)) return false;
    pendingRequestIds.delete(requestId);
    return appendModels(
      message?.result,
      injectedModels,
      routableModels,
      conflictingModels,
    );
  }

  function learnGrokThreads(data, grokThreadIds, providerId, grokThreadModels, grokModels) {
    if (typeof grokThreadIds?.add !== "function" || typeof providerId !== "string") return false;
    const before = typeof grokThreadIds.size === "number" ? grokThreadIds.size : null;
    const message = data?.message || data?.response || data;
    const roots = [message?.result, message?.params, message];

    function learn(value) {
      if (!value || typeof value !== "object") return;
      const provider = value.modelProvider ?? value.model_provider;
      const id = value.id ?? value.threadId ?? value.thread_id;
      const model = value.model;
      if (provider === providerId && typeof id === "string" && id) {
        grokThreadIds.add(id);
        if (hasModel(grokModels, model) && typeof grokThreadModels?.set === "function") {
          grokThreadModels.set(id, model);
        }
      }
      if (provider === providerId && value.thread && typeof value.thread.id === "string") {
        grokThreadIds.add(value.thread.id);
        if (
          hasModel(grokModels, value.thread.model)
          && typeof grokThreadModels?.set === "function"
        ) {
          grokThreadModels.set(value.thread.id, value.thread.model);
        }
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
