import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const assets = new URL("../assets/", import.meta.url);

class FakeElement {
  constructor(tagName = "div") {
    this.tagName = tagName.toUpperCase();
    this.attributes = new Map();
    this.children = [];
    this.className = "";
    this.isConnected = true;
    this.listeners = new Map();
    this.parentNode = null;
    this.rects = [{}];
    this.style = {};
    this.tabIndex = 0;
    this.textContent = "";
  }

  append(...children) {
    for (const child of children) {
      child.parentNode = this;
      this.children.push(child);
    }
  }

  get firstChild() {
    return this.children[0] || null;
  }

  get firstElementChild() {
    return this.children[0] || null;
  }

  prepend(...children) {
    for (const child of [...children].reverse()) {
      child.parentNode = this;
      this.children.unshift(child);
    }
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }

  dispatch(type, event = {}) {
    this.listeners.get(type)?.({ preventDefault() {}, ...event });
  }

  getAttribute(name) {
    return this.attributes.get(name) ?? null;
  }

  getClientRects() {
    return this.rects;
  }

  getBoundingClientRect() {
    return this.rects[0] || { height: 0, width: 0 };
  }

  querySelector(selector) {
    if (selector === "[data-codex-administrator-model-manager]") {
      return this.find((node) => node.getAttribute("data-codex-administrator-model-manager") === "true");
    }
    if (selector === '[role="menuitem"]') {
      return this.find((node) => node.getAttribute("role") === "menuitem");
    }
    if (selector === "[data-codex-administrator-model-manager-dialog]") {
      return this.find((node) => node.getAttribute("data-codex-administrator-model-manager-dialog") === "true");
    }
    return null;
  }

  querySelectorAll(selector) {
    const matches = [];
    this.findAll((node) => {
      if (selector === '[role="menuitem"]') return node.getAttribute("role") === "menuitem";
      if (selector === 'input[type="checkbox"]') {
        return node.tagName === "INPUT" && node.type === "checkbox";
      }
      return false;
    }, matches);
    return matches;
  }

  find(predicate) {
    for (const child of this.children) {
      if (predicate(child)) return child;
      const nested = child.find(predicate);
      if (nested) return nested;
    }
    return null;
  }

  findAll(predicate, matches) {
    for (const child of this.children) {
      if (predicate(child)) matches.push(child);
      child.findAll(predicate, matches);
    }
  }

  remove() {
    if (!this.parentNode) return;
    this.parentNode.children = this.parentNode.children.filter((child) => child !== this);
    this.parentNode = null;
    this.isConnected = false;
  }

  removeChild(child) {
    this.children = this.children.filter((candidate) => candidate !== child);
    child.parentNode = null;
    return child;
  }

  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }
}

function fixture({ nestedTrack = false } = {}) {
  const trigger = new FakeElement("button");
  trigger.setAttribute("aria-expanded", "true");
  trigger.setAttribute("aria-controls", "model-menu");
  const menu = new FakeElement();
  menu.setAttribute("id", "model-menu");
  menu.setAttribute("role", "menu");
  menu.setAttribute("data-state", "open");
  const content = new FakeElement();
  const hiddenTemplate = new FakeElement();
  hiddenTemplate.setAttribute("role", "menuitem");
  hiddenTemplate.className = "hidden-keyboard-control";
  hiddenTemplate.rects = [{ height: 1, width: 1 }];
  const template = new FakeElement();
  template.setAttribute("role", "menuitem");
  template.className = "native-menu-item";
  template.rects = [{ height: 30, width: 120 }];
  const track = nestedTrack ? new FakeElement() : content;
  track.append(hiddenTemplate, template);
  if (nestedTrack) content.append(track);
  menu.append(content);

  const documentRef = {
    createElement(tagName) {
      return new FakeElement(tagName);
    },
    getElementById(id) {
      return id === "model-menu" ? menu : null;
    },
    querySelectorAll(selector) {
      assert.equal(
        selector,
        'button[data-codex-intelligence-trigger="true"][aria-expanded="true"][aria-controls]',
      );
      return [trigger];
    },
  };
  return { content, documentRef, menu, track, trigger };
}

