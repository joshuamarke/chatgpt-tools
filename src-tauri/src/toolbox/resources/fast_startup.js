(function chatgptToolsFastStartup() {
  const config = window.__CHATGPT_TOOLS_FAST_STARTUP__;
  if (!config || config.enabled !== true) {
    return { ok: true, enabled: false, skipped: true };
  }
  if (window.__chatgptToolsFastStartupInstalled === "1") {
    return { ok: true, enabled: true, skipped: true };
  }
  window.__chatgptToolsFastStartupInstalled = "1";
  const timeoutMs = Math.max(
    100,
    Math.min(Number(config.statsigTimeoutMs) || 800, 3000)
  );
  const statsigHosts = new Set([
    "ab.chatgpt.com",
    "featureassets.org",
    "prodregistryv2.org",
    "api.statsigcdn.com",
    "statsigapi.net",
    "cloudflare-dns.com",
  ]);

  const isStatsigUrl = (input) => {
    try {
      const url = new URL(
        typeof input === "string" ? input : input?.url ?? "",
        window.location.href
      );
      return statsigHosts.has(url.hostname);
    } catch {
      return false;
    }
  };

  const timeoutSignal = (signal) => {
    const controller = new AbortController();
    const timer = window.setTimeout(() => controller.abort(), timeoutMs);
    const clear = () => window.clearTimeout(timer);
    if (signal) {
      if (signal.aborted) controller.abort();
      else signal.addEventListener("abort", () => controller.abort(), { once: true });
    }
    return { signal: controller.signal, clear };
  };

  const patchFetch = () => {
    if (typeof window.fetch !== "function" || window.fetch.__chatgptToolsFastStartupPatched)
      return;
    const originalFetch = window.fetch.bind(window);
    const patchedFetch = (input, init = undefined) => {
      if (!isStatsigUrl(input)) return originalFetch(input, init);
      const { signal, clear } = timeoutSignal(init?.signal);
      const nextInit = { ...(init || {}), signal };
      return originalFetch(input, nextInit).finally(clear);
    };
    patchedFetch.__chatgptToolsFastStartupPatched = true;
    window.fetch = patchedFetch;
  };

  const markStatsigReady = (client) => {
    if (
      !client ||
      typeof client !== "object" ||
      client.__chatgptToolsFastStartupReadyPatched
    )
      return;
    client.__chatgptToolsFastStartupReadyPatched = true;
    const markReady = () => {
      try {
        if (client.loadingStatus && client.loadingStatus !== "Ready") {
          client.loadingStatus = "Ready";
        }
      } catch {
        /* ignore */
      }
      try {
        if (typeof client.$emt === "function") {
          client.$emt({ name: "values_updated" });
        }
      } catch {
        /* ignore */
      }
    };
    if (typeof client.initializeAsync === "function") {
      const originalInitializeAsync = client.initializeAsync.bind(client);
      client.initializeAsync = (...args) =>
        Promise.race([
          originalInitializeAsync(...args).catch(() => null),
          new Promise((resolve) => window.setTimeout(() => resolve(null), timeoutMs)),
        ]).finally(markReady);
    }
    markReady();
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

  const patchStatsigRoot = () => statsigClients().forEach(markStatsigReady);

  patchFetch();
  patchStatsigRoot();
  const startedAt = Date.now();
  const timer = window.setInterval(() => {
    patchFetch();
    patchStatsigRoot();
    if (Date.now() - startedAt > 5000) window.clearInterval(timer);
  }, 50);

  return { ok: true, enabled: true, timeoutMs: timeoutMs };
})()
