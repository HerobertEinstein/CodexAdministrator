(() => {
  "use strict";

  const moduleReference = /["']\.\/(vscode-api-[A-Za-z0-9._-]+\.js)["']/;

  function findRendererApiModuleUrl(entryUrl, source) {
    if (typeof entryUrl !== "string" || typeof source !== "string") return null;
    const match = source.match(moduleReference);
    if (!match) return null;
    try {
      const entry = new URL(entryUrl);
      const module = new URL(`./${match[1]}`, entry);
      if (module.protocol !== entry.protocol || module.host !== entry.host) return null;
      return module.href;
    } catch {
      return null;
    }
  }

  function findRendererApiExport(moduleNamespace) {
    if (!moduleNamespace || typeof moduleNamespace !== "object") return null;
    for (const value of Object.values(moduleNamespace)) {
      if (
        !value
        || typeof value !== "object"
        || typeof value.postMessage !== "function"
        || typeof value.getState !== "function"
        || typeof value.setState !== "function"
      ) {
        continue;
      }
      const descriptor = Object.getOwnPropertyDescriptor(value, "postMessage");
      if (descriptor && descriptor.writable === false && typeof descriptor.set !== "function") {
        continue;
      }
      return value;
    }
    return null;
  }

  async function discoverRendererApi({ documentRef, fetchFn, importModule }) {
    if (
      !documentRef
      || typeof fetchFn !== "function"
      || typeof importModule !== "function"
    ) {
      return null;
    }
    const entryScript = Array.from(documentRef.scripts || []).find(
      (script) => script?.type === "module" && typeof script.src === "string" && script.src,
    );
    if (!entryScript) return null;
    const response = await fetchFn(entryScript.src);
    if (!response || response.ok === false || typeof response.text !== "function") return null;
    const moduleUrl = findRendererApiModuleUrl(entryScript.src, await response.text());
    if (!moduleUrl) return null;
    return findRendererApiExport(await importModule(moduleUrl));
  }

  globalThis.__codexAdministratorRendererApiDiscovery = Object.freeze({
    discoverRendererApi,
    findRendererApiExport,
    findRendererApiModuleUrl,
  });
})();