async function loadApi(globalValues = {}) {
  const source = await readFile(new URL("model-picker-mount.js", assets), "utf8");
  const context = vm.createContext({ globalThis: { ...globalValues } });
  vm.runInContext(source, context);
  return context.globalThis.__codexAdministratorModelPickerMount;
}

test("model picker mount follows the official trigger aria-controls relationship", async () => {
  const api = await loadApi();
  const { documentRef, menu } = fixture();

  assert.equal(api.resolveControlledModelMenu(documentRef), menu);

  documentRef.querySelectorAll = () => [];
  assert.equal(api.resolveControlledModelMenu(documentRef), null);

  const duplicate = fixture().trigger;
  documentRef.querySelectorAll = () => [duplicate, duplicate];
  assert.equal(api.resolveControlledModelMenu(documentRef), null);
});

test("manager entry is native-styled, idempotent, keyboard accessible, and disposable", async () => {
  const api = await loadApi();
  const { content, documentRef, menu } = fixture();
  let opened = 0;

  const first = api.ensureManagerEntry({
    documentRef,
    label: "Manage Grok models",
    menu,
    onOpen: () => { opened += 1; },
  });
  const second = api.ensureManagerEntry({
    documentRef,
    label: "Manage Grok models",
    menu,
    onOpen: () => { opened += 1; },
  });

  assert.equal(first, second);
  assert.equal(first.className, "native-menu-item");
  assert.equal(first.getAttribute("role"), "menuitem");
  assert.equal(first.getAttribute("data-codex-administrator-model-manager"), "true");
  assert.equal(first.tabIndex, -1);
  assert.equal(content.children.length, 4);
  assert.equal(content.children[0], first);
  assert.equal(content.children[1].getAttribute("role"), "separator");

  first.dispatch("click");
  first.dispatch("keydown", { key: "Enter" });
  first.dispatch("keydown", { key: " " });
  let pointerStopped = false;
  first.dispatch("pointerdown", { stopPropagation() { pointerStopped = true; } });
  assert.equal(opened, 4);
  assert.equal(pointerStopped, true);
  assert.equal(first.getAttribute("data-radix-collection-item"), "");

  api.removeManagerEntry(menu);
  assert.equal(content.children.length, 2);
  assert.equal(menu.querySelector("[data-codex-administrator-model-manager]"), null);
});

test("manager entry stays above the native view track with its own pointer stacking layer", async () => {
  const api = await loadApi();
  const { content, documentRef, menu, track } = fixture({ nestedTrack: true });

  const entry = api.ensureManagerEntry({
    documentRef,
    label: "Manage Grok models",
    menu,
    onOpen() {},
  });

  assert.equal(content.children.length, 3);
  assert.equal(content.children[0], entry);
  assert.equal(content.children[1].getAttribute("role"), "separator");
  assert.equal(content.children[2], track);
  assert.equal(entry.style.position, "relative");
  assert.equal(entry.style.zIndex, "1");
});

test("controller requires two stable frames and unmounts after two closed frames", async () => {
  const api = await loadApi();
  const { content, documentRef, menu } = fixture();
  const controller = api.createController({
    documentRef,
    label: "Manage Grok models",
    onOpen() {},
  });

  controller.reconcile();
  assert.equal(content.children.length, 2);
  controller.reconcile();
  assert.equal(content.children.length, 4);
  assert.equal(controller.health().mounted, true);

  menu.setAttribute("data-state", "closed");
  controller.reconcile();
  assert.equal(content.children.length, 4);
  controller.reconcile();
  assert.equal(content.children.length, 2);
  assert.equal(controller.health().mounted, false);

  controller.dispose();
  assert.equal(controller.health().disposed, true);
});

