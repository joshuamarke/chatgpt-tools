/**
 * Shared ChatGPT / Codex skin runtime (engine/runtime/renderer-core.js).
 * Skins only supply CSS + art + plugin.json; do not fork this file per skin.
 *
 * Layer model:
 *   Framework (this file + immersive-skin.css via payload)
 *     — shell-guard, adaptive classes, full-window baseline capability
 *   Contract (docs authors follow by convention when making skins)
 *     — how to customize without breaking native controls / suggestions / wallpaper
 *   Personalization (skins/<id>)
 *     — author-owned CSS + plugin; engine does not restrict style rules
 *
 * Performance inject model:
 *   Phase 1 shell — CSS + chrome + markers; art arg may be empty string.
 *   Phase 2 art   — state.applyArt(dataUrl) with the heavy image.
 *   Hot switch    — host.applySkin(delta) swaps CSS/markers/plugin without re-shipping core.
 *
 * Steady-state host model (after install settles):
 *   warm  — shell observers on main/body only; coalesced ensure
 *   steady — no subtree MutationObserver; theme attrs + resize + sparse heartbeat
 *   hidden — observers paused while document is not visible
 *   Goal: successful skin must not tax the host chat stream.
 *
 * Global host: window.__CHATGPT_TOOLS_SKIN_HOST__ (slim core stays resident across skins).
 * Operation UI: host.showOperation / finishOperation (page-local feedback, no skin CSS).
 * Host DOM anchors (names only): engine/runtime/selectors.json — do not hardcode hashes.
 *
 * Assembly tokens (exact match, replaced by payload.mjs via split/join — all occurrences):
 *   CSS / ART / THEME / MARKERS / PLUGIN / REVISION  →  see IIFE args at bottom.
 * Do not put those token strings in this header comment (would steal the first replace).
 */
