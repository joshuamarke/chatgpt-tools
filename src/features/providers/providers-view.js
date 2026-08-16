/**
 * Providers management main view (Codex / Grok tabs).
 * Depends on: window.providerAPI, showToast, showConfirm (from app.js).
 *
 * Flow: 预设 → 填 Key → 仅保存 / 保存并启用 → 列表启用。
 */
(function () {
  const SOURCE_CODEX = "codex";
  const SOURCE_GROK = "grok";
  const SOURCE_STORAGE_KEY = "chatgpt-tools.providers.app";

  /** @type {"codex"|"grok"} */
  let app = readStoredApp();
  /** @type {any | null} */
  let listPayload = null;
  /** @type {any[]} */
  let presets = [];
  let loading = false;
  let loadSeq = 0;
  let bound = false;
  /** @type {string | null} */
  let editingId = null;
  let formBusy = false;
  /** @type {string} */
  let formCategory = "custom";
  /** @type {boolean} */
  let formIsOfficial = false;
  /** When true, save uses advanced config.toml as source of truth. */
  let advancedDirty = false;
  /** @type {Array<{id:string,ownedBy?:string}>} */
  let fetchedModels = [];
  let probeBusy = false;
  let importBusy = false;
  let fetchModelsSeq = 0;

  /**
   * Common model context-window presets (tokens).
   * Matched by substring / regex against model id (supports vendor prefixes like `x-ai/grok-4.5`).
   * Longer / more specific patterns should come first.
   */
  const MODEL_CONTEXT_PRESETS = [
    { re: /gpt-5\.6-sol|gpt-5-6-sol/i, window: 372000, label: "GPT-5.6-sol · 372K" },
    { re: /gpt-5\.5|gpt-5-5/i, window: 400000, label: "GPT-5.5 · 400K" },
    { re: /gpt-5\.1|gpt-5-1|gpt-5\.2|gpt-5-2/i, window: 400000, label: "GPT-5.x · 400K" },
    { re: /gpt-4\.1|gpt-4-1/i, window: 1047576, label: "GPT-4.1 · 1M" },
    { re: /gpt-4o|gpt-4-turbo/i, window: 128000, label: "GPT-4o · 128K" },
    { re: /o3-pro|o3-mini|o4-mini|\bo3\b|\bo1\b/i, window: 200000, label: "OpenAI o-series · 200K" },
    { re: /grok-4\.5|grok-4-5/i, window: 5000000, label: "Grok 4.5 · 5M" },
    { re: /grok-4|grok-3|grok-2/i, window: 131072, label: "Grok 3/4 · 128K" },
    { re: /claude-(opus|sonnet|haiku)-4|claude-4/i, window: 200000, label: "Claude 4 · 200K" },
    { re: /claude-3\.5|claude-3-5|claude-3\.7|claude-3-7/i, window: 200000, label: "Claude 3.5/3.7 · 200K" },
    { re: /gemini-2\.5-pro|gemini-2-5-pro/i, window: 1048576, label: "Gemini 2.5 Pro · 1M" },
    { re: /gemini-2\.5|gemini-2-5|gemini-2\.0|gemini-1\.5/i, window: 1048576, label: "Gemini 1.5/2.x · 1M" },
    { re: /deepseek-reasoner|deepseek-r1/i, window: 128000, label: "DeepSeek R1 · 128K" },
    { re: /deepseek-chat|deepseek-v3|deepseek/i, window: 128000, label: "DeepSeek · 128K" },
    { re: /kimi-k2|moonshot|kimi/i, window: 256000, label: "Kimi · 256K" },
    { re: /qwen2\.5|qwen3|qwen/i, window: 131072, label: "Qwen · 128K" },
  ];

  /** Quick-pick values shown in catalog context datalist. */
  const CONTEXT_WINDOW_QUICK_PICKS = [
    { value: 128000, label: "128K" },
    { value: 200000, label: "200K" },
    { value: 256000, label: "256K" },
    { value: 372000, label: "372K" },
    { value: 400000, label: "400K" },
    { value: 500000, label: "500K" },
    { value: 1048576, label: "1M" },
    { value: 2000000, label: "2M" },
    { value: 5000000, label: "5M" },
  ];

  function $(id) {
    return document.getElementById(id);
  }

  /**
   * Guess context window for a model id from built-in presets.
   * @param {string} modelId
   * @returns {number|null}
   */
  function guessContextWindow(modelId) {
    const id = String(modelId || "").trim();
    if (!id) return null;
    const bare = id.includes("/") ? id.slice(id.lastIndexOf("/") + 1) : id;
    for (const p of MODEL_CONTEXT_PRESETS) {
      if (p.re.test(id) || p.re.test(bare)) return p.window;
    }
    return null;
  }

  /**
   * Apply guessed context when the field is empty (or force overwrite).
   * @param {string} modelId
   * @param {string|number|null|undefined} current
   * @param {{force?: boolean}} [opts]
   * @returns {number|null}
   */
  function resolveContextWindow(modelId, current, opts) {
    const force = !!opts?.force;
    const n = Number.parseInt(String(current ?? "").trim(), 10);
    if (!force && Number.isFinite(n) && n > 0) return n;
    return guessContextWindow(modelId);
  }

  function readStoredApp() {
    try {
      const v = localStorage.getItem(SOURCE_STORAGE_KEY);
      if (v === SOURCE_GROK || v === SOURCE_CODEX) return v;
    } catch {
      /* ignore */
    }
    return SOURCE_CODEX;
  }

  function persistApp() {
    try {
      localStorage.setItem(SOURCE_STORAGE_KEY, app);
    } catch {
      /* ignore */
    }
  }

  function isGrok() {
    return app === SOURCE_GROK;
  }

  function escapeHtml(s) {
    return String(s ?? "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function toast(msg, type) {
    if (typeof window.showToast === "function") {
      window.showToast(msg, type || "");
    } else {
      console.log("[providers]", type || "info", msg);
    }
  }

  async function confirm(opts) {
    if (typeof window.showConfirm === "function") return window.showConfirm(opts);
    return window.confirm(typeof opts === "string" ? opts : opts?.message || "");
  }

  function isFormOpen() {
    const modal = $("providerFormModal");
    return !!(modal && !modal.hidden && modal.classList.contains("show"));
  }

  /** Match app.js modal convention: hidden + .show */
  function showModal(el) {
    if (!el) return;
    el.hidden = false;
    el.classList.add("show");
  }

  function hideModal(el) {
    if (!el) return;
    el.hidden = true;
    el.classList.remove("show");
  }

  function setTabsActive() {
    const codex = $("provTabCodex");
    const grok = $("provTabGrok");
    if (codex) {
      const on = app === SOURCE_CODEX;
      codex.classList.toggle("is-active", on);
      codex.setAttribute("aria-selected", on ? "true" : "false");
    }
    if (grok) {
      const on = app === SOURCE_GROK;
      grok.classList.toggle("is-active", on);
      grok.setAttribute("aria-selected", on ? "true" : "false");
    }
  }

  function updateLead() {
    const lead = $("provLead");
    if (!lead) return;
    lead.textContent = isGrok()
      ? "管理 Grok 供应商：添加渠道、启用切换，可选本地路由与故障转移。"
      : "管理 Codex 供应商：添加渠道、启用切换，可选本地路由与故障转移。";
    syncCodexAuthRow();
    syncRouteToggle();
  }

  function syncCodexAuthRow() {
    const row = $("provCodexAuthRow");
    const box = $("provPreserveCodexAuth");
    if (!row) return;
    const show = !isGrok();
    row.hidden = !show;
    if (!show || !box) return;
    const preserved =
      listPayload?.preserveCodexOfficialAuth !== undefined
        ? !!listPayload.preserveCodexOfficialAuth
        : true;
    box.checked = preserved;
  }

  function updateLiveBar() {
    const bar = $("provLiveBar");
    if (!bar) return;
    const live = listPayload?.liveStatus;
    const currentId = listPayload?.current || "";
    const current = (listPayload?.providers || []).find((p) => p.id === currentId);
    const currentName = (current?.name || currentId || "").trim();

    // Always show the bar so the active provider name is visible even before live probe.
    bar.hidden = false;
    const title = $("provLiveTitle");
    const detail = $("provLiveDetail");
    const meta = $("provLiveMeta");
    const dot = $("provLiveDot");

    const titleText = currentName
      ? `当前启用 · ${currentName}`
      : "当前启用 · 未选择";
    if (title) {
      title.textContent = titleText;
      title.title = currentName
        ? `当前启用的供应商：${currentName}`
        : "尚未在本工具中启用供应商（与本机 live 配置无关）";
    }

    if (!live) {
      if (dot) dot.dataset.state = "off";
      bar.dataset.state = "off";
      if (detail) {
        detail.textContent = "正在读取本机配置状态…";
        detail.title = "";
      }
      if (meta) meta.innerHTML = "";
      return;
    }

    const matches = !!live.currentMatchesLive;
    const exists = !!live.configExists;
    const detailCode = live.detailCode || "";
    // unlinked = live exists but no archive is marked enabled (not the same as drift).
    let state = "warn";
    if (!exists) {
      state = "off";
    } else if (matches) {
      state = "ok";
    } else if (detailCode === "unlinked" || !currentId) {
      state = "off";
    } else {
      state = "warn";
    }
    bar.dataset.state = state;
    if (dot) dot.dataset.state = state;

    if (detail) {
      // Keep drift / path summary under the active provider title.
      const summary = (live.summary || "").trim();
      let detailText = summary || "—";
      if (!exists) {
        detailText = summary || "尚未检测到本机配置文件";
      } else if (!matches && summary) {
        detailText = summary;
      } else if (matches && summary) {
        detailText = summary;
      }
      detail.textContent = detailText;
      detail.title = detailText;
    }

    if (meta) {
      const base = (live.baseUrl || "").trim();
      const parts = [];
      if (base) {
        const safe = escapeHtml(base);
        parts.push(`<span class="prov-live-chip" title="${safe}">${safe}</span>`);
      }
      const code = live.detailCode || "";
      if (code === "route_half" || code === "route_desync") {
        parts.push(
          `<button type="button" class="chip-btn prov-live-fix-btn" id="btnProvRepairRoute" title="重新投影本地路由">修复路由</button>`
        );
      }
      meta.innerHTML = parts.join("");
      $("btnProvRepairRoute")?.addEventListener("click", () => {
        onRepairRoute().catch((err) => toast(err?.message || String(err), "error"));
      });
    }
  }

  function categoryLabel(cat) {
    if (cat === "official") return "官方";
    if (cat === "third_party") return "第三方";
    if (cat === "custom") return "自定义";
    if (cat === "aggregator") return "聚合";
    return cat || "自定义";
  }

  /** Same ChatGPT / Codex mark used on the providers + sessions tabs. */
  function logoSvgChatgpt() {
    return `<svg viewBox="0 0 180 180" aria-hidden="true" focusable="false"><path fill="currentColor" d="M101.228 164.247C96.2776 164.247 91.5751 163.307 87.1201 161.426C82.6651 159.545 78.7051 156.921 75.2401 153.555C71.4781 154.842 67.5676 155.486 63.5086 155.486C56.8756 155.486 50.7376 153.852 45.0946 150.585C39.4516 147.318 34.8976 142.863 31.4326 137.22C28.0666 131.577 26.3836 125.291 26.3836 118.361C26.3836 115.49 26.7796 112.371 27.5716 109.005C23.6116 105.342 20.5426 101.135 18.3646 96.3828C16.1866 91.5318 15.0976 86.4828 15.0976 81.2358C15.0976 75.8898 16.2361 70.7418 18.5131 65.7918C20.7901 60.8418 23.9581 56.5848 28.0171 53.0208C32.1751 49.3578 36.9766 46.8333 42.4216 45.4473C43.5106 39.8043 45.7876 34.7553 49.2526 30.3003C52.8166 25.7463 57.1726 22.1823 62.3206 19.6083C67.4686 17.0343 72.9631 15.7473 78.8041 15.7473C83.7541 15.7473 88.4566 16.6878 92.9116 18.5688C97.3666 20.4498 101.327 23.0733 104.792 26.4393C108.554 25.1523 112.464 24.5088 116.523 24.5088C123.156 24.5088 129.294 26.1423 134.937 29.4093C140.58 32.6763 145.085 37.1313 148.451 42.7743C151.916 48.4173 153.648 54.7038 153.648 61.6338C153.648 64.5048 153.252 67.6233 152.46 70.9893C156.42 74.6523 159.489 78.9093 161.667 83.7603C163.845 88.5123 164.934 93.5118 164.934 98.7588C164.934 104.105 163.796 109.253 161.519 114.203C159.242 119.153 156.024 123.459 151.866 127.122C147.807 130.686 143.055 133.161 137.61 134.547C136.521 140.19 134.195 145.239 130.631 149.694C127.166 154.248 122.859 157.812 117.711 160.386C112.563 162.96 107.069 164.247 101.228 164.247ZM64.5481 145.685C69.4981 145.685 73.8046 144.645 77.4676 142.566L105.386 126.528C106.376 125.835 106.871 124.895 106.871 123.707V110.936L70.9336 131.577C68.7556 132.864 66.5776 132.864 64.3996 131.577L36.3331 115.391C36.3331 115.688 36.2836 116.034 36.1846 116.43C36.1846 116.826 36.1846 117.42 36.1846 118.212C36.1846 123.261 37.3726 127.914 39.7486 132.171C42.2236 136.329 45.6391 139.596 49.9951 141.972C54.3511 144.447 59.2021 145.685 64.5481 145.685ZM66.0331 121.479C66.6271 121.776 67.1716 121.925 67.6666 121.925C68.1616 121.925 68.6566 121.776 69.1516 121.479L80.2891 115.094L44.5006 94.3038C42.3226 93.0168 41.2336 91.0863 41.2336 88.5123V56.2878C36.2836 58.4658 32.3236 61.8318 29.3536 66.3858C26.3836 70.8408 24.8986 75.7908 24.8986 81.2358C24.8986 86.0868 26.1361 90.7398 28.6111 95.1948C31.0861 99.6498 34.3036 103.016 38.2636 105.293L66.0331 121.479ZM101.228 154.446C106.475 154.446 111.227 153.258 115.484 150.882C119.741 148.506 123.107 145.239 125.582 141.081C128.057 136.923 129.294 132.27 129.294 127.122V95.0463C129.294 93.8583 128.799 92.9673 127.809 92.3733L116.523 85.8393V127.271C116.523 129.845 115.434 131.775 113.256 133.062L85.1896 149.249C90.0406 152.714 95.3866 154.446 101.228 154.446ZM106.871 100.095V79.8993L90.09 70.3953L73.1611 79.8993V100.095L90.09 109.599L106.871 100.095ZM63.5086 52.7238C63.5086 50.1498 64.5976 48.2193 66.7756 46.9323L94.8421 30.7458C89.9911 27.2808 84.6451 25.5483 78.8041 25.5483C73.5571 25.5483 68.8051 26.7363 64.5481 29.1123C60.2911 31.4883 56.9251 34.7553 54.4501 38.9133C52.0741 43.0713 50.8861 47.7243 50.8861 52.8723V84.7998C50.8861 85.9878 51.3811 86.9283 52.3711 87.6213L63.5086 94.1553V52.7238ZM138.947 123.707C143.897 121.529 147.807 118.163 150.678 113.609C153.648 109.055 155.133 104.105 155.133 98.7588C155.133 93.9078 153.896 89.2548 151.421 84.7998C148.946 80.3448 145.728 76.9788 141.768 74.7018L113.999 58.6638C113.405 58.2678 112.86 58.1193 112.365 58.2183C111.87 58.2183 111.375 58.3668 110.88 58.6638L99.7426 64.9008L135.68 85.8393C136.769 86.4333 137.561 87.2253 138.056 88.2153C138.65 89.1063 138.947 90.1953 138.947 91.4823V123.707ZM109.098 48.2688C111.276 46.8828 113.454 46.8828 115.632 48.2688L143.847 64.7523C143.847 64.0593 143.847 63.1683 143.847 62.0793C143.847 57.3273 142.659 52.8228 140.283 48.5658C138.006 44.2098 134.69 40.7448 130.334 38.1708C126.077 35.5968 121.127 34.3098 115.484 34.3098C110.534 34.3098 106.227 35.3493 102.564 37.4283L74.6461 53.4663C73.6561 54.1593 73.1611 55.0998 73.1611 56.2878V69.0588L109.098 48.2688Z"/></svg>`;
  }

  /** Same Grok / xAI mark used on the providers + sessions tabs. */
  function logoSvgGrok() {
    return `<svg viewBox="0 0 34 33" aria-hidden="true" focusable="false"><path fill="currentColor" d="M13.2371 21.0407L24.3186 12.8506C24.8619 12.4491 25.6384 12.6057 25.8973 13.2294C27.2597 16.5185 26.651 20.4712 23.9403 23.1851C21.2297 25.8989 17.4581 26.4941 14.0108 25.1386L10.2449 26.8843C15.6463 30.5806 22.2053 29.6665 26.304 25.5601C29.5551 22.3051 30.562 17.8683 29.6205 13.8673L29.629 13.8758C28.2637 7.99809 29.9647 5.64871 33.449 0.844576C33.5314 0.730667 33.6139 0.616757 33.6964 0.5L29.1113 5.09055V5.07631L13.2343 21.0436"/><path fill="currentColor" d="M10.9503 23.0313C7.07343 19.3235 7.74185 13.5853 11.0498 10.2763C13.4959 7.82722 17.5036 6.82767 21.0021 8.2971L24.7595 6.55998C24.0826 6.07017 23.215 5.54334 22.2195 5.17313C17.7198 3.31926 12.3326 4.24192 8.67479 7.90126C5.15635 11.4239 4.0499 16.8403 5.94992 21.4622C7.36924 24.9165 5.04257 27.3598 2.69884 29.826C1.86829 30.7002 1.0349 31.5745 0.36364 32.5L10.9474 23.0341"/></svg>`;
  }

  /** Generic briefcase glyph for custom / third-party provider cards. */
  function providerIconSvgGeneric() {
    return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 7.5h15v3.2a2.3 2.3 0 0 1-2.3 2.3H6.8A2.3 2.3 0 0 1 4.5 10.7V7.5z"/><path d="M7 13v3.5a1.5 1.5 0 0 0 1.5 1.5h7A1.5 1.5 0 0 0 17 16.5V13"/><path d="M9.5 7.5V5.8A1.3 1.3 0 0 1 10.8 4.5h2.4A1.3 1.3 0 0 1 14.5 5.8V7.5"/></svg>`;
  }

  function providerIconSvg(p) {
    if (p?.category === "official") {
      return isGrok() ? logoSvgGrok() : logoSvgChatgpt();
    }
    return providerIconSvgGeneric();
  }

  function providerIconClass(p) {
    if (p.category === "official") {
      // Brand logo: filled mark; current state still gets a subtle ring via card.
      const brand = isGrok() ? "ico-logo-grok" : "ico-logo-chatgpt";
      return `prov-card-ico ico-brand-logo ${brand}${p.isCurrent ? " ico-current" : ""}`;
    }
    if (p.isCurrent) return "prov-card-ico ico-current";
    if (p.ready === false) return "prov-card-ico ico-warn";
    if (isGrok()) return "prov-card-ico ico-grok";
    return "prov-card-ico";
  }

  function renderList() {
    const list = $("provList");
    const empty = $("provEmpty");
    if (!list || !empty) return;

    const providers = listPayload?.providers || [];
    if (loading && !providers.length) {
      list.hidden = true;
      empty.hidden = false;
      empty.dataset.state = "loading";
      empty.innerHTML =
        `<div class="session-empty-title">正在加载供应商…</div>` +
        `<div class="session-empty-detail">读取本机配置档案</div>`;
      return;
    }

    if (!providers.length) {
      list.hidden = true;
      empty.hidden = false;
      empty.dataset.state = "empty";
      empty.innerHTML =
        `<div class="session-empty-title">暂无供应商</div>` +
        `<div class="session-empty-detail">点击工具栏「添加供应商」选择渠道预设，或使用「从本机配置导入」。</div>`;
      return;
    }

    empty.hidden = true;
    list.hidden = false;
    const routing = !!listPayload?.takeoverEnabled;
    list.innerHTML = providers
      .map((p) => {
        const active = p.isCurrent ? "is-current" : "";
        const routingCls = routing && p.isCurrent ? "is-routing" : "";
        const badge = p.isCurrent
          ? `<span class="prov-badge prov-badge-on">${routing ? "路由中" : "使用中"}</span>`
          : "";
        const cat = `<span class="prov-badge">${escapeHtml(categoryLabel(p.category))}</span>`;
        const readyBadge =
          p.category === "official" || p.ready
            ? ""
            : `<span class="prov-badge prov-badge-warn" title="缺少 Base URL 或 API Key">未就绪</span>`;
        const detailCode = listPayload?.liveStatus?.detailCode || "";
        let driftBadge = "";
        if (p.isCurrent && p.matchesLive === false) {
          if (detailCode === "route_half" || detailCode === "route_desync") {
            driftBadge = `<span class="prov-badge prov-badge-broken" title="${escapeHtml(listPayload?.liveStatus?.summary || "")}">路由异常</span>`;
          } else {
            driftBadge = `<span class="prov-badge prov-badge-warn" title="与本机正在使用的配置不一致">本机漂移</span>`;
          }
        }
        // P1/P2 and circuit health only matter under local routing (same as「加入队列」).
        // Queue can still be curated in 路由设置 when FO auto-switch is off.
        const foBadge =
          routing && p.failoverPriority
            ? `<span class="prov-badge prov-badge-fo" title="故障转移优先级">P${escapeHtml(String(p.failoverPriority))}</span>`
            : "";
        const healthBadge =
          routing && p.health && p.health !== "unknown"
            ? `<span class="prov-badge prov-badge-health-${escapeHtml(p.health)}" title="健康状态">${escapeHtml(
                p.health === "healthy"
                  ? "健康"
                  : p.health === "open"
                    ? "熔断"
                    : "降级"
              )}</span>`
            : "";

        const chips = [];
        if (p.model) {
          chips.push(
            `<span class="prov-meta-chip" title="模型">模型 ${escapeHtml(p.model)}</span>`
          );
        }
        if (p.wireApi) {
          chips.push(
            `<span class="prov-meta-chip" title="wire_api">wire ${escapeHtml(p.wireApi)}</span>`
          );
        }
        if (p.baseUrl) {
          chips.push(
            `<span class="prov-meta-chip is-url" title="${escapeHtml(p.baseUrl)}">${escapeHtml(p.baseUrl)}</span>`
          );
        }
        if (p.apiKeyPreview) {
          chips.push(
            `<span class="prov-meta-chip" title="API Key">Key ${escapeHtml(p.apiKeyPreview)}</span>`
          );
        }
        const meta = chips.length
          ? `<div class="prov-card-meta">${chips.join("")}</div>`
          : "";
        const notes = p.notes
          ? `<div class="prov-card-notes" title="${escapeHtml(p.notes)}">${escapeHtml(p.notes)}</div>`
          : "";
        const canSwitch =
          !p.isCurrent &&
          (p.category === "official" || p.ready !== false) &&
          !(routing && isGrok() && p.category === "official");
        const switchTitle = routing
          ? canSwitch
            ? "热切换上游（本地路由）"
            : "当前不可启用"
          : canSwitch
            ? "写入本机配置并启用"
            : "请先补全 Base URL 与 API Key";
        const switchBtn = p.isCurrent
          ? ""
          : `<button type="button" class="chip-btn chip-primary prov-act" data-act="switch" data-id="${escapeHtml(p.id)}" ${canSwitch ? "" : "disabled"} title="${switchTitle}">启用</button>`;
        // Failover queue actions only make sense under local routing; hide otherwise.
        // Queue can still be curated in 路由设置 even when FO auto-switch is off.
        const foToggle =
          routing &&
          p.category !== "official" &&
          !(isGrok() && p.category === "official")
            ? `<button type="button" class="chip-btn prov-act" data-act="failover" data-id="${escapeHtml(p.id)}" title="${p.inFailoverQueue ? "移出故障转移队列" : "加入故障转移队列"}">${p.inFailoverQueue ? "队列中" : "加入队列"}</button>`
            : "";
        const delBtn =
          p.category === "official"
            ? ""
            : `<button type="button" class="chip-btn chip-danger prov-act" data-act="delete" data-id="${escapeHtml(p.id)}" ${p.isCurrent ? "disabled" : ""}>删除</button>`;
        return `
<article class="prov-card ${active} ${routingCls}" data-id="${escapeHtml(p.id)}">
  <div class="${providerIconClass(p)}" aria-hidden="true">${providerIconSvg(p)}</div>
  <div class="prov-card-main">
    <div class="prov-card-title-row">
      <h3 class="prov-card-name" title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</h3>
      ${badge}${cat}${readyBadge}${driftBadge}${foBadge}${healthBadge}
    </div>
    ${meta}
    ${notes}
  </div>
  <div class="prov-card-actions">
    ${switchBtn}
    ${foToggle}
    <button type="button" class="chip-btn prov-act" data-act="edit" data-id="${escapeHtml(p.id)}">编辑</button>
    ${delBtn}
  </div>
</article>`;
      })
      .join("");
  }

  function syncRouteToggle() {
    const btn = $("provRouteToggle");
    if (!btn) return;
    const on = !!listPayload?.takeoverEnabled;
    btn.setAttribute("aria-checked", on ? "true" : "false");
    btn.classList.toggle("is-on", on);
    const host = $("provRouteListenHost");
    const port = $("provRouteListenPort");
    const log = $("provRouteLogging");
    const fo = $("provRouteAutoFailover");
    const retries = $("provRouteMaxRetries");
    const egress = $("provRouteEgressProxy");
    const proxy = listPayload?.proxy || {};
    if (host && document.activeElement !== host) host.value = proxy.listenAddress || "127.0.0.1";
    if (port && document.activeElement !== port) port.value = String(proxy.listenPort || 18964);
    if (log) log.checked = proxy.enableLogging !== false;
    // Default ON: missing / undefined → checked; only explicit false unchecks
    if (fo) fo.checked = listPayload?.autoFailoverEnabled !== false;
    if (egress && document.activeElement !== egress) {
      egress.value = proxy.egressProxy || "";
    }
    // max retries from last settings load if present
    if (retries && window.__provAppProxySettings?.maxRetries != null) {
      retries.value = String(window.__provAppProxySettings.maxRetries);
    }
  }

  async function loadAppProxySettings() {
    try {
      if (!window.providerAPI?.getAppProxySettings) return;
      window.__provAppProxySettings = await window.providerAPI.getAppProxySettings(app);
      const retries = $("provRouteMaxRetries");
      if (retries && window.__provAppProxySettings?.maxRetries != null) {
        retries.value = String(window.__provAppProxySettings.maxRetries);
      }
      const fo = $("provRouteAutoFailover");
      if (fo) {
        const v = window.__provAppProxySettings?.autoFailoverEnabled;
        fo.checked = v !== false;
      }
    } catch (err) {
      console.warn("loadAppProxySettings", err);
    }
  }

  async function onToggleRoute() {
    const btn = $("provRouteToggle");
    const currentlyOn = btn?.getAttribute("aria-checked") === "true";
    const next = !currentlyOn;
    if (next) {
      const ok = await confirm({
        title: "开启本地路由",
        message: isGrok()
          ? "将为 Grok 开启本机代理，切换供应商时可热生效。关闭后恢复直连。重启Grok 生效。"
          : "将为 Codex 开启本机代理，切换供应商时可热生效；官方登录不会被清除。关闭后恢复直连。重启Codex生效。",
        confirmText: "开启本地路由",
        variant: "primary",
      });
      if (!ok) return;
    }
    try {
      const res = await window.providerAPI.setTakeover(app, next);
      const warnings = (res?.warnings || [])
        .filter(Boolean)
        .filter((w) => !/注入|白名单|catalog|CDP|auth\.json|config\.toml|model_providers/i.test(w));
      toast(
        warnings[0] || (next ? "本地路由已开启" : "本地路由已关闭"),
        "ok"
      );
      await loadList();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function onSaveRouteSettings() {
    try {
      const host = ($("provRouteListenHost")?.value || "127.0.0.1").trim();
      const port = Number($("provRouteListenPort")?.value || 18964);
      const enableLogging = !!$("provRouteLogging")?.checked;
      const egressProxy = ($("provRouteEgressProxy")?.value || "").trim();
      if (!Number.isFinite(port) || port < 1024 || port > 65535) {
        toast("端口需在 1024–65535", "error");
        return;
      }
      const prevProxy = listPayload?.proxy || {};
      await window.providerAPI.updateProxyConfig({
        listenAddress: host,
        listenPort: port,
        enableLogging,
        logRetentionDays: Number(prevProxy.logRetentionDays) || 7,
        egressProxy,
      });
      const maxRetries = Number($("provRouteMaxRetries")?.value ?? 3);
      const cur = window.__provAppProxySettings || {};
      await window.providerAPI.updateAppProxySettings({
        app,
        takeoverEnabled: !!listPayload?.takeoverEnabled,
        autoFailoverEnabled: !!$("provRouteAutoFailover")?.checked,
        maxRetries: Number.isFinite(maxRetries) ? maxRetries : 3,
        circuit: cur.circuit || {
          failureThreshold: 4,
          successThreshold: 2,
          timeoutSeconds: 60,
          errorRateThreshold: 0.6,
          minRequests: 10,
        },
        streamingFirstByteTimeout: cur.streamingFirstByteTimeout || 60,
        streamingIdleTimeout: cur.streamingIdleTimeout || 120,
        nonStreamingTimeout: cur.nonStreamingTimeout || 600,
      });
      const fo = !!$("provRouteAutoFailover")?.checked;
      const prevFo = listPayload?.autoFailoverEnabled !== false;
      if (fo !== prevFo) {
        await window.providerAPI.setAutoFailover(app, fo);
      }
      toast("路由设置已保存", "ok");
      closeRouteModal();
      await loadList();
      await loadAppProxySettings();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  function openRouteModal() {
    const modal = $("provRouteModal");
    if (!modal) return;
    syncRouteToggle();
    setProbeStatus("provRoutePortStatus", "", "");
    loadAppProxySettings().catch(() => {});
    refreshFoQueuePanel().catch(() => {});
    showModal(modal);
  }

  function closeRouteModal() {
    hideModal($("provRouteModal"));
  }

  // ── Request logs modal ─────────────────────────────────────────────────
  /** @type {{ page: number, pageSize: number, total: number, selectedId: string|null }} */
  const logsState = {
    page: 0,
    pageSize: 20,
    total: 0,
    selectedId: null,
    loading: false,
    searchTimer: null,
  };

  function logsFilters() {
    const appFilter = ($("provLogsFilterApp")?.value || "all").trim();
    const status = ($("provLogsFilterStatus")?.value || "all").trim();
    const q = ($("provLogsFilterQ")?.value || "").trim();
    return {
      app: appFilter === "all" ? null : appFilter,
      statusClass: status === "all" ? null : status,
      q: q || null,
      page: logsState.page,
      pageSize: logsState.pageSize,
    };
  }

  function formatLogTime(ts) {
    if (!ts) return "—";
    try {
      const d = new Date(Number(ts) * 1000);
      if (Number.isNaN(d.getTime())) return "—";
      const pad = (n) => String(n).padStart(2, "0");
      return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
    } catch {
      return "—";
    }
  }

  function formatLatency(ms) {
    const n = Number(ms) || 0;
    if (n < 1000) return `${n} ms`;
    return `${(n / 1000).toFixed(n < 10000 ? 1 : 0)} s`;
  }

  /** Compact token count: 266556 → "266.6k", 1200 → "1.2k", 999 → "999". */
  function formatTokenCount(n) {
    const v = Number(n) || 0;
    if (v < 1000) return String(v);
    const k = v / 1000;
    // 1 decimal under 100k, integer at/above 100k (e.g. 266.6k, 1.2M later)
    if (k < 1000) {
      return `${k.toFixed(k >= 100 ? 0 : 1)}k`;
    }
    const m = k / 1000;
    return `${m.toFixed(m >= 100 ? 0 : 1)}M`;
  }

  /** Compact in/out token display for the log table. */
  function formatTokens(r) {
    const inn = Number(r?.inputTokens) || 0;
    const out = Number(r?.outputTokens) || 0;
    if (!inn && !out) return "—";
    return `${formatTokenCount(inn)}→${formatTokenCount(out)}`;
  }

  function formatFirstToken(ms) {
    if (ms == null || ms === "") return "—";
    const n = Number(ms);
    if (!Number.isFinite(n) || n < 0) return "—";
    return formatLatency(n);
  }

  /** Same client label in list + detail (aligned with AppKind::display_name). */
  function formatLogApp(app) {
    const a = String(app || "").trim().toLowerCase();
    if (a === "grok" || a === "grokbuild" || a === "grok-build") return "Grok Build";
    if (a === "codex" || a === "chatgpt" || a === "openai") return "Codex";
    return app ? String(app) : "—";
  }

  /** Status text color only (no badge pill): 2xx green, 4xx amber, 5xx/err red. */
  function statusCodeClass(code, hasErr) {
    const c = Number(code) || 0;
    if (c >= 200 && c < 300) return "prov-logs-status is-ok";
    if (c >= 400 && c < 500) return "prov-logs-status is-warn";
    if (c >= 500 || c === 0 || hasErr) return "prov-logs-status is-error";
    return "prov-logs-status";
  }

  async function openLogsModal() {
    const modal = $("provLogsModal");
    if (!modal) return;
    logsState.page = 0;
    hideDetail();
    // Default filter to current app tab
    const appSel = $("provLogsFilterApp");
    if (appSel && !appSel.dataset.userTouched) {
      appSel.value = app || "all";
    }
    showModal(modal);
    try {
      if (window.providerAPI?.getLogRetentionDays) {
        const days = await window.providerAPI.getLogRetentionDays();
        const input = $("provLogsRetentionDays");
        if (input) input.value = String(days || 7);
      }
    } catch {
      /* ignore */
    }
    await loadLogs();
  }

  function closeLogsModal() {
    hideDetail();
    hideModal($("provLogsModal"));
  }

  async function loadLogs() {
    if (!window.providerAPI?.listRequestLogs) {
      toast("日志接口不可用", "error");
      return;
    }
    if (logsState.loading) return;
    logsState.loading = true;
    const tbody = $("provLogsTbody");
    const totalEl = $("provLogsTotal");
    try {
      const page = await window.providerAPI.listRequestLogs(logsFilters());
      logsState.total = Number(page?.total) || 0;
      logsState.page = Number(page?.page) || 0;
      logsState.pageSize = Number(page?.pageSize) || 20;
      if (totalEl) totalEl.textContent = `共 ${logsState.total} 条`;

      const kicker = $("provLogsKicker");
      if (kicker) {
        const on = page?.loggingEnabled !== false;
        kicker.textContent = on
          ? "本地路由转发记录 · 只记录本地路由转发，未开启本地路由的不参与统计。"
          : "当前未启用请求日志（路由设置），仅显示历史记录";
      }
      if (page?.retentionDays != null) {
        const input = $("provLogsRetentionDays");
        if (input && document.activeElement !== input) {
          input.value = String(page.retentionDays);
        }
      }

      const rows = Array.isArray(page?.data) ? page.data : [];
      if (!tbody) return;
      if (!rows.length) {
        tbody.innerHTML = `<tr class="prov-logs-empty-row"><td colspan="7">
          <div class="session-empty">
            <div class="session-empty-title">暂无请求日志</div>
            <div class="session-empty-detail">开启本地路由并在路由设置中勾选「请求日志」后，经代理的请求会显示在这里。</div>
          </div>
        </td></tr>`;
        hideDetail();
      } else {
        tbody.innerHTML = rows
          .map((r) => {
            const sel = r.id === logsState.selectedId ? " is-selected" : "";
            const sc = statusCodeClass(r.statusCode, !!r.errorMessage);
            const stream = r.isStreaming
              ? ` <span class="prov-badge prov-badge-fo" title="流式请求">流式</span>`
              : "";
            const appLabel = formatLogApp(r.app);
            const latTitle =
              r.firstTokenMs != null
                ? `总耗时 ${formatLatency(r.latencyMs)} · 首字节 ${formatFirstToken(r.firstTokenMs)}`
                : `总耗时 ${formatLatency(r.latencyMs)}`;
            const tok = formatTokens(r);
            const innExact = Number(r.inputTokens) || 0;
            const outExact = Number(r.outputTokens) || 0;
            const tokTitle =
              innExact || outExact
                ? `输入 ${innExact.toLocaleString()} · 输出 ${outExact.toLocaleString()}`
                : "无 token 统计";
            return `<tr class="prov-logs-row${sel}" data-log-id="${escapeHtml(r.id)}" tabindex="0">
              <td class="tabular-nums">${escapeHtml(formatLogTime(r.createdAt))}</td>
              <td>${escapeHtml(appLabel)}</td>
              <td title="${escapeHtml(r.providerName || r.providerId || "")}">${escapeHtml(r.providerName || r.providerId || "—")}</td>
              <td title="${escapeHtml(r.model || "")}"><span class="prov-meta-chip" title="模型">${escapeHtml(r.model || "—")}</span>${stream}</td>
              <td class="tabular-nums" title="${escapeHtml(latTitle)}">${escapeHtml(formatLatency(r.latencyMs))}</td>
              <td class="tabular-nums" title="${escapeHtml(tokTitle)}">${escapeHtml(tok)}</td>
              <td><span class="${sc}">${r.statusCode || "—"}</span></td>
            </tr>`;
          })
          .join("");
        // Keep detail if selection still present
        if (logsState.selectedId && rows.some((r) => r.id === logsState.selectedId)) {
          const hit = rows.find((r) => r.id === logsState.selectedId);
          if (hit) renderDetail(hit);
        } else {
          hideDetail();
        }
      }
      renderPager();
    } catch (err) {
      toast(err?.message || String(err), "error");
    } finally {
      logsState.loading = false;
    }
  }

  function renderPager() {
    const pager = $("provLogsPager");
    if (!pager) return;
    const totalPages = Math.max(1, Math.ceil(logsState.total / logsState.pageSize) || 1);
    const show = logsState.total > logsState.pageSize;
    pager.hidden = !show;
    const label = $("provLogsPageLabel");
    if (label) label.textContent = `第 ${logsState.page + 1} / ${totalPages} 页`;
    const prev = $("btnProvLogsPrev");
    const next = $("btnProvLogsNext");
    if (prev) prev.disabled = logsState.page <= 0;
    if (next) next.disabled = logsState.page + 1 >= totalPages;
  }

  function clearLogRowSelection() {
    $("provLogsTbody")
      ?.querySelectorAll(".prov-logs-row.is-selected")
      .forEach((tr) => tr.classList.remove("is-selected"));
  }

  function hideDetail() {
    const el = $("provLogsDetail");
    if (el) {
      el.hidden = true;
      el.setAttribute("hidden", "");
    }
    logsState.selectedId = null;
    clearLogRowSelection();
  }

  function isDetailVisible() {
    const el = $("provLogsDetail");
    return !!(el && !el.hidden);
  }

  function renderDetail(r) {
    const el = $("provLogsDetail");
    const grid = $("provLogsDetailGrid");
    if (!el || !grid || !r) return;
    el.hidden = false;
    el.removeAttribute("hidden");
    const statusCls = statusCodeClass(r.statusCode, !!r.errorMessage);
    const rows = [
      ["请求 ID", escapeHtml(r.id)],
      ["时间", escapeHtml(formatLogTime(r.createdAt))],
      ["应用", escapeHtml(formatLogApp(r.app))],
      ["供应商", escapeHtml(`${r.providerName || "—"} (${r.providerId || "—"})`)],
      ["模型", escapeHtml(r.model || "—")],
      ["方法", escapeHtml(r.method || "—")],
      ["路径", escapeHtml(r.path || "—")],
      [
        "状态",
        `<span class="${statusCls}">${escapeHtml(String(r.statusCode ?? "—"))}</span>`,
      ],
      ["耗时", escapeHtml(formatLatency(r.latencyMs))],
      ["首字节", escapeHtml(formatFirstToken(r.firstTokenMs))],
      [
        "输入 Token",
        escapeHtml(formatTokenCount(r.inputTokens)),
      ],
      [
        "输出 Token",
        escapeHtml(formatTokenCount(r.outputTokens)),
      ],
      ["流式", escapeHtml(r.isStreaming ? "是" : "否")],
      ["尝试次数", escapeHtml(String(r.attempt ?? 1))],
      ["错误", escapeHtml(r.errorMessage || "—")],
    ];
    grid.innerHTML = rows
      .map(([k, v]) => `<div class="ov-meta-row"><dt>${escapeHtml(k)}</dt><dd>${v}</dd></div>`)
      .join("");
  }

  function setLogRowSelected(id) {
    $("provLogsTbody")
      ?.querySelectorAll(".prov-logs-row")
      .forEach((tr) => {
        tr.classList.toggle("is-selected", tr.getAttribute("data-log-id") === id);
      });
  }

  /**
   * Open detail for a row; click the same selected row again to collapse.
   */
  async function onSelectLogRow(id) {
    if (!id) return;
    // Toggle off when re-clicking the open row
    if (logsState.selectedId === id && isDetailVisible()) {
      hideDetail();
      return;
    }
    logsState.selectedId = id;
    setLogRowSelected(id);
    try {
      let detail = null;
      if (window.providerAPI?.getRequestLog) {
        detail = await window.providerAPI.getRequestLog(id);
      }
      // Selection may have changed while awaiting
      if (logsState.selectedId !== id) return;
      if (detail) {
        renderDetail(detail);
      } else {
        hideDetail();
        toast("未找到该条日志", "error");
      }
    } catch (err) {
      if (logsState.selectedId === id) hideDetail();
      toast(err?.message || String(err), "error");
    }
  }

  async function onClearLogs() {
    const ok = await confirm({
      title: "清空请求日志",
      message: "将删除全部本地请求日志，且不可恢复。是否继续？",
      confirmText: "清空",
      variant: "danger",
    });
    if (!ok) return;
    try {
      const res = await window.providerAPI.clearRequestLogs();
      const n = Number(res?.deleted) || 0;
      toast(n ? `已删除 ${n} 条日志` : "日志已清空", "ok");
      logsState.page = 0;
      hideDetail();
      await loadLogs();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function onSaveRetention() {
    const raw = Number($("provLogsRetentionDays")?.value || 7);
    const days = Math.min(365, Math.max(1, Number.isFinite(raw) ? Math.round(raw) : 7));
    try {
      const saved = await window.providerAPI.setLogRetentionDays(days);
      const input = $("provLogsRetentionDays");
      if (input) input.value = String(saved || days);
      // Keep GlobalProxyConfig in sync when possible
      try {
        const cfg = await window.providerAPI.getProxyConfig();
        if (cfg && Number(cfg.logRetentionDays) !== Number(saved)) {
          await window.providerAPI.updateProxyConfig({
            ...cfg,
            logRetentionDays: saved || days,
          });
        }
      } catch {
        /* optional */
      }
      toast(`已设置保留 ${saved || days} 天`, "ok");
      logsState.page = 0;
      await loadLogs();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  function setPortStatus(ok, message) {
    // Reuse provider form probe status styles (is-ok / is-error)
    setProbeStatus("provRoutePortStatus", message || "", ok ? "ok" : "error");
  }

  async function onCheckPort() {
    const host = ($("provRouteListenHost")?.value || "127.0.0.1").trim();
    const port = Number($("provRouteListenPort")?.value || 18964);
    try {
      if (!window.providerAPI?.checkListenPort) {
        toast("检测接口不可用", "error");
        return;
      }
      const r = await window.providerAPI.checkListenPort(host, port);
      if (r?.available) {
        setPortStatus(true, r.message || `${host}:${port} 可用`);
        toast("端口可用", "ok");
      } else {
        const sug = r?.suggestedPort;
        setPortStatus(
          false,
          (r?.message || "端口不可用") +
            (sug ? ` — 可改用 ${sug}` : "")
        );
        if (sug && $("provRouteListenPort")) {
          const use = await confirm({
            title: "端口被占用",
            message: `${host}:${port} 无法绑定。\n是否改用建议端口 ${sug}？`,
            confirmText: `使用 ${sug}`,
            variant: "primary",
          });
          if (use) {
            $("provRouteListenPort").value = String(sug);
            setPortStatus(true, `已填入建议端口 ${sug}，请保存设置`);
          }
        }
        toast(r?.message || "端口不可用", "error");
      }
    } catch (err) {
      setPortStatus(false, err?.message || String(err));
      toast(err?.message || String(err), "error");
    }
  }

  async function onToggleFailover(id) {
    const p = (listPayload?.providers || []).find((x) => x.id === id);
    if (!p) return;
    try {
      if (p.inFailoverQueue) {
        await window.providerAPI.removeFromFailover(app, id);
        toast("已移出故障转移队列", "ok");
      } else {
        await window.providerAPI.addToFailover(app, id);
        toast("已加入故障转移队列", "ok");
      }
      await loadList();
      await refreshFoQueuePanel();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function onRepairRoute() {
    try {
      const res = await window.providerAPI.repairTakeover(app);
      const w = (res?.warnings || [])
        .filter(Boolean)
        .filter((x) => !/注入|白名单|catalog|CDP/i.test(x));
      toast(w[0] || "已修复本地路由", "ok");
      await loadList();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function refreshFoQueuePanel() {
    const listEl = $("provFoQueueList");
    const sel = $("provFoQueueAddSelect");
    if (!listEl || !window.providerAPI?.getFailoverQueue) return;
    try {
      const queue = (await window.providerAPI.getFailoverQueue(app)) || [];
      if (!queue.length) {
        listEl.innerHTML =
          `<div class="prov-fo-queue-empty ui-empty-inline">队列为空。从下方下拉添加备用供应商，或在列表卡片上点「加入队列」。</div>`;
      } else {
        listEl.innerHTML = queue
          .map((item, i) => {
            const cur = item.isCurrent ? "is-current" : "";
            let healthBadge = "";
            if (item.health === "healthy") {
              healthBadge = `<span class="prov-badge prov-badge-health-healthy">健康</span>`;
            } else if (item.health === "open") {
              healthBadge = `<span class="prov-badge prov-badge-health-open">熔断</span>`;
            } else if (item.health === "degraded") {
              healthBadge = `<span class="prov-badge prov-badge-health-degraded">降级</span>`;
            }
            const curBadge = item.isCurrent
              ? `<span class="prov-badge prov-badge-route">当前</span>`
              : "";
            const switchBtn = item.isCurrent
              ? `<button type="button" class="chip-btn" disabled title="已是当前启用">使用中</button>`
              : `<button type="button" class="chip-btn chip-primary" data-fo-act="switch" data-id="${escapeHtml(item.providerId)}" title="切换为当前供应商（本地路由开启时热生效）">切换</button>`;
            return `<div class="prov-fo-queue-row ${cur}" data-fo-id="${escapeHtml(item.providerId)}">
  <span class="prov-fo-queue-idx" title="优先级">P${i + 1}</span>
  <div class="prov-fo-queue-main">
    <span class="prov-fo-queue-name" title="${escapeHtml(item.providerName)}">${escapeHtml(item.providerName)}</span>
    <span class="prov-fo-queue-meta">${curBadge}${healthBadge}</span>
  </div>
  <span class="prov-fo-queue-actions">
    ${switchBtn}
    <button type="button" class="chip-btn" data-fo-act="up" data-id="${escapeHtml(item.providerId)}" title="上移提高优先级">↑</button>
    <button type="button" class="chip-btn" data-fo-act="down" data-id="${escapeHtml(item.providerId)}" title="下移降低优先级">↓</button>
    <button type="button" class="chip-btn chip-danger" data-fo-act="remove" data-id="${escapeHtml(item.providerId)}" title="移出队列">移除</button>
  </span>
</div>`;
          })
          .join("");
      }
      // Fill add select with providers not already in queue
      const inQ = new Set(queue.map((q) => q.providerId));
      const candidates = (listPayload?.providers || []).filter(
        (p) => p.category !== "official" && !inQ.has(p.id)
      );
      if (sel) {
        sel.innerHTML =
          `<option value="">添加供应商到队列…</option>` +
          candidates
            .map(
              (p) =>
                `<option value="${escapeHtml(p.id)}">${escapeHtml(p.name)}</option>`
            )
            .join("");
      }
    } catch (err) {
      listEl.innerHTML = `<div class="prov-fo-queue-empty">加载队列失败：${escapeHtml(err?.message || err)}</div>`;
    }
  }

  async function onFoQueueAction(act, id) {
    try {
      const queue = (await window.providerAPI.getFailoverQueue(app)) || [];
      const ids = queue.map((q) => q.providerId);
      const idx = ids.indexOf(id);
      if (act === "switch") {
        await switchFromFailoverQueue(id, ids);
        return;
      }
      if (act === "remove") {
        await window.providerAPI.removeFromFailover(app, id);
      } else if (act === "up" && idx > 0) {
        const next = ids.slice();
        [next[idx - 1], next[idx]] = [next[idx], next[idx - 1]];
        await window.providerAPI.reorderFailover(app, next);
      } else if (act === "down" && idx >= 0 && idx < ids.length - 1) {
        const next = ids.slice();
        [next[idx + 1], next[idx]] = [next[idx], next[idx + 1]];
        await window.providerAPI.reorderFailover(app, next);
      } else {
        return;
      }
      await loadList();
      await refreshFoQueuePanel();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  /**
   * Manually switch to a provider listed in the failover queue.
   * Under local routing this is a hot switch; also pin it to queue front (P1).
   */
  async function switchFromFailoverQueue(id, queueIds) {
    const p = (listPayload?.providers || []).find((x) => x.id === id);
    if (!p) {
      toast("供应商不在列表中", "error");
      return;
    }
    if (p.isCurrent) {
      toast("已是当前启用", "ok");
      return;
    }
    if (p.ready === false && p.category !== "official") {
      toast("该供应商尚未就绪：请先编辑并填写 Base URL 与 API Key", "error");
      return;
    }
    const routing = !!listPayload?.takeoverEnabled;
    const name = p.name || id;
    if (!routing) {
      const ok = await confirm({
        title: "切换供应商",
        message: `尚未开启本地路由。切换「${name}」将写入本机 live 配置（与列表「启用」相同）。\n\n建议先开启本地路由，以便队列内热切换。`,
        confirmText: "仍然切换",
        variant: "primary",
      });
      if (!ok) return;
    }
    try {
      const res = await window.providerAPI.switch(app, id);
      // Pin switched provider to front so FO order matches sticky primary.
      const rest = (queueIds || []).filter((x) => x !== id);
      const nextOrder = [id, ...rest];
      try {
        if (window.providerAPI?.reorderFailover) {
          await window.providerAPI.reorderFailover(app, nextOrder);
        }
      } catch (reorderErr) {
        console.warn("reorder after queue switch", reorderErr);
      }
      const msg =
        res?.message ||
        (routing ? `已热切换至「${name}」` : `已启用「${name}」`);
      toast(msg, "ok");
      await loadList();
      await refreshFoQueuePanel();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function loadPresets() {
    try {
      if (!window.providerAPI?.presets) {
        presets = [];
        return;
      }
      presets = (await window.providerAPI.presets(app)) || [];
    } catch (err) {
      console.warn("loadPresets failed", err);
      presets = [];
    }
  }

  function setRefreshLoading(on) {
    const btn = $("btnProvRefresh");
    if (!btn) return;
    btn.disabled = !!on;
    btn.classList.toggle("is-loading", !!on);
    btn.setAttribute("aria-busy", on ? "true" : "false");
    const label = btn.querySelector(".chip-label");
    if (label) {
      if (on) {
        if (!btn.dataset.labelIdle) {
          btn.dataset.labelIdle = label.textContent || "刷新";
        }
        label.textContent = "刷新中…";
      } else {
        label.textContent = btn.dataset.labelIdle || "刷新";
      }
    }
  }

  async function loadList(opts) {
    const showRefreshLoading = opts?.refresh === true;
    const seq = ++loadSeq;
    loading = true;
    if (showRefreshLoading) setRefreshLoading(true);
    renderList();
    updateLiveBar();
    try {
      if (!window.providerAPI) throw new Error("providerAPI 未加载");
      const [data] = await Promise.all([
        window.providerAPI.list(app),
        loadPresets(),
      ]);
      if (seq !== loadSeq) return;
      listPayload = data;
      loading = false;
      updateLead();
      updateLiveBar();
      syncRouteToggle();
      renderList();
      loadAppProxySettings().catch(() => {});
      if (showRefreshLoading) toast("供应商列表已刷新", "ok");
    } catch (err) {
      if (seq !== loadSeq) return;
      loading = false;
      listPayload = null;
      updateLiveBar();
      const empty = $("provEmpty");
      const list = $("provList");
      if (list) list.hidden = true;
      if (empty) {
        empty.hidden = false;
        empty.dataset.state = "error";
        empty.innerHTML =
          `<div class="session-empty-title">加载失败</div>` +
          `<div class="session-empty-detail">${escapeHtml(err?.message || err)}</div>`;
      }
      toast(err?.message || String(err), "error");
    } finally {
      if (seq === loadSeq) setRefreshLoading(false);
    }
  }

  function fillPresetSelect() {
    const sel = $("provFormPreset");
    if (!sel) return;
    const options = [
      { value: "", label: "手动填写 / 不使用预设" },
      ...presets.map((p) => ({
        value: p.id,
        label: p.name || p.id,
      })),
    ];
    if (window.UiSelect?.setOptions) {
      window.UiSelect.setOptions(sel, options, "");
    } else {
      sel.innerHTML = options
        .map(
          (o) =>
            `<option value="${escapeHtml(o.value)}">${escapeHtml(o.label)}</option>`
        )
        .join("");
      sel.value = "";
    }
  }

  function refreshFormSelects() {
    if (!window.UiSelect?.refresh) return;
    [
      "provFormPreset",
      "provFormWireApi",
      "provFormReasoning",
      "provFormApiBackend",
      "provFormModel",
    ].forEach((id) => {
      const el = $(id);
      if (el) window.UiSelect.refresh(el);
    });
  }

  function applyPreset(presetId) {
    const p = presets.find((x) => x.id === presetId);
    if (!p) {
      formCategory = "custom";
      const hint = $("provFormPresetHint");
      if (hint) {
        hint.textContent = isGrok()
          ? "选择常见渠道可自动填充 Base URL、模型、api_backend（Grok 独立预设）。"
          : "选择常见渠道可自动填充 Base URL、模型与 wire_api（Codex 独立预设）。";
      }
      return;
    }
    formCategory = p.category || "custom";
    // Preset changes endpoints — clear fetched model suggestions.
    clearFetchedModels();
    setProbeStatus("provModelStatus", "", "");
    setProbeStatus("provConnStatus", "", "");
    setField("provFormName", p.name || "");
    setField("provFormWebsite", p.websiteUrl || "");
    setField("provFormBaseUrl", p.baseUrl || "");
    setField("provFormModel", p.model || "");
    setField("provFormNotes", p.notes || "");
    setField("provFormConfigToml", "");
    advancedDirty = false;
    updateAdvancedHint();

    const wire = $("provFormWireApi");
    if (wire && !wire.disabled) {
      wire.value = p.wireApi === "chat" ? "chat" : "responses";
    }
    const reasoning = $("provFormReasoning");
    if (reasoning) reasoning.value = "high";

    // Grok-specific defaults from preset: identity = supplier name, not upstream id
    if (isGrok()) {
      const backend = $("provFormApiBackend");
      if (backend) {
        backend.value =
          p.wireApi === "chat" || p.wireApi === "chat_completions"
            ? "chat_completions"
            : "responses";
      }
      const grokCw = guessContextWindow(p.model || "grok-4.5") || 500000;
      setField("provFormContextWindow", String(grokCw));
    } else {
      // Codex: seed model mapping so enable writes full model_catalog_json
      // (third-party: DeepSeek / Claude / Gemini / Grok / … must all be listed).
      const presetModels = Array.isArray(p.models)
        ? p.models.map((x) => String(x || "").trim()).filter(Boolean)
        : [];
      if (presetModels.length) {
        renderCatalogRows(
          presetModels.map((id) => {
            const row = { model: id, displayName: id };
            const cw = guessContextWindow(id);
            if (cw) row.contextWindow = cw;
            return row;
          })
        );
      } else {
        const m = (p.model || "").trim();
        if (m) {
          const row = { model: m, displayName: m };
          const cw = guessContextWindow(m);
          if (cw) row.contextWindow = cw;
          renderCatalogRows([row]);
        }
      }
    }

    const hint = $("provFormPresetHint");
    if (hint) {
      hint.textContent = p.notes
        ? p.notes
        : `已填充 ${p.name} 的默认端点，请粘贴 API Key。`;
    }
    clearFormError();
    clearFieldErrors();
    refreshFormSelects();
    $("provFormApiKey")?.focus();
  }

  function updateAdvancedHint() {
    const hint = $("provFormAdvancedHint");
    if (!hint) return;
    hint.textContent = advancedDirty
      ? isGrok()
        ? "已修改 config.toml：保存只提取 [models].default 与 [model.\"身份\"]，不会覆盖 MCP / ui 等其它段落。"
        : "已修改 config.toml：保存只提取 model / model_provider / model_providers.<id>，不会覆盖 MCP / desktop 等其它段落。"
      : isGrok()
        ? "此处为供应商路由片段。启用时只改 [models].default 与对应 [model.\"身份\"]，其它段落保持本机原样。"
        : "此处为供应商路由片段。启用时只改 model / model_provider / model_providers.<id>，其它段落保持本机原样。";
  }

  function markStructuredEdit() {
    if (!advancedDirty) return;
    // User edited structured fields after touching advanced → prefer form fields
    advancedDirty = false;
    updateAdvancedHint();
  }

  function syncAppSpecificFields() {
    const codexFields = $("provFormCodexFields");
    const grokFields = $("provFormGrokFields");
    const lock = formIsOfficial;
    if (codexFields) codexFields.hidden = isGrok() || lock;
    if (grokFields) grokFields.hidden = !isGrok() || lock;

    const modelLabel = $("provFormModelLabel");
    const modelHint = $("provFormModelHint");
    if (modelLabel) {
      modelLabel.textContent = isGrok() ? "模型 id" : "模型";
    }
    if (modelHint) {
      modelHint.hidden = false;
      if (isGrok()) {
        modelHint.textContent =
          "默认调用 API 的模型 id（[model.<id>].model）。拉取成功后点开下拉可选全部模型；也可搜索或回车手填。身份默认用供应商名，选择器显示名与此 id 一致。";
      } else {
        modelHint.textContent =
          "默认使用模型。桌面/CLI 可选列表 = 下方「模型映射」全量（须含 DeepSeek/Claude/Gemini/Grok 等实际要用的 id）。";
      }
    }

    const wire = $("provFormWireApi");
    if (wire) wire.disabled = lock || isGrok();
    const reasoning = $("provFormReasoning");
    if (reasoning) reasoning.disabled = lock || isGrok();
    ["provFormApiBackend", "provFormContextWindow"].forEach(
      (id) => {
        const el = $(id);
        if (el) el.disabled = lock || !isGrok();
      }
    );
    // Codex model mapping is Codex-only — never show for Grok / official.
    const catalogWrap = $("provFormCatalogWrap");
    if (catalogWrap) {
      const showCatalog = !isGrok() && !lock;
      catalogWrap.hidden = !showCatalog;
      if (showCatalog) {
        catalogWrap.removeAttribute("hidden");
        // Ensure at least one editable row so users don't save an empty mapping.
        const body = $("provCatalogBody");
        if (body && !body.querySelector("tr")) {
          const seed =
            ($("provFormModel")?.value || "").trim() || "gpt-5.5";
          renderCatalogRows([{ model: seed, displayName: seed }]);
        }
      } else {
        // Avoid leftover rows bleeding into Grok form state.
        const body = $("provCatalogBody");
        if (body && isGrok()) body.innerHTML = "";
      }
      const addBtn = $("btnProvCatalogAdd");
      if (addBtn) {
        addBtn.disabled = !showCatalog;
        addBtn.hidden = !showCatalog;
      }
      // Fetch models lives next to「添加模型」for Codex mapping.
      const fetchBtn = $("btnProvFetchModels");
      if (fetchBtn && !isGrok()) {
        fetchBtn.disabled = !showCatalog || lock;
        fetchBtn.hidden = !showCatalog || lock;
      }
    }
  }

  function setModelSelect(value, extraIds) {
    const sel = $("provFormModel");
    if (!sel) return;
    const selected = String(value || "").trim();
    const seen = new Set();
    const options = [];
    const push = (id) => {
      const v = String(id || "").trim();
      if (!v || seen.has(v)) return;
      seen.add(v);
      options.push({ value: v, label: v });
    };
    // Fetched list is authoritative: never keep leftover / hardcoded ids.
    if (Array.isArray(extraIds)) {
      extraIds.forEach(push);
    } else {
      push(selected);
    }
    if (!options.length) {
      options.push({ value: "", label: "选择或搜索模型" });
    }
    const next =
      selected && options.some((o) => o.value === selected)
        ? selected
        : options[0].value;
    if (window.UiSelect?.setOptions) {
      window.UiSelect.setOptions(sel, options, next);
    } else {
      sel.innerHTML = options
        .map(
          (o) =>
            `<option value="${escapeHtml(o.value)}">${escapeHtml(o.label)}</option>`
        )
        .join("");
      sel.value = next;
    }
  }

  function setField(id, value) {
    if (id === "provFormModel") {
      setModelSelect(value);
      return;
    }
    const el = $(id);
    if (el) el.value = value ?? "";
  }

  function setFormError(msg) {
    const box = $("provFormError");
    if (!box) {
      if (msg) toast(msg, "error");
      return;
    }
    if (!msg) {
      box.hidden = true;
      box.textContent = "";
      return;
    }
    box.hidden = false;
    box.textContent = msg;
  }

  function clearFormError() {
    setFormError("");
  }

  function markFieldError(id, on) {
    const el = $(id);
    if (!el) return;
    el.classList.toggle("is-invalid", !!on);
    const field = el.closest?.(".prov-field");
    if (field) field.classList.toggle("has-error", !!on);
    // Shared ui-select trigger mirrors invalid state.
    const wrap = el.closest?.(".ui-select");
    if (wrap) wrap.classList.toggle("is-invalid", !!on);
  }

  function clearFieldErrors() {
    [
      "provFormName",
      "provFormBaseUrl",
      "provFormModel",
      "provFormApiKey",
      "provFormWebsite",
      "provFormContextWindow",
    ].forEach((id) => markFieldError(id, false));
  }

  function openForm(mode, detail) {
    const modal = $("providerFormModal");
    if (!modal) {
      toast("找不到供应商表单，请刷新应用", "error");
      return;
    }

    editingId = mode === "edit" ? detail?.id || null : null;
    formIsOfficial = !!detail?.isOfficial;
    formCategory = detail?.category || "custom";
    if (formCategory === "official") formIsOfficial = true;
    // Editing always prefers structured fields unless user edits advanced TOML.
    advancedDirty = false;
    resetProbeUi();

    const title = $("providerFormTitle");
    if (title) {
      title.textContent = mode === "edit" ? "编辑供应商" : "添加供应商";
    }

    fillPresetSelect();
    const presetWrap = $("provFormPresetWrap");
    if (presetWrap) {
      // Section may use [hidden] on the whole block
      if (mode === "edit" || formIsOfficial) {
        presetWrap.hidden = true;
      } else {
        presetWrap.hidden = false;
        presetWrap.removeAttribute("hidden");
      }
    }
    const presetSel = $("provFormPreset");
    if (presetSel) {
      presetSel.value = "";
      presetSel.disabled = formIsOfficial;
    }

    setField("provFormName", detail?.name || "");
    setField("provFormWebsite", detail?.websiteUrl || "");
    setField("provFormBaseUrl", detail?.baseUrl || "");
    setField(
      "provFormModel",
      detail?.model || (isGrok() ? "grok-4.5" : "gpt-5.5")
    );
    // Echo API Key in the form (edit + add); eye toggle controls mask.
    setField("provFormApiKey", detail?.apiKey || "");
    setApiKeyVisible(false);
    setField("provFormNotes", detail?.notes || "");
    setField("provFormUserAgent", detail?.customUserAgent || "");
    setField("provFormProxyHeaders", detail?.localProxyHeadersJson || "");
    setField("provFormProxyBody", detail?.localProxyBodyJson || "");
    const uaPreset = $("provFormUserAgentPreset");
    if (uaPreset) uaPreset.value = "";
    // Show stored TOML for reference, but do not treat as dirty until edited.
    setField("provFormConfigToml", detail?.configToml || "");
    updateAdvancedHint();
    // Model catalog table only applies to Codex third-party profiles.
    if (isGrok() || formIsOfficial) {
      renderCatalogRows([]);
    } else {
      const existingCatalog = Array.isArray(detail?.modelCatalog)
        ? detail.modelCatalog
        : [];
      if (existingCatalog.length) {
        renderCatalogRows(existingCatalog);
      } else {
        // New channel or legacy archive without mapping: seed one row from model.
        const seedModel =
          (detail?.model || "").trim() ||
          ($("provFormModel")?.value || "").trim() ||
          "gpt-5.5";
        renderCatalogRows([{ model: seedModel, displayName: seedModel }]);
      }
    }

    const wire = $("provFormWireApi");
    if (wire) {
      wire.value =
        detail?.wireApi === "chat" ? "chat" : detail?.wireApi || "responses";
    }
    const reasoning = $("provFormReasoning");
    if (reasoning) {
      const effort = (detail?.reasoningEffort || "high").toLowerCase();
      reasoning.value = ["high", "medium", "low", "minimal"].includes(effort)
        ? effort
        : "high";
    }

    const backend = $("provFormApiBackend");
    if (backend) {
      const b = (detail?.apiBackend || "responses").toLowerCase();
      backend.value =
        b === "chat" || b === "chat_completions"
          ? "chat_completions"
          : "responses";
    }
    setField(
      "provFormContextWindow",
      detail?.contextWindow > 0 ? String(detail.contextWindow) : "500000"
    );

    clearFormError();
    clearFieldErrors();
    syncAppSpecificFields();

    const keyHint = $("provFormApiKeyHint");
    if (keyHint) {
      if (formIsOfficial) {
        keyHint.textContent = "官方供应商不使用自定义 API Key";
      } else if (isGrok()) {
        keyHint.textContent = "必填才能启用；编辑时会回显已保存的 Key。";
      } else {
        keyHint.textContent =
          "必填才能启用；编辑时会回显已保存的 Key（可保留官方登录时不覆盖 ChatGPT 登录）。";
      }
    }

    const baseInput = $("provFormBaseUrl");
    if (baseInput && mode === "add" && !detail?.baseUrl) {
      baseInput.placeholder = isGrok()
        ? "https://api.x.ai/v1 或中转地址"
        : "https://api.openai.com/v1 或中转地址";
    }

    const lock = formIsOfficial;
    [
      "provFormBaseUrl",
      "provFormModel",
      "provFormApiKey",
      "provFormConfigToml",
      "provFormWireApi",
      "provFormReasoning",
      "provFormApiBackend",
      "provFormContextWindow",
      "provFormUserAgent",
      "provFormUserAgentPreset",
      "provFormProxyHeaders",
      "provFormProxyBody",
    ].forEach((id) => {
      const el = $(id);
      if (!el) return;
      el.disabled = lock;
    });
    const btnConn = $("btnProvTestConn");
    if (btnConn) {
      btnConn.disabled = lock;
      btnConn.hidden = lock;
    }
    // Grok: fetch beside default model. Codex: fetch beside「添加模型」(catalog head).
    const btnModelsMain = $("btnProvFetchModelsMain");
    if (btnModelsMain) {
      const showMain = isGrok() && !lock;
      btnModelsMain.hidden = !showMain;
      btnModelsMain.disabled = !showMain;
    }
    const btnToggleKey = $("btnProvToggleApiKey");
    if (btnToggleKey) {
      btnToggleKey.disabled = lock;
      btnToggleKey.hidden = lock;
    }
    syncAppSpecificFields();

    const nameEl = $("provFormName");
    if (nameEl) nameEl.disabled = false;
    const notesEl = $("provFormNotes");
    if (notesEl) notesEl.disabled = false;
    const websiteEl = $("provFormWebsite");
    if (websiteEl) websiteEl.disabled = false;

    const advanced = $("provFormAdvanced");
    if (advanced) {
      advanced.hidden = lock;
      advanced.open = false;
    }
    const officialNote = $("provFormOfficialNote");
    if (officialNote) officialNote.hidden = !lock;

    const appLabel = $("provFormAppLabel");
    if (appLabel) appLabel.textContent = isGrok() ? "Grok Build" : "Codex";

    const saveOnly = $("btnProviderFormSave");
    const saveEnable = $("btnProviderFormSaveEnable");
    if (saveOnly) {
      saveOnly.disabled = false;
      saveOnly.hidden = false;
      saveOnly.textContent = "仅保存";
    }
    if (saveEnable) {
      saveEnable.hidden = lock;
      saveEnable.disabled = false;
      saveEnable.textContent = "保存并启用";
    }

    formBusy = false;
    showModal(modal);
    refreshFormSelects();

    requestAnimationFrame(() => {
      try {
        refreshFormSelects();
        const focusId = formIsOfficial
          ? "provFormName"
          : mode === "add"
            ? "provFormPreset"
            : "provFormName";
        // Prefer custom trigger when ui-select is mounted.
        const native = $(focusId);
        const trigger = native?.closest?.(".ui-select")?.querySelector?.(
          ".ui-select-trigger"
        );
        (trigger || native)?.focus?.();
      } catch {
        /* ignore */
      }
    });
  }

  function closeForm() {
    const modal = $("providerFormModal");
    hideModal(modal);
    editingId = null;
    formBusy = false;
    formCategory = "custom";
    formIsOfficial = false;
    advancedDirty = false;
    setApiKeyVisible(false);
    resetProbeUi();
    clearFormError();
    clearFieldErrors();
  }

  // ── Connectivity + model list ───────────────────────────────────────────

  function resetProbeUi() {
    fetchModelsSeq += 1;
    probeBusy = false;
    setProbeStatus("provConnStatus", "", "");
    setProbeStatus("provModelStatus", "", "");
    setProbeStatus("provCatalogStatus", "", "");
    clearFetchedModels();
    const btnConn = $("btnProvTestConn");
    const btnModels = $("btnProvFetchModels");
    if (btnConn) {
      btnConn.disabled = formIsOfficial;
      btnConn.textContent = "测试连通";
    }
    if (btnModels) {
      btnModels.disabled = formIsOfficial;
      btnModels.textContent = "拉取模型";
    }
    const btnModelsMain = $("btnProvFetchModelsMain");
    if (btnModelsMain) {
      btnModelsMain.disabled = formIsOfficial;
      btnModelsMain.textContent = "拉取模型";
    }
  }

  /**
   * @param {string} id
   * @param {string} text
   * @param {""|"ok"|"warn"|"error"|"loading"} kind
   */
  function setProbeStatus(id, text, kind) {
    const el = $(id);
    if (!el) return;
    if (!text) {
      el.hidden = true;
      el.textContent = "";
      el.className = "prov-probe-status";
      return;
    }
    el.hidden = false;
    el.textContent = text;
    el.className = "prov-probe-status" + (kind ? ` is-${kind}` : "");
  }

  /** Password eye toggle (SVG), not a text “显示” button. */
  function setApiKeyVisible(visible) {
    const input = $("provFormApiKey");
    const btn = $("btnProvToggleApiKey");
    const eye = $("provApiKeyEye");
    const eyeOff = $("provApiKeyEyeOff");
    if (input) input.type = visible ? "text" : "password";
    if (eye) eye.hidden = !!visible;
    if (eyeOff) eyeOff.hidden = !visible;
    if (btn) {
      const label = visible ? "隐藏 API Key" : "显示 API Key";
      btn.title = label;
      btn.setAttribute("aria-label", label);
      btn.setAttribute("aria-pressed", visible ? "true" : "false");
    }
  }

  function toggleApiKeyVisible() {
    const input = $("provFormApiKey");
    if (!input || input.disabled) return;
    setApiKeyVisible(input.type === "password");
  }

  function resolveFormApiKey() {
    return ($("provFormApiKey")?.value || "").trim();
  }

  function resolveFormUserAgent() {
    return ($("provFormUserAgent")?.value || "").trim();
  }

  // ── Codex model catalog rows ────────────────────────────────────────────

  function renderCatalogRows(rows) {
    const body = $("provCatalogBody");
    if (!body) return;
    const list = Array.isArray(rows) ? rows : [];
    if (!list.length) {
      body.innerHTML = "";
      return;
    }
    body.innerHTML = list
      .map((row, i) => catalogRowHtml(row, i))
      .join("");
  }

  function catalogRowHtml(row, index) {
    const modelRaw = row?.model || "";
    const model = escapeHtml(modelRaw);
    const display = escapeHtml(row?.displayName || row?.display_name || "");
    let cw =
      row?.contextWindow || row?.context_window
        ? String(row.contextWindow || row.context_window)
        : "";
    // Auto-fill known mainstream models when context is empty.
    if (!cw && modelRaw) {
      const guessed = guessContextWindow(modelRaw);
      if (guessed) cw = String(guessed);
    }
    const listId = `provCtxPresetList-${index}`;
    const opts = CONTEXT_WINDOW_QUICK_PICKS.map(
      (p) =>
        `<option value="${p.value}" label="${escapeHtml(p.label)}">${escapeHtml(
          p.label
        )}</option>`
    ).join("");
    return `<tr data-catalog-idx="${index}">
      <td><input type="text" data-cat="model" value="${model}" placeholder="model-id" spellcheck="false" autocomplete="off" /></td>
      <td><input type="text" data-cat="displayName" value="${display}" placeholder="可选" spellcheck="false" autocomplete="off" /></td>
      <td class="prov-cat-ctx">
        <input type="number" data-cat="contextWindow" value="${escapeHtml(
          cw
        )}" placeholder="128000" min="1" step="1" inputmode="numeric" list="${listId}" title="可选手动填写，或从预设选择；识别到常见模型会自动填入" />
        <datalist id="${listId}">${opts}</datalist>
      </td>
      <td><button type="button" class="icon-btn delete-btn prov-catalog-del" data-cat-del title="删除行" aria-label="删除行">×</button></td>
    </tr>`;
  }

  function collectCatalogRows() {
    if (isGrok() || formIsOfficial) return [];
    const body = $("provCatalogBody");
    if (!body) return [];
    const rows = [];
    body.querySelectorAll("tr").forEach((tr) => {
      const model = (tr.querySelector('[data-cat="model"]')?.value || "").trim();
      if (!model) return;
      const displayName = (
        tr.querySelector('[data-cat="displayName"]')?.value || ""
      ).trim();
      const cwRaw = (
        tr.querySelector('[data-cat="contextWindow"]')?.value || ""
      ).trim();
      const item = { model };
      if (displayName) item.displayName = displayName;
      if (cwRaw) {
        const n = Number.parseInt(cwRaw, 10);
        if (Number.isFinite(n) && n > 0) item.contextWindow = n;
      }
      rows.push(item);
    });
    // No mapping rows → seed from the default model field so enable always
    // projects model_catalog_json (mapping drives /model list).
    if (!rows.length) {
      const fallback = ($("provFormModel")?.value || "").trim();
      if (fallback) {
        const item = { model: fallback, displayName: fallback };
        const cw = guessContextWindow(fallback);
        if (cw) item.contextWindow = cw;
        rows.push(item);
      }
    }
    return rows;
  }

  function addCatalogRow(seed) {
    const body = $("provCatalogBody");
    if (!body) return;
    const model =
      (seed && seed.model) ||
      ($("provFormModel")?.value || "").trim() ||
      "";
    const row = seed
      ? { ...seed }
      : {
          model,
          displayName: "",
        };
    if (!row.contextWindow) {
      const cw = resolveContextWindow(row.model, row.contextWindow);
      if (cw) row.contextWindow = cw;
    }
    body.insertAdjacentHTML(
      "beforeend",
      catalogRowHtml(row, body.children.length)
    );
  }

  /**
   * When user edits model id in mapping table, fill context if empty (or re-guess).
   * @param {HTMLInputElement} modelInput
   */
  function onCatalogModelIdChange(modelInput) {
    if (!modelInput || modelInput.getAttribute("data-cat") !== "model") return;
    const tr = modelInput.closest("tr");
    if (!tr) return;
    const cwInput = tr.querySelector('[data-cat="contextWindow"]');
    if (!cwInput) return;
    const model = (modelInput.value || "").trim();
    const existing = (cwInput.value || "").trim();
    // Only auto-fill when empty, so user overrides are preserved.
    if (existing) return;
    const guessed = guessContextWindow(model);
    if (guessed) {
      cwInput.value = String(guessed);
      markStructuredEdit();
    }
  }

  /** Clear fetched model suggestions; keep the current model selection. */
  function clearFetchedModels() {
    fetchedModels = [];
    const current = ($("provFormModel")?.value || "").trim();
    if (current) setModelSelect(current);
  }

  /**
   * Normalize /models payload into `{ id, ownedBy? }[]`.
   * Accepts objects with id/model, plain strings, and snake_case owned_by.
   * @param {any} models
   * @returns {Array<{id:string,ownedBy?:string}>}
   */
  function normalizeFetchedModels(models) {
    const raw = Array.isArray(models)
      ? models
      : Array.isArray(models?.data)
        ? models.data
        : Array.isArray(models?.models)
          ? models.models
          : [];
    const seen = new Set();
    const out = [];
    for (const item of raw) {
      let id = "";
      let ownedBy;
      if (typeof item === "string") {
        id = item.trim();
      } else if (item && typeof item === "object") {
        id = String(item.id || item.model || item.slug || "").trim();
        ownedBy = item.ownedBy || item.owned_by || undefined;
      }
      if (!id || seen.has(id)) continue;
      seen.add(id);
      out.push(ownedBy ? { id, ownedBy: String(ownedBy) } : { id });
    }
    return out;
  }

  /**
   * Write fetched models into the Codex mapping table (full replace).
   * Ensures the mapping panel is visible, then fills every model id as a row.
   * @param {Array<{id:string,ownedBy?:string}>} models
   * @returns {number} rows written
   */
  function fillCatalogFromFetchedModels(models) {
    if (isGrok() || formIsOfficial) return 0;
    const list = Array.isArray(models) ? models : [];
    if (!list.length) return 0;

    const catalogWrap = $("provFormCatalogWrap");
    if (catalogWrap) {
      catalogWrap.hidden = false;
      catalogWrap.removeAttribute("hidden");
    }
    // Keep previous displayName / contextWindow when the same model id already exists.
    const prevById = new Map();
    collectCatalogRows().forEach((row) => {
      if (row?.model) prevById.set(row.model, row);
    });

    const rows = list.map((m) => {
      const id = m.id;
      const prev = prevById.get(id);
      const row = {
        model: id,
        displayName: prev?.displayName || id,
      };
      const cw = resolveContextWindow(id, prev?.contextWindow);
      if (cw) row.contextWindow = cw;
      return row;
    });
    renderCatalogRows(rows);
    markStructuredEdit();
    return rows.length;
  }

  /**
   * Fill the shared UiSelect with fetched model ids (same control as api_backend).
   * For Codex, also project the full list into the model-mapping table
   * (catalog must contain every model that should appear in /model).
   * @param {any} models
   * @param {{selectFirst?: boolean, fillCatalog?: boolean}} [opts]
   * @returns {{ count: number, catalogRows: number, models: Array<{id:string,ownedBy?:string}> }}
   */
  function applyFetchedModels(models, opts) {
    const selectFirst = opts?.selectFirst !== false;
    const fillCatalog = opts?.fillCatalog !== false;
    fetchedModels = normalizeFetchedModels(models);
    const ids = fetchedModels.map((m) => m.id);
    const current = ($("provFormModel")?.value || "").trim();
    const selected =
      selectFirst && ids.length ? ids[0] : current || ids[0] || "";
    setModelSelect(selected, ids);
    if (ids.length) {
      markStructuredEdit();
      markFieldError("provFormModel", false);
    }
    // Codex: always auto-fill mapping table when pulling models.
    let catalogRows = 0;
    if (fillCatalog && !isGrok() && !formIsOfficial && fetchedModels.length) {
      catalogRows = fillCatalogFromFetchedModels(fetchedModels);
    }
    return {
      count: fetchedModels.length,
      catalogRows,
      models: fetchedModels,
    };
  }

  async function onTestConnectivity() {
    if (probeBusy || formIsOfficial) return;
    if (!window.providerAPI?.testConnectivity) {
      toast("连通测试接口不可用，请重启应用", "error");
      return;
    }
    const baseUrl = ($("provFormBaseUrl")?.value || "").trim();
    if (!baseUrl) {
      markFieldError("provFormBaseUrl", true);
      setProbeStatus("provConnStatus", "请先填写 Base URL", "error");
      toast("请先填写 Base URL", "error");
      return;
    }
    if (!(baseUrl.startsWith("http://") || baseUrl.startsWith("https://"))) {
      markFieldError("provFormBaseUrl", true);
      setProbeStatus(
        "provConnStatus",
        "Base URL 必须以 http:// 或 https:// 开头",
        "error"
      );
      return;
    }
    markFieldError("provFormBaseUrl", false);
    probeBusy = true;
    const btn = $("btnProvTestConn");
    if (btn) {
      btn.disabled = true;
      btn.textContent = "测试中…";
    }
    setProbeStatus("provConnStatus", "正在探测 Base URL 可达性…", "loading");
    try {
      const res = await window.providerAPI.testConnectivity(
        baseUrl,
        8,
        resolveFormUserAgent() || null
      );
      const kind = res?.success
        ? res.status === "degraded"
          ? "warn"
          : "ok"
        : "error";
      setProbeStatus(
        "provConnStatus",
        res?.message || (res?.success ? "可达" : "不可达"),
        kind
      );
      if (res?.success) {
        toast(res.message || "连通正常", res.status === "degraded" ? "warn" : "ok");
      } else {
        toast(res?.message || "连通失败", "error");
      }
    } catch (err) {
      const msg = err?.message || String(err);
      setProbeStatus("provConnStatus", msg, "error");
      toast(msg, "error");
    } finally {
      probeBusy = false;
      if (btn) {
        btn.disabled = formIsOfficial;
        btn.textContent = "测试连通";
      }
    }
  }

  async function onFetchModels() {
    if (probeBusy || formIsOfficial) return;
    if (!window.providerAPI?.fetchModels) {
      toast("拉取模型接口不可用，请重启应用", "error");
      return;
    }
    const baseUrl = ($("provFormBaseUrl")?.value || "").trim();
    const apiKey = resolveFormApiKey();
    if (!baseUrl) {
      markFieldError("provFormBaseUrl", true);
      setProbeStatus("provModelStatus", "请先填写 Base URL", "error");
      toast("请先填写 Base URL", "error");
      return;
    }
    if (!(baseUrl.startsWith("http://") || baseUrl.startsWith("https://"))) {
      markFieldError("provFormBaseUrl", true);
      setProbeStatus(
        "provModelStatus",
        "Base URL 必须以 http:// 或 https:// 开头",
        "error"
      );
      return;
    }
    if (!apiKey) {
      markFieldError("provFormApiKey", true);
      setProbeStatus("provModelStatus", "请先填写 API Key", "error");
      toast("拉取模型需要 API Key", "error");
      return;
    }
    markFieldError("provFormBaseUrl", false);
    markFieldError("provFormApiKey", false);

    const seq = ++fetchModelsSeq;
    probeBusy = true;
    const btns = ["btnProvFetchModels", "btnProvFetchModelsMain"]
      .map((id) => $(id))
      .filter(Boolean);
    btns.forEach((btn) => {
      btn.disabled = true;
      btn.textContent = "拉取中…";
    });
    setProbeStatus(
      "provModelStatus",
      "正在请求 /models（将自动尝试多个候选路径）…",
      "loading"
    );
    if (!isGrok() && !formIsOfficial) {
      setProbeStatus(
        "provCatalogStatus",
        "正在拉取模型并写入映射表…",
        "loading"
      );
    }
    try {
      const models = await window.providerAPI.fetchModels(
        baseUrl,
        apiKey,
        null,
        resolveFormUserAgent() || null
      );
      if (seq !== fetchModelsSeq) return;
      // Normalize + fill default model, datalist, and Codex mapping table.
      const applied = applyFetchedModels(models, {
        selectFirst: true,
        fillCatalog: true,
      });
      if (!applied.count) {
        clearFetchedModels();
        setProbeStatus(
          "provModelStatus",
          "接口返回空列表，请手填模型 id，或检查 Key / Base URL。",
          "warn"
        );
        setProbeStatus("provCatalogStatus", "未获取到模型，映射表未改动。", "warn");
        toast("未获取到模型", "warn");
      } else {
        const first = applied.models[0]?.id || "";
        const catalogNote =
          applied.catalogRows > 0
            ? `，已自动填入模型映射 ${applied.catalogRows} 行`
            : "";
        setProbeStatus(
          "provModelStatus",
          `已获取 ${applied.count} 个模型，默认模型：${first}${catalogNote}`,
          "ok"
        );
        if (applied.catalogRows > 0) {
          setProbeStatus(
            "provCatalogStatus",
            `已自动填入 ${applied.catalogRows} 个模型到映射表（可继续编辑显示名 / 上下文）。`,
            "ok"
          );
        } else {
          setProbeStatus("provCatalogStatus", "", "");
        }
        toast(
          applied.catalogRows > 0
            ? `已获取 ${applied.count} 个模型，并写入映射表`
            : `已获取 ${applied.count} 个模型`,
          "ok"
        );
        // Scroll mapping table into view so the fill is obvious.
        if (applied.catalogRows > 0) {
          $("provFormCatalogWrap")?.scrollIntoView?.({
            block: "nearest",
            behavior: "smooth",
          });
        }
      }
    } catch (err) {
      if (seq !== fetchModelsSeq) return;
      const msg = err?.message || String(err);
      setProbeStatus("provModelStatus", msg, "error");
      setProbeStatus("provCatalogStatus", msg, "error");
      toast(msg, "error");
    } finally {
      if (seq === fetchModelsSeq) {
        probeBusy = false;
        btns.forEach((btn) => {
          btn.disabled = formIsOfficial;
          btn.textContent = "拉取模型";
        });
        // Re-apply visibility (Codex catalog vs Grok main).
        syncAppSpecificFields();
        const btnModelsMain = $("btnProvFetchModelsMain");
        if (btnModelsMain) {
          const showMain = isGrok() && !formIsOfficial;
          btnModelsMain.hidden = !showMain;
          btnModelsMain.disabled = !showMain;
        }
      }
    }
  }

  function collectForm() {
    const name = ($("provFormName")?.value || "").trim();
    const websiteRaw = ($("provFormWebsite")?.value || "").trim();
    const baseUrl = ($("provFormBaseUrl")?.value || "").trim() || null;
    const model = ($("provFormModel")?.value || "").trim() || null;
    const apiKey = ($("provFormApiKey")?.value || "").trim() || null;
    const notes = ($("provFormNotes")?.value || "").trim() || null;
    const customUserAgent = resolveFormUserAgent() || null;
    const localProxyHeadersJson =
      ($("provFormProxyHeaders")?.value || "").trim() || null;
    const localProxyBodyJson =
      ($("provFormProxyBody")?.value || "").trim() || null;
    let modelCatalog = collectCatalogRows();
    const configTomlRaw = ($("provFormConfigToml")?.value || "").trim();
    const wireRaw = ($("provFormWireApi")?.value || "responses").trim();
    const wireApi = wireRaw === "chat" ? "chat" : "responses";
    const reasoningRaw = ($("provFormReasoning")?.value || "high").trim();
    const reasoningEffort = ["high", "medium", "low", "minimal"].includes(
      reasoningRaw
    )
      ? reasoningRaw
      : "high";
    const backendRaw = ($("provFormApiBackend")?.value || "responses").trim();
    const apiBackend =
      backendRaw === "chat" || backendRaw === "chat_completions"
        ? "chat_completions"
        : "responses";
    const cwRaw = ($("provFormContextWindow")?.value || "").trim();
    let contextWindow = null;
    if (cwRaw) {
      const n = Number.parseInt(cwRaw, 10);
      if (Number.isFinite(n) && n > 0) contextWindow = n;
    }

    let category = formCategory || "custom";
    if (category === "official" && !formIsOfficial) category = "custom";
    if (formIsOfficial) category = "official";

    // Critical: only send config.toml when user explicitly edited advanced mode.
    // Otherwise form fields win (fixes "edit then save has no effect").
    const useConfigToml = advancedDirty && !!configTomlRaw;

    // Prefer first mapped model when the default model field is empty.
    let modelOut = model;
    if (!useConfigToml && !modelOut && modelCatalog.length) {
      modelOut = modelCatalog[0].model || null;
    }

    // Always merge default model into catalog.
    // Prevents "only GPT shows" when user only filled the single model field.
    if (!isGrok() && !formIsOfficial && modelOut) {
      const has = modelCatalog.some((r) => r.model === modelOut);
      if (!has) {
        modelCatalog = [
          { model: modelOut, displayName: modelOut },
          ...modelCatalog,
        ];
      }
    }

    return {
      name,
      websiteUrl: websiteRaw || null,
      baseUrl: useConfigToml ? null : baseUrl,
      model: useConfigToml ? null : modelOut,
      apiKey,
      notes,
      configToml: useConfigToml ? configTomlRaw : null,
      useConfigToml,
      wireApi: isGrok() || formIsOfficial || useConfigToml ? null : wireApi,
      reasoningEffort:
        isGrok() || formIsOfficial || useConfigToml ? null : reasoningEffort,
      profile: null,
      modelDisplayName: null,
      apiBackend:
        !isGrok() || formIsOfficial || useConfigToml ? null : apiBackend,
      contextWindow:
        !isGrok() || formIsOfficial || useConfigToml ? null : contextWindow,
      customUserAgent: formIsOfficial ? null : customUserAgent,
      localProxyHeadersJson: formIsOfficial ? null : localProxyHeadersJson,
      localProxyBodyJson: formIsOfficial ? null : localProxyBodyJson,
      modelCatalog:
        isGrok() || formIsOfficial ? null : modelCatalog,
      keepExistingApiKey: true,
      category: formIsOfficial ? "official" : category,
    };
  }

  /**
   * Client-side checks before IPC. Returns error message or null.
   * @param {object} req
   * @param {boolean} activate
   */
  function validateClient(req, activate) {
    clearFieldErrors();
    if (!req.name) {
      markFieldError("provFormName", true);
      return "请填写供应商名称";
    }
    if (formIsOfficial) return null;

    const hasToml = !!(req.configToml && req.configToml.trim());
    if (!hasToml) {
      if (!req.baseUrl) {
        markFieldError("provFormBaseUrl", true);
        return activate
          ? "启用前请填写 Base URL"
          : "请填写 Base URL（或展开高级模式粘贴完整 TOML）";
      }
      if (
        !(
          req.baseUrl.startsWith("http://") ||
          req.baseUrl.startsWith("https://")
        )
      ) {
        markFieldError("provFormBaseUrl", true);
        return "Base URL 必须以 http:// 或 https:// 开头";
      }
    }

    if (req.useConfigToml) {
      if (!req.configToml || !req.configToml.trim()) {
        return "高级模式已启用，请填写 config.toml";
      }
    }

    if (activate) {
      if (!editingId && !req.apiKey) {
        markFieldError("provFormApiKey", true);
        return "启用前请填写 API Key";
      }
      // edit + activate without new key is OK if keepExistingApiKey
    }

    if (isGrok() && !req.useConfigToml && req.contextWindow != null) {
      if (!(Number.isFinite(req.contextWindow) && req.contextWindow > 0)) {
        markFieldError("provFormContextWindow", true);
        return "context_window 必须是正整数";
      }
    }

    if (req.websiteUrl) {
      try {
        // eslint-disable-next-line no-new
        new URL(req.websiteUrl);
      } catch {
        markFieldError("provFormWebsite", true);
        return "网站地址不是合法 URL（可留空）";
      }
    }
    return null;
  }

  async function submitForm(activate) {
    if (formBusy) return;
    if (!window.providerAPI) {
      setFormError("providerAPI 未加载，请通过 npm run dev 启动应用");
      return;
    }

    const req = collectForm();
    const clientErr = validateClient(req, activate);
    if (clientErr) {
      setFormError(clientErr);
      toast(clientErr, "error");
      return;
    }
    clearFormError();

    if (activate) {
      const mapCount = Array.isArray(req.modelCatalog)
        ? req.modelCatalog.length
        : 0;
      const ok = await confirm({
        title: "保存并启用",
        message: isGrok()
          ? `将保存并启用「${req.name}」，写入 Grok 本机配置。`
          : `将保存并启用「${req.name}」，写入 Codex 本机配置。` +
            (mapCount
              ? `\n已包含 ${mapCount} 个模型映射。`
              : "\n尚未添加模型映射，启用后可再编辑补充。"),
        confirmText: "保存并启用",
        variant: "primary",
      });
      if (!ok) return;
    }

    formBusy = true;
    const btnSave = $("btnProviderFormSave");
    const btnEnable = $("btnProviderFormSaveEnable");
    if (btnSave) btnSave.disabled = true;
    if (btnEnable) btnEnable.disabled = true;

    try {
      const payload = {
        name: req.name,
        websiteUrl: req.websiteUrl,
        baseUrl: req.baseUrl,
        model: req.model,
        apiKey: req.apiKey,
        notes: req.notes,
        configToml: req.configToml,
        useConfigToml: !!req.useConfigToml,
        wireApi: req.wireApi,
        reasoningEffort: req.reasoningEffort,
        profile: req.profile,
        modelDisplayName: req.modelDisplayName,
        apiBackend: req.apiBackend,
        contextWindow: req.contextWindow,
        customUserAgent: req.customUserAgent,
        localProxyHeadersJson: req.localProxyHeadersJson,
        localProxyBodyJson: req.localProxyBodyJson,
        modelCatalog: req.modelCatalog,
        keepExistingApiKey: true,
        category: formIsOfficial ? undefined : req.category,
        activate: !!activate,
      };

      let result;
      if (editingId) {
        result = await window.providerAPI.update(app, editingId, payload);
        toast(activate ? "已保存并启用" : "已保存供应商", "ok");
      } else {
        result = await window.providerAPI.add(app, payload);
        toast(activate ? "已添加并启用" : "已添加供应商", "ok");
      }
      // Unlock / catalog projection: backend best-effort, silent (no inject toasts).
      if (activate && !isGrok()) {
        try {
          await window.providerAPI.refreshModelUnlock?.();
        } catch {
          /* ignore */
        }
      }
      void result;
      closeForm();
      await loadList();
    } catch (err) {
      const msg = err?.message || String(err);
      setFormError(msg);
      toast(msg, "error");
      // highlight likely fields from backend messages
      if (/base.?url|Base URL/i.test(msg)) markFieldError("provFormBaseUrl", true);
      if (/api.?key|API Key/i.test(msg)) markFieldError("provFormApiKey", true);
      if (/名称|name/i.test(msg)) markFieldError("provFormName", true);
      if (/context_window|上下文/i.test(msg))
        markFieldError("provFormContextWindow", true);
      if (/config\.toml|TOML|高级/i.test(msg)) {
        const advanced = $("provFormAdvanced");
        if (advanced) advanced.open = true;
      }
    } finally {
      formBusy = false;
      if (btnSave) btnSave.disabled = false;
      if (btnEnable) btnEnable.disabled = false;
    }
  }

  async function onSwitch(id) {
    const p = (listPayload?.providers || []).find((x) => x.id === id);
    if (p && p.ready === false && p.category !== "official") {
      toast("该供应商尚未就绪：请先编辑并填写 Base URL 与 API Key", "error");
      return;
    }
    const ok = await confirm({
      title: "启用供应商",
      message: isGrok()
        ? `将启用「${p?.name || id}」并写入 Grok 本机配置。`
        : `将启用「${p?.name || id}」并写入 Codex 本机配置。`,
      confirmText: "启用",
      variant: "primary",
    });
    if (!ok) return;
    try {
      const res = await window.providerAPI.switch(app, id);
      // Only surface actionable warnings (e.g. empty model map); never inject/catalog dumps.
      const warn = (res?.warnings || [])
        .filter(Boolean)
        .filter((w) => !/注入|白名单|catalog|model_catalog|CDP|experimental_bearer|auth\.json|config\.toml|CLI\s*\/model/i.test(w));
      toast(res?.message || "已启用", "ok");
      if (warn.length) {
        setTimeout(() => toast(warn[0], "warn"), 400);
      }
      await loadList();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function onEdit(id) {
    try {
      if (!window.providerAPI) throw new Error("providerAPI 未加载");
      const detail = await window.providerAPI.get(app, id);
      if (!detail) throw new Error("未返回供应商详情");
      openForm("edit", detail);
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function onDelete(id) {
    const p = (listPayload?.providers || []).find((x) => x.id === id);
    const ok = await confirm({
      title: "删除供应商",
      message: `确定删除「${p?.name || id}」？此操作不可恢复（不会修改本机正在使用的配置，仅删除本工具中的档案）。`,
      confirmText: "删除",
      variant: "danger",
    });
    if (!ok) return;
    try {
      await window.providerAPI.remove(app, id);
      toast("已删除", "ok");
      await loadList();
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function onImportLive() {
    if (importBusy) return;
    const ok = await confirm({
      title: "从本机配置导入",
      message: isGrok()
        ? "读取 Grok 本机配置，另存为一条供应商档案（不会自动启用）。同一渠道已导入过则不会再创建。"
        : "读取 Codex 本机配置，另存为一条供应商档案（不会自动启用）。同一渠道已导入过则不会再创建。",
      confirmText: "导入",
    });
    if (!ok) return;
    const btn = $("btnProvImportLive");
    importBusy = true;
    if (btn) btn.disabled = true;
    try {
      const created = await window.providerAPI.importLive(app);
      toast(`已导入：${created?.name || "新供应商"}`, "ok");
      await loadList();
    } catch (err) {
      toast(err?.message || String(err), "error");
    } finally {
      importBusy = false;
      if (btn) btn.disabled = false;
    }
  }

  async function onAddClick() {
    try {
      await loadPresets();
    } catch {
      /* ignore */
    }
    openForm("add", null);
  }

  function isRouteModalOpen() {
    const m = $("provRouteModal");
    return !!(m && !m.hidden && m.classList.contains("show"));
  }

  function onFormKeydown(e) {
    const confirmModal = $("confirmModal");
    if (confirmModal && !confirmModal.hidden) return;

    if (e.key === "Escape") {
      if (isRouteModalOpen()) {
        e.preventDefault();
        e.stopPropagation();
        closeRouteModal();
        return;
      }
      if (isFormOpen()) {
        e.preventDefault();
        e.stopPropagation();
        if (!formBusy) closeForm();
      }
      return;
    }
    if (!isFormOpen()) return;
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      const enableBtn = $("btnProviderFormSaveEnable");
      if (enableBtn && !enableBtn.hidden && !enableBtn.disabled) {
        submitForm(true);
      } else {
        submitForm(false);
      }
    }
  }

  function bind() {
    if (bound) return;
    bound = true;

    // Mount shared dropdowns once (sessions uses the same UiSelect).
    if (window.UiSelect?.mountAll) {
      window.UiSelect.mountAll($("providerFormModal") || document);
    }

    $("provTabCodex")?.addEventListener("click", () => {
      if (app === SOURCE_CODEX) return;
      app = SOURCE_CODEX;
      persistApp();
      setTabsActive();
      closeForm();
      loadList();
    });
    $("provTabGrok")?.addEventListener("click", () => {
      if (app === SOURCE_GROK) return;
      app = SOURCE_GROK;
      persistApp();
      setTabsActive();
      closeForm();
      loadList();
    });

    $("provPreserveCodexAuth")?.addEventListener("change", async (e) => {
      const enabled = !!e.target?.checked;
      try {
        if (!window.providerAPI?.setPreserveCodexAuth) {
          throw new Error("providerAPI 未加载");
        }
        const next = await window.providerAPI.setPreserveCodexAuth(enabled);
        if (listPayload) listPayload.preserveCodexOfficialAuth = !!next;
        toast(
          enabled
            ? "已开启：第三方启用将保留 auth.json 官方登录"
            : "已关闭：第三方启用会写入 auth.json（可能覆盖官方登录）",
          enabled ? "ok" : "warn"
        );
      } catch (err) {
        e.target.checked = !enabled;
        toast(err?.message || String(err), "error");
      }
    });

    $("btnProvRefresh")?.addEventListener("click", () => {
      if (loading) return;
      loadList({ refresh: true });
    });
    $("btnProvAdd")?.addEventListener("click", () => {
      onAddClick().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("btnProvImportLive")?.addEventListener("click", () => onImportLive());

    $("provList")?.addEventListener("click", (e) => {
      const btn = e.target.closest?.("[data-act]");
      if (!btn || btn.disabled) return;
      const act = btn.getAttribute("data-act");
      const id = btn.getAttribute("data-id");
      if (!id) return;
      if (act === "switch") onSwitch(id);
      else if (act === "edit") onEdit(id);
      else if (act === "delete") onDelete(id);
      else if (act === "failover") onToggleFailover(id);
    });

    $("provRouteToggle")?.addEventListener("click", () => {
      onToggleRoute().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("btnProvRouteSettings")?.addEventListener("click", () => openRouteModal());
    $("btnProvRouteLogs")?.addEventListener("click", () => {
      openLogsModal().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("btnProvRouteModalClose")?.addEventListener("click", () => closeRouteModal());
    $("btnProvRouteModalCancel")?.addEventListener("click", () => closeRouteModal());
    $("provRouteModal")?.addEventListener("click", (e) => {
      if (e.target === $("provRouteModal") && !formBusy) closeRouteModal();
    });
    $("btnProvLogsModalClose")?.addEventListener("click", () => closeLogsModal());
    $("btnProvLogsDetailClose")?.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      hideDetail();
    });
    $("provLogsModal")?.addEventListener("click", (e) => {
      if (e.target === $("provLogsModal") && !formBusy) closeLogsModal();
    });
    // Esc: first collapse detail, then close modal
    document.addEventListener("keydown", (e) => {
      if (e.key !== "Escape") return;
      const modal = $("provLogsModal");
      if (!modal || modal.hidden || !modal.classList.contains("show")) return;
      if (isDetailVisible()) {
        e.preventDefault();
        e.stopPropagation();
        hideDetail();
      }
    }, true);
    $("btnProvLogsRefresh")?.addEventListener("click", () => {
      loadLogs().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("btnProvLogsClear")?.addEventListener("click", () => {
      onClearLogs().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("btnProvLogsRetentionSave")?.addEventListener("click", () => {
      onSaveRetention().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("provLogsFilterApp")?.addEventListener("change", () => {
      $("provLogsFilterApp").dataset.userTouched = "1";
      logsState.page = 0;
      loadLogs().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("provLogsFilterStatus")?.addEventListener("change", () => {
      logsState.page = 0;
      loadLogs().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("provLogsFilterQ")?.addEventListener("input", () => {
      if (logsState.searchTimer) clearTimeout(logsState.searchTimer);
      logsState.searchTimer = setTimeout(() => {
        logsState.page = 0;
        loadLogs().catch((err) => toast(err?.message || String(err), "error"));
      }, 300);
    });
    $("btnProvLogsPrev")?.addEventListener("click", () => {
      if (logsState.page <= 0) return;
      logsState.page -= 1;
      loadLogs().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("btnProvLogsNext")?.addEventListener("click", () => {
      const totalPages = Math.max(1, Math.ceil(logsState.total / logsState.pageSize) || 1);
      if (logsState.page + 1 >= totalPages) return;
      logsState.page += 1;
      loadLogs().catch((err) => toast(err?.message || String(err), "error"));
    });
    $("provLogsTbody")?.addEventListener("click", (e) => {
      const tr = e.target?.closest?.("tr[data-log-id]");
      if (!tr) return;
      onSelectLogRow(tr.getAttribute("data-log-id")).catch((err) =>
        toast(err?.message || String(err), "error")
      );
    });
    $("provLogsTbody")?.addEventListener("keydown", (e) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      const tr = e.target?.closest?.("tr[data-log-id]");
      if (!tr) return;
      e.preventDefault();
      onSelectLogRow(tr.getAttribute("data-log-id")).catch((err) =>
        toast(err?.message || String(err), "error")
      );
    });
    $("provFoQueueList")?.addEventListener("click", (e) => {
      const btn = e.target?.closest?.("[data-fo-act]");
      if (!btn) return;
      e.preventDefault();
      e.stopPropagation();
      const act = btn.getAttribute("data-fo-act");
      const id = btn.getAttribute("data-id");
      if (act && id) {
        onFoQueueAction(act, id).catch((err) =>
          toast(err?.message || String(err), "error")
        );
      }
    });
    $("btnProvFoQueueAdd")?.addEventListener("click", async () => {
      const id = $("provFoQueueAddSelect")?.value;
      if (!id) {
        toast("请选择要加入队列的供应商", "error");
        return;
      }
      try {
        await window.providerAPI.addToFailover(app, id);
        toast("已加入故障转移队列", "ok");
        await loadList();
        await refreshFoQueuePanel();
      } catch (err) {
        toast(err?.message || String(err), "error");
      }
    });
    $("btnProvRouteSaveCfg")?.addEventListener("click", () => {
      onSaveRouteSettings().catch((err) =>
        toast(err?.message || String(err), "error")
      );
    });
    $("btnProvRouteCheckPort")?.addEventListener("click", () => {
      onCheckPort().catch((err) => toast(err?.message || String(err), "error"));
    });

    $("provFormPreset")?.addEventListener("change", (e) => {
      const id = e.target?.value || "";
      if (id) applyPreset(id);
      else {
        formCategory = "custom";
        const hint = $("provFormPresetHint");
        if (hint) {
          hint.textContent = isGrok()
            ? "选择常见渠道可自动填充 Base URL、模型、api_backend（Grok 独立预设）。"
            : "选择常见渠道可自动填充 Base URL、模型与 wire_api（Codex 独立预设）。";
        }
      }
    });

    // Clear field error on input; structured edits drop advanced mode preference
    [
      "provFormName",
      "provFormBaseUrl",
      "provFormModel",
      "provFormApiKey",
      "provFormWebsite",
      "provFormContextWindow",
    ].forEach((id) => {
      $(id)?.addEventListener("input", () => {
        markFieldError(id, false);
        clearFormError();
        if (
          id === "provFormBaseUrl" ||
          id === "provFormContextWindow"
        ) {
          markStructuredEdit();
        }
      });
    });
    ["provFormWireApi", "provFormReasoning", "provFormApiBackend"].forEach(
      (id) => {
        $(id)?.addEventListener("change", () => {
          clearFormError();
          markStructuredEdit();
        });
      }
    );

    $("provFormConfigToml")?.addEventListener("input", () => {
      advancedDirty = true;
      updateAdvancedHint();
      clearFormError();
    });

    $("btnProviderFormCancel")?.addEventListener("click", () => {
      if (!formBusy) closeForm();
    });
    $("btnProviderFormClose")?.addEventListener("click", () => {
      if (!formBusy) closeForm();
    });
    $("btnProviderFormSave")?.addEventListener("click", () => submitForm(false));
    $("btnProviderFormSaveEnable")?.addEventListener("click", () =>
      submitForm(true)
    );
    $("btnProvTestConn")?.addEventListener("click", () => {
      onTestConnectivity().catch((err) =>
        toast(err?.message || String(err), "error")
      );
    });
    const onFetchClick = () => {
      onFetchModels().catch((err) =>
        toast(err?.message || String(err), "error")
      );
    };
    $("btnProvFetchModels")?.addEventListener("click", onFetchClick);
    $("btnProvFetchModelsMain")?.addEventListener("click", onFetchClick);
    $("btnProvToggleApiKey")?.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      toggleApiKeyVisible();
    });
    $("provFormUserAgentPreset")?.addEventListener("change", (e) => {
      const v = e.target?.value || "";
      if (v) {
        setField("provFormUserAgent", v);
        e.target.value = "";
      }
    });
    $("btnProvCatalogAdd")?.addEventListener("click", () => addCatalogRow());
    $("provCatalogBody")?.addEventListener("change", (e) => {
      const t = e.target;
      if (t?.getAttribute?.("data-cat") === "model") {
        onCatalogModelIdChange(t);
      }
    });
    $("provCatalogBody")?.addEventListener("blur", (e) => {
      const t = e.target;
      if (t?.getAttribute?.("data-cat") === "model") {
        onCatalogModelIdChange(t);
      }
    }, true);
    $("provFormModel")?.addEventListener("change", () => {
      if (!isGrok() || formIsOfficial) return;
      const cwEl = $("provFormContextWindow");
      if (!cwEl) return;
      if ((cwEl.value || "").trim()) return;
      const guessed = guessContextWindow(($("provFormModel")?.value || "").trim());
      if (guessed) {
        cwEl.value = String(guessed);
        markStructuredEdit();
      }
    });
    // Wire/reasoning/apiBackend/model selects: keep ui-select label in sync after programmatic sets
    ["provFormWireApi", "provFormReasoning", "provFormApiBackend", "provFormModel"].forEach(
      (id) => {
        $(id)?.addEventListener("change", () => {
          if (window.UiSelect?.refresh) window.UiSelect.refresh($(id));
        });
      }
    );
    $("provCatalogBody")?.addEventListener("click", (e) => {
      const btn = e.target?.closest?.("[data-cat-del]");
      if (!btn) return;
      btn.closest("tr")?.remove();
    });
    // Changing base URL invalidates fetched model suggestions
    $("provFormBaseUrl")?.addEventListener("change", () => {
      if (fetchedModels.length) {
        clearFetchedModels();
        setProbeStatus(
          "provModelStatus",
          "Base URL 已变更，已清空模型建议；需要时可重新拉取。",
          "warn"
        );
      }
      setProbeStatus("provConnStatus", "", "");
    });
    $("providerFormModal")?.addEventListener("click", (e) => {
      if (e.target?.id === "providerFormModal" && !formBusy) closeForm();
    });
    // Capture so we run before app.js help/wallpaper Escape handlers if needed
    document.addEventListener("keydown", onFormKeydown, true);
  }

  function enter() {
    bind();
    setTabsActive();
    updateLead();
    loadList();
  }

  function leave() {
    closeForm();
  }

  /** Reload list after tray / external provider switch (no-op if not mounted). */
  function reload() {
    if (!bound) return;
    loadList();
  }

  window.providersView = { enter, leave, reload };
})();