test("manager state keeps only Grok models and exposes a reviewed multi-addon catalog without credentials", async () => {
  const api = await loadApi();
  const state = api.normalizeManagerState(
    {
      actionPath: "/responses",
      actionPathAuto: true,
      baseUrl: "https://ai.hebox.net/v1",
      codexPlusDetected: true,
      credentialPresent: true,
      modelConflicts: ["grok-4.5"],
      rendererAddonCatalog: [{
        display_name: "Codex Dream Skin",
        id: "codex-dream-skin",
        project_revision: "reviewed-commit",
      }, {
        displayName: "Reviewed Toolbar",
        id: "reviewed-toolbar",
        projectRevision: "toolbar-commit",
      }],
      rendererAddonReports: [{
        id: "codex-dream-skin",
        project_revision: "reviewed-commit",
        reason: null,
        state: "enabled",
      }],
      rendererAddons: [{
        enabled: true,
        id: "codex-dream-skin",
        source_root: "C:\\Injectors\\Codex-Dream-Skin",
      }, {
        enabled: false,
        id: "reviewed-toolbar",
        source_root: "",
      }],
      syncNativeAuth: true,
      syncNativeSessions: false,
    },
    [
      { model: "grok-4.5", displayName: "Grok 4.5" },
      { model: "gpt-5.6", displayName: "GPT-5.6" },
    ],
  );

  assert.equal(state.baseUrl, "https://ai.hebox.net/v1");
  assert.equal(state.actionPath, "/responses");
  assert.deepEqual(Array.from(state.models, (model) => model.id), ["grok-4.5"]);
  assert.deepEqual(Array.from(state.selectedModels), ["grok-4.5"]);
  assert.equal(state.credentialPresent, true);
  assert.equal(state.codexPlusDetected, true);
  assert.deepEqual(Array.from(state.modelConflicts), ["grok-4.5"]);
  assert.deepEqual(Array.from(state.rendererAddons, (addon) => ({
    displayName: addon.displayName,
    enabled: addon.enabled,
    id: addon.id,
    revision: addon.revision,
    sourceRoot: addon.sourceRoot,
    state: addon.state,
  })), [{
    displayName: "Codex Dream Skin",
    enabled: true,
    id: "codex-dream-skin",
    revision: "reviewed-commit",
    sourceRoot: "C:\\Injectors\\Codex-Dream-Skin",
    state: "enabled",
  }, {
    displayName: "Reviewed Toolbar",
    enabled: false,
    id: "reviewed-toolbar",
    revision: "toolbar-commit",
    sourceRoot: "",
    state: "disabled",
  }]);
  assert.deepEqual(Array.from(api.buildRendererAddonPayload(state.rendererAddons), (addon) => ({
    enabled: addon.enabled,
    id: addon.id,
    source_root: addon.source_root,
  })), [{
    enabled: true,
    id: "codex-dream-skin",
    source_root: "C:\\Injectors\\Codex-Dream-Skin",
  }]);
  assert.equal("apiKey" in state, false);
  assert.equal("sourceRoot" in state, false);
  assert.equal(typeof api.openManagerDialog, "function");
  assert.equal(typeof api.closeManagerDialog, "function");
});

test("applying filtered results preserves selected models hidden by search", async () => {
  const api = await loadApi();
  const body = new FakeElement("body");
  const view = {
    addEventListener() {},
    removeEventListener() {},
  };
  const documentRef = {
    body,
    createElement(tagName) {
      return new FakeElement(tagName);
    },
    defaultView: view,
    querySelector(selector) {
      return body.querySelector(selector);
    },
  };
  const requests = [];
  api.openManagerDialog({
    documentRef,
    injectedModels: [
      { model: "grok-alpha", displayName: "Grok Alpha" },
      { model: "grok-beta", displayName: "Grok Beta" },
    ],
    modelPicker: {
      actionPath: "/responses",
      actionPathAuto: true,
      baseUrl: "https://ai.hebox.net/v1",
      credentialPresent: true,
      syncNativeAuth: true,
      syncNativeSessions: false,
    },
    async request(operation, payload) {
      requests.push({ operation, payload });
      return { restart_required: true };
    },
  });

  const search = body.find((node) => node.type === "search");
  const apply = body.find((node) => node.textContent === "Apply and restart");
  assert.ok(search);
  assert.ok(apply);

  search.value = "beta";
  search.dispatch("input");
  apply.dispatch("click");
  await Promise.resolve();

  assert.equal(requests.length, 1);
  assert.equal(requests[0].operation, "config.apply");
  assert.deepEqual(Array.from(requests[0].payload.selected_models), ["grok-alpha", "grok-beta"]);
});

