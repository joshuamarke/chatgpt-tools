/**
 * ChatGPT Tools — Codex desktop model whitelist unlock.
 *
 * Codex desktop gates the model picker with a Statsig / in-app whitelist.
 * Non-whitelisted slugs (DeepSeek / Claude / Gemini / Grok / …) either hide or
 * collapse to a single「自定义 / Custom」row. This script patches data layers
 * (Statsig / app-server / Response.json / React state) only — never rewrites
 * arbitrary DOM text labels (that mis-hits other controls).
 *
 * Host sets window.__CGT_MODEL_UNLOCK_MODELS__ = string[]
 * Optional: window.__CGT_MODEL_UNLOCK_META__ = { [slug]: { displayName, description } }
 */
(function chatgptToolsModelUnlock() {
  "use strict";

  function unique(list) {
    const out = [];
    const seen = new Set();
    for (const raw of list || []) {
      const s = String(raw || "").trim();
      if (!s || seen.has(s)) continue;
      seen.add(s);
      out.push(s);
    }
    return out;
  }

  function readSeedModels() {
    return unique(
      (Array.isArray(window.__CGT_MODEL_UNLOCK_MODELS__) &&
        window.__CGT_MODEL_UNLOCK_MODELS__) ||
        []
    );
  }

  function readMeta() {
    const m = window.__CGT_MODEL_UNLOCK_META__;
    return m && typeof m === "object" ? m : {};
  }

  // Script revision — bump when patch heuristics change. Older installs must
  // fall through and re-bind hooks (early-return would keep the buggy closures).
  const SCRIPT_VERSION = 7;

  function sameModelList(a, b) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    const as = a.slice().sort();
    const bs = b.slice().sort();
    for (let i = 0; i < as.length; i++) if (as[i] !== bs[i]) return false;
    return true;
  }

  function hooksHealthy() {
    return (
      typeof window.__cgtPatchJsonPayload === "function" &&
      typeof window.__cgtModelBurstRefresh === "function" &&
      !!window.__cgtModelPoll &&
      !!window.__cgtModelMo
    );
  }

  // Re-inject short-circuit ONLY when fully healthy.
  // After official clear / SPA remount / missing hooks → fall through to full install.
  if (
    window.__chatgptToolsModelUnlock === "1" &&
    window.__cgtModelUnlockVersion === SCRIPT_VERSION &&
    !window.__cgtModelUnlockCleared &&
    hooksHealthy()
  ) {
    const nextModels = readSeedModels();
    const nextMeta = readMeta();
    const prev = window.__cgtModelNames || [];
    const unchanged = sameModelList(prev, nextModels);
    window.__cgtModelNames = nextModels;
    window.__cgtModelMeta = nextMeta;
    // Only burst when the list actually changed (new third-party models).
    if (!unchanged && nextModels.length) {
      try {
        if (typeof window.__cgtModelBurstRefresh === "function") {
          window.__cgtModelBurstRefresh(2500);
        }
      } catch (_) {}
    }
    return {
      ok: true,
      already: true,
      skippedSameModels: unchanged,
      version: SCRIPT_VERSION,
      models: nextModels.length,
      names: nextModels.slice(0, 24),
    };
  }

  // Full install / rebind
  window.__chatgptToolsModelUnlock = "1";
  window.__cgtModelUnlockCleared = false;
  window.__cgtModelNames = readSeedModels();
  window.__cgtModelMeta = readMeta();
  window.__cgtModelListRequestIds = window.__cgtModelListRequestIds || new Set();
  window.__cgtModelPatchFailures = window.__cgtModelPatchFailures || [];
  window.__cgtModelUnlockVersion = SCRIPT_VERSION;

  function modelNames() {
    return window.__cgtModelNames || [];
  }

  function metaFor(name) {
    const meta = window.__cgtModelMeta || {};
    const exact = meta[name];
    if (exact && typeof exact === "object") return exact;
    const key = Object.keys(meta).find((k) => k.toLowerCase() === String(name).toLowerCase());
    return key ? meta[key] : null;
  }

  function displayOf(name) {
    const m = metaFor(name);
    const d = m && (m.displayName || m.display_name || m.name);
    return (d && String(d).trim()) || name;
  }

  function descriptionOf(name) {
    const m = metaFor(name);
    const d = m && (m.description || m.desc);
    return (d && String(d).trim()) || displayOf(name);
  }

  /** Full model descriptor — never label as generic "Custom". */
  function descriptor(name) {
    return {
      model: name,
      id: name,
      slug: name,
      name: name,
      displayName: displayOf(name),
      display_name: displayOf(name),
      title: displayOf(name),
      label: displayOf(name),
      description: descriptionOf(name),
      hidden: false,
      isDefault: modelNames()[0] === name,
      is_default: modelNames()[0] === name,
      supportedInApi: true,
      supported_in_api: true,
      visibility: "list",
      defaultReasoningEffort: "medium",
      default_reasoning_effort: "medium",
      supportedReasoningEfforts: ["low", "medium", "high", "xhigh"].map((reasoningEffort) => ({
        reasoningEffort,
        description: reasoningEffort + " effort",
      })),
      supported_reasoning_efforts: ["low", "medium", "high", "xhigh"].map((effort) => ({
        effort,
        description: effort + " effort",
      })),
    };
  }

  /**
   * modelArrayLooksPatchable requires `typeof item.model === "string"`.
   * Thread/recent-chat rows often ALSO carry a selected `model` field — those
   * must still be rejected, or unlock descriptors get pushed into the sidebar
   * as fake threads (`local:grok-4.5`, `local:gpt-5.4`, …).
   */
  function isThreadOrSessionLike(item) {
    if (!item || typeof item !== "object") return false;
    const keys = [
      "thread_id",
      "threadId",
      "session_id",
      "sessionId",
      "rollout_path",
      "rolloutPath",
      "cwd",
      "thread_name",
      "threadName",
      "display_title",
      "displayTitle",
      "has_user_event",
      "hasUserEvent",
      "first_user_message",
      "firstUserMessage",
      "git_branch",
      "gitBranch",
      "source_kind",
      "sourceKind",
      "observation_sequence",
      "missing_candidate",
      "model_provider",
      "modelProvider",
      "archived",
      "preview",
      "recency_at",
      "recencyAt",
    ];
    if (keys.some((k) => k in item && item[k] != null && item[k] !== "")) return true;

    const id = typeof item.id === "string" ? item.id.trim() : "";
    // Real Codex thread ids (ulid-ish) or host-qualified local:<uuid>
    if (/^019[a-f0-9-]{20,}$/i.test(id)) return true;
    if (/^local:019[a-f0-9-]{20,}$/i.test(id)) return true;
    // Already-polluted fake rows from older unlock builds
    if (/^local:(gpt-|grok-|claude-|deepseek|o[34])/i.test(id)) return true;

    // Conversation row: id + human title that is not the model slug itself
    const title =
      (typeof item.title === "string" && item.title.trim()) ||
      (typeof item.name === "string" && item.name.trim()) ||
      "";
    const model =
      (typeof item.model === "string" && item.model.trim()) ||
      (typeof item.slug === "string" && item.slug.trim()) ||
      "";
    if (id && title && (!model || title !== model)) return true;
    if (
      id &&
      !model &&
      ("updated_at" in item ||
        "updatedAt" in item ||
        "created_at" in item ||
        "createdAt" in item ||
        "archived" in item)
    ) {
      return true;
    }
    return false;
  }

  /**
   * Only objects with a string `model` field count, and never
   * session/thread rows that merely reference a selected model.
   */
  function looksModelItem(item) {
    if (!item || typeof item !== "object" || isThreadOrSessionLike(item)) return false;
    return typeof item.model === "string" && item.model.trim().length > 0;
  }

  function looksModelArray(value, allowEmpty) {
    if (!Array.isArray(value)) return false;
    if (value.length === 0) return !!allowEmpty;
    return value.every(looksModelItem);
  }

  function modelKey(item) {
    if (!item || typeof item !== "object" || isThreadOrSessionLike(item)) return "";
    if (typeof item.model === "string" && item.model.trim()) return item.model.trim();
    return "";
  }

  function looksStringArray(value) {
    return Array.isArray(value) && value.length > 0 && value.every((item) => typeof item === "string");
  }

  function ensureNameInStringArray(arr) {
    if (!Array.isArray(arr)) return false;
    const names = modelNames();
    if (!names.length) return false;
    let changed = false;
    names.forEach((n) => {
      if (!arr.includes(n)) {
        arr.push(n);
        changed = true;
      }
    });
    return changed;
  }

  function ensureNameInSet(set) {
    if (!(set instanceof Set)) return false;
    const names = modelNames();
    if (!names.length) return false;
    let changed = false;
    names.forEach((n) => {
      if (!set.has(n)) {
        set.add(n);
        changed = true;
      }
    });
    return changed;
  }

  function patchModelArray(models, allowEmpty) {
    if (!looksModelArray(models, allowEmpty)) return false;
    const names = modelNames();
    if (!names.length) return false;
    let changed = false;
    const existing = new Map();
    models.forEach((item) => {
      const key = modelKey(item);
      if (key) existing.set(key, item);
      if (names.includes(key)) {
        if (item.hidden !== false) {
          item.hidden = false;
          changed = true;
        }
        // Force real display name (avoid「自定义」)
        const dn = displayOf(key);
        if (item.displayName !== dn) {
          item.displayName = dn;
          changed = true;
        }
        if (item.display_name !== dn) {
          item.display_name = dn;
          changed = true;
        }
        if (item.title !== dn) {
          item.title = dn;
          changed = true;
        }
        if (item.label !== dn) {
          item.label = dn;
          changed = true;
        }
        if (!item.model) {
          item.model = key;
          changed = true;
        }
        if (!item.slug) {
          item.slug = key;
          changed = true;
        }
      }
    });
    names.forEach((n) => {
      if (!existing.has(n)) {
        models.push(descriptor(n));
        changed = true;
      }
    });
    return changed;
  }

  function patchNameArray(models) {
    if (!looksStringArray(models) && !(Array.isArray(models) && models.length === 0)) {
      // allow empty string arrays only when we know it's a model name list
      if (!Array.isArray(models) || !models.every((x) => typeof x === "string")) return false;
    }
    return ensureNameInStringArray(models);
  }

  function looksModelListContext(value) {
    if (!value || typeof value !== "object") return false;
    return (
      "defaultModel" in value ||
      "default_model" in value ||
      "availableModels" in value ||
      "available_models" in value ||
      "allowedModels" in value ||
      "allowed_models" in value ||
      "modelWhitelist" in value ||
      "model_whitelist" in value ||
      "supportedModels" in value ||
      "supported_models" in value ||
      // Explicit model collection keys only — do not treat generic `items`/`list`
      // as models unless the parent already looks like a model payload.
      (Array.isArray(value.models) && looksModelArray(value.models, true)) ||
      (Array.isArray(value.data) && looksModelArray(value.data, true))
    );
  }

  function patchContainer(value) {
    if (!value || typeof value !== "object") return false;
    let changed = false;

    // Mirror patchModelContainer keys only — never generic items/list
    // (those are frequently recent-conversation collections).
    if (
      patchModelArray(
        value.models,
        "defaultModel" in value ||
          "availableModels" in value ||
          "available_models" in value
      )
    )
      changed = true;
    if (patchNameArray(value.models)) changed = true;
    if (patchModelArray(value.data)) changed = true;
    if (patchModelArray(value.result)) changed = true;
    if (value.pages && value.pages[0] && patchModelArray(value.pages[0].data)) changed = true;
    if (value.result && patchModelArray(value.result.data)) changed = true;
    if (value.result && patchModelArray(value.result.models)) changed = true;
    if (value.message && value.message.result && patchModelArray(value.message.result.data))
      changed = true;
    if (value.message && value.message.result && patchModelArray(value.message.result.models))
      changed = true;

    if (ensureNameInSet(value.availableModels)) changed = true;
    if (ensureNameInSet(value.available_models)) changed = true;
    if (ensureNameInStringArray(value.availableModels)) changed = true;
    if (ensureNameInStringArray(value.available_models)) changed = true;
    if (ensureNameInStringArray(value.allowedModels)) changed = true;
    if (ensureNameInStringArray(value.allowed_models)) changed = true;
    if (ensureNameInStringArray(value.modelWhitelist)) changed = true;
    if (ensureNameInStringArray(value.model_whitelist)) changed = true;
    if (ensureNameInStringArray(value.supportedModels)) changed = true;
    if (ensureNameInStringArray(value.supported_models)) changed = true;

    // Remove our models from hidden lists
    const names = modelNames();
    for (const key of ["hiddenModels", "hidden_models", "disabledModels", "disabled_models"]) {
      if (Array.isArray(value[key])) {
        const before = value[key].length;
        value[key] = value[key].filter((n) => {
          const s = typeof n === "string" ? n : modelKey(n);
          return !names.includes(s);
        });
        if (value[key].length !== before) changed = true;
      }
    }

    // defaultModel object / string
    if (value.defaultModel == null && names.length > 0) {
      value.defaultModel = descriptor(names[0]);
      changed = true;
    } else if (typeof value.defaultModel === "string" && names.includes(value.defaultModel)) {
      // keep string form but ensure display paths exist
    } else if (value.defaultModel && typeof value.defaultModel === "object") {
      const k = modelKey(value.defaultModel);
      if (names.includes(k)) {
        const dn = displayOf(k);
        if (value.defaultModel.displayName !== dn) {
          value.defaultModel.displayName = dn;
          value.defaultModel.display_name = dn;
          value.defaultModel.hidden = false;
          changed = true;
        }
      }
    }

    return changed;
  }

  const SKIP_GRAPH_KEYS = new Set([
    "ownerDocument",
    "parentElement",
    "parentNode",
    "children",
    "childNodes",
    "style",
    "__proto__",
    // Conversation / session graphs — never walk these for model inject.
    "threads",
    "thread",
    "sessions",
    "session",
    "conversations",
    "conversation",
    "history",
    "messages",
    "turns",
    "timeline",
    "recents",
    "recentThreads",
    "recent_threads",
    "threadList",
    "thread_list",
    "localThreadCatalog",
    "local_thread_catalog",
    "inbox",
    "inboxItems",
    "inbox_items",
  ]);

  function patchGraph(root, visited, depth) {
    if (!root || typeof root !== "object" || visited.has(root) || depth > 6) return false;
    visited.add(root);
    let changed = patchContainer(root);
    if (
      root instanceof Element ||
      root === window ||
      root === document ||
      root === document.body ||
      root === document.documentElement
    ) {
      return changed;
    }
    // Skip huge binary-ish blobs
    if (ArrayBuffer.isView && ArrayBuffer.isView(root)) return changed;
    let keys;
    try {
      keys = Object.keys(root);
    } catch (_) {
      return changed;
    }
    for (const key of keys) {
      if (SKIP_GRAPH_KEYS.has(key)) continue;
      // Skip keys that clearly name conversation/history collections.
      if (/thread|session|conversation|history|message|inbox|timeline|recent/i.test(key)) {
        // Still allow *model* nested under those only when key itself is model-ish.
        if (!/model/i.test(key)) continue;
      }
      let value;
      try {
        value = root[key];
      } catch (_) {
        continue;
      }
      if (value && typeof value === "object" && patchGraph(value, visited, depth + 1))
        changed = true;
    }
    return changed;
  }

  async function patchJsonPayload(payload) {
    if (!modelNames().length) return payload;
    if (!payload || typeof payload !== "object") return payload;
    try {
      // Shallow + model-context only. Full deep walks used to push model
      // descriptors into recent-conversation JSON (arrays of {id, title, …}).
      patchContainer(payload);
      if (looksModelListContext(payload) || looksModelArray(payload, true)) {
        patchGraph(payload, new WeakSet(), 0);
      } else if (payload.result && typeof payload.result === "object") {
        patchContainer(payload.result);
        if (looksModelListContext(payload.result) || looksModelArray(payload.result, true)) {
          patchGraph(payload.result, new WeakSet(), 0);
        }
      }
    } catch (e) {
      window.__cgtModelPatchFailures.push(String((e && e.stack) || e));
    }
    return payload;
  }

  // Live entry points so version upgrades rebind without stacking wrappers forever.
  window.__cgtPatchJsonPayload = patchJsonPayload;
  window.__cgtPatchModelContainer = patchContainer;
  window.__cgtPatchModelArray = patchModelArray;

  // ── Response.json ──────────────────────────────────────────────
  if (!window.__cgtModelJsonPatch || window.__cgtModelJsonPatchVersion !== SCRIPT_VERSION) {
    const origJson =
      window.__cgtModelJsonOriginal ||
      Response.prototype.json;
    if (typeof origJson === "function") {
      window.__cgtModelJsonOriginal = origJson;
      window.__cgtModelJsonPatch = true;
      window.__cgtModelJsonPatchVersion = SCRIPT_VERSION;
      Response.prototype.json = async function cgtPatchedJson(...args) {
        const payload = await window.__cgtModelJsonOriginal.apply(this, args);
        const patch = window.__cgtPatchJsonPayload;
        return typeof patch === "function" ? await patch(payload) : payload;
      };
    }
  }

  // ── Statsig: patch every dynamic config that carries model lists ─
  function patchStatsigConfig(config) {
    if (!config || typeof config !== "object") return config;
    const names = modelNames();
    if (!names.length) return config;

    const value = config.value;
    if (!value || typeof value !== "object") return config;

    let changed = false;
    const next = { ...value };

    for (const key of [
      "available_models",
      "availableModels",
      "allowed_models",
      "allowedModels",
      "model_whitelist",
      "modelWhitelist",
      "supported_models",
      "supportedModels",
      "models",
    ]) {
      if (Array.isArray(value[key])) {
        const arr = value[key].slice();
        if (arr.every((x) => typeof x === "string")) {
          names.forEach((n) => {
            if (!arr.includes(n)) {
              arr.push(n);
              changed = true;
            }
          });
          next[key] = arr;
        } else if (looksModelArray(arr, true)) {
          if (patchModelArray(arr, true)) {
            next[key] = arr;
            changed = true;
          }
        }
      }
    }

    // hidden lists
    for (const key of ["hidden_models", "hiddenModels"]) {
      if (Array.isArray(value[key])) {
        const filtered = value[key].filter((n) => {
          const s = typeof n === "string" ? n : modelKey(n);
          return !names.includes(s);
        });
        if (filtered.length !== value[key].length) {
          next[key] = filtered;
          changed = true;
        }
      }
    }

    if (names[0] && (value.default_model || value.defaultModel)) {
      // keep official default if set; only fill when empty
    } else if (names[0] && !value.default_model && !value.defaultModel) {
      next.default_model = names[0];
      next.defaultModel = names[0];
      changed = true;
    }

    if (!changed) return config;
    try {
      config.value = next;
      return config;
    } catch (_) {
      return { ...config, value: next };
    }
  }

  function statsigClients() {
    const root = window.__STATSIG__ || globalThis.__STATSIG__;
    if (!root || typeof root !== "object") return [];
    const clients = [
      root.firstInstance,
      typeof root.instance === "function" ? root.instance() : null,
      root.client,
    ];
    if (root.instances && typeof root.instances === "object") {
      clients.push(...Object.values(root.instances));
    }
    // Some builds stash clients on window
    if (window.__statsig_client) clients.push(window.__statsig_client);
    return clients.filter((c, i, arr) => c && typeof c === "object" && arr.indexOf(c) === i);
  }

  function patchStatsig() {
    statsigClients().forEach((client) => {
      if (typeof client.getDynamicConfig === "function" && !client.__cgtModelWhitelistPatched) {
        const orig = client.getDynamicConfig.bind(client);
        client.getDynamicConfig = function cgtGetDynamicConfig(name, options) {
          return patchStatsigConfig(orig(name, options));
        };
        client.__cgtModelWhitelistPatched = true;
      }
      // Known Codex model whitelist config ids + brute force common getters
      if (typeof client.getDynamicConfig === "function") {
        for (const id of ["107580212", "model_whitelist", "codex_models", "available_models"]) {
          try {
            patchStatsigConfig(client.getDynamicConfig(id, { disableExposureLog: true }));
          } catch (_) {}
        }
      }
      // getConfig alias
      if (typeof client.getConfig === "function" && !client.__cgtGetConfigPatched) {
        const origCfg = client.getConfig.bind(client);
        client.getConfig = function cgtGetConfig(name, options) {
          return patchStatsigConfig(origCfg(name, options));
        };
        client.__cgtGetConfigPatched = true;
      }
    });
  }

  // ── App-server / MCP model list ────────────────────────────────
  if (!window.__cgtModelMessagePatch) {
    window.__cgtModelMessagePatch = true;
    const origDispatch = window.dispatchEvent;
    window.dispatchEvent = function cgtDispatch(event) {
      try {
        const detail = event && event.detail;
        const request = detail && detail.request;
        if (
          event &&
          event.type === "codex-message-from-view" &&
          detail &&
          detail.type === "mcp-request" &&
          request &&
          (request.method === "model/list" ||
            request.method === "list-models-for-host" ||
            request.method === "models/list")
        ) {
          request.params = { ...(request.params || {}), includeHidden: true };
          if (request.id != null) {
            window.__cgtModelListRequestIds.add(String(request.id));
          }
        }
        if (event && event.type === "message") {
          patchMcpData(event.data);
        }
      } catch (e) {
        window.__cgtModelPatchFailures.push(String((e && e.stack) || e));
      }
      return origDispatch.call(this, event);
    };
    window.addEventListener(
      "message",
      (event) => {
        try {
          patchMcpData(event && event.data);
        } catch (e) {
          window.__cgtModelPatchFailures.push(String((e && e.stack) || e));
        }
      },
      true
    );
  }

  function patchMcpData(data) {
    if (!data || typeof data !== "object") return false;
    // Only patch mcp-responses whose request id was recorded for model/list.
    if (data.type === "mcp-response") {
      const message = data.message || data.response;
      const requestId = message && message.id != null ? String(message.id) : "";
      if (
        window.__cgtModelListRequestIds.size > 0 &&
        (!requestId || !window.__cgtModelListRequestIds.has(requestId))
      ) {
        return false;
      }
      if (requestId) window.__cgtModelListRequestIds.delete(requestId);
      return (
        patchContainer(data) ||
        patchContainer(message) ||
        patchContainer(message && message.result) ||
        patchContainer(message && message.result && message.result.data)
      );
    }
    return false;
  }

  // ── Patch any object with sendRequest (app-server clients) ─────
  function patchSendRequestClient(client) {
    if (!client || typeof client.sendRequest !== "function") return false;
    if (client.__cgtModelSendRequestPatched) return true;
    const original = client.sendRequest.bind(client);
    client.sendRequest = async function cgtSendRequest(method, params, options) {
      let nextParams = params;
      const m = String(method || "");
      if (
        m === "model/list" ||
        m === "list-models-for-host" ||
        m === "models/list" ||
        m.endsWith("list-models-for-host")
      ) {
        nextParams = { ...(params || {}), includeHidden: true };
      }
      // Nested: send-cli-request-for-host
      if (m === "send-cli-request-for-host" && params && typeof params === "object") {
        const inner = String(params.method || "");
        if (inner === "model/list" || inner === "list-models-for-host") {
          nextParams = {
            ...params,
            params: { ...(params.params || {}), includeHidden: true },
          };
        }
      }
      const result = await original(method, nextParams, options);
      try {
        const methodName =
          m === "send-cli-request-for-host" && params && params.method
            ? String(params.method)
            : m;
        // Only patches list-models-for-host app-server results.
        if (methodName === "list-models-for-host") {
          if (Array.isArray(result)) patchModelArray(result, true);
          if (result && typeof result === "object") {
            if (Array.isArray(result.data)) patchModelArray(result.data, true);
            if (Array.isArray(result.models)) patchModelArray(result.models, true);
            patchContainer(result);
            patchGraph(result, new WeakSet(), 0);
          }
        }
      } catch (e) {
        window.__cgtModelPatchFailures.push(String((e && e.stack) || e));
      }
      return result;
    };
    client.__cgtModelSendRequestPatched = true;
    return true;
  }

  function scanAndPatchSendRequestRoots() {
    const roots = [
      window,
      window.__CODEX__,
      window.__codex__,
      window.codex,
      window.__APP__,
      window.__app__,
    ].filter(Boolean);
    let patched = 0;
    const visited = new WeakSet();
    function walk(obj, depth) {
      if (!obj || typeof obj !== "object" || depth > 3 || visited.has(obj)) return;
      visited.add(obj);
      try {
        if (patchSendRequestClient(obj)) patched += 1;
      } catch (_) {}
      let keys;
      try {
        keys = Object.keys(obj);
      } catch (_) {
        return;
      }
      for (const k of keys.slice(0, 40)) {
        let v;
        try {
          v = obj[k];
        } catch (_) {
          continue;
        }
        if (v && typeof v === "object") walk(v, depth + 1);
      }
    }
    roots.forEach((r) => walk(r, 0));
    return patched;
  }

  // ── React fiber ────────────────────────────────────────────────
  function fiberKeys(el) {
    return Object.keys(el).filter(
      (k) =>
        k.startsWith("__reactFiber") ||
        k.startsWith("__reactInternalInstance") ||
        k.startsWith("__reactProps")
    );
  }

  /**
   * Skip workspace chrome / sidebar so fiber walks never touch the
   * recent-conversation list (that was the main pollution path).
   */
  function isSidebarOrWorkspaceChrome(node) {
    if (!node || node.nodeType !== 1) return false;
    try {
      if (
        node.closest &&
        node.closest(
          '[data-app-action-sidebar-section-heading="Chats"], [data-app-action-sidebar-section-heading="Projects"], [data-app-action-sidebar-thread-id], [data-app-action-sidebar-project-row], [data-app-action-sidebar-project-id], [data-thread-title]'
        )
      ) {
        return true;
      }
    } catch (_) {}
    return false;
  }

  function patchReactNodes() {
    const selector =
      "[role='menu'], [role='dialog'], [role='listbox'], [role='combobox'], [data-radix-popper-content-wrapper]";
    // Do NOT walk document.body — that reaches the Chats sidebar fiber tree.
    return [...document.querySelectorAll(selector)].filter(
      (node) => node && !isSidebarOrWorkspaceChrome(node)
    );
  }

  function patchReact() {
    const visited = new WeakSet();
    let changed = false;
    for (const node of patchReactNodes().slice(0, 220)) {
      for (const key of fiberKeys(node)) {
        try {
          if (patchGraph(node[key], visited, 0)) changed = true;
        } catch (_) {}
      }
    }
    return changed;
  }

  // Note: do NOT rewrite visible DOM text (e.g.「自定义」→ displayName).
  // That mis-hits unrelated controls. Display names come only from data-layer
  // patches (Statsig / model list / React state / Response.json).

  function refreshPass() {
    if (!modelNames().length) return false;
    let changed = false;
    try {
      patchStatsig();
      scanAndPatchSendRequestRoots();
      if (patchReact()) changed = true;
    } catch (e) {
      window.__cgtModelPatchFailures.push(String((e && e.stack) || e));
    }
    return changed;
  }

  function burstRefresh(durationMs) {
    // Short, low-frequency burst — avoid 10s × 120ms hammering after every inject.
    const until = Date.now() + (durationMs || 2500);
    window.__cgtModelBurstUntil = Math.max(window.__cgtModelBurstUntil || 0, until);
    if (window.__cgtModelBurstTimer) return;
    const tick = () => {
      window.__cgtModelBurstTimer = 0;
      if (!modelNames().length) return;
      refreshPass();
      if (Date.now() < (window.__cgtModelBurstUntil || 0)) {
        window.__cgtModelBurstTimer = window.setTimeout(tick, 400);
      }
    };
    tick();
  }

  // Expose for same-version re-inject early return (function declarations hoist).
  window.__cgtModelBurstRefresh = burstRefresh;

  // MutationObserver: only model menus/dialogs — never soft-refresh on every DOM
  // mutation (that re-walked fibers and re-polluted recent chats).
  if (!window.__cgtModelMo || window.__cgtModelMoVersion !== SCRIPT_VERSION) {
    try {
      if (window.__cgtModelMo && typeof window.__cgtModelMo.disconnect === "function") {
        window.__cgtModelMo.disconnect();
      }
      window.__cgtModelMoVersion = SCRIPT_VERSION;
      window.__cgtModelMo = new MutationObserver((mutations) => {
        if (!modelNames().length) return;
        const selector =
          "[role='menu'], [role='dialog'], [role='listbox'], [data-radix-popper-content-wrapper]";
        const hit = mutations.some((m) =>
          [...m.addedNodes].some((node) => {
            if (!node || node.nodeType !== 1 || isSidebarOrWorkspaceChrome(node)) return false;
            return (
              (node.matches && node.matches(selector)) ||
              (node.querySelector && node.querySelector(selector))
            );
          })
        );
        if (hit) burstRefresh(2500);
      });
      if (document.documentElement) {
        window.__cgtModelMo.observe(document.documentElement, {
          childList: true,
          subtree: true,
        });
      }
    } catch (_) {}
  }

  // Soft poll: Statsig + app-server hooks only — not full React body walks.
  // 8s is enough for SPA remounts; empty model list = no-op (official cleared).
  if (!window.__cgtModelPoll || window.__cgtModelPollVersion !== SCRIPT_VERSION) {
    if (window.__cgtModelPoll) clearInterval(window.__cgtModelPoll);
    window.__cgtModelPollVersion = SCRIPT_VERSION;
    window.__cgtModelPoll = setInterval(() => {
      try {
        if (!modelNames().length) return;
        patchStatsig();
        scanAndPatchSendRequestRoots();
      } catch (_) {}
    }, 8000);
  }

  // Initial install + short burst (only when we have models to unlock)
  if (modelNames().length) {
    patchStatsig();
    scanAndPatchSendRequestRoots();
    burstRefresh(2500);
  }

  return {
    ok: true,
    already: false,
    version: window.__cgtModelUnlockVersion,
    models: modelNames().length,
    names: modelNames().slice(0, 24),
    statsigClients: statsigClients().length,
  };
})();