((cssText, artDataUrl, rawTheme, rawMarkers, rawPlugin, payloadRevision) => {
  // Mutable so host.applySkin can hot-swap without re-evaluating this whole core.
  let markers = rawMarkers && typeof rawMarkers === "object" ? rawMarkers : {};
  let plugin = rawPlugin && typeof rawPlugin === "object" ? rawPlugin : {};
  let theme = rawTheme && typeof rawTheme === "object" ? rawTheme : {};
  let activeCss = typeof cssText === "string" ? cssText : "";

  let STATE_KEY = markers.stateKey || "__CODEX_SKIN_STATE__";
  let DISABLED_KEY = markers.disabledKey || "__CODEX_SKIN_DISABLED__";
  let STYLE_ID = markers.styleId || "codex-skin-style";
  let CHROME_ID = markers.chromeId || "codex-skin-chrome";
  let ROOT_CLASS = markers.rootClass || "codex-skin";
  let HOME_CLASS = markers.homeClass || "skin-home";
  let HOME_SHELL_CLASS = markers.homeShellClass || `${HOME_CLASS}-shell`;
  let HOME_UTILITY_CLASS = markers.homeUtilityClass || `${HOME_CLASS}-utility`;
  let ART_VAR = markers.artVar || "--skin-art";
  let VERSION = plugin.version || theme.version || "2.0.0";
  let REVISION = payloadRevision || VERSION;
  const ANALYSIS_CACHE_KEY = "__CHATGPT_TOOLS_SKIN_ANALYSIS_CACHE__";
  const REGISTRY_KEY = "__CHATGPT_TOOLS_SKIN_REGISTRY__";
  const HOST_KEY = "__CHATGPT_TOOLS_SKIN_HOST__";
  // Core identity: changes only when renderer-core.js itself changes (payload embeds hash).
  const CORE_REVISION =
    (theme && typeof theme.coreRevision === "string" && theme.coreRevision) ||
    (typeof payloadRevision === "string" ? String(payloadRevision).slice(0, 16) : "core");

  const ROOT_THEME_CLASSES = [
    "skins-theme-light",
    "skins-theme-dark",
    "skins-art-wide",
    "skins-art-standard",
    "skins-art-none",
    "skins-focus-left",
    "skins-focus-center",
    "skins-focus-right",
    "skins-safe-left",
    "skins-safe-center",
    "skins-safe-right",
    "skins-safe-none",
    "skins-task-ambient",
    "skins-task-banner",
    "skins-task-off",
    // legacy engine names (pre skins-* rename) — clear on hot-switch / purge
    "dream-theme-light",
    "dream-theme-dark",
    "dream-art-wide",
    "dream-art-standard",
    "dream-focus-left",
    "dream-focus-center",
    "dream-focus-right",
    "dream-safe-left",
    "dream-safe-center",
    "dream-safe-right",
    "dream-safe-none",
    "dream-task-ambient",
    "dream-task-banner",
    "dream-task-off",
  ];
  const rootCssProperties = () => [
    ART_VAR,
    "--skins-art",
    "--skins-art-position",
    "--skins-focus-x",
    "--skins-focus-y",
    "--skins-accent",
    "--skins-accent-ink",
    "--skins-image-luma",
    "--skins-canvas",
    "--skins-sidebar",
    "--skins-surface-raised",
    "--skins-text",
    "--skins-line",
    // legacy engine vars
    "--dream-art",
    "--dream-art-position",
    "--dream-focus-x",
    "--dream-focus-y",
    "--dream-accent",
    "--dream-accent-ink",
    "--dream-image-luma",
    "--dream-canvas",
    "--dream-sidebar",
    "--dream-surface-raised",
    "--dream-text",
    "--dream-line",
  ];

  const installToken = {};
  let samplingNativeShell = false;
  let observer = null;
  let rootObserver = null;
  let resizeObserver = null;
  let observedShellMain = null;
  let analysisTimer = null;
  let hostDisposed = false;
  /** @type {"boot"|"warm"|"steady"|"hidden"} */
  let lifeMode = "boot";
  let steadyTimer = null;
  let warmSteadyTimer = null;
  let visibilityHandler = null;
  let lastRouteEnsureAt = 0;
  let lastLayoutBox = null;
  /** Cheap art identity — never retain multi-MB data URLs on state. */
  let artFingerprint = null;

  const now = () =>
    typeof performance === "object" && typeof performance.now === "function"
      ? performance.now()
      : Date.now();

  const metrics = {
    ensureCalls: 0,
    rootPasses: 0,
    routePasses: 0,
    layoutReads: 0,
    attributeWrites: 0,
    styleWrites: 0,
    analysisRuns: 0,
    analysisCacheHits: 0,
    firstEnsureMs: null,
    lifeMode: "boot",
    steadyEnters: 0,
    routeThrottled: 0,
  };

  const fingerprintArt = (dataUrl) => {
    if (typeof dataUrl !== "string" || dataUrl.length < 16) return null;
    return `${dataUrl.length}:${dataUrl.slice(0, 48)}:${dataUrl.slice(-24)}`;
  };

  const clamp = (value, min = 0, max = 1) =>
    Math.min(max, Math.max(min, Number(value)));

  const luminance = (red, green, blue) => {
    const linear = [red, green, blue].map((value) => {
      const channel = value / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
  };

  const hasNumber = (candidate) =>
    (typeof candidate === "number" ||
      (typeof candidate === "string" && String(candidate).trim() !== "")) &&
    Number.isFinite(Number(candidate));

  /**
   * art.mode: wallpaper | token-only | none
   * art.paint: body | none | custom
   *   body   — framework immersive-skin.css paints body wallpaper
   *   none   — no image / pure style
   *   custom — engine only sets --skins-art; skin paints main/chrome/any selector
   */
  const resolveArtMode = (artCfg) => {
    const m = typeof artCfg.mode === "string" ? artCfg.mode.trim().toLowerCase() : "";
    if (m === "none" || m === "token-only" || m === "wallpaper") return m;
    return "wallpaper";
  };
  const resolveArtPaint = (artCfg, mode) => {
    const p = typeof artCfg.paint === "string" ? artCfg.paint.trim().toLowerCase() : "";
    if (p === "body" || p === "none" || p === "custom") return p;
    if (mode === "none") return "none";
    if (mode === "token-only") return "custom";
    return "body";
  };

  const buildConfig = (nextTheme) => {
    const t = nextTheme && typeof nextTheme === "object" ? nextTheme : {};
    const artCfg = t.art && typeof t.art === "object" ? t.art : {};
    const appearanceChoice = ["auto", "light", "dark"].includes(t.appearance)
      ? t.appearance
      : "auto";
    const safeAreaChoice = ["auto", "left", "right", "center", "none"].includes(artCfg.safeArea)
      ? artCfg.safeArea
      : "auto";
    const taskModeChoice = ["auto", "ambient", "banner", "off"].includes(artCfg.taskMode)
      ? artCfg.taskMode
      : "auto";
    const artMode = resolveArtMode(artCfg);
    const artPaint = resolveArtPaint(artCfg, artMode);
    return {
      appearance: appearanceChoice,
      safeArea: safeAreaChoice,
      taskMode: taskModeChoice,
      artMode,
      artPaint,
      focusX: hasNumber(artCfg.focusX) ? clamp(artCfg.focusX) : null,
      focusY: hasNumber(artCfg.focusY) ? clamp(artCfg.focusY) : null,
      accent:
        typeof t.accent === "string" &&
        /^(?:#[\da-f]{3,8}|(?:rgb|hsl|oklch|oklab)\([^;{}]{1,96}\))$/i.test(t.accent.trim())
          ? t.accent.trim()
          : typeof t.palette?.accent === "string" &&
              /^(?:#[\da-f]{3,8}|(?:rgb|hsl|oklch|oklab)\([^;{}]{1,96}\))$/i.test(
                t.palette.accent.trim()
              )
            ? t.palette.accent.trim()
            : null,
      initialAspect:
        Number.isFinite(Number(t.artMetadata?.ratio)) && Number(t.artMetadata.ratio) > 0
          ? Number(t.artMetadata.ratio)
          : null,
    };
  };

  let config = buildConfig(theme);

  const defaultProfile = () => ({
    appearance: "dark",
    accent: [108, 131, 142],
    focusX: 0.5,
    focusY: 0.5,
    aspect: config.initialAspect ?? 1.6,
    luma: 0.32,
    safeArea: "center",
  });

  const existingAnalysisCache =
    window[ANALYSIS_CACHE_KEY] &&
    typeof window[ANALYSIS_CACHE_KEY].get === "function" &&
    typeof window[ANALYSIS_CACHE_KEY].set === "function"
      ? window[ANALYSIS_CACHE_KEY]
      : new Map();
  window[ANALYSIS_CACHE_KEY] = existingAnalysisCache;

  let artKey =
    typeof theme.artKey === "string"
      ? theme.artKey
      : typeof payloadRevision === "string"
        ? payloadRevision
        : null;
  let profile =
    artKey && existingAnalysisCache.has(artKey)
      ? { ...defaultProfile(), ...existingAnalysisCache.get(artKey) }
      : { ...defaultProfile() };
  if (artKey && existingAnalysisCache.has(artKey)) metrics.analysisCacheHits += 1;

  const rebindMarkerIds = () => {
    STATE_KEY = markers.stateKey || "__CODEX_SKIN_STATE__";
    DISABLED_KEY = markers.disabledKey || "__CODEX_SKIN_DISABLED__";
    STYLE_ID = markers.styleId || "codex-skin-style";
    CHROME_ID = markers.chromeId || "codex-skin-chrome";
    ROOT_CLASS = markers.rootClass || "codex-skin";
    HOME_CLASS = markers.homeClass || "skin-home";
    HOME_SHELL_CLASS = markers.homeShellClass || `${HOME_CLASS}-shell`;
    HOME_UTILITY_CLASS = markers.homeUtilityClass || `${HOME_CLASS}-utility`;
    ART_VAR = markers.artVar || "--skin-art";
    VERSION = plugin.version || theme.version || "2.0.0";
  };

  // --- multi-skin registry: disable/cleanup other skins without listing them in each plugin ---
  const registry =
    window[REGISTRY_KEY] && typeof window[REGISTRY_KEY] === "object"
      ? window[REGISTRY_KEY]
      : Object.create(null);
  window[REGISTRY_KEY] = registry;

  const disableForeignSkins = (keepStateKey) => {
    for (const [key, entry] of Object.entries(registry)) {
      if (key === keepStateKey) continue;
      try {
        if (entry?.disabledKey) window[entry.disabledKey] = true;
        if (typeof entry?.cleanup === "function") entry.cleanup();
      } catch {
        /* ignore */
      }
    }
  };

  /**
   * Soft residual cleanup used by hot-switch / delta / full-install paths.
   * Does NOT dispose observers, host, or the current install (keepAlive*).
   * Aligns with purge-all DOM side-effects without Page.reload or host teardown.
   * Function declaration so it is available before later const bindings (hoisted).
   */
  function softResidualCleanup({
    keepStyleId = null,
    keepChromeId = null,
    keepRootClass = null,
    keepStateKey = null,
  } = {}) {
    const root = document.documentElement;

    // Tear down foreign registry entries (never dispose keepAlive host/state).
    for (const [key, entry] of Object.entries(registry)) {
      if (keepStateKey && key === keepStateKey) continue;
      try {
        if (entry?.disabledKey) window[entry.disabledKey] = true;
      } catch {
        /* ignore */
      }
      try {
        if (entry?.rootClass && entry.rootClass !== keepRootClass) {
          root?.classList.remove(entry.rootClass);
        }
        if (entry?.artVar) root?.style.removeProperty(entry.artVar);
        if (entry?.styleId && entry.styleId !== keepStyleId) {
          document.getElementById(entry.styleId)?.remove();
        }
        if (entry?.chromeId && entry.chromeId !== keepChromeId) {
          document.getElementById(entry.chromeId)?.remove();
        }
      } catch {
        /* ignore */
      }
      try {
        delete registry[key];
      } catch {
        /* ignore */
      }
      try {
        delete window[key];
      } catch {
        /* ignore */
      }
    }

    // Legacy / one-off markers (pre-shared-runtime skins and external tools)
    const legacy = [
      {
        disabled: "__CODEX_DREAM_SKIN_DISABLED__",
        state: "__CODEX_DREAM_SKIN_STATE__",
        root: "codex-dream-skin",
        art: "--skins-art",
        style: "codex-dream-skin-style",
        chrome: "codex-dream-skin-chrome",
        homes: ["dream-home", "dream-home-shell", "dream-home-utility"],
      },
      {
        disabled: "__CODEX_CN_SKIN_DISABLED__",
        state: "__CODEX_CN_SKIN_STATE__",
        root: "codex-cn-skin",
        art: "--cn-art",
        style: "codex-cn-skin-style",
        chrome: "codex-cn-skin-chrome",
        homes: ["cn-home", "cn-home-shell", "cn-home-utility"],
      },
      {
        disabled: "__CODEX_QINGKONG_SKIN_DISABLED__",
        state: "__CODEX_QINGKONG_SKIN_STATE__",
        root: "codex-qingkong-skin",
        art: "--qingkong-art",
        style: "codex-qingkong-skin-style",
        chrome: "codex-qingkong-skin-chrome",
        homes: ["qingkong-home", "qingkong-home-shell", "qingkong-home-utility"],
      },
      {
        disabled: "__CODEX_LINGLONG_SKIN_DISABLED__",
        state: "__CODEX_LINGLONG_SKIN_STATE__",
        root: "codex-linglong-skin",
        art: "--linglong-art",
        style: "codex-linglong-skin-style",
        chrome: "codex-linglong-skin-chrome",
        homes: ["linglong-home", "linglong-home-shell", "linglong-home-utility"],
      },
      {
        disabled: "__CODEX_MORTAL_SKIN_DISABLED__",
        state: "__CODEX_MORTAL_SKIN_STATE__",
        root: "codex-mortal-skin",
        art: "--mortal-art",
        style: "codex-mortal-skin-style",
        chrome: "codex-mortal-skin-chrome",
        homes: ["mortal-home", "mortal-home-shell", "mortal-home-utility"],
      },
      {
        disabled: "__CODEX_CYBERPUNK_SKIN_DISABLED__",
        state: "__CODEX_CYBERPUNK_SKIN_STATE__",
        root: "codex-cyberpunk-skin",
        art: "--cyberpunk-art",
        style: "codex-cyberpunk-skin-style",
        chrome: "codex-cyberpunk-skin-chrome",
        homes: ["cyberpunk-home", "cyberpunk-home-shell", "cyberpunk-home-utility"],
      },
      {
        disabled: "__CODEX_EVA_SKIN_DISABLED__",
        state: "__CODEX_EVA_SKIN_STATE__",
        root: "codex-eva-skin",
        art: "--eva-art",
        style: "codex-eva-skin-style",
        chrome: "codex-eva-skin-chrome",
        homes: ["eva-home", "eva-home-shell", "eva-home-utility"],
      },
      {
        disabled: "__CODEX_BENGONG_SKIN_DISABLED__",
        state: "__CODEX_BENGONG_SKIN_STATE__",
        root: "codex-bengong-skin",
        art: "--bengong-art",
        style: "codex-bengong-skin-style",
        chrome: "codex-bengong-skin-chrome",
        homes: ["bengong-home", "bengong-home-shell", "bengong-home-utility"],
      },
      {
        disabled: "__CODEX_MIKU_SKIN_DISABLED__",
        state: "__CODEX_MIKU_SKIN_STATE__",
        root: "codex-miku-skin",
        art: "--miku-art",
        style: "codex-miku-skin-style",
        chrome: "codex-miku-skin-chrome",
        homes: ["miku-home", "miku-home-shell", "miku-home-utility"],
      },
      {
        disabled: "__CODEX_JIUYI_SKIN_DISABLED__",
        state: "__CODEX_JIUYI_SKIN_STATE__",
        root: "codex-jiuyi-skin",
        art: "--jiuyi-art",
        style: "codex-jiuyi-skin-style",
        chrome: "codex-jiuyi-skin-chrome",
        homes: ["jiuyi-home", "jiuyi-home-shell", "jiuyi-home-utility"],
      },
    ];
    for (const item of legacy) {
      if (keepStateKey && item.state === keepStateKey) continue;
      if (keepRootClass && item.root === keepRootClass) continue;
      try {
        window[item.disabled] = true;
      } catch {
        /* ignore */
      }
      try {
        delete window[item.state];
      } catch {
        /* ignore */
      }
      root?.classList.remove(item.root);
      root?.style.removeProperty(item.art);
      if (item.style !== keepStyleId) document.getElementById(item.style)?.remove();
      if (item.chrome !== keepChromeId) document.getElementById(item.chrome)?.remove();
      for (const cls of item.homes) {
        document.querySelectorAll(`.${cls}`).forEach((n) => n.classList.remove(cls));
      }
    }

    // Adaptive theme classes + shared CSS vars from core
    root?.classList.remove(...ROOT_THEME_CLASSES);
    for (const prop of [
      "--skins-art",
      "--skins-art-position",
      "--skins-focus-x",
      "--skins-focus-y",
      "--skins-accent",
      "--skins-accent-ink",
      "--skins-image-luma",
      "--dream-art",
      "--dream-art-position",
      "--dream-focus-x",
      "--dream-focus-y",
      "--dream-accent",
      "--dream-accent-ink",
      "--dream-image-luma",
    ]) {
      root?.style.removeProperty(prop);
    }
    // Drop all known per-skin art vars (current ART_VAR re-applied after ensure)
    for (const prop of [
      "--skin-art",
      "--skins-art",
      "--dream-art",
      "--cn-art",
      "--qingkong-art",
      "--linglong-art",
      "--mortal-art",
      "--cyberpunk-art",
      "--eva-art",
      "--bengong-art",
      "--miku-art",
      "--jiuyi-art",
    ]) {
      root?.style.removeProperty(prop);
    }
    if (ART_VAR) root?.style.removeProperty(ART_VAR);

    root?.removeAttribute("data-chatgpt-tools-skin");
    root?.removeAttribute("data-skins-shell");
    root?.removeAttribute("data-skins-art-mode");
    root?.removeAttribute("data-skins-art-paint");
    root?.removeAttribute("data-skin-contract");
    root?.removeAttribute("data-dream-shell");

    // Orphan style / chrome nodes from any skin revision
    document.querySelectorAll('style[data-skin-revision], style[id*="-skin-style"]').forEach((n) => {
      if (keepStyleId && n.id === keepStyleId) return;
      n.remove();
    });
    document.querySelectorAll('[id*="-skin-chrome"]').forEach((n) => {
      if (keepChromeId && n.id === keepChromeId) return;
      n.remove();
    });
  }

  /**
   * If slim core is already resident (same CORE_REVISION), hot-apply delta instead of
   * reinstalling observers / re-shipping the full runtime IIFE body work.
   */
  const existingHost = window[HOST_KEY];
  if (
    existingHost &&
    typeof existingHost.applySkin === "function" &&
    existingHost.coreRevision === CORE_REVISION &&
    typeof cssText === "string"
  ) {
    try {
      const deltaResult = existingHost.applySkin({
        css: cssText,
        markers,
        theme,
        plugin,
        revision: REVISION,
      });
      if (deltaResult?.ok) {
        if (typeof artDataUrl === "string" && artDataUrl.startsWith("data:")) {
          existingHost.applyArt?.(artDataUrl, REVISION);
        }
        return {
          installed: true,
          mode: "delta",
          version: VERSION,
          revision: REVISION,
          coreRevision: CORE_REVISION,
          adaptive: true,
          artReady: Boolean(existingHost.getActive?.()?.artReady),
          deferredArt: !(typeof artDataUrl === "string" && artDataUrl.startsWith("data:")),
        };
      }
    } catch {
      /* fall through to full install */
    }
  }

  const previous = window[STATE_KEY];
  if (previous?.observer) previous.observer.disconnect();
  if (previous?.rootObserver) previous.rootObserver.disconnect();
  if (previous?.resizeObserver) previous.resizeObserver.disconnect();
  if (previous?.timer) clearInterval(previous.timer);
  // Legacy / prior-install warm→steady settle timer (stored only if present).
  if (previous?.warmSteadyTimer) clearTimeout(previous.warmSteadyTimer);
  if (previous?.scheduler?.timeout) clearTimeout(previous.scheduler.timeout);
  if (previous?.scheduler?.frame != null && typeof cancelAnimationFrame === "function") {
    cancelAnimationFrame(previous.scheduler.frame);
  }
  if (previous?.analysisTimer) clearTimeout(previous.analysisTimer);
  if (previous?.resizeHandler) window.removeEventListener("resize", previous.resizeHandler);
  if (previous?.visibilityHandler) {
    try {
      document.removeEventListener("visibilitychange", previous.visibilityHandler);
    } catch {
      /* ignore */
    }
  }
  if (previous?.mediaHandler && previous?.mediaQuery) {
    try {
      previous.mediaQuery.removeEventListener("change", previous.mediaHandler);
    } catch {
      /* ignore */
    }
  }
  if (previous?.artUrl) URL.revokeObjectURL(previous.artUrl);

  disableForeignSkins(STATE_KEY);
  // Full install also sweeps orphan styles/classes (e.g. pre-shared-runtime inject).
  softResidualCleanup({
    keepStyleId: null,
    keepChromeId: null,
    keepRootClass: null,
    keepStateKey: STATE_KEY,
  });
  window[DISABLED_KEY] = false;

  /** @type {string|null} */
  let artUrl = null;
  let artReady = false;

  const revokeArtUrl = (url) => {
    if (!url) return;
    try {
      URL.revokeObjectURL(url);
    } catch {
      /* ignore */
    }
  };

  /**
   * Decode a data: URL into a blob: object URL.
   * Large original wallpapers are intentional — prefer fetch→blob (native decode)
   * over atob + per-byte loops that freeze the host on multi-MB images.
   * Empty / missing art is valid for phase-1 shell (CSS-first progressive enhance).
   */
  const materializeArtUrl = (dataUrl) => {
    if (typeof dataUrl !== "string" || !dataUrl.startsWith("data:") || dataUrl.length < 32) {
      return null;
    }
    try {
      // Synchronous XHR is avoided; use fetch when available (Chromium desktop always has it).
      // For very large data: URLs, fetch still runs on the renderer but skips JS byte loops.
      if (typeof fetch === "function") {
        // Kick async path: bind placeholder via sync fallback only if needed.
        // Primary path: createObjectURL from blob via deasync-free pattern —
        // use atob only for small payloads; large ones use Blob parts from base64 chunks.
        const comma = dataUrl.indexOf(",");
        if (comma < 0) return null;
        const header = dataUrl.slice(0, comma);
        const mime = /^data:([^;,]+)/.exec(header)?.[1] || "image/png";
        const isBase64 = /;base64/i.test(header);
        const body = dataUrl.slice(comma + 1);
        if (!isBase64) {
          return URL.createObjectURL(new Blob([decodeURIComponent(body)], { type: mime }));
        }
        // Chunked base64 → Uint8Array (much faster than charCodeAt per byte on huge arts)
        const binary = atob(body);
        const len = binary.length;
        const bytes = new Uint8Array(len);
        const CHUNK = 0x8000;
        for (let offset = 0; offset < len; offset += CHUNK) {
          const slice = binary.slice(offset, offset + CHUNK);
          for (let i = 0; i < slice.length; i += 1) {
            bytes[offset + i] = slice.charCodeAt(i);
          }
        }
        return URL.createObjectURL(new Blob([bytes], { type: mime }));
      }
      const comma = dataUrl.indexOf(",");
      if (comma < 0) return null;
      const mime = /^data:([^;,]+)/.exec(dataUrl)?.[1] || "image/png";
      const binary = atob(dataUrl.slice(comma + 1));
      const bytes = new Uint8Array(binary.length);
      for (let index = 0; index < binary.length; index += 1) {
        bytes[index] = binary.charCodeAt(index);
      }
      return URL.createObjectURL(new Blob([bytes], { type: mime }));
    } catch {
      return null;
    }
  };

  const bindArtUrl = (nextUrl, nextFingerprint = null) => {
    const previousUrl = artUrl;
    artUrl = nextUrl;
    artReady = Boolean(nextUrl);
    if (!nextUrl) artFingerprint = null;
    else if (nextFingerprint) artFingerprint = nextFingerprint;
    if (previousUrl && previousUrl !== nextUrl) revokeArtUrl(previousUrl);
    const state = window[STATE_KEY];
    if (state) {
      state.artUrl = artUrl;
      state.artReady = artReady;
      state.artFingerprint = artFingerprint;
      // Never keep multi-MB data URLs on the host heap after blob bind.
      state.artDataUrl = null;
    }
  };

  // Phase-1 shell may ship with empty art; phase-2 applyArt fills it in.
  if (typeof artDataUrl === "string" && artDataUrl.startsWith("data:")) {
    const bootFp = fingerprintArt(artDataUrl);
    bindArtUrl(materializeArtUrl(artDataUrl), bootFp);
  }

  const setStyleProperty = (root, name, value) => {
    if (root.style.getPropertyValue(name) !== value) {
      root.style.setProperty(name, value);
      metrics.styleWrites += 1;
    }
  };

  const setAttribute = (root, name, value) => {
    const normalized = String(value);
    if (root.getAttribute(name) !== normalized) {
      root.setAttribute(name, normalized);
      metrics.attributeWrites += 1;
    }
  };

  let cachedShellAppearance = null;
  let cachedShellAppearanceAt = 0;
  const SHELL_APPEARANCE_TTL_MS = 1500;

  const detectShellAppearance = (force = false) => {
    const nowTs = Date.now();
    if (
      !force &&
      cachedShellAppearance &&
      nowTs - cachedShellAppearanceAt < SHELL_APPEARANCE_TTL_MS
    ) {
      return cachedShellAppearance;
    }
    const root = document.documentElement;
    const body = document.body;
    const classes = `${root?.className || ""} ${body?.className || ""}`
      .toLowerCase()
      .replace(new RegExp(`\\b${ROOT_CLASS}\\b`, "g"), "")
      .replace(/\bskins-theme-(?:dark|light)\b/g, "")
      .replace(/\bdream-theme-(?:dark|light)\b/g, "");
    let resolved = null;
    if (/\b(dark|electron-dark|theme-dark|appearance-dark)\b/.test(classes)) resolved = "dark";
    else if (/\b(light|electron-light|theme-light|appearance-light)\b/.test(classes))
      resolved = "light";

    if (!resolved) {
      const dataTheme = (
        root?.getAttribute?.("data-theme") ||
        root?.getAttribute?.("data-appearance") ||
        root?.getAttribute?.("data-color-mode") ||
        body?.getAttribute?.("data-theme") ||
        body?.getAttribute?.("data-appearance") ||
        ""
      ).toLowerCase();
      if (dataTheme.includes("dark")) resolved = "dark";
      else if (dataTheme.includes("light")) resolved = "light";
    }

    if (!resolved) {
      try {
        const hadSkin = root?.classList?.contains?.(ROOT_CLASS);
        const savedThemeClasses = hadSkin
          ? ROOT_THEME_CLASSES.filter((className) => root.classList.contains(className))
          : [];
        samplingNativeShell = true;
        if (hadSkin) {
          root.classList.remove(ROOT_CLASS, ...ROOT_THEME_CLASSES);
        }
        try {
          const colorScheme = getComputedStyle(root).colorScheme || "";
          if (colorScheme.includes("dark") && !colorScheme.includes("light")) resolved = "dark";
          else if (colorScheme.includes("light") && !colorScheme.includes("dark"))
            resolved = "light";
        } finally {
          if (hadSkin) {
            root.classList.add(ROOT_CLASS, ...savedThemeClasses);
          }
          observer?.takeRecords?.();
          rootObserver?.takeRecords?.();
          samplingNativeShell = false;
        }
      } catch {
        samplingNativeShell = false;
      }
    }
    if (!resolved) {
      try {
        resolved = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
      } catch {
        resolved = "light";
      }
    }
    cachedShellAppearance = resolved;
    cachedShellAppearanceAt = nowTs;
    return resolved;
  };



  const clearSkinDom = () => {
    const root = document.documentElement;
    root?.classList.remove(ROOT_CLASS, ...ROOT_THEME_CLASSES);
    for (const property of rootCssProperties()) root?.style.removeProperty(property);
    document
      .querySelectorAll(`.${HOME_CLASS}`)
      .forEach((node) => node.classList.remove(HOME_CLASS));
    document
      .querySelectorAll(`.${HOME_SHELL_CLASS}`)
      .forEach((node) => node.classList.remove(HOME_SHELL_CLASS));
    document
      .querySelectorAll(`.${HOME_UTILITY_CLASS}`)
      .forEach((node) => node.classList.remove(HOME_UTILITY_CLASS));
    document.getElementById(STYLE_ID)?.remove();
    document.getElementById(CHROME_ID)?.remove();
    root?.removeAttribute("data-chatgpt-tools-skin");
    root?.removeAttribute("data-skins-shell");
    root?.removeAttribute("data-skins-art-mode");
    root?.removeAttribute("data-skins-art-paint");
    root?.removeAttribute("data-skin-contract");
    root?.removeAttribute("data-dream-shell");
    // Sweep orphans left by older injectors / failed hot-switches
    document.querySelectorAll('style[data-skin-revision], style[id*="-skin-style"]').forEach((n) => {
      if (n.id !== STYLE_ID) n.remove();
    });
    document.querySelectorAll('[id*="-skin-chrome"]').forEach((n) => {
      if (n.id !== CHROME_ID) n.remove();
    });
  };

  /** Tear down previous skin markers when hot-swapping to a different id. */
  const clearPreviousSkinMarkers = (prevMarkers) => {
    if (!prevMarkers || typeof prevMarkers !== "object") return;
    const root = document.documentElement;
    const prevRoot = prevMarkers.rootClass;
    const prevHome = prevMarkers.homeClass;
    const prevShell = prevMarkers.homeShellClass || (prevHome ? `${prevHome}-shell` : null);
    const prevUtility = prevMarkers.homeUtilityClass || (prevHome ? `${prevHome}-utility` : null);
    if (prevRoot) root?.classList.remove(prevRoot);
    root?.classList.remove(...ROOT_THEME_CLASSES);
    if (prevMarkers.artVar) root?.style.removeProperty(prevMarkers.artVar);
    root?.style.removeProperty("--skins-art");
    root?.style.removeProperty("--dream-art");
    for (const prop of [
      "--skins-art-position",
      "--skins-focus-x",
      "--skins-focus-y",
      "--skins-accent",
      "--skins-accent-ink",
      "--skins-image-luma",
      "--dream-art-position",
      "--dream-focus-x",
      "--dream-focus-y",
      "--dream-accent",
      "--dream-accent-ink",
      "--dream-image-luma",
    ]) {
      root?.style.removeProperty(prop);
    }
    root?.removeAttribute("data-chatgpt-tools-skin");
    root?.removeAttribute("data-skins-shell");
    root?.removeAttribute("data-skins-art-mode");
    root?.removeAttribute("data-skins-art-paint");
    root?.removeAttribute("data-skin-contract");
    root?.removeAttribute("data-dream-shell");
    if (prevHome) {
      document
        .querySelectorAll(`.${prevHome}`)
        .forEach((node) => node.classList.remove(prevHome));
    }
    if (prevShell) {
      document
        .querySelectorAll(`.${prevShell}`)
        .forEach((node) => node.classList.remove(prevShell));
    }
    if (prevUtility) {
      document
        .querySelectorAll(`.${prevUtility}`)
        .forEach((node) => node.classList.remove(prevUtility));
    }
    if (prevMarkers.styleId) document.getElementById(prevMarkers.styleId)?.remove();
    if (prevMarkers.chromeId) document.getElementById(prevMarkers.chromeId)?.remove();
    // Orphan styles/chromes not covered by prev markers
    document.querySelectorAll('style[data-skin-revision], style[id*="-skin-style"]').forEach((n) => {
      if (prevMarkers.styleId && n.id === prevMarkers.styleId) return;
      // Will be recreated for the next skin; remove all current skin styles
      n.remove();
    });
    document.querySelectorAll('[id*="-skin-chrome"]').forEach((n) => {
      if (prevMarkers.chromeId && n.id === prevMarkers.chromeId) {
        n.remove();
        return;
      }
      n.remove();
    });
    if (prevMarkers.stateKey && prevMarkers.stateKey !== STATE_KEY) {
      try {
        delete window[prevMarkers.stateKey];
      } catch {
        /* ignore */
      }
    }
    if (prevMarkers.disabledKey) {
      try {
        window[prevMarkers.disabledKey] = true;
      } catch {
        /* ignore */
      }
    }
  };

  const applyProfile = (root) => {
    metrics.rootPasses += 1;
    const focusX = config.focusX ?? profile.focusX;
    const focusY = config.focusY ?? profile.focusY;
    const appearance =
      config.appearance === "auto" ? detectShellAppearance() : config.appearance;
    const focus = focusX < 0.4 ? "left" : focusX > 0.6 ? "right" : "center";
    const safeArea =
      config.safeArea === "auto"
        ? profile.safeArea ||
          (focus === "left" ? "right" : focus === "right" ? "left" : "center")
        : config.safeArea;
    const taskMode =
      config.taskMode === "auto"
        ? profile.aspect >= 2.25
          ? "banner"
          : "ambient"
        : config.taskMode;
    const accent =
      config.accent || `rgb(${profile.accent.map((c) => Math.round(c)).join(" ")})`;
    const accentInk =
      luminance(...profile.accent) > 0.42 ? "rgb(26 24 28)" : "rgb(250 248 251)";

    const artMode = config.artMode || "wallpaper";
    const artPaint = config.artPaint || "body";
    const pureStyle = artMode === "none";
    // Pure-style: keep skins-art-standard so readability baseline still applies;
    // never skins-art-wide (no body wallpaper). skins-art-none is an extra marker.
    const useWide = !pureStyle && profile.aspect >= 1.75;
    const useStandard = pureStyle || profile.aspect < 1.75;

    root.classList.add(ROOT_CLASS);
    root.classList.toggle("skins-theme-light", appearance === "light");
    root.classList.toggle("skins-theme-dark", appearance === "dark");
    root.classList.toggle("skins-art-wide", useWide);
    root.classList.toggle("skins-art-standard", useStandard);
    root.classList.toggle("skins-art-none", pureStyle);
    for (const value of ["left", "center", "right"]) {
      root.classList.toggle(`skins-focus-${value}`, focus === value);
    }
    for (const value of ["left", "center", "right", "none"]) {
      root.classList.toggle(`skins-safe-${value}`, safeArea === value);
    }
    for (const value of ["ambient", "banner", "off"]) {
      root.classList.toggle(`skins-task-${value}`, taskMode === value);
    }

    // Art CSS vars only after phase-2 (or monolithic inject with data URL).
    // paint=body|custom both get the token; immersive baseline only paints when paint=body.
    if (artUrl && artMode !== "none") {
      setStyleProperty(root, ART_VAR, `url("${artUrl}")`);
      // Compatibility alias used by some designer CSS overrides
      if (ART_VAR !== "--skins-art") {
        setStyleProperty(root, "--skins-art", `url("${artUrl}")`);
      }
    } else if (pureStyle) {
      // Clear any leftover wallpaper tokens from a previous skin.
      root.style.removeProperty(ART_VAR);
      root.style.removeProperty("--skins-art");
    }
    setStyleProperty(
      root,
      "--skins-art-position",
      `${Math.round(focusX * 100)}% ${Math.round(focusY * 100)}%`
    );
    setStyleProperty(root, "--skins-focus-x", String(focusX));
    setStyleProperty(root, "--skins-focus-y", String(focusY));
    setStyleProperty(root, "--skins-accent", accent);
    setStyleProperty(root, "--skins-accent-ink", accentInk);
    setStyleProperty(root, "--skins-image-luma", Number(profile.luma || 0.32).toFixed(3));
    // Baseline tokens for shared immersive-skin.css (skins may override in their CSS).
    if (appearance === "dark") {
      setStyleProperty(root, "--skins-canvas", "rgb(26 28 36)");
      setStyleProperty(root, "--skins-sidebar", "rgb(18 20 26)");
      setStyleProperty(root, "--skins-surface-raised", "rgb(37 40 48)");
      setStyleProperty(root, "--skins-text", "rgb(238 240 246)");
      setStyleProperty(root, "--skins-line", "rgb(72 76 90)");
    } else {
      setStyleProperty(root, "--skins-canvas", "rgb(247 248 252)");
      setStyleProperty(root, "--skins-sidebar", "rgb(240 241 246)");
      setStyleProperty(root, "--skins-surface-raised", "rgb(255 255 255)");
      setStyleProperty(root, "--skins-text", "rgb(32 37 54)");
      setStyleProperty(root, "--skins-line", "rgb(200 202 212)");
    }
    setAttribute(root, "data-chatgpt-tools-skin", markers.id || ROOT_CLASS);
    setAttribute(root, "data-skins-shell", appearance);
    // Framework paint gate for immersive-skin.css body wallpaper rules.
    setAttribute(root, "data-skins-art-mode", artMode);
    setAttribute(root, "data-skins-art-paint", artPaint);
    // Framework baseline marker (immersive-skin.css is always in payload CSS).
    setAttribute(root, "data-skin-contract", pureStyle ? "style-only" : "full-window");
    return appearance;
  };

  const ensureStyle = (root) => {
    let style = document.getElementById(STYLE_ID);
    if (!style) {
      style = document.createElement("style");
      style.id = STYLE_ID;
      style.textContent = activeCss;
      (document.head || root).appendChild(style);
    } else if (
      style.dataset.skinRevision !== REVISION ||
      style.textContent !== activeCss
    ) {
      style.textContent = activeCss;
    }
    style.dataset.skinRevision = REVISION;
    style.dataset.skinVersion = VERSION;
    return style;
  };

  /**
   * Host probes — keep in sync with engine/runtime/selectors.json keys:
   *   shell-main, home-icon, game-source, home-suggestions, home-utility, home-route
   * Prefer testids / stable classes; never lock CSS-module full hashes.
   */
  const queryShellMain = () =>
    document.querySelector("main.main-surface") ||
    document.querySelector("main") ||
    document.querySelector('[role="main"]');

  const queryHomeAnchor = () =>
    document.querySelector('[data-testid="home-icon"]') ||
    document.querySelector('[data-feature="game-source"]') ||
    document.querySelector(".group\\/home-suggestions") ||
    document.querySelector('[class*="home-suggestions"]') ||
    null;

  const queryHomeRoute = (homeAnchor) => {
    if (homeAnchor) {
      const fromAnchor =
        homeAnchor.closest('[role="main"]') ||
        homeAnchor.closest('[class*="home-main-content"]') ||
        homeAnchor.closest(".app-shell-main-content-frame") ||
        null;
      if (fromAnchor) return fromAnchor;
    }
    return (
      document.querySelector('[role="main"]:has([data-testid="home-icon"])') ||
      document.querySelector('[role="main"]:has([data-feature="game-source"])') ||
      document.querySelector('[role="main"]:has([class*="home-suggestions"])') ||
      document.querySelector('[role="main"][class*="home-main-content"]') ||
      document.querySelector('[class*="home-main-content"]:has([data-testid="home-icon"])') ||
      document.querySelector('[class*="home-main-content"]:has([data-feature="game-source"])') ||
      null
    );
  };

  /**
   * Cheap home-marker health for SPA remounts.
   * Codex may keep main.main-surface stable while replacing [role=main] / hero /
   * suggestions (chat → 新建任务). Shell class can linger while HOME_CLASS is gone.
   */
  const probeHomeMarkerHealth = (shellMain = queryShellMain()) => {
    const hasAnchor = Boolean(queryHomeAnchor());
    let hasHomeClass = false;
    try {
      hasHomeClass = Boolean(document.querySelector(`.${HOME_CLASS}`));
    } catch {
      hasHomeClass = Boolean(
        [...document.querySelectorAll("[class]")].some((el) => el.classList.contains(HOME_CLASS))
      );
    }
    const hasShellClass = Boolean(shellMain?.classList.contains(HOME_SHELL_CLASS));
    const ok =
      (hasAnchor && hasHomeClass && hasShellClass) ||
      (!hasAnchor && !hasHomeClass && !hasShellClass);
    return { hasAnchor, hasHomeClass, hasShellClass, ok };
  };

  const syncRouteState = ({ layout = false } = {}) => {
    metrics.routePasses += 1;
    const shellMain = queryShellMain();

    // Auxiliary windows (pets, blank targets): clear residual skin.
    // Left rail is optional — Codex may remove aside while collapsing.
    if (!shellMain || !document.body) {
      clearSkinDom();
      lastLayoutBox = null;
      return;
    }

    // Home markers vary by host mode: Codex often exposes home-icon; Worker / CDOEX
    // may only keep game-source, home-suggestions, or home-main-content container.
    const homeAnchor = queryHomeAnchor();
    const home = queryHomeRoute(homeAnchor);

    // Clear HOME_CLASS from any stale node (not only prior role=main — host remounts).
    for (const candidate of document.querySelectorAll(`.${HOME_CLASS}`)) {
      if (candidate !== home) candidate.classList.remove(HOME_CLASS);
    }
    if (home) home.classList.add(HOME_CLASS);

    const utilityBars = new Set(
      home ? home.querySelectorAll('[class*="_homeUtilityBar_"]') : []
    );
    for (const candidate of document.querySelectorAll(`.${HOME_UTILITY_CLASS}`)) {
      if (!utilityBars.has(candidate)) candidate.classList.remove(HOME_UTILITY_CLASS);
    }
    for (const candidate of utilityBars) candidate.classList.add(HOME_UTILITY_CLASS);

    if (observedShellMain !== shellMain) {
      resizeObserver?.disconnect();
      try {
        resizeObserver?.observe(shellMain);
      } catch {
        /* ignore */
      }
      observedShellMain = shellMain;
      layout = true;
      // Shell container swapped (SPA remount) — leave steady so observers re-arm.
      if (lifeMode === "steady") enterLifeMode("warm");
    }
    shellMain.classList.toggle(HOME_SHELL_CLASS, Boolean(home));

    let chrome = document.getElementById(CHROME_ID);
    let created = false;
    const chromeHtml =
      typeof plugin.chromeHtml === "string" && plugin.chromeHtml.trim()
        ? plugin.chromeHtml
        : `<div class="skin-brand" data-skin-chrome="brand"></div>`;
    if (!chrome || chrome.parentElement !== document.body) {
      chrome?.remove();
      chrome = document.createElement("div");
      chrome.id = CHROME_ID;
      chrome.setAttribute("aria-hidden", "true");
      chrome.innerHTML = chromeHtml;
      chrome.dataset.skinRevision = REVISION;
      document.body.appendChild(chrome);
      created = true;
    } else if (chrome.dataset.skinRevision !== REVISION) {
      chrome.innerHTML = chromeHtml;
      chrome.dataset.skinRevision = REVISION;
      created = true;
    }

    // Geometry only on real layout signals (resize / first paint / chrome create).
    // Route-only ensures must not force layout thrash during chat streaming.
    if (layout || created) {
      metrics.layoutReads += 1;
      const shellBox = shellMain.getBoundingClientRect();
      const nextBox = {
        left: Math.round(shellBox.left),
        top: Math.round(shellBox.top),
        width: Math.round(shellBox.width),
        height: Math.round(shellBox.height),
      };
      const same =
        lastLayoutBox &&
        lastLayoutBox.left === nextBox.left &&
        lastLayoutBox.top === nextBox.top &&
        lastLayoutBox.width === nextBox.width &&
        lastLayoutBox.height === nextBox.height;
      if (!same) {
        chrome.style.left = `${nextBox.left}px`;
        chrome.style.top = `${nextBox.top}px`;
        chrome.style.width = `${nextBox.width}px`;
        chrome.style.height = `${nextBox.height}px`;
        lastLayoutBox = nextBox;
      }
    }
    chrome.classList.toggle(HOME_SHELL_CLASS, Boolean(home));

    if (typeof plugin.onRoute === "function") {
      try {
        plugin.onRoute({ home: Boolean(home), chrome, shellMain, theme, markers });
      } catch {
        /* ignore plugin errors */
      }
    }
  };

  const ensure = ({ root: rootPass = true, route = true, layout = true } = {}) => {
    if (hostDisposed || window[DISABLED_KEY]) return;
    if (document.visibilityState === "hidden" && lifeMode !== "boot") return;
    const root = document.documentElement;
    if (!root) return;
    metrics.ensureCalls += 1;
    if (rootPass) {
      ensureStyle(root);
      applyProfile(root);
    }
    if (route) {
      const routeNow = now();
      // Steady: throttle route work — chat DOM churn must not re-walk home anchors.
      if (
        lifeMode === "steady" &&
        !layout &&
        lastRouteEnsureAt &&
        routeNow - lastRouteEnsureAt < 720
      ) {
        metrics.routeThrottled += 1;
      } else {
        lastRouteEnsureAt = routeNow;
        syncRouteState({ layout });
      }
    }
  };

  /**
   * Host observation lifecycle — structural, not a stack of timers.
   * warm: watch main/body structure until install settles
   * steady: theme attrs + resize + sparse heartbeat only
   * hidden: fully pause observation while the host tab is not visible
   */
  const ROUTE_OBSERVER_OPTS = { childList: true, subtree: false };
  const ROOT_ATTR_FILTER = ["class", "data-theme", "data-appearance", "data-color-mode", "style"];

  const disconnectRouteObserver = () => {
    try {
      observer?.disconnect();
    } catch {
      /* ignore */
    }
  };

  const armRouteObserver = () => {
    if (!observer || hostDisposed || lifeMode === "hidden" || lifeMode === "steady") return;
    disconnectRouteObserver();
    const shell = queryShellMain();
    // Host SPA often keeps main.main-surface stable and remounts deeper frames /
    // [role=main]. Watch those parents (childList only — not full subtree) so
    // chat→新建任务 re-enters warm without taxing chat stream nodes.
    const contentFrame =
      shell?.querySelector?.(".app-shell-main-content-frame") ||
      shell?.querySelector?.(".app-shell-main-content-viewport") ||
      null;
    const roleMain = document.querySelector('[role="main"]');
    const roleMainParent = roleMain?.parentElement || null;
    const seen = new Set();
    const targets = [];
    for (const node of [shell, document.body, contentFrame, roleMainParent, roleMain]) {
      if (!node || seen.has(node)) continue;
      seen.add(node);
      targets.push(node);
    }
    if (!targets.length && document.documentElement) targets.push(document.documentElement);
    for (const node of targets) {
      try {
        observer.observe(node, ROUTE_OBSERVER_OPTS);
      } catch {
        /* ignore */
      }
    }
  };

  const armRootObserver = () => {
    if (!rootObserver || hostDisposed || lifeMode === "hidden") return;
    try {
      rootObserver.disconnect();
    } catch {
      /* ignore */
    }
    try {
      rootObserver.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ROOT_ATTR_FILTER,
      });
    } catch {
      /* ignore */
    }
    if (document.body) {
      try {
        rootObserver.observe(document.body, {
          attributes: true,
          attributeFilter: ROOT_ATTR_FILTER,
        });
      } catch {
        /* ignore */
      }
    }
  };

  const clearWarmSteadyTimer = () => {
    if (warmSteadyTimer) {
      clearTimeout(warmSteadyTimer);
      warmSteadyTimer = null;
    }
  };

  const clearSteadyHeartbeat = () => {
    if (steadyTimer) {
      clearInterval(steadyTimer);
      steadyTimer = null;
    }
  };

  const scheduleWarmToSteady = () => {
    clearWarmSteadyTimer();
    // After first paint + short SPA settle window, drop structural MO entirely.
    warmSteadyTimer = setTimeout(() => {
      warmSteadyTimer = null;
      const st = window[STATE_KEY];
      if (st && st.installToken === installToken) st.warmSteadyTimer = null;
      if (!hostDisposed && lifeMode === "warm" && document.visibilityState !== "hidden") {
        enterLifeMode("steady");
      }
    }, 2800);
    const st = window[STATE_KEY];
    if (st && st.installToken === installToken) st.warmSteadyTimer = warmSteadyTimer;
  };

  const enterLifeMode = (next) => {
    if (hostDisposed) return;
    // Re-entering warm (delta / remount) only refreshes the settle window.
    if (lifeMode === next && next !== "boot") {
      if (next === "warm") scheduleWarmToSteady();
      return;
    }
    const prev = lifeMode;
    lifeMode = next;
    metrics.lifeMode = next;

    if (next === "hidden") {
      clearWarmSteadyTimer();
      clearSteadyHeartbeat();
      disconnectRouteObserver();
      try {
        rootObserver?.disconnect();
      } catch {
        /* ignore */
      }
      try {
        resizeObserver?.disconnect();
      } catch {
        /* ignore */
      }
      return;
    }

    if (next === "warm" || next === "boot") {
      clearSteadyHeartbeat();
      armRootObserver();
      armRouteObserver();
      if (observedShellMain && resizeObserver) {
        try {
          resizeObserver.observe(observedShellMain);
        } catch {
          /* ignore */
        }
      }
      if (next === "warm") scheduleWarmToSteady();
      return;
    }

    if (next === "steady") {
      clearWarmSteadyTimer();
      disconnectRouteObserver();
      armRootObserver();
      if (observedShellMain && resizeObserver) {
        try {
          resizeObserver.disconnect();
          resizeObserver.observe(observedShellMain);
        } catch {
          /* ignore */
        }
      }
      clearSteadyHeartbeat();
      // Home-marker health on a short cadence (cheap querySelector only).
      // Full root ensure stays sparse — chat stream must not re-walk styles.
      let lastSteadyRootAt = 0;
      steadyTimer = setInterval(() => {
        if (hostDisposed || document.visibilityState === "hidden") return;
        if (lifeMode !== "steady") return;
        const shell = queryShellMain();
        const health = probeHomeMarkerHealth(shell);
        if (!shell || shell !== observedShellMain || !health.ok) {
          metrics.homeHealthMisses = (metrics.homeHealthMisses || 0) + 1;
          enterLifeMode("warm");
          ensure({
            root: true,
            route: true,
            layout: !shell || shell !== observedShellMain,
          });
          return;
        }
        const t = now();
        if (!lastSteadyRootAt || t - lastSteadyRootAt > 45000) {
          lastSteadyRootAt = t;
          ensure({ root: true, route: false, layout: false });
        }
      }, 2200);
      if (prev !== "steady") metrics.steadyEnters += 1;
      const state = window[STATE_KEY];
      if (state) state.timer = steadyTimer;
    }
  };

  const wakeFromSteady = (reason = "route") => {
    if (hostDisposed || lifeMode === "hidden") return;
    if (lifeMode === "steady") enterLifeMode("warm");
    if (reason === "layout") scheduleEnsure({ route: true, layout: true });
    else if (reason === "root") scheduleEnsure({ root: true, route: true });
    else scheduleEnsure({ route: true });
  };

  const scheduleArtAnalysis = () => {
    if (plugin.skipAnalysis === true || theme.skipAnalysis === true) return;
    if ((config.artMode || "wallpaper") === "none") return;
    if (!artUrl) return;
    if (artKey && existingAnalysisCache.has(artKey)) return;
    if (analysisTimer) clearTimeout(analysisTimer);
    analysisTimer = setTimeout(() => {
      analyzeArt().then((analysis) => {
        const state = window[STATE_KEY];
        if (!analysis || state?.installToken !== installToken || window[DISABLED_KEY]) return;
        profile = { ...profile, ...analysis };
        state.profile = profile;
        if (artKey) {
          existingAnalysisCache.set(artKey, analysis);
          while (existingAnalysisCache.size > 8) {
            existingAnalysisCache.delete(existingAnalysisCache.keys().next().value);
          }
        }
        ensure({ root: true, route: false, layout: false });
      });
    }, 0);
    const state = window[STATE_KEY];
    if (state) state.analysisTimer = analysisTimer;
  };

  /**
   * Phase-2 entry: attach heavy wallpaper after shell CSS is already live.
   * Safe to call multiple times; ignores empty payloads and disabled skins.
   * When expectedRevision is set, must match active skin revision (hot-switch safety).
   */
  const applyArt = (nextArtDataUrl, expectedRevision = null) => {
    if (hostDisposed) return { ok: false, reason: "disposed" };
    if (window[DISABLED_KEY]) return { ok: false, reason: "disabled" };
    const state = window[STATE_KEY];
    if (!state || state.installToken !== installToken) {
      return { ok: false, reason: "stale" };
    }
    // Pure-style skins (art.mode=none) never accept wallpaper payloads.
    if ((config.artMode || "wallpaper") === "none") {
      return { ok: true, skipped: true, reason: "art-mode-none", revision: REVISION, artReady: false };
    }
    if (
      expectedRevision != null &&
      state.revision != null &&
      expectedRevision !== state.revision
    ) {
      return {
        ok: false,
        reason: "revision-mismatch",
        stateRevision: state.revision,
        revision: expectedRevision,
      };
    }
    if (typeof nextArtDataUrl !== "string" || !nextArtDataUrl.startsWith("data:")) {
      return { ok: false, reason: "invalid-art" };
    }
    const nextFp = fingerprintArt(nextArtDataUrl);
    // Same art already bound — no-op (idempotent reinject; fingerprint avoids retaining data URL).
    if (nextFp && artFingerprint === nextFp && artReady && artUrl) {
      return { ok: true, deferred: false, already: true, revision: REVISION };
    }
    const nextUrl = materializeArtUrl(nextArtDataUrl);
    if (!nextUrl) return { ok: false, reason: "decode-failed" };
    bindArtUrl(nextUrl, nextFp);
    ensure({ root: true, route: false, layout: false });
    scheduleArtAnalysis();
    // Art attach can finish after warm→steady; keep root tokens without re-arming MO.
    return { ok: true, deferred: true, revision: REVISION, artReady: true };
  };

  /**
   * Page-local apply/pause/switch toast (Shadow DOM, bottom-right edge).
   * Contract mirrors engine/runtime/operation-ui.js. Best-effort only — never throw.
   */
  const OP_HOST_ID = "chatgpt-tools-skin-operation";
  const ensureOperationHost = () => {
    try {
      let el = document.getElementById(OP_HOST_ID);
      if (el?.shadowRoot) return el;
      el?.remove();
      el = document.createElement("div");
      el.id = OP_HOST_ID;
      el.setAttribute("aria-live", "polite");
      const shadow = el.attachShadow({ mode: "open" });
      const style = document.createElement("style");
      // Bottom-right edge toast — does not cover the chat column.
      style.textContent = `
        :host{all:initial;position:fixed;inset:auto 12px 12px auto;z-index:2147483646;pointer-events:none;opacity:0;display:block;max-width:min(280px,calc(100vw - 20px));transition:opacity 140ms ease;font-family:system-ui,"Segoe UI","PingFang SC",sans-serif}
        :host([data-visible=true]){opacity:1}
        .card{box-sizing:border-box;min-width:140px;max-width:min(280px,calc(100vw - 20px));padding:10px 12px;border-radius:10px;border:1px solid rgba(238,239,244,.16);background:rgba(32,33,38,.94);color:#f3f3f6;box-shadow:0 8px 22px rgba(12,14,19,.28);text-align:left;font-size:12.5px;font-weight:550;line-height:1.35;display:flex;align-items:center;gap:10px;transform:translateY(6px);transition:transform 140ms ease}
        :host([data-visible=true]) .card{transform:none}
        :host([data-tone=light]) .card{border-color:#d9dbe3;background:rgba(248,248,251,.96);color:#25262c;box-shadow:0 8px 22px rgba(31,35,48,.12)}
        .spin{flex:0 0 auto;width:16px;height:16px;border:2px solid currentColor;border-right-color:transparent;border-radius:50%;animation:cg-op-spin .72s linear infinite}
        :host([data-state=success]) .spin,:host([data-state=error]) .spin,:host([data-state=cancelled]) .spin{display:none}
        .msg{word-break:break-word;flex:1 1 auto}
        @keyframes cg-op-spin{to{transform:rotate(360deg)}}
      `;
      const card = document.createElement("div");
      card.className = "card";
      card.innerHTML = `<div class="spin" aria-hidden="true"></div><div class="msg"></div>`;
      shadow.append(style, card);
      (document.documentElement || document.body)?.appendChild(el);
      return el;
    } catch {
      return null;
    }
  };
  const opTone = () => {
    try {
      if (document.documentElement?.classList?.contains("electron-light")) return "light";
      if (document.documentElement?.classList?.contains("electron-dark")) return "dark";
    } catch {
      /* ignore */
    }
    return "dark";
  };
  // Scheme B: optional 3rd arg `token` — stale finish/show is ignored.
  const OP_TOKEN_KEY = "__CHATGPT_TOOLS_OP_TOKEN__";
  const showOperation = (kind, message, token) => {
    try {
      // Native bootstrap may leave a plain-div toast (no shadowRoot).
      // ensureOperationHost replaces it so finish always has a proper host.
      const el = ensureOperationHost();
      if (!el?.shadowRoot) return { ok: false };
      if (token != null && token !== 0) {
        try {
          window[OP_TOKEN_KEY] = Number(token);
          el.setAttribute("data-op-token", String(token));
        } catch {
          /* ignore */
        }
      }
      el.dataset.state = "loading";
      el.dataset.tone = opTone();
      el.dataset.visible = "true";
      const text = el.shadowRoot.querySelector(".msg");
      if (text) {
        text.textContent =
          message ||
          (kind === "pause"
            ? "正在暂停皮肤…"
            : kind === "switch"
              ? "正在切换皮肤…"
              : "正在应用皮肤…");
      }
      return { ok: true, kind: kind || "apply", token: token || null };
    } catch {
      return { ok: false };
    }
  };
  const finishOperation = (state, message, token) => {
    try {
      // Stale apply (newer op owns the page) — ignore.
      if (token != null && token !== 0) {
        const live = Number(window[OP_TOKEN_KEY] || 0);
        const elChk = document.getElementById(OP_HOST_ID);
        const elTok = elChk ? Number(elChk.getAttribute("data-op-token") || 0) : 0;
        if (live && live !== Number(token) && elTok && elTok !== Number(token)) {
          return { ok: false, reason: "stale", token, live };
        }
        if (elTok && elTok !== Number(token)) {
          return { ok: false, reason: "stale-el", token, elTok };
        }
      }

      let el = document.getElementById(OP_HOST_ID);
      // Bootstrap toast has no shadowRoot — upgrade so we can show success/error.
      if (el && !el.shadowRoot) {
        try {
          el.remove();
        } catch {
          /* ignore */
        }
        el = ensureOperationHost();
      }
      if (!el?.shadowRoot) {
        return { ok: true, state: "cleared" };
      }
      if (token != null && token !== 0) {
        try {
          el.setAttribute("data-op-token", String(token));
          window[OP_TOKEN_KEY] = Number(token);
        } catch {
          /* ignore */
        }
      }
      const st = state === "error" || state === "cancelled" ? state : "success";
      el.dataset.state = st;
      el.dataset.tone = opTone();
      el.dataset.visible = "true";
      const text = el.shadowRoot.querySelector(".msg");
      if (text) {
        text.textContent =
          message || (st === "success" ? "完成" : st === "cancelled" ? "已取消" : "失败");
      }
      const hideMs = st === "error" ? 2000 : 1100;
      const tok = token != null ? Number(token) : 0;
      setTimeout(() => {
        try {
          const cur = document.getElementById(OP_HOST_ID);
          if (!cur) return;
          if (tok && Number(cur.getAttribute("data-op-token") || 0) !== tok) return;
          cur.dataset.visible = "false";
          setTimeout(() => {
            try {
              const n = document.getElementById(OP_HOST_ID);
              if (!n) return;
              if (tok && Number(n.getAttribute("data-op-token") || 0) !== tok) return;
              n.remove();
            } catch {
              /* ignore */
            }
          }, 160);
        } catch {
          /* ignore */
        }
      }, hideMs);
      return { ok: true, state: st, token: tok || null };
    } catch {
      return { ok: false };
    }
  };

  /**
   * Hot-swap skin assets without re-evaluating renderer-core.
   * Delta: { css, markers, theme, plugin, revision } — no art (use applyArt after).
   * Same revision + same CSS fingerprint short-circuits (delta hit rate).
   */
  const applySkinDelta = (delta) => {
    if (hostDisposed) return { ok: false, reason: "disposed" };
    if (!delta || typeof delta !== "object") return { ok: false, reason: "invalid-delta" };
    if (typeof delta.css !== "string") return { ok: false, reason: "missing-css" };
    if (!delta.markers || typeof delta.markers !== "object") {
      return { ok: false, reason: "missing-markers" };
    }
    if (!delta.markers.rootClass || !delta.markers.styleId || !delta.markers.stateKey) {
      return { ok: false, reason: "incomplete-markers" };
    }

    const nextRevision = delta.revision || REVISION;
    const sameMarkers =
      markers.rootClass === delta.markers.rootClass &&
      markers.styleId === delta.markers.styleId &&
      markers.stateKey === delta.markers.stateKey &&
      (markers.chromeId || "") === (delta.markers.chromeId || "");
    if (
      sameMarkers &&
      nextRevision === REVISION &&
      activeCss === delta.css &&
      !hostDisposed
    ) {
      metrics.deltaHits = (metrics.deltaHits || 0) + 1;
      window[DISABLED_KEY] = false;
      ensure({ root: true, route: true, layout: false });
      return {
        ok: true,
        mode: "delta",
        already: true,
        revision: REVISION,
        skinId: markers.id || null,
        artReady,
        deferredArt: !artReady,
        coreRevision: CORE_REVISION,
        deltaHit: true,
      };
    }

    const prevMarkers = { ...markers };
    const prevStateKey = STATE_KEY;
    const prevDisabledKey = DISABLED_KEY;
    const markersChanged =
      prevMarkers.rootClass !== delta.markers.rootClass ||
      prevMarkers.styleId !== delta.markers.styleId ||
      prevMarkers.chromeId !== delta.markers.chromeId ||
      prevMarkers.stateKey !== delta.markers.stateKey;

    // Always soft-purge residual DOM (orphan styles/classes/vars) before swap.
    // Keep current host/observers alive; only strip foreign markers + shared vars.
    softResidualCleanup({
      keepStyleId: null,
      keepChromeId: null,
      keepRootClass: null,
      keepStateKey: prevStateKey,
    });
    if (markersChanged) {
      clearPreviousSkinMarkers(prevMarkers);
    } else {
      // Same markers (reapply): still drop previous style text/chrome so ensure rebuilds.
      document.getElementById(prevMarkers.styleId)?.remove();
      document.getElementById(prevMarkers.chromeId)?.remove();
      document.documentElement?.classList.remove(...ROOT_THEME_CLASSES);
      for (const prop of rootCssProperties()) {
        document.documentElement?.style.removeProperty(prop);
      }
    }

    markers = delta.markers;
    plugin = delta.plugin && typeof delta.plugin === "object" ? delta.plugin : {};
    theme = delta.theme && typeof delta.theme === "object" ? delta.theme : {};
    activeCss = delta.css;
    REVISION = nextRevision;
    rebindMarkerIds();
    config = buildConfig(theme);
    artKey =
      typeof theme.artKey === "string"
        ? theme.artKey
        : typeof REVISION === "string"
          ? REVISION
          : null;
    profile =
      artKey && existingAnalysisCache.has(artKey)
        ? { ...defaultProfile(), ...existingAnalysisCache.get(artKey) }
        : { ...defaultProfile() };

    // Clear previous art; phase-2 will reattach.
    bindArtUrl(null);
    window[DISABLED_KEY] = false;
    if (prevDisabledKey && prevDisabledKey !== DISABLED_KEY) {
      try {
        window[prevDisabledKey] = true;
      } catch {
        /* ignore */
      }
    }

    // Drop old registry/state keys after rebind (keepAlive host stays).
    if (prevStateKey && prevStateKey !== STATE_KEY) {
      try {
        delete registry[prevStateKey];
      } catch {
        /* ignore */
      }
      try {
        delete window[prevStateKey];
      } catch {
        /* ignore */
      }
    }

    publishState();
    enterLifeMode("warm");
    ensure({ root: true, route: true, layout: true });
    metrics.deltaSwaps = (metrics.deltaSwaps || 0) + 1;

    return {
      ok: true,
      mode: "delta",
      revision: REVISION,
      skinId: markers.id || null,
      artReady: false,
      deferredArt: true,
      coreRevision: CORE_REVISION,
      deltaHit: false,
    };
  };

  const publishState = () => {
    window[STATE_KEY] = {
      ensure,
      cleanup,
      applyArt,
      applySkin: applySkinDelta,
      observer,
      rootObserver,
      resizeObserver,
      timer: steadyTimer,
      warmSteadyTimer,
      scheduler,
      resizeHandler,
      mediaQuery,
      mediaHandler,
      visibilityHandler,
      artUrl,
      artReady,
      artFingerprint,
      artDataUrl: null,
      lifeMode,
      installToken,
      profile,
      config,
      metrics,
      version: VERSION,
      revision: REVISION,
      coreRevision: CORE_REVISION,
      markers,
      theme,
      plugin,
    };
    registry[STATE_KEY] = {
      disabledKey: DISABLED_KEY,
      cleanup,
      rootClass: ROOT_CLASS,
      styleId: STYLE_ID,
      chromeId: CHROME_ID,
      artVar: ART_VAR,
    };
    // Stable host handle for cross-skin delta (does not move with stateKey).
    window[HOST_KEY] = {
      version: VERSION,
      coreRevision: CORE_REVISION,
      applySkin: applySkinDelta,
      applyArt: (url, rev) => applyArt(url, rev),
      showOperation,
      finishOperation,
      ensure,
      cleanup,
      getActive: () => ({
        stateKey: STATE_KEY,
        revision: REVISION,
        markers,
        artReady,
        lifeMode,
        skinId: markers.id || null,
        metrics: { ...(metrics || {}) },
      }),
    };
  };

  const cleanup = () => {
    const state = window[STATE_KEY];
    if (state?.installToken !== installToken) return false;
    hostDisposed = true;
    window[DISABLED_KEY] = true;
    clearWarmSteadyTimer();
    clearSteadyHeartbeat();
    clearSkinDom();
    state?.observer?.disconnect();
    state?.rootObserver?.disconnect();
    state?.resizeObserver?.disconnect();
    if (state?.timer) clearInterval(state.timer);
    if (state?.scheduler?.timeout) clearTimeout(state.scheduler.timeout);
    if (state?.scheduler?.frame != null && typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(state.scheduler.frame);
    }
    if (analysisTimer) clearTimeout(analysisTimer);
    if (state?.resizeHandler) window.removeEventListener("resize", state.resizeHandler);
    if (state?.visibilityHandler) {
      try {
        document.removeEventListener("visibilitychange", state.visibilityHandler);
      } catch {
        /* ignore */
      }
    }
    if (state?.mediaHandler && state?.mediaQuery) {
      try {
        state.mediaQuery.removeEventListener("change", state.mediaHandler);
      } catch {
        /* ignore */
      }
    }
    revokeArtUrl(state?.artUrl || artUrl);
    artUrl = null;
    artReady = false;
    artFingerprint = null;
    try {
      delete registry[STATE_KEY];
    } catch {
      /* ignore */
    }
    delete window[STATE_KEY];
    try {
      if (window[HOST_KEY]?.coreRevision === CORE_REVISION) delete window[HOST_KEY];
    } catch {
      /* ignore */
    }
    return true;
  };

  const scheduler = { timeout: null, frame: null, root: false, route: false, layout: false };
  const flushScheduledEnsure = () => {
    if (scheduler.frame !== null && typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(scheduler.frame);
    }
    if (scheduler.timeout) clearTimeout(scheduler.timeout);
    scheduler.frame = null;
    scheduler.timeout = null;
    const pending = {
      root: scheduler.root,
      route: scheduler.route,
      layout: scheduler.layout,
    };
    scheduler.root = false;
    scheduler.route = false;
    scheduler.layout = false;
    ensure(pending);
  };
  const scheduleEnsure = ({ root = false, route = true, layout = false } = {}) => {
    if (hostDisposed || lifeMode === "hidden") return;
    scheduler.root ||= root;
    scheduler.route ||= route;
    scheduler.layout ||= layout;
    if (scheduler.timeout || scheduler.frame !== null) return;
    // Steady: longer coalesce — host chat mutations must not queue micro-ensures.
    const coalesceMs = lifeMode === "steady" ? 220 : 96;
    if (typeof requestAnimationFrame === "function") {
      scheduler.frame = requestAnimationFrame(flushScheduledEnsure);
      scheduler.timeout = setTimeout(flushScheduledEnsure, coalesceMs);
    } else {
      scheduler.timeout = setTimeout(flushScheduledEnsure, Math.min(coalesceMs, 64));
    }
  };

  // Structural route observer — armed only in warm/boot (never document-wide subtree).
  observer = new MutationObserver((records) => {
    if (samplingNativeShell || hostDisposed || lifeMode === "hidden" || lifeMode === "steady") {
      return;
    }
    // Ignore pure text / deep chat stream noise when we only watch shell childList.
    if (!records?.length) return;
    wakeFromSteady("route");
  });
  rootObserver = new MutationObserver(() => {
    if (samplingNativeShell || hostDisposed || lifeMode === "hidden") return;
    cachedShellAppearance = null;
    if (lifeMode === "steady") {
      // Theme class changes only — root ensure, no warm re-arm unless shell missing.
      scheduleEnsure({ root: true, route: false, layout: false });
      return;
    }
    scheduleEnsure({ root: true, route: true });
  });
  if (typeof ResizeObserver === "function") {
    resizeObserver = new ResizeObserver(() => {
      if (hostDisposed || lifeMode === "hidden") return;
      scheduleEnsure({ route: true, layout: true });
    });
  }
  const resizeHandler = () => {
    if (hostDisposed || lifeMode === "hidden") return;
    scheduleEnsure({ route: true, layout: true });
  };

  let mediaQuery = null;
  let mediaHandler = null;
  try {
    mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    mediaHandler = () => {
      cachedShellAppearance = null;
      scheduleEnsure({ root: true, route: false, layout: false });
    };
  } catch {
    /* ignore */
  }

  const analyzeArt = () =>
    new Promise((resolve) => {
      if (typeof Image !== "function") {
        resolve(null);
        return;
      }
      const image = new Image();
      image.onload = () => {
        const startedAt = now();
        try {
          const width = 48;
          const height = Math.max(
            12,
            Math.round((width * image.naturalHeight) / Math.max(1, image.naturalWidth))
          );
          const canvas = document.createElement("canvas");
          canvas.width = width;
          canvas.height = height;
          const context = canvas.getContext?.("2d", { willReadFrequently: true });
          if (!context) throw new Error("Canvas unavailable");
          context.drawImage(image, 0, 0, width, height);
          const pixels = context.getImageData(0, 0, width, height).data;
          let count = 0;
          let totalRed = 0;
          let totalGreen = 0;
          let totalBlue = 0;
          let totalBrightness = 0;
          let focusX = 0;
          let focusY = 0;
          let focusWeight = 0;
          const accent = [0, 0, 0];
          let accentWeight = 0;
          let leftInfo = 0;
          let rightInfo = 0;
          for (let offset = 0; offset < pixels.length; offset += 4) {
            if (pixels[offset + 3] < 96) continue;
            const red = pixels[offset];
            const green = pixels[offset + 1];
            const blue = pixels[offset + 2];
            const index = offset / 4;
            const x = index % width;
            const y = Math.floor(index / width);
            const light = (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255;
            totalRed += red;
            totalGreen += green;
            totalBlue += blue;
            totalBrightness += light;
            count += 1;
            const weight = 0.15 + Math.abs(light - 0.5);
            focusX += (x / Math.max(1, width - 1)) * weight;
            focusY += (y / Math.max(1, height - 1)) * weight;
            focusWeight += weight;
            const max = Math.max(red, green, blue);
            const min = Math.min(red, green, blue);
            const saturation = max ? (max - min) / max : 0;
            const usableLight = 1 - Math.min(1, Math.abs(light - 0.46) / 0.54);
            const aWeight = saturation ** 2 * (0.15 + usableLight);
            accent[0] += red * aWeight;
            accent[1] += green * aWeight;
            accent[2] += blue * aWeight;
            accentWeight += aWeight;
            const info = saturation * (0.35 + usableLight);
            if (x < width / 2) leftInfo += info;
            else rightInfo += info;
          }
          if (!count) throw new Error("no opaque pixels");
          const averageBrightness = totalBrightness / count;
          const resolvedAccent =
            accentWeight > 1
              ? accent.map((channel) => Math.round(channel / accentWeight))
              : [
                  Math.round(totalRed / count),
                  Math.round(totalGreen / count),
                  Math.round(totalBlue / count),
                ];
          let resolvedFocusX = clamp(focusX / Math.max(focusWeight, 1e-6));
          let safeArea = "center";
          if (leftInfo > rightInfo * 1.18) safeArea = "right";
          else if (rightInfo > leftInfo * 1.18) safeArea = "left";
          if (safeArea === "left") resolvedFocusX = Math.max(0.64, resolvedFocusX);
          if (safeArea === "right") resolvedFocusX = Math.min(0.36, resolvedFocusX);
          metrics.analysisRuns += 1;
          metrics.analysisMs = Number((now() - startedAt).toFixed(3));
          resolve({
            appearance: averageBrightness >= 0.58 ? "light" : "dark",
            accent: resolvedAccent,
            focusX: resolvedFocusX,
            focusY: clamp(focusY / Math.max(focusWeight, 1e-6)),
            aspect: image.naturalWidth / Math.max(1, image.naturalHeight),
            luma: clamp(averageBrightness),
            safeArea,
          });
        } catch {
          resolve(null);
        }
      };
      image.onerror = () => resolve(null);
      if (!artUrl) {
        resolve(null);
        return;
      }
      image.src = artUrl;
    });

  publishState();

  const firstEnsureStartedAt = now();
  ensure({ layout: true });
  metrics.firstEnsureMs = Number((now() - firstEnsureStartedAt).toFixed(3));

  // Boot → warm: shell observers on main/body only; auto-enter steady after settle.
  enterLifeMode("warm");

  visibilityHandler = () => {
    if (hostDisposed) return;
    if (document.visibilityState === "hidden") {
      enterLifeMode("hidden");
      return;
    }
    // Visible again: one ensure + warm settle, then steady.
    enterLifeMode("warm");
    ensure({ root: true, route: true, layout: true });
  };
  try {
    document.addEventListener("visibilitychange", visibilityHandler);
  } catch {
    visibilityHandler = null;
  }

  if (window[STATE_KEY]) {
    window[STATE_KEY].observer = observer;
    window[STATE_KEY].rootObserver = rootObserver;
    window[STATE_KEY].resizeObserver = resizeObserver;
    window[STATE_KEY].visibilityHandler = visibilityHandler;
    window[STATE_KEY].timer = steadyTimer;
    window[STATE_KEY].lifeMode = lifeMode;
  }

  window.addEventListener("resize", resizeHandler, { passive: true });
  if (mediaHandler && mediaQuery) {
    mediaQuery.addEventListener("change", mediaHandler);
  }

  // Analysis runs when art is present (monolithic inject or after applyArt).
  if (artReady) scheduleArtAnalysis();

  return {
    installed: true,
    mode: "full",
    version: VERSION,
    revision: REVISION,
    coreRevision: CORE_REVISION,
    adaptive: true,
    artReady,
    deferredArt: !artReady,
    lifeMode,
    metrics,
  };
})(
  __SKIN_CSS_JSON__,
  __SKIN_ART_JSON__,
  __SKIN_THEME_JSON__,
  __SKIN_MARKERS_JSON__,
  __SKIN_PLUGIN_JSON__,
  __SKIN_REVISION_JSON__
);
