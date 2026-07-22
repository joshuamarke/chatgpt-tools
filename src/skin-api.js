/**
 * Frontend bridge: same `window.skinAPI` surface as the Electron preload.
 * Implementation uses Tauri 2 invoke + plugins.
 */
(function () {
  function getInvoke() {
    // withGlobalTauri: true → window.__TAURI__
    const core = window.__TAURI__?.core;
    if (core?.invoke) return core.invoke.bind(core);
    throw new Error(
      "Tauri API 不可用。请通过 `npm run dev` 或安装包启动 ChatGPT Tools，不要直接用浏览器打开 index.html。"
    );
  }

  /** 等待 Tauri 注入完成（dev 启动瞬间偶发未就绪） */
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

  window.skinAPI = {
    status: () => invoke("status"),
    /** Lightweight ChatGPT lifecycle for polling (no skins / previews). */
    hostStatus: (opts = {}) =>
      invoke("host_status", { force: opts.force === true }),
    detect: () => invoke("detect"),
    apply: (skinId, opts = {}) =>
      invoke("apply", {
        skinId,
        // Default off: hot-switch without restarting ChatGPT/Codex.
        // When GUI checks「自动重启」, pass restart: true so the host relaunches.
        restart: opts.restart === true,
      }),
    restore: (opts = {}) =>
      invoke("restore", {
        restoreTheme: opts.restoreTheme !== false,
      }),
    /** Live pause: flag + CDP remove (never false-success when host is injectable). */
    pause: () => invoke("pause"),
    /** Clear pause and re-apply last session skin. */
    resume: (opts = {}) =>
      invoke("resume", {
        restart: opts.restart === true,
      }),
    openPath: (p) => invoke("open_path", { target: p }),
    openExternal: (url) => invoke("open_external", { url }),
    exportSkin: (skinId) => invoke("export_skin", { skinId }),
    importSkin: () => invoke("import_skin"),
    chooseWallpaper: () => invoke("choose_wallpaper"),
    designWallpaper: (payload) => invoke("design_wallpaper", { payload }),
    deleteSkin: (skinId) => invoke("delete_skin", { skinId }),
    revealExport: (filePath) => invoke("reveal_export", { filePath }),
    chooseApp: () => invoke("choose_app"),
    clearAppPath: () => invoke("clear_app_path"),
    enginePaths: () => invoke("engine_paths"),
    engineVersion: () => invoke("engine_version"),
  };
})();
