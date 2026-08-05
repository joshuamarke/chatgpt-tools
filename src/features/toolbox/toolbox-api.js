/**
 * Frontend bridge for toolbox settings.
 * Independent of window.skinAPI / sessionAPI / providerAPI.
 *
 * getSettings returns preferences + effective runtime (third-party gate).
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

  window.toolboxAPI = {
    /** @returns {Promise<{ forceChineseLocale: boolean, pluginMarketplaceUnlock: boolean, fastStartup: boolean, computerUseGuardEnabled: boolean, thirdPartyActive: boolean, forceChineseEffective: boolean, pluginMarketplaceUnlockEffective: boolean }>} */
    getSettings: () => invoke("get_toolbox_settings"),
    /**
     * @param {{ forceChineseLocale?: boolean, pluginMarketplaceUnlock?: boolean, fastStartup?: boolean, computerUseGuardEnabled?: boolean }} patch
     */
    updateSettings: (patch) => {
      const args = {};
      if (patch && typeof patch.forceChineseLocale === "boolean") {
        args.forceChineseLocale = patch.forceChineseLocale;
      }
      if (patch && typeof patch.pluginMarketplaceUnlock === "boolean") {
        args.pluginMarketplaceUnlock = patch.pluginMarketplaceUnlock;
      }
      if (patch && typeof patch.fastStartup === "boolean") {
        args.fastStartup = patch.fastStartup;
      }
      if (patch && typeof patch.computerUseGuardEnabled === "boolean") {
        args.computerUseGuardEnabled = patch.computerUseGuardEnabled;
      }
      return invoke("update_toolbox_settings", args);
    },
    applyComputerUseGuard: () => invoke("apply_computer_use_guard_now"),
    pluginMarketplaceStatus: () => invoke("plugin_marketplace_status"),
    /** Downloads openai/plugins from GitHub — requires network. */
    repairPluginMarketplace: () => invoke("repair_plugin_marketplace"),
  };
})();
