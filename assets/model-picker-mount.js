(() => {
  "use strict";

  const managerAttribute = "data-codex-administrator-model-manager";
  const separatorAttribute = "data-codex-administrator-model-manager-separator";
  const dialogAttribute = "data-codex-administrator-model-manager-dialog";
  const mountedEntries = new WeakMap();

  function readRendererAddonHealth(modelPicker) {
    if (modelPicker?.rendererAddonHealth) return modelPicker.rendererAddonHealth;
    try {
      return globalThis.__codexAdministratorRendererAddons?.health?.() || null;
    } catch {
      return null;
    }
  }

  function rendererAddonStatusText(addon) {
    if (addon?.state === "enabled") {
      if (addon.runtimeState === "active") {
        return `Reviewed build active${addon.revision ? ` (${addon.revision.slice(0, 8)})` : ""}`;
      }
      if (addon.runtimeState === "failed") {
        return `Runtime failed: ${addon.runtimeReason || "unknown_failure"}`;
      }
      if (addon.runtimeState === "pending") return "Reviewed build enabled; waiting for renderer readiness";
      if (addon.runtimeState === "unhealthy") return "Runtime unhealthy; restart or disable this addon";
      return `Reviewed build enabled${addon.revision ? ` (${addon.revision.slice(0, 8)})` : ""}`;
    }
    return addon?.reason
      ? `Disabled: ${addon.reason}${addon.blockedBy ? ` (blocked by ${addon.blockedBy})` : ""}`
      : "Optional; third-party install, start, and restore scripts are never executed";
  }

  function normalizeManagerState(modelPicker, injectedModels) {
    const seen = new Set();
    const models = [];
    for (const model of Array.isArray(injectedModels) ? injectedModels : []) {
      const id = typeof model?.model === "string" ? model.model : model?.id;
      if (typeof id !== "string" || !id.toLowerCase().startsWith("grok-") || seen.has(id)) {
        continue;
      }
      seen.add(id);
      models.push({
        id,
        displayName: typeof model?.displayName === "string" && model.displayName
          ? model.displayName
          : id,
      });
    }
    const configuredRendererAddons = Array.isArray(modelPicker?.rendererAddons)
      ? modelPicker.rendererAddons
      : [];
    const rendererAddonCatalog = Array.isArray(modelPicker?.rendererAddonCatalog)
      ? modelPicker.rendererAddonCatalog
      : [];
    const rendererAddonReports = Array.isArray(modelPicker?.rendererAddonReports)
      ? modelPicker.rendererAddonReports
      : [];
    const settingsById = new Map(configuredRendererAddons
      .filter((addon) => typeof addon?.id === "string")
      .map((addon) => [addon.id, addon]));
    const reportsById = new Map(rendererAddonReports
      .filter((report) => typeof report?.id === "string")
      .map((report) => [report.id, report]));
    const rendererAddonHealth = readRendererAddonHealth(modelPicker);
    const runtimeActiveById = new Map((Array.isArray(rendererAddonHealth?.active)
      ? rendererAddonHealth.active
      : []).filter((entry) => typeof entry?.id === "string").map((entry) => [entry.id, entry]));
    const runtimeFailedById = new Map((Array.isArray(rendererAddonHealth?.failed)
      ? rendererAddonHealth.failed
      : []).filter((entry) => typeof entry?.id === "string").map((entry) => [entry.id, entry]));
    const runtimePendingById = new Map((Array.isArray(rendererAddonHealth?.pending)
      ? rendererAddonHealth.pending
      : []).filter((entry) => typeof entry?.id === "string").map((entry) => [entry.id, entry]));
    const catalogById = new Map();
    for (const entry of rendererAddonCatalog) {
      if (typeof entry?.id !== "string" || !/^[a-z0-9-]{1,64}$/.test(entry.id)) continue;
      catalogById.set(entry.id, entry);
    }
    for (const setting of configuredRendererAddons) {
      if (
        typeof setting?.id === "string"
        && /^[a-z0-9-]{1,64}$/.test(setting.id)
        && !catalogById.has(setting.id)
      ) {
        catalogById.set(setting.id, { id: setting.id });
      }
    }
    const normalizedRendererAddons = [...catalogById.values()]
      .sort((left, right) => left.id.localeCompare(right.id))
      .map((entry) => {
        const setting = settingsById.get(entry.id);
        const report = reportsById.get(entry.id);
        const revision = typeof report?.project_revision === "string"
          ? report.project_revision
          : typeof report?.projectRevision === "string"
            ? report.projectRevision
            : typeof entry?.project_revision === "string"
              ? entry.project_revision
              : typeof entry?.projectRevision === "string"
                ? entry.projectRevision
                : "";
        const runtimeActive = runtimeActiveById.get(entry.id);
        const runtimeFailure = runtimeFailedById.get(entry.id);
        const runtimePending = runtimePendingById.get(entry.id);
        const revisionMismatch = [runtimeActive, runtimeFailure, runtimePending]
          .some((runtime) => runtime && runtime.revision !== revision);
        const runtimeState = revisionMismatch
          ? "unhealthy"
          : runtimeFailure
            ? "failed"
            : runtimePending
              ? "pending"
              : runtimeActive
                ? runtimeActive.ok === true ? "active" : "unhealthy"
                : "unknown";
        return {
          blockedBy: typeof report?.blocked_by === "string"
            ? report.blocked_by
            : typeof report?.blockedBy === "string"
              ? report.blockedBy
              : "",
          displayName: typeof entry?.display_name === "string" && entry.display_name
            ? entry.display_name
            : typeof entry?.displayName === "string" && entry.displayName
              ? entry.displayName
              : entry.id,
          enabled: setting?.enabled === true,
          id: entry.id,
          reason: typeof report?.reason === "string" ? report.reason : "",
          revision,
          runtimeReason: revisionMismatch
            ? "revision_mismatch"
            : typeof runtimeFailure?.reason === "string" ? runtimeFailure.reason : "",
          runtimeState,
          sourceRoot: typeof setting?.source_root === "string"
            ? setting.source_root
            : typeof setting?.sourceRoot === "string"
              ? setting.sourceRoot
              : "",
          state: typeof report?.state === "string" ? report.state : "disabled",
        };
      });
    const modelConflicts = Array.isArray(modelPicker?.modelConflicts)
      ? [...new Set(modelPicker.modelConflicts.filter((model) => typeof model === "string"))]
      : [];
    const modelIds = new Set(models.map((model) => model.id));
    const selectedModels = Array.isArray(modelPicker?.selectedModels)
      ? [...new Set(modelPicker.selectedModels.filter((model) => modelIds.has(model)))]
      : models.map((model) => model.id);
    return {
      actionPath: typeof modelPicker?.actionPath === "string" ? modelPicker.actionPath : "/responses",
      actionPathAuto: modelPicker?.actionPathAuto !== false,
      baseUrl: typeof modelPicker?.baseUrl === "string" ? modelPicker.baseUrl : "",
      credentialPresent: modelPicker?.credentialPresent === true,
      codexPlusDetected: modelPicker?.codexPlusDetected === true,
      modelConflicts,
      models,
      rendererAddons: normalizedRendererAddons,
      selectedModels,
      syncNativeAuth: modelPicker?.syncNativeAuth !== false,
      syncNativeSessions: modelPicker?.syncNativeSessions === true,
      syncNativeGoals: modelPicker?.syncNativeGoals === true,
      syncNativeSkills: modelPicker?.syncNativeSkills !== false,
    };
  }

  function buildRendererAddonPayload(addons) {
    return (Array.isArray(addons) ? addons : [])
      .filter((addon) => addon?.enabled === true)
      .filter((addon) => typeof addon.id === "string" && /^[a-z0-9-]{1,64}$/.test(addon.id))
      .filter((addon) => typeof addon.sourceRoot === "string")
      .map((addon) => ({
        enabled: true,
        id: addon.id,
        source_root: addon.sourceRoot,
      }));
  }

  function setStyles(element, styles) {
    if (!element?.style) return;
    Object.assign(element.style, styles);
  }

  function createTextElement(documentRef, tagName, text, attributes = {}) {
    const element = documentRef.createElement(tagName);
    element.textContent = text;
    for (const [name, value] of Object.entries(attributes)) {
      element.setAttribute(name, value);
    }
    return element;
  }

  function closeManagerDialog(documentRef) {
    const dialog = documentRef?.querySelector?.(`[${dialogAttribute}]`);
    if (!dialog) return false;
    dialog.__codexAdministratorCleanup?.();
    dialog.remove?.();
    return true;
  }

  function openManagerDialog({ documentRef, injectedModels, modelPicker, request }) {
    if (!documentRef?.body || typeof documentRef.createElement !== "function") return null;
    const existing = documentRef.querySelector?.(`[${dialogAttribute}]`);
    if (existing) return existing;
    const state = normalizeManagerState(modelPicker, injectedModels);
    const view = documentRef.defaultView || globalThis;

    const overlay = documentRef.createElement("div");
    overlay.setAttribute(dialogAttribute, "true");
    overlay.setAttribute("role", "presentation");
    setStyles(overlay, {
      alignItems: "center",
      background: "color-mix(in srgb, var(--main-surface-background, #111) 40%, transparent)",
      display: "flex",
      inset: "0",
      justifyContent: "center",
      padding: "24px",
      position: "fixed",
      zIndex: "80",
    });

    const panel = documentRef.createElement("section");
    panel.setAttribute("aria-label", "Grok model settings");
    panel.setAttribute("aria-modal", "true");
    panel.setAttribute("role", "dialog");
    panel.tabIndex = -1;
    setStyles(panel, {
      background: "var(--dropdown-background, var(--main-surface-background, #202020))",
      border: "1px solid color-mix(in srgb, currentColor 16%, transparent)",
      borderRadius: "18px",
      boxShadow: "0 24px 80px rgba(0,0,0,0.36)",
      color: "var(--text-primary, inherit)",
      display: "flex",
      flexDirection: "column",
      gap: "16px",
      maxHeight: "min(760px, calc(100vh - 48px))",
      maxWidth: "620px",
      overflow: "auto",
      padding: "20px",
      width: "min(620px, calc(100vw - 48px))",
    });

    const header = documentRef.createElement("div");
    setStyles(header, { alignItems: "center", display: "flex", gap: "12px", justifyContent: "space-between" });
    const heading = createTextElement(documentRef, "h2", "Grok models");
    setStyles(heading, { fontSize: "18px", fontWeight: "600", lineHeight: "24px", margin: "0" });
    const closeButton = createTextElement(documentRef, "button", "Close", { type: "button" });
    setStyles(closeButton, {
      background: "transparent",
      border: "0",
      color: "var(--text-secondary, inherit)",
      cursor: "pointer",
      font: "inherit",
      padding: "6px 8px",
    });
    header.append(heading, closeButton);

    const status = createTextElement(
      documentRef,
      "p",
      state.credentialPresent ? "API key saved securely" : "API key required",
      { "aria-live": "polite" },
    );
    setStyles(status, { color: "var(--text-secondary, inherit)", fontSize: "13px", margin: "0" });
    const compatibilityWarning = createTextElement(
      documentRef,
      "p",
      state.modelConflicts.length > 0
        ? `Model ownership conflict: ${state.modelConflicts.join(", ")}. Disable Codex++ modelWhitelistUnlock for overlapping IDs; Codex Administrator will not hijack them.`
        : state.codexPlusDetected
          ? "Codex++ detected; model IDs remain single-owner and other Codex++ features stay untouched."
          : "",
    );
    setStyles(compatibilityWarning, {
      color: "var(--text-warning, #d97706)",
      display: compatibilityWarning.textContent ? "block" : "none",
      fontSize: "12px",
      margin: "0",
    });

    function field(labelText, input) {
      const wrapper = documentRef.createElement("label");
      setStyles(wrapper, { display: "flex", flexDirection: "column", gap: "6px" });
      const label = createTextElement(documentRef, "span", labelText);
      setStyles(label, { color: "var(--text-secondary, inherit)", fontSize: "13px", fontWeight: "500" });
      setStyles(input, {
        background: "var(--main-surface-secondary, rgba(127,127,127,0.10))",
        border: "1px solid var(--border-light, rgba(127,127,127,0.22))",
        borderRadius: "10px",
        color: "var(--text-primary, inherit)",
        font: "inherit",
        minHeight: "38px",
        outline: "none",
        padding: "8px 10px",
      });
      wrapper.append(label, input);
      return wrapper;
    }

    const baseUrlInput = documentRef.createElement("input");
    baseUrlInput.type = "url";
    baseUrlInput.value = state.baseUrl;
    baseUrlInput.autocomplete = "off";
    const apiKeyInput = documentRef.createElement("input");
    apiKeyInput.type = "password";
    apiKeyInput.autocomplete = "new-password";
    apiKeyInput.maxLength = 2048;
    apiKeyInput.placeholder = state.credentialPresent ? "Leave blank to keep the saved key" : "Enter API key";
    const actionPathInput = documentRef.createElement("input");
    actionPathInput.type = "text";
    actionPathInput.value = state.actionPath;
    actionPathInput.autocomplete = "off";

    const actionRow = documentRef.createElement("div");
    setStyles(actionRow, { alignItems: "end", display: "grid", gap: "10px", gridTemplateColumns: "1fr auto" });
    actionRow.append(field("Action Path", actionPathInput));
    const autoLabel = documentRef.createElement("label");
    setStyles(autoLabel, { alignItems: "center", cursor: "pointer", display: "flex", gap: "7px", minHeight: "38px" });
    const autoInput = documentRef.createElement("input");
    autoInput.type = "checkbox";
    autoInput.checked = state.actionPathAuto;
    autoLabel.append(autoInput, createTextElement(documentRef, "span", "Auto"));

    const refreshButton = createTextElement(documentRef, "button", "Refresh models", { type: "button" });
    setStyles(refreshButton, {
      alignSelf: "start",
      background: "var(--text-primary, #fff)",
      border: "0",
      borderRadius: "999px",
      color: "var(--main-surface-background, #111)",
      cursor: "pointer",
      font: "inherit",
      fontWeight: "600",
      padding: "9px 14px",
    });

    const searchInput = documentRef.createElement("input");
    searchInput.type = "search";
    searchInput.placeholder = "Search Grok models";
    searchInput.autocomplete = "off";
    const modelList = documentRef.createElement("div");
    modelList.setAttribute("aria-label", "Injected Grok models");
    modelList.setAttribute("role", "group");
    setStyles(modelList, {
      border: "1px solid var(--border-light, rgba(127,127,127,0.22))",
      borderRadius: "12px",
      display: "flex",
      flexDirection: "column",
      gap: "2px",
      maxHeight: "190px",
      overflow: "auto",
      padding: "6px",
    });

    function renderModels(query = "") {
      while (modelList.firstChild) modelList.removeChild(modelList.firstChild);
      const normalizedQuery = query.trim().toLowerCase();
      const visible = state.models.filter((model) => model.id.toLowerCase().includes(normalizedQuery));
      if (visible.length === 0) {
        const empty = createTextElement(documentRef, "p", "No Grok models found");
        setStyles(empty, { color: "var(--text-secondary, inherit)", fontSize: "13px", margin: "8px" });
        modelList.append(empty);
        return;
      }
      for (const model of visible) {
        const label = documentRef.createElement("label");
        label.setAttribute("data-codex-administrator-model-id", model.id);
        setStyles(label, {
          alignItems: "center",
          borderRadius: "8px",
          cursor: "pointer",
          display: "flex",
          gap: "9px",
          padding: "8px",
        });
        const checkbox = documentRef.createElement("input");
        checkbox.type = "checkbox";
        checkbox.checked = state.selectedModels.includes(model.id);
        checkbox.value = model.id;
        checkbox.addEventListener("change", () => {
          const selected = new Set(state.selectedModels);
          if (checkbox.checked) selected.add(model.id);
          else selected.delete(model.id);
          state.selectedModels = state.models
            .map((candidate) => candidate.id)
            .filter((id) => selected.has(id));
        });
        label.append(checkbox, createTextElement(documentRef, "span", model.displayName));
        modelList.append(label);
      }
    }
    renderModels();
    searchInput.addEventListener("input", () => renderModels(searchInput.value));

    function toggle(labelText, checked) {
      const label = documentRef.createElement("label");
      setStyles(label, { alignItems: "center", cursor: "pointer", display: "flex", gap: "8px" });
      const input = documentRef.createElement("input");
      input.type = "checkbox";
      input.checked = checked;
      label.append(input, createTextElement(documentRef, "span", labelText));
      return { input, label };
    }
    const syncAuth = toggle("Sync native login", state.syncNativeAuth);
    const syncSessions = toggle("Automatically sync native tasks on launch", state.syncNativeSessions);
    const syncGoals = toggle("Synchronize shared task Goal intent through official RPC", state.syncNativeGoals);
    const syncSkills = toggle("Automatically sync custom Skills on launch", state.syncNativeSkills);
    syncGoals.input.disabled = !syncSessions.input.checked;
    syncSessions.input.addEventListener("change", () => {
      syncGoals.input.disabled = !syncSessions.input.checked;
      if (!syncSessions.input.checked) syncGoals.input.checked = false;
    });
    const syncGroup = documentRef.createElement("div");
    setStyles(syncGroup, { display: "flex", flexDirection: "column", gap: "9px" });
    syncGroup.append(syncAuth.label, syncSessions.label, syncGoals.label, syncSkills.label);

    const addonHeading = createTextElement(documentRef, "h3", "Compatible injectors");
    setStyles(addonHeading, { fontSize: "14px", fontWeight: "600", margin: "2px 0 0" });
    const addonGroup = documentRef.createElement("div");
    setStyles(addonGroup, {
      border: "1px solid var(--border-light, rgba(127,127,127,0.22))",
      borderRadius: "12px",
      display: "flex",
      flexDirection: "column",
      gap: "9px",
      padding: "12px",
    });
    addonGroup.append(addonHeading);
    const addonControls = [];
    for (const addon of state.rendererAddons) {
      const control = toggle(addon.displayName, addon.enabled);
      const sourceRootInput = documentRef.createElement("input");
      sourceRootInput.type = "text";
      sourceRootInput.value = addon.sourceRoot;
      sourceRootInput.autocomplete = "off";
      sourceRootInput.placeholder = "Absolute path to the reviewed external checkout";
      sourceRootInput.disabled = !control.input.checked;
      control.input.addEventListener("change", () => {
        sourceRootInput.disabled = !control.input.checked;
      });
      const statusText = rendererAddonStatusText(addon);
      const addonStatus = createTextElement(documentRef, "p", statusText);
      setStyles(addonStatus, {
        color: "var(--text-secondary, inherit)",
        fontSize: "12px",
        margin: "0",
      });
      const addonCard = documentRef.createElement("div");
      addonCard.setAttribute("data-codex-administrator-renderer-addon", addon.id);
      setStyles(addonCard, {
        borderTop: "1px solid var(--border-light, rgba(127,127,127,0.16))",
        display: "flex",
        flexDirection: "column",
        gap: "8px",
        paddingTop: "10px",
      });
      addonCard.append(control.label, field("External checkout", sourceRootInput), addonStatus);
      addonGroup.append(addonCard);
      addonControls.push({ addon, control, sourceRootInput });
    }
    if (addonControls.length === 0) {
      const empty = createTextElement(documentRef, "p", "No reviewed renderer adapters are available for this host build.");
      setStyles(empty, { color: "var(--text-secondary, inherit)", fontSize: "12px", margin: "0" });
      addonGroup.append(empty);
    }

    const footer = documentRef.createElement("div");
    setStyles(footer, { alignItems: "center", display: "flex", gap: "10px", justifyContent: "flex-end" });
    const cancelButton = createTextElement(documentRef, "button", "Cancel", { type: "button" });
    const applyButton = createTextElement(documentRef, "button", "Apply and restart", { type: "button" });
    for (const button of [cancelButton, applyButton]) {
      setStyles(button, {
        background: "transparent",
        border: "1px solid var(--border-light, rgba(127,127,127,0.22))",
        borderRadius: "999px",
        color: "var(--text-primary, inherit)",
        cursor: "pointer",
        font: "inherit",
        padding: "9px 14px",
      });
    }
    setStyles(applyButton, {
      background: "var(--text-primary, #fff)",
      color: "var(--main-surface-background, #111)",
      fontWeight: "600",
    });
    footer.append(cancelButton, applyButton);

    async function callBroker(operation, payload) {
      if (typeof request !== "function") throw new Error("Secure broker is unavailable");
      return request(operation, payload);
    }

    refreshButton.addEventListener("click", async () => {
      refreshButton.disabled = true;
      status.textContent = "Refreshing Grok models...";
      const credential = apiKeyInput.value;
      apiKeyInput.value = "";
      try {
        const result = await callBroker("models.discover", {
          action_path: actionPathInput.value,
          action_path_auto: autoInput.checked,
          base_url: baseUrlInput.value,
          credential,
        });
        const next = normalizeManagerState(result?.model_picker || result, result?.models || []);
        state.actionPath = next.actionPath;
        state.actionPathAuto = next.actionPathAuto;
        state.baseUrl = next.baseUrl;
        state.credentialPresent = next.credentialPresent;
        state.models = next.models;
        state.selectedModels = next.selectedModels;
        actionPathInput.value = state.actionPath;
        autoInput.checked = state.actionPathAuto;
        baseUrlInput.value = state.baseUrl;
        renderModels(searchInput.value);
        status.textContent = `${state.models.length} Grok model(s) available`;
      } catch (error) {
        status.textContent = error instanceof Error ? error.message : "Model refresh failed";
      } finally {
        refreshButton.disabled = false;
      }
    });

    applyButton.addEventListener("click", async () => {
      applyButton.disabled = true;
      status.textContent = "Applying isolated instance settings...";
      const availableModels = new Set(state.models.map((model) => model.id));
      const selectedModels = state.selectedModels
        .filter((model) => availableModels.has(model))
        .filter((model) => model.toLowerCase().startsWith("grok-"));
      try {
        const result = await callBroker("config.apply", {
          action_path: actionPathInput.value,
          action_path_auto: autoInput.checked,
          base_url: baseUrlInput.value,
          renderer_addons: buildRendererAddonPayload(addonControls.map(({ addon, control, sourceRootInput }) => ({
            ...addon,
            enabled: control.input.checked,
            sourceRoot: sourceRootInput.value,
          }))),
          selected_models: selectedModels,
          sync_native_auth: syncAuth.input.checked,
          sync_native_sessions: syncSessions.input.checked,
          sync_native_goals: syncGoals.input.checked,
          sync_native_skills: syncSkills.input.checked,
        });
        status.textContent = result?.restart_required === false
          ? "Settings applied"
          : "Settings saved; restarting the isolated Codex instance...";
      } catch (error) {
        status.textContent = error instanceof Error ? error.message : "Settings could not be applied";
        applyButton.disabled = false;
      }
    });

    const close = () => closeManagerDialog(documentRef);
    closeButton.addEventListener("click", close);
    cancelButton.addEventListener("click", close);
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) close();
    });
    const onKeyDown = (event) => {
      if (event.key === "Escape") close();
    };
    view.addEventListener?.("keydown", onKeyDown, true);
    overlay.__codexAdministratorCleanup = () => view.removeEventListener?.("keydown", onKeyDown, true);

    actionRow.append(autoLabel);
    panel.append(
      header,
      status,
      compatibilityWarning,
      field("Base URL", baseUrlInput),
      field("API Key", apiKeyInput),
      actionRow,
      refreshButton,
      field("Search", searchInput),
      modelList,
      syncGroup,
      addonGroup,
      footer,
    );
    overlay.append(panel);
    documentRef.body.append(overlay);
    panel.focus?.();
    return overlay;
  }

  function resolveControlledModelMenu(documentRef) {
    if (!documentRef || typeof documentRef.querySelectorAll !== "function") return null;
    const triggers = documentRef.querySelectorAll(
      'button[data-codex-intelligence-trigger="true"][aria-expanded="true"][aria-controls]',
    );
    if (!triggers || triggers.length !== 1) return null;
    const menuId = triggers[0].getAttribute("aria-controls");
    if (!menuId || typeof documentRef.getElementById !== "function") return null;
    const menu = documentRef.getElementById(menuId);
    if (
      !menu
      || menu.isConnected === false
      || menu.getAttribute?.("role") !== "menu"
      || menu.getAttribute?.("data-state") === "closed"
      || menu.getAttribute?.("aria-hidden") === "true"
      || typeof menu.getClientRects !== "function"
      || menu.getClientRects().length === 0
    ) {
      return null;
    }
    return menu;
  }

  function ensureManagerEntry({ documentRef, label, menu, onOpen }) {
    if (!documentRef || !menu || typeof documentRef.createElement !== "function") return null;
    const existing = menu.querySelector?.(`[${managerAttribute}]`);
    if (existing) return existing;

    const fallbackContainer = menu.firstElementChild || menu.children?.[0] || menu;
    const menuItems = [...menu.querySelectorAll?.('[role="menuitem"]') || []];
    const template = menuItems.find((item) => {
      if (item.getAttribute?.("aria-hidden") === "true") return false;
      const rect = item.getBoundingClientRect?.();
      return rect && rect.width >= 16 && rect.height >= 16;
    }) || menuItems[0] || null;
    const container = fallbackContainer;
    const separator = documentRef.createElement("div");
    separator.setAttribute("role", "separator");
    separator.setAttribute("aria-hidden", "true");
    separator.setAttribute(separatorAttribute, "true");
    separator.className = "mx-2 my-1 h-px bg-token-border";

    const entry = documentRef.createElement("div");
    entry.setAttribute("role", "menuitem");
    entry.setAttribute(managerAttribute, "true");
    entry.setAttribute("data-orientation", "vertical");
    entry.setAttribute("data-radix-collection-item", "");
    entry.tabIndex = -1;
    entry.className = template?.className || "";
    entry.textContent = typeof label === "string" && label ? label : "Manage Grok models";
    setStyles(entry, { position: "relative", zIndex: "1" });

    const open = () => {
      if (typeof onOpen === "function") onOpen();
    };
    entry.addEventListener("click", open);
    entry.addEventListener("pointerdown", (event) => {
      event.preventDefault?.();
      event.stopPropagation?.();
      open();
    });
    entry.addEventListener("keydown", (event) => {
      if (event?.key !== "Enter" && event?.key !== " ") return;
      event.preventDefault?.();
      open();
    });
    if (typeof container.prepend === "function") {
      container.prepend(entry, separator);
    } else {
      const first = container.firstChild || null;
      container.insertBefore?.(separator, first);
      container.insertBefore?.(entry, separator);
    }
    mountedEntries.set(menu, { entry, separator });
    return entry;
  }

  function removeManagerEntry(menu) {
    if (!menu) return false;
    const mounted = mountedEntries.get(menu);
    const entry = mounted?.entry || menu.querySelector?.(`[${managerAttribute}]`);
    if (!entry) return false;
    mounted?.separator?.remove?.();
    entry.remove?.();
    mountedEntries.delete(menu);
    return true;
  }

  function createController({ documentRef, label, onOpen }) {
    let candidate = null;
    let candidateFrames = 0;
    let closedFrames = 0;
    let disposed = false;
    let mountedMenu = null;

    function reconcile() {
      if (disposed) return false;
      const next = resolveControlledModelMenu(documentRef);
      if (next) {
        closedFrames = 0;
        if (next === candidate) {
          candidateFrames += 1;
        } else {
          candidate = next;
          candidateFrames = 1;
        }
        if (candidateFrames < 2) return false;
        if (mountedMenu && mountedMenu !== next) removeManagerEntry(mountedMenu);
        mountedMenu = next;
        return Boolean(ensureManagerEntry({ documentRef, label, menu: next, onOpen }));
      }

      candidate = null;
      candidateFrames = 0;
      closedFrames += 1;
      if (closedFrames >= 2 && mountedMenu) {
        removeManagerEntry(mountedMenu);
        mountedMenu = null;
      }
      return false;
    }

    function dispose() {
      if (mountedMenu) removeManagerEntry(mountedMenu);
      mountedMenu = null;
      candidate = null;
      disposed = true;
      return true;
    }

    function health() {
      return {
        disposed,
        mounted: Boolean(mountedMenu?.isConnected),
      };
    }

    return Object.freeze({ dispose, health, reconcile });
  }

  globalThis.__codexAdministratorModelPickerMount = Object.freeze({
    buildRendererAddonPayload,
    closeManagerDialog,
    createController,
    ensureManagerEntry,
    normalizeManagerState,
    openManagerDialog,
    rendererAddonStatusText,
    removeManagerEntry,
    resolveControlledModelMenu,
  });
})();
