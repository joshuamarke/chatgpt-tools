/**
 * Settings main view (sidebar「设置」).
 * Switches reuse providers「本地路由」.prov-route-switch appearance.
 * Depends on: window.toolboxAPI, showToast (from app.js).
 */
(function () {
  /** @type {Record<string, any> | null} */
  let settings = null;
  let bound = false;
  let saving = false;

  /** @type {Record<string, { id: string, key: string }>} */
  const SWITCHES = {
    forceChinese: { id: "swToolboxForceChinese", key: "forceChineseLocale" },
    fastStartup: { id: "swToolboxFastStartup", key: "fastStartup" },
    computerUseGuard: { id: "swToolboxComputerUseGuard", key: "computerUseGuardEnabled" },
    pluginUnlock: { id: "swToolboxPluginUnlock", key: "pluginMarketplaceUnlock" },
  };

  function $(id) {
    return document.getElementById(id);
  }

  function isWindows() {
    try {
      return /\bWindows\b/i.test(navigator.userAgent || "");
    } catch {
      return true;
    }
  }

  /** @param {HTMLElement | null} btn @param {boolean} on */
  function setSwitch(btn, on) {
    if (!btn) return;
    btn.setAttribute("aria-checked", on ? "true" : "false");
  }

  /** @param {HTMLElement | null} btn */
  function switchOn(btn) {
    return btn?.getAttribute("aria-checked") === "true";
  }

  function applyToUi() {
    if (!settings) return;
    setSwitch(
      $(SWITCHES.forceChinese.id),
      settings.forceChineseLocale === true
    );
    setSwitch($(SWITCHES.fastStartup.id), settings.fastStartup === true);
    setSwitch(
      $(SWITCHES.computerUseGuard.id),
      settings.computerUseGuardEnabled === true
    );
    setSwitch(
      $(SWITCHES.pluginUnlock.id),
      settings.pluginMarketplaceUnlock === true
    );

    const guard = $(SWITCHES.computerUseGuard.id);
    if (guard && !isWindows()) {
      guard.title = "此功能主要在 Windows 上生效";
    }
  }

  async function loadSettings() {
    try {
      settings = await window.toolboxAPI.getSettings();
      applyToUi();
      void loadPluginStatus();
    } catch (err) {
      settings = {
        forceChineseLocale: false,
        pluginMarketplaceUnlock: false,
        fastStartup: false,
        computerUseGuardEnabled: false,
        thirdPartyActive: false,
        forceChineseEffective: false,
        pluginMarketplaceUnlockEffective: false,
      };
      applyToUi();
      window.showToast?.(err?.message || String(err), "error");
    }
  }

  async function loadPluginStatus() {
    const el = $("toolboxPluginStatus");
    const net = $("toolboxPluginNetHint");
    if (!el || !window.toolboxAPI?.pluginMarketplaceStatus) return;
    try {
      const st = await window.toolboxAPI.pluginMarketplaceStatus();
      const local = st?.local || {};
      const parts = [];
      if (local.needsRepair) {
        parts.push("本地目录未就绪 — 请先「修复插件市场」");
      } else {
        parts.push(
          local.configRegistered ? "本地目录已就绪" : "本地目录已就绪（待注册）"
        );
      }
      if (st?.pluginUnlockEffective) {
        parts.push(local.needsRepair ? "解锁已开但目录缺失" : "解锁生效中");
      } else if (st?.thirdPartyActive) {
        parts.push("解锁未开启");
      } else {
        parts.push("官方渠道不启用解锁");
      }
      el.textContent = "插件市场：" + parts.join(" · ");
      if (net && st?.networkHint) net.textContent = st.networkHint;
    } catch (err) {
      el.textContent = "插件市场状态读取失败 — " + (err?.message || String(err));
    }
  }

  async function runRepair() {
    const btnLocal = $("btnToolboxRepairPlugins");
    const btnStatus = $("btnToolboxPluginStatus");
    [btnLocal, btnStatus].forEach((b) => {
      if (b) b.disabled = true;
    });
    try {
      const ok = window.confirm?.(
        "将从 GitHub 下载 openai/plugins 并写入本机 Codex 目录。\n\n需要能访问 GitHub。网络不通时请先配置代理。\n\n是否继续？"
      );
      if (ok === false) return;
      window.showToast?.("正在从 GitHub 下载 openai/plugins…", "ok");
      const r = await window.toolboxAPI.repairPluginMarketplace();
      window.showToast?.(r?.message || "插件市场修复完成", "ok");
      await loadPluginStatus();
      await loadSettings();
    } catch (err) {
      window.showToast?.(err?.message || String(err), "error");
      await loadPluginStatus();
    } finally {
      [btnLocal, btnStatus].forEach((b) => {
        if (b) b.disabled = false;
      });
    }
  }

  /**
   * @param {string} key
   * @param {boolean} value
   * @param {HTMLElement | null} btn
   */
  async function setFlag(key, value, btn) {
    if (saving) return;
    saving = true;
    const all = Object.values(SWITCHES).map((s) => $(s.id));
    all.forEach((el) => {
      if (el) el.disabled = true;
    });
    // Optimistic UI
    setSwitch(btn, value);
    try {
      const patch = {};
      patch[key] = value;
      settings = await window.toolboxAPI.updateSettings(patch);
      applyToUi();
      const third = settings?.thirdPartyActive === true;
      const labels = {
        forceChineseLocale: value
          ? third
            ? "已开启 Codex 中文界面"
            : "已开启 Codex 中文界面（切换到第三方后生效）"
          : "已关闭 Codex 中文界面",
        pluginMarketplaceUnlock: value
          ? third
            ? "已开启插件市场解锁"
            : "已开启插件市场解锁（切换到第三方后生效）"
          : "已关闭插件市场解锁",
        fastStartup: value ? "已开启快速启动（需重启 Codex）" : "已关闭快速启动",
        computerUseGuardEnabled: value
          ? "已启用 Computer Use Guard"
          : "已关闭 Computer Use Guard",
      };
      window.showToast?.(labels[key] || "设置已保存", "ok");
      if (key === "computerUseGuardEnabled" && value) {
        try {
          const r = await window.toolboxAPI.applyComputerUseGuard();
          if (r?.changed) window.showToast?.("Computer Use 配置已更新", "ok");
        } catch {
          /* non-fatal */
        }
      }
      if (key === "pluginMarketplaceUnlock") void loadPluginStatus();
    } catch (err) {
      applyToUi();
      window.showToast?.(err?.message || String(err), "error");
    } finally {
      saving = false;
      all.forEach((el) => {
        if (el) el.disabled = false;
      });
    }
  }

  function bindSwitch(def) {
    const btn = $(def.id);
    if (!btn || btn.dataset.bound === "1") return;
    btn.dataset.bound = "1";
    btn.addEventListener("click", () => {
      const next = !switchOn(btn);
      void setFlag(def.key, next, btn);
    });
  }

  function bind() {
    if (bound) return;
    bound = true;
    Object.values(SWITCHES).forEach(bindSwitch);

    $("btnToolboxPluginStatus")?.addEventListener("click", () => {
      void loadPluginStatus();
    });
    $("btnToolboxRepairPlugins")?.addEventListener("click", () => {
      void runRepair();
    });

    try {
      const eventApi = window.__TAURI__?.event;
      eventApi?.listen?.("provider-switched", () => {
        loadSettings();
      });
    } catch {
      /* ignore */
    }
  }

  window.toolboxView = {
    enter() {
      bind();
      loadSettings();
    },
    leave() {},
    reload() {
      loadSettings();
    },
  };
})();
