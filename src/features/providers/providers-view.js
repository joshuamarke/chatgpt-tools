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
  let fetchModelsSeq = 0;

  function $(id) {
    return document.getElementById(id);
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
      ? "管理 Grok Build 供应商。内置 Grok Official 走官方登录与默认模型；自定义渠道写 [model.*]，启用时写入 ~/.grok/config.toml（保留 MCP）。"
      : "管理 Codex 供应商。内置 OpenAI Official 走 ChatGPT / Platform 官方路由；第三方渠道启用时写入 ~/.codex/auth.json + config.toml。";
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
        : "尚未选择启用的供应商";
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
    let state = "warn";
    if (!exists) {
      state = "off";
    } else if (matches) {
      state = "ok";
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
      // Only show base_url (truncate visually via CSS); full URL in title.
      const base = (live.baseUrl || "").trim();
      if (base) {
        const safe = escapeHtml(base);
        meta.innerHTML = `<span class="prov-live-chip" title="${safe}">${safe}</span>`;
      } else {
        meta.innerHTML = "";
      }
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
    list.innerHTML = providers
      .map((p) => {
        const active = p.isCurrent ? "is-current" : "";
        const badge = p.isCurrent
          ? `<span class="prov-badge prov-badge-on">使用中</span>`
          : "";
        const cat = `<span class="prov-badge">${escapeHtml(categoryLabel(p.category))}</span>`;
        const readyBadge =
          p.category === "official" || p.ready
            ? ""
            : `<span class="prov-badge prov-badge-warn" title="缺少 Base URL 或 API Key">未就绪</span>`;
        const driftBadge =
          p.isCurrent && p.matchesLive === false
            ? `<span class="prov-badge prov-badge-warn" title="与本机正在使用的配置不一致">本机漂移</span>`
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
          !p.isCurrent && (p.category === "official" || p.ready !== false);
        const switchBtn = p.isCurrent
          ? ""
          : `<button type="button" class="chip-btn chip-primary prov-act" data-act="switch" data-id="${escapeHtml(p.id)}" ${canSwitch ? "" : "disabled"} title="${canSwitch ? "写入本机配置并启用" : "请先补全 Base URL 与 API Key"}">启用</button>`;
        const delBtn =
          p.category === "official"
            ? ""
            : `<button type="button" class="chip-btn chip-danger prov-act" data-act="delete" data-id="${escapeHtml(p.id)}" ${p.isCurrent ? "disabled" : ""}>删除</button>`;
        return `
<article class="prov-card ${active}" data-id="${escapeHtml(p.id)}">
  <div class="${providerIconClass(p)}" aria-hidden="true">${providerIconSvg(p)}</div>
  <div class="prov-card-main">
    <div class="prov-card-title-row">
      <h3 class="prov-card-name" title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</h3>
      ${badge}${cat}${readyBadge}${driftBadge}
    </div>
    ${meta}
    ${notes}
  </div>
  <div class="prov-card-actions">
    ${switchBtn}
    <button type="button" class="chip-btn prov-act" data-act="edit" data-id="${escapeHtml(p.id)}">编辑</button>
    ${delBtn}
  </div>
</article>`;
      })
      .join("");
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

  async function loadList() {
    const seq = ++loadSeq;
    loading = true;
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
      renderList();
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

    // Grok-specific defaults from preset
    if (isGrok()) {
      setField("provFormProfile", p.model || "grok-4.5");
      const backend = $("provFormApiBackend");
      if (backend) {
        backend.value =
          p.wireApi === "chat" || p.wireApi === "chat_completions"
            ? "chat_completions"
            : "responses";
      }
      setField("provFormContextWindow", "500000");
    } else {
      // Codex: seed model mapping so enable writes model_catalog_json
      const m = (p.model || "").trim();
      if (m) renderCatalogRows([{ model: m, displayName: m }]);
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
      ? "已修改 config.toml：保存时会把此处内容叠加到完整配置上（保留 MCP / desktop 等未改段落），并优先于上方结构化字段。"
      : "显示完整配置（含 MCP 等）。未改动时保存仍以结构化字段为准，并自动保留其余段落。";
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
      modelLabel.textContent = isGrok() ? "上游模型 id" : "模型";
    }
    if (modelHint) {
      modelHint.hidden = false;
      if (isGrok()) {
        modelHint.textContent =
          "写入 [model.*].model。可手填；点右侧「拉取模型」获取列表并填入第一项。";
      } else {
        modelHint.textContent =
          "写入 Codex 顶层 model。完整列表请在下方「模型映射」中拉取或添加。";
      }
    }

    const wire = $("provFormWireApi");
    if (wire) wire.disabled = lock || isGrok();
    const reasoning = $("provFormReasoning");
    if (reasoning) reasoning.disabled = lock || isGrok();
    ["provFormProfile", "provFormApiBackend", "provFormContextWindow"].forEach(
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

  function setField(id, value) {
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
      "provFormProfile",
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

    setField(
      "provFormProfile",
      detail?.profile || detail?.model || "grok-4.5"
    );
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
        keyHint.textContent =
          "编辑时会回显已保存的 Key。必填才能启用；写入 ~/.grok/config.toml 的 [model.*].api_key";
      } else {
        keyHint.textContent =
          "编辑时会回显已保存的 Key。必填才能启用；写入 ~/.codex/auth.json（OPENAI_API_KEY）";
      }
    }

    const modelInput = $("provFormModel");
    if (modelInput) {
      modelInput.placeholder = isGrok() ? "grok-4.5" : "gpt-5.5";
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
      "provFormProfile",
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

  // ── Connectivity + model list (ported from cc-switch) ───────────────────

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
    const model = escapeHtml(row?.model || "");
    const display = escapeHtml(row?.displayName || row?.display_name || "");
    const cw =
      row?.contextWindow || row?.context_window
        ? String(row.contextWindow || row.context_window)
        : "";
    return `<tr data-catalog-idx="${index}">
      <td><input type="text" data-cat="model" value="${model}" placeholder="model-id" spellcheck="false" autocomplete="off" /></td>
      <td><input type="text" data-cat="displayName" value="${display}" placeholder="可选" spellcheck="false" autocomplete="off" /></td>
      <td><input type="number" data-cat="contextWindow" value="${escapeHtml(
        cw
      )}" placeholder="128000" min="1" step="1" inputmode="numeric" /></td>
      <td><button type="button" class="prov-catalog-del" data-cat-del title="删除行" aria-label="删除行">×</button></td>
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
    // projects model_catalog_json (cc-switch: mapping drives /model list).
    if (!rows.length) {
      const fallback = ($("provFormModel")?.value || "").trim();
      if (fallback) rows.push({ model: fallback, displayName: fallback });
    }
    return rows;
  }

  function addCatalogRow(seed) {
    const body = $("provCatalogBody");
    if (!body) return;
    const row = seed || {
      model: ($("provFormModel")?.value || "").trim() || "",
      displayName: "",
    };
    body.insertAdjacentHTML(
      "beforeend",
      catalogRowHtml(row, body.children.length)
    );
  }

  /** Clear model datalist suggestions; keep current free-text value. */
  function clearFetchedModels() {
    fetchedModels = [];
    const list = $("provFormModelList");
    if (list) list.innerHTML = "";
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
      if (prev?.contextWindow) row.contextWindow = prev.contextWindow;
      return row;
    });
    renderCatalogRows(rows);
    markStructuredEdit();
    return rows.length;
  }

  /**
   * Fill editable model input + datalist suggestions (id only, no ownedBy suffix).
   * For Codex, also project the full list into the model-mapping table (Codex++
   * model_list style — catalog must contain every model that should appear in /model).
   * @param {any} models
   * @param {{selectFirst?: boolean, fillCatalog?: boolean}} [opts]
   * @returns {{ count: number, catalogRows: number, models: Array<{id:string,ownedBy?:string}> }}
   */
  function applyFetchedModels(models, opts) {
    const selectFirst = opts?.selectFirst !== false;
    const fillCatalog = opts?.fillCatalog !== false;
    fetchedModels = normalizeFetchedModels(models);
    const list = $("provFormModelList");
    const input = $("provFormModel");
    if (list) {
      // Only model id — no "· openai" / ownedBy suffix.
      list.innerHTML = fetchedModels
        .map((m) => `<option value="${escapeHtml(m.id)}"></option>`)
        .join("");
    }
    if (selectFirst && fetchedModels.length && input) {
      input.value = fetchedModels[0].id;
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
    const modelCatalog = collectCatalogRows();
    const configTomlRaw = ($("provFormConfigToml")?.value || "").trim();
    const wireRaw = ($("provFormWireApi")?.value || "responses").trim();
    const wireApi = wireRaw === "chat" ? "chat" : "responses";
    const reasoningRaw = ($("provFormReasoning")?.value || "high").trim();
    const reasoningEffort = ["high", "medium", "low", "minimal"].includes(
      reasoningRaw
    )
      ? reasoningRaw
      : "high";
    const profile = ($("provFormProfile")?.value || "").trim() || null;
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

    // Prefer first mapped model when the default model field is empty (cc-switch).
    let modelOut = model;
    if (!useConfigToml && !modelOut && modelCatalog.length) {
      modelOut = modelCatalog[0].model || null;
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
      profile: !isGrok() || formIsOfficial || useConfigToml ? null : profile,
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
      const ok = await confirm({
        title: "保存并启用",
        message:
          `将保存「${req.name}」并写回本机正在使用的配置：\n` +
          (isGrok()
            ? "· Grok：~/.grok/config.toml（尽量保留 MCP 段）\n"
            : "· Codex：~/.codex/auth.json + config.toml（含 wire_api）\n") +
          `\n写入后通常需要重启对应客户端 / CLI 才能完全生效。`,
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

      if (editingId) {
        await window.providerAPI.update(app, editingId, payload);
        toast(activate ? "已保存并启用" : "已保存供应商", "ok");
      } else {
        await window.providerAPI.add(app, payload);
        toast(activate ? "已添加并启用" : "已添加供应商", "ok");
      }
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
      title: "切换供应商",
      message:
        `将启用「${p?.name || id}」并写回本机正在使用的配置。\n\n` +
        (isGrok()
          ? "Grok：写入 ~/.grok/config.toml（尽量保留现有 MCP 段）。\n"
          : "Codex：双写 auth.json（API Key）与 config.toml（含 wire_api）。\n") +
        `切换后通常需要重启对应客户端 / CLI 才能完全生效。`,
      confirmText: "启用",
      variant: "primary",
    });
    if (!ok) return;
    try {
      const res = await window.providerAPI.switch(app, id);
      const warn = (res?.warnings || []).filter(Boolean);
      toast(res?.message || "已切换", warn.length ? "warn" : "ok");
      if (warn.length) {
        setTimeout(() => toast(warn.join("；"), "warn"), 500);
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
    const ok = await confirm({
      title: "从本机配置导入",
      message:
        `读取本机正在使用的配置，另存为一条供应商档案（不会自动启用）：\n\n` +
        (isGrok()
          ? "· 路径：~/.grok/config.toml\n· 内容：当前默认模型 / base_url / api_key 等"
          : "· 路径：~/.codex/auth.json + config.toml\n· 内容：当前 model_provider / base_url / API Key 等"),
      confirmText: "导入",
    });
    if (!ok) return;
    try {
      const created = await window.providerAPI.importLive(app);
      toast(`已导入：${created?.name || "新供应商"}`, "ok");
      await loadList();
    } catch (err) {
      toast(err?.message || String(err), "error");
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

  function onFormKeydown(e) {
    if (!isFormOpen()) return;
    // Confirm dialog uses capture Escape; if confirm is open, let it win
    const confirmModal = $("confirmModal");
    if (confirmModal && !confirmModal.hidden) return;

    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (!formBusy) closeForm();
      return;
    }
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

    $("btnProvRefresh")?.addEventListener("click", () => loadList());
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
      "provFormProfile",
      "provFormContextWindow",
    ].forEach((id) => {
      $(id)?.addEventListener("input", () => {
        markFieldError(id, false);
        clearFormError();
        if (
          id === "provFormBaseUrl" ||
          id === "provFormModel" ||
          id === "provFormProfile" ||
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
    // Wire/reasoning/apiBackend selects: keep ui-select label in sync after programmatic sets
    ["provFormWireApi", "provFormReasoning", "provFormApiBackend"].forEach(
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

  window.providersView = { enter, leave };
})();
