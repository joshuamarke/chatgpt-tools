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
    /**
     * Launch ChatGPT when offline.
     * Re-applies last session skin if present; otherwise cold-starts host only.
     */
    startHost: () => invoke("start_host"),
    /**
     * Hard restart ChatGPT (stop + relaunch with debug port).
     * Re-applies last session skin if present.
     */
    restartHost: () => invoke("restart_host"),
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
    /**
     * Local environment probe (ChatGPT/Codex desktop, Codex CLI, Grok Build, npm/node).
     * @param {{ force?: boolean }} [opts]
     */
    envCheck: (opts = {}) =>
      invoke("env_check", { force: opts.force === true }),
    /**
     * Probe a single Overview environment (card-level refresh).
     * @param {string} id chatgpt-desktop | codex-cli | grok-build | node | npm
     */
    envCheckTool: (id) =>
      invoke("env_check_tool", { id: String(id || "").trim() }),
    /**
     * Open the OS default terminal and run an allow-listed npm install command.
     * Windows: cmd /K via start; macOS: Terminal.app via osascript.
     * @param {string} command
     */
    openInstallTerminal: (command) =>
      invoke("open_install_terminal", { command: String(command || "") }),
    /** Open or focus the independent Skin DevTools window. */
    openDevtools: () => invoke("open_devtools"),

    // ── Host inspect (real-window Overlay pick → Elements) ──
    inspectConnect: () => invoke("inspect_connect"),
    inspectDisconnect: () => invoke("inspect_disconnect"),
    inspectStatus: () => invoke("inspect_status"),
    inspectSetPicking: (enabled) =>
      invoke("inspect_set_picking", { enabled: enabled === true }),
    inspectPoll: (opts = {}) =>
      invoke("inspect_poll", { waitMs: opts.waitMs ?? 0 }),
    inspectGetDocument: (opts = {}) =>
      invoke("inspect_get_document", { depth: opts.depth ?? 2 }),
    inspectGetChildren: (nodeId) =>
      invoke("inspect_get_children", { nodeId: Number(nodeId) }),
    inspectSelectNode: (nodeId) =>
      invoke("inspect_select_node", { nodeId: Number(nodeId) }),
    inspectHighlight: (nodeId) =>
      invoke("inspect_highlight", { nodeId: Number(nodeId) }),

    // ── Cloud CDN (catalog / announcements / secure download — Rust only) ──
    cloudStatus: (opts = {}) =>
      invoke("cloud_status", { force: opts.force === true }),
    /** Soft CDN sync (TTL + disk cache). force:true bypasses soft TTL. */
    cloudRefresh: (opts = {}) =>
      invoke("cloud_refresh", { force: opts.force === true }),
    cloudAnnouncements: (opts = {}) =>
      invoke("cloud_announcements", { refresh: opts.refresh === true }),
    cloudMarkAnnouncementRead: (id) =>
      invoke("cloud_mark_announcement_read", { id }),
    /** Download by catalog skin id only — never pass arbitrary URLs. */
    cloudDownloadSkin: (skinId) =>
      invoke("cloud_download_skin", { skinId }),
    /**
     * Ensure catalog preview thumbnails are on disk; returns data-URLs.
     * @param {string[]|null|undefined} [skinIds] subset; omit/empty = all catalog skins with preview
     */
    cloudEnsurePreviews: (skinIds) =>
      invoke("cloud_ensure_previews", {
        skinIds:
          Array.isArray(skinIds) && skinIds.length > 0
            ? skinIds.map((id) => String(id))
            : null,
      }),
    /**
     * Installed app version from the Tauri package (Cargo / tauri.conf).
     * Prefer this over any hardcoded GUI constant.
     * @returns {Promise<string>}
     */
    getAppVersion: async () => {
      const getVersion = window.__TAURI__?.app?.getVersion;
      if (typeof getVersion === "function") {
        return String(await getVersion());
      }
      throw new Error("无法读取应用版本（Tauri app API 不可用）");
    },

    /**
     * @deprecated App version checks use GitHub via tauri-plugin-updater (`checkAppUpdate`).
     * Kept for tooling; skin catalog minAppVersion filters still use cloud.
     */
    cloudCheckUpdate: () => invoke("cloud_check_update"),

    /**
     * Check GitHub Releases `latest.json` via tauri-plugin-updater (multi-endpoint fallback).
     * Release notes come from the GitHub Release body embedded in latest.json.
     * @returns {Promise<null|{ rid:number, currentVersion:string, version:string, date?:string, body?:string, rawJson?:any }>}
     */
    checkAppUpdate: (opts = {}) =>
      invoke("plugin:updater|check", opts && typeof opts === "object" ? opts : {}),

    /**
     * Download + install a pending updater package, then caller should relaunch.
     * @param {number|string} rid Update resource id from checkAppUpdate
     * @param {(ev:any)=>void} [onEvent] progress channel messages
     */
    installAppUpdate: async (rid, onEvent) => {
      const inv = await waitForInvoke();
      const args = { rid };
      const Channel = window.__TAURI__?.core?.Channel;
      if (typeof onEvent === "function" && typeof Channel === "function") {
        const channel = new Channel();
        channel.onmessage = onEvent;
        args.onEvent = channel;
      }
      await inv("plugin:updater|download_and_install", args);
    },

    /** Relaunch after a successful installAppUpdate. */
    relaunchApp: () => invoke("plugin:process|restart"),

    /** About / contact from CDN — independent of app version check. */
    cloudAbout: (opts = {}) =>
      invoke("cloud_about", { refresh: opts.refresh === true }),
    cloudClearSkinCache: (skinId) =>
      invoke("cloud_clear_skin_cache", {
        skinId: skinId == null || skinId === "" ? null : String(skinId),
      }),
  };
})();
