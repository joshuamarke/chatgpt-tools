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
    /** Codex: keep ChatGPT OAuth in auth.json when enabling third-party. */
    getPreserveCodexAuth: () => invoke("get_preserve_codex_official_auth"),
    /** @param {boolean} enabled */
    setPreserveCodexAuth: (enabled) =>
      invoke("set_preserve_codex_official_auth", { enabled: !!enabled }),
    /** Local routing */
    getProxyStatus: () => invoke("get_proxy_status"),
    getProxyConfig: () => invoke("get_proxy_config"),
    updateProxyConfig: (config) => invoke("update_proxy_config", { config }),
    getTakeoverStatus: () => invoke("get_proxy_takeover_status"),
    setTakeover: (app, enabled) =>
      invoke("set_proxy_takeover", { app, enabled: !!enabled }),
    getAppProxySettings: (app) => invoke("get_app_proxy_settings", { app }),
    updateAppProxySettings: (settings) =>
      invoke("update_app_proxy_settings", { settings }),
    setAutoFailover: (app, enabled) =>
      invoke("set_auto_failover", { app, enabled: !!enabled }),
    getFailoverQueue: (app) => invoke("get_failover_queue", { app }),
    addToFailover: (app, providerId) =>
      invoke("add_to_failover_queue", { app, providerId }),
    removeFromFailover: (app, providerId) =>
      invoke("remove_from_failover_queue", { app, providerId }),
    reorderFailover: (app, providerIds) =>
      invoke("reorder_failover_queue", { app, providerIds }),
    resetCircuit: (app, providerId) =>
      invoke("reset_provider_circuit", { app, providerId }),
    stopProxyWithRestore: () => invoke("stop_proxy_with_restore"),
    repairTakeover: (app) => invoke("repair_proxy_takeover", { app }),
    /** @param {string} host @param {number} port */
    checkListenPort: (host, port) =>
      invoke("check_proxy_listen_port", { host, port: Number(port) || 0 }),
    /** Proxy request logs */
    listRequestLogs: (filters) =>
      invoke("list_proxy_request_logs", { filters: filters || {} }),
    getRequestLog: (id) => invoke("get_proxy_request_log", { id }),
    clearRequestLogs: () => invoke("clear_proxy_request_logs"),
    getLogRetentionDays: () => invoke("get_proxy_log_retention_days"),
    setLogRetentionDays: (days) =>
      invoke("set_proxy_log_retention_days", { days: Number(days) || 7 }),
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
    /**
     * Re-inject Codex desktop model whitelist from live catalog (best-effort).
     * @returns {Promise<{attempted:boolean,ok:boolean,models:string[],message:string}>}
     */
    refreshModelUnlock: () => invoke("refresh_codex_model_unlock"),
  };
})();