test("checkbox edits survive search rerenders", async () => {
  const api = await loadApi();
  const body = new FakeElement("body");
  const documentRef = {
    body,
    createElement(tagName) {
      return new FakeElement(tagName);
    },
    defaultView: {
      addEventListener() {},
      removeEventListener() {},
    },
    querySelector(selector) {
      return body.querySelector(selector);
    },
  };
  api.openManagerDialog({
    documentRef,
    injectedModels: [
      { model: "grok-alpha", displayName: "Grok Alpha" },
      { model: "grok-beta", displayName: "Grok Beta" },
    ],
    modelPicker: {
      actionPath: "/responses",
      actionPathAuto: true,
      baseUrl: "https://ai.hebox.net/v1",
      credentialPresent: true,
    },
    async request() {
      return { restart_required: true };
    },
  });

  const initialAlpha = body.find((node) => node.value === "grok-alpha");
  const search = body.find((node) => node.type === "search");
  initialAlpha.checked = false;
  initialAlpha.dispatch("change");
  search.value = "beta";
  search.dispatch("input");
  search.value = "alpha";
  search.dispatch("input");

  const rerenderedAlpha = body.find((node) => node.value === "grok-alpha");
  assert.equal(rerenderedAlpha.checked, false);
});

test("manager reports renderer installation failure instead of claiming the addon is active", async () => {
  const api = await loadApi({
    __codexAdministratorRendererAddons: {
      health() {
        return {
          active: [],
          failed: [{ id: "codex-dream-skin", reason: "install_failed", revision: "reviewed-commit" }],
          pending: [],
        };
      },
    },
  });
  const state = api.normalizeManagerState({
    rendererAddonCatalog: [{
      display_name: "Codex Dream Skin",
      id: "codex-dream-skin",
      project_revision: "reviewed-commit",
    }],
    rendererAddonReports: [{
      id: "codex-dream-skin",
      project_revision: "reviewed-commit",
      reason: null,
      state: "enabled",
    }],
    rendererAddons: [{
      enabled: true,
      id: "codex-dream-skin",
      source_root: "C:\\Injectors\\Codex-Dream-Skin",
    }],
  }, []);

  assert.equal(state.rendererAddons[0].runtimeState, "failed");
  assert.equal(state.rendererAddons[0].runtimeReason, "install_failed");
  assert.equal(api.rendererAddonStatusText(state.rendererAddons[0]), "Runtime failed: install_failed");
});

test("manager does not accept runtime health from a different addon revision", async () => {
  const api = await loadApi({
    __codexAdministratorRendererAddons: {
      health() {
        return {
          active: [{ id: "codex-dream-skin", ok: true, revision: "old-commit" }],
          failed: [],
          pending: [],
        };
      },
    },
  });
  const state = api.normalizeManagerState({
    rendererAddonCatalog: [{
      display_name: "Codex Dream Skin",
      id: "codex-dream-skin",
      project_revision: "new-commit",
    }],
    rendererAddonReports: [{
      id: "codex-dream-skin",
      project_revision: "new-commit",
      reason: null,
      state: "enabled",
    }],
    rendererAddons: [{
      enabled: true,
      id: "codex-dream-skin",
      source_root: "C:\\Injectors\\Codex-Dream-Skin",
    }],
  }, []);

  assert.equal(state.rendererAddons[0].runtimeState, "unhealthy");
  assert.equal(state.rendererAddons[0].runtimeReason, "revision_mismatch");
  assert.doesNotMatch(api.rendererAddonStatusText(state.rendererAddons[0]), /active/i);
});
