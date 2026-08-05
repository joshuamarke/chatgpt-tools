(function chatgptToolsForceChineseLocale() {
  const config = window.__CHATGPT_TOOLS_FORCE_CHINESE_LOCALE__;
  if (!config) return { ok: false, reason: "no-config" };
  const enabled = config.enabled === true;
  const locale =
    typeof config.locale === "string" && config.locale ? config.locale : "zh-CN";
  const installationKey = `2:${enabled ? "on" : "off"}:${locale}`;
  if (window.__chatgptToolsForceChineseLocaleInstalled === installationKey) {
    return { ok: true, skipped: true, enabled: enabled, locale: locale };
  }
  window.__chatgptToolsForceChineseLocaleInstalled = installationKey;
  const languages = [locale, "zh", "en-US", "en"];
  const managedLocaleStorageKey = "chatgptTools.forceChineseLocale.managed.v1";
  const localeReloadStorageKey = "chatgptTools.forceChineseLocale.reload.v1";

  const readManagedLocale = () => {
    try {
      const value = JSON.parse(
        window.localStorage.getItem(managedLocaleStorageKey) || "null"
      );
      return value && typeof value === "object" ? value : null;
    } catch {
      return null;
    }
  };

  const writeManagedLocale = (value) => {
    try {
      if (value) {
        window.localStorage.setItem(
          managedLocaleStorageKey,
          JSON.stringify(value)
        );
      } else {
        window.localStorage.removeItem(managedLocaleStorageKey);
      }
    } catch {
      /* ignore */
    }
  };

  const waitForElectronBridge = () =>
    new Promise((resolve) => {
      const startedAt = Date.now();
      const check = () => {
        const bridge = window.electronBridge;
        if (bridge && typeof bridge.sendMessageFromView === "function") {
          resolve(bridge);
          return;
        }
        if (Date.now() - startedAt >= 5000) {
          resolve(null);
          return;
        }
        window.setTimeout(check, 50);
      };
      check();
    });

  const callCodexSettingApi = (bridge, method, params) =>
    new Promise((resolve, reject) => {
      const requestId =
        typeof crypto?.randomUUID === "function"
          ? crypto.randomUUID()
          : `cgt-locale-${Date.now()}-${Math.random().toString(16).slice(2)}`;
      let timeout;
      const cleanup = () => {
        window.clearTimeout(timeout);
        window.removeEventListener("message", onMessage);
      };
      const onMessage = (event) => {
        const message = event?.data;
        if (
          !message ||
          message.type !== "fetch-response" ||
          message.requestId !== requestId
        )
          return;
        cleanup();
        if (message.responseType !== "success") {
          reject(new Error(message.error || `Codex ${method} failed`));
          return;
        }
        try {
          resolve(JSON.parse(message.bodyJsonString || "null"));
        } catch (error) {
          reject(error);
        }
      };
      window.addEventListener("message", onMessage);
      timeout = window.setTimeout(() => {
        cleanup();
        reject(new Error(`Codex ${method} timed out`));
      }, 5000);
      const message = {
        type: "fetch",
        requestId,
        method: "POST",
        url: `vscode://codex/${method}`,
        body: JSON.stringify({ params }),
      };
      Promise.resolve(bridge.sendMessageFromView(message)).catch((error) => {
        cleanup();
        reject(error);
      });
    });

  const reloadAfterLocaleChange = (value) => {
    const marker = JSON.stringify(value);
    try {
      if (window.sessionStorage.getItem(localeReloadStorageKey) === marker)
        return;
      window.sessionStorage.setItem(localeReloadStorageKey, marker);
    } catch {
      /* ignore */
    }
    window.location.reload();
  };

  const clearLocaleReloadMarker = () => {
    try {
      window.sessionStorage.removeItem(localeReloadStorageKey);
    } catch {
      /* ignore */
    }
  };

  const syncOfficialLocaleSetting = async () => {
    const managed = readManagedLocale();
    if (!enabled && !managed) return;
    const bridge = await waitForElectronBridge();
    if (!bridge) return;
    const response = await callCodexSettingApi(bridge, "get-setting", {
      key: "localeOverride",
    });
    const currentValue = response?.value ?? null;

    if (enabled) {
      if (currentValue === locale) {
        clearLocaleReloadMarker();
        return;
      }
      if (!managed) {
        writeManagedLocale({ appliedLocale: locale, previousValue: currentValue });
      }
      await callCodexSettingApi(bridge, "set-setting", {
        key: "localeOverride",
        value: locale,
      });
      reloadAfterLocaleChange(locale);
      return;
    }

    if (currentValue !== managed.appliedLocale) {
      writeManagedLocale(null);
      clearLocaleReloadMarker();
      return;
    }
    const previousValue = managed.previousValue ?? null;
    await callCodexSettingApi(bridge, "set-setting", {
      key: "localeOverride",
      value: previousValue,
    });
    writeManagedLocale(null);
    reloadAfterLocaleChange(previousValue);
  };

  syncOfficialLocaleSetting().catch(() => {});
  if (!enabled) return { ok: true, enabled: false, locale: locale };

  const defineNavigatorGetter = (name, value) => {
    try {
      Object.defineProperty(Navigator.prototype, name, {
        configurable: true,
        get: () => value,
      });
    } catch {
      try {
        Object.defineProperty(navigator, name, {
          configurable: true,
          get: () => value,
        });
      } catch {
        /* ignore */
      }
    }
  };

  defineNavigatorGetter("language", locale);
  defineNavigatorGetter("languages", languages);

  const patchI18nConfig = (dynamicConfig) => {
    if (!dynamicConfig || typeof dynamicConfig !== "object") return dynamicConfig;
    const value =
      dynamicConfig.value && typeof dynamicConfig.value === "object"
        ? dynamicConfig.value
        : {};
    const nextValue = {
      ...value,
      enable_i18n: true,
      locale_source: "SYSTEM",
    };
    try {
      dynamicConfig.value = nextValue;
    } catch {
      /* ignore */
    }
    if (
      typeof dynamicConfig.get === "function" &&
      !dynamicConfig.__chatgptToolsForceChineseLocaleGetPatched
    ) {
      const originalGet = dynamicConfig.get.bind(dynamicConfig);
      dynamicConfig.get = (key, fallback) => {
        if (key === "enable_i18n") return true;
        if (key === "locale_source") return "SYSTEM";
        return originalGet(key, fallback);
      };
      dynamicConfig.__chatgptToolsForceChineseLocaleGetPatched = true;
    }
    return dynamicConfig;
  };

  const statsigClients = () => {
    const root = window.__STATSIG__ || globalThis.__STATSIG__;
    if (!root || typeof root !== "object") return [];
    const clients = [
      root.firstInstance,
      typeof root.instance === "function" ? root.instance() : null,
    ];
    if (root.instances && typeof root.instances === "object") {
      clients.push(...Object.values(root.instances));
    }
    return clients.filter(
      (client, index, array) =>
        client && typeof client === "object" && array.indexOf(client) === index
    );
  };

  const patchStatsigClient = (client) => {
    if (!client || typeof client !== "object") return;
    if (typeof client.getDynamicConfig !== "function") return;
    if (!client.__chatgptToolsForceChineseLocalePatched) {
      const originalGetDynamicConfig = client.getDynamicConfig.bind(client);
      client.getDynamicConfig = (name, options) => {
        const result = originalGetDynamicConfig(name, options);
        return name === "72216192" ? patchI18nConfig(result) : result;
      };
      client.__chatgptToolsForceChineseLocalePatched = true;
    }
    try {
      patchI18nConfig(
        client.getDynamicConfig("72216192", { disableExposureLog: true })
      );
    } catch {
      /* ignore */
    }
  };

  const patchStatsigRoot = (root) => {
    if (
      !root ||
      typeof root !== "object" ||
      root.__chatgptToolsForceChineseLocaleRootPatched
    )
      return;
    root.__chatgptToolsForceChineseLocaleRootPatched = true;
    ["firstInstance", "instance"].forEach((key) => {
      let current;
      try {
        current = root[key];
      } catch {
        return;
      }
      patchStatsigClient(
        typeof current === "function" && key === "instance"
          ? current.call(root)
          : current
      );
      try {
        Object.defineProperty(root, key, {
          configurable: true,
          get: () => current,
          set: (next) => {
            current = next;
            patchStatsigClient(
              typeof next === "function" && key === "instance"
                ? next.call(root)
                : next
            );
          },
        });
      } catch {
        /* ignore */
      }
    });
  };

  const installStatsigRootSetter = () => {
    const descriptor = Object.getOwnPropertyDescriptor(window, "__STATSIG__");
    if (descriptor && descriptor.configurable === false) return;
    let currentRoot = window.__STATSIG__;
    patchStatsigRoot(currentRoot);
    try {
      Object.defineProperty(window, "__STATSIG__", {
        configurable: true,
        get: () => currentRoot,
        set: (next) => {
          currentRoot = next;
          patchStatsigRoot(next);
          statsigClients().forEach(patchStatsigClient);
        },
      });
    } catch {
      /* ignore */
    }
  };

  const patchStatsigI18nConfig = () => {
    installStatsigRootSetter();
    const root = window.__STATSIG__ || globalThis.__STATSIG__;
    patchStatsigRoot(root);
    statsigClients().forEach((client) => {
      if (typeof client.getDynamicConfig !== "function") return;
      patchStatsigClient(client);
    });
  };

  patchStatsigI18nConfig();
  const startedAt = Date.now();
  const timer = window.setInterval(() => {
    patchStatsigI18nConfig();
    if (Date.now() - startedAt > 5000) window.clearInterval(timer);
  }, 50);

  return { ok: true, enabled: true, locale: locale };
})()
