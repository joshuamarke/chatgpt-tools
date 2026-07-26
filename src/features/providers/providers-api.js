/**
 * Frontend bridge for API provider management (Codex + Grok Build).
 * Independent of window.skinAPI / sessionAPI.
 */
(function () {
  function getInvoke() {
    const core = window.__TAURI__?.core;
    if (core?.invoke) return core.invoke.bind(core);
    throw new Error(
      "Tauri API 不可用。请通过 `npm run dev` 或安装包启动 ChatGPT Tools。"
    );
  }

  async function waitForInvoke(timeoutMs = 8000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      try {
        return getInvoke();
      } catch {
        await new Promise((r) => setTimeout(r, 50));
      }
    }
    return getInvoke();
  }

  async function invoke(cmd, args) {
    try {
      const inv = await waitForInvoke();
      return await inv(cmd, args || {});
    } catch (err) {
      const msg =
        typeof err === "string"
          ? err
          : err?.message || err?.toString?.() || JSON.stringify(err);
      throw new Error(msg);
    }
  }

  window.providerAPI = {
    /** @param {"codex"|"grok"} app */
    list: (app) => invoke("list_providers", { app }),
    /**
     * @param {"codex"|"grok"} app
     * @param {string} id
     */
    get: (app, id) => invoke("get_provider", { app, id }),
    /**
     * @param {"codex"|"grok"} app
     * @param {object} request
     */
    add: (app, request) => invoke("add_provider", { app, request }),
    /**
     * @param {"codex"|"grok"} app
     * @param {string} id
     * @param {object} request
     */
    update: (app, id, request) =>
      invoke("update_provider", { app, id, request }),
    /**
     * @param {"codex"|"grok"} app
     * @param {string} id
     */
    remove: (app, id) => invoke("delete_provider", { app, id }),
    /**
     * @param {"codex"|"grok"} app
     * @param {string} id
     */
    switch: (app, id) => invoke("switch_provider", { app, id }),
    /**
     * @param {"codex"|"grok"} app
     * @param {string} [name]
     */
    importLive: (app, name) =>
      invoke("import_live_as_provider", { app, name: name || null }),
    /** @param {"codex"|"grok"} app */
    paths: (app) => invoke("provider_paths_info", { app }),
    /** Built-in channel presets for the add form. @param {"codex"|"grok"} app */
    presets: (app) => invoke("list_provider_presets", { app }),
    /** Force re-write current provider to live config. @param {"codex"|"grok"} app */
    reapply: (app) => invoke("reapply_current_provider", { app }),
    /**
     * Lightweight base_url reachability probe (any HTTP response = reachable).
     * @param {string} baseUrl
     * @param {number} [timeoutSecs]
     * @returns {Promise<{success:boolean,status:string,message:string,responseTimeMs?:number,httpStatus?:number,url:string}>}
     */
    testConnectivity: (baseUrl, timeoutSecs, customUserAgent) =>
      invoke("test_provider_connectivity", {
        baseUrl,
        timeoutSecs: timeoutSecs ?? null,
        customUserAgent: customUserAgent || null,
      }),
    /**
     * Fetch OpenAI-compatible model list (GET …/models candidates).
     * @param {string} baseUrl
     * @param {string} apiKey
     * @param {string} [modelsUrl]
     * @param {string} [customUserAgent]
     * @returns {Promise<Array<{id:string,ownedBy?:string}>>}
     */
    fetchModels: (baseUrl, apiKey, modelsUrl, customUserAgent) =>
      invoke("fetch_provider_models", {
        baseUrl,
        apiKey,
        modelsUrl: modelsUrl || null,
        customUserAgent: customUserAgent || null,
      }),
  };
})();
