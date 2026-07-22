const grid = document.getElementById("skinGrid");
const toast = document.getElementById("toast");
const overlay = document.getElementById("overlay");
const overlayText = document.getElementById("overlayText");
const pillCodex = document.getElementById("pillCodex");
const pillActive = document.getElementById("pillActive");
const chkRestart = document.getElementById("chkRestart");
const searchInput = document.getElementById("searchInput");
const resultCount = document.getElementById("resultCount");
const categoryNav = document.getElementById("categoryNav");

/** @type {{ skins: any[], codexRunning?: boolean, debugReady?: boolean } | null} */
let latestStatus = null;
/** @type {Record<string, any> | null} last host_status / status host fields */
let latestHost = null;
let activeCategory = "all";
let searchQuery = "";
/** Busy overlay active — pause host polling */
let uiBusy = false;
/** In-flight host_status promise (single-flight) */
let hostPollInflight = null;
let hostPollTimer = null;
let hostPollFailCount = 0;
/** Last skins signature so host-only updates do not rebuild the grid */
let lastSkinsSig = "";

/** 换肤时是否重启客户端：默认关闭（热换）；用户勾选后写入 localStorage */
const RESTART_PREF_KEY = "chatgpt-tools-restart-on-apply";
try {
  const savedRestart = localStorage.getItem(RESTART_PREF_KEY);
  if (chkRestart) {
    // Default off unless user previously opted in.
    chkRestart.checked = savedRestart === "1" || savedRestart === "true";
    chkRestart.addEventListener("change", () => {
      localStorage.setItem(RESTART_PREF_KEY, chkRestart.checked ? "1" : "0");
    });
  }
} catch {
  /* ignore storage errors */
}

/** 分类与皮肤 tags / 名称的匹配规则（对齐设计图侧栏） */
const CATEGORY_RULES = {
  all: () => true,
  anime: (skin) => matchAny(skin, ["动漫", "初音", "EVA", "虚拟歌姬", "初号机", "miku", "eva"]),
  tech: (skin) => matchAny(skin, ["科幻", "赛博", "霓虹", "科技", "极光", "cyber", "灵笼"]),
  nature: (skin) => matchAny(skin, ["自然", "风景", "水墨", "国风", "山水", "朱砂", "墨韵"]),
  game: (skin) => matchAny(skin, ["游戏", "修仙", "仙侠", "御剑", "凡人", "龙焰"]),
  art: (skin) => matchAny(skin, ["艺术", "创意", "粉紫", "梦幻", "dream", "Fiona"]),
  minimal: (skin) => matchAny(skin, ["简约", "极简", "干净", "minimal"]),
  favorite: (skin) => {
    try {
      const fav = JSON.parse(localStorage.getItem("chatgpt-tools-favorites") || "[]");
      return fav.includes(skin.id);
    } catch {
      return false;
    }
  },
};

function matchAny(skin, keywords) {
  const hay = `${skin.name || ""} ${(skin.tags || []).join(" ")} ${skin.description || ""} ${skin.id || ""}`.toLowerCase();
  return keywords.some((k) => hay.includes(String(k).toLowerCase()));
}

function getFavorites() {
  try {
    return new Set(JSON.parse(localStorage.getItem("chatgpt-tools-favorites") || "[]"));
  } catch {
    return new Set();
  }
}

function toggleFavorite(id) {
  const fav = getFavorites();
  if (fav.has(id)) fav.delete(id);
  else fav.add(id);
  localStorage.setItem("chatgpt-tools-favorites", JSON.stringify([...fav]));
}

function showToast(message, type = "") {
  toast.hidden = false;
  toast.className = `toast ${type}`.trim();
  toast.textContent = message;
  clearTimeout(showToast._t);
  showToast._t = setTimeout(() => {
    toast.hidden = true;
  }, 4200);
}

/**
 * 公共确认对话框（与现有 modal 主题一致）
 * @param {object|string} opts 文案配置，或直接传 message 字符串
 * @param {string} [opts.title="确认操作"]
 * @param {string} [opts.message=""]
 * @param {string} [opts.confirmText="确定"]
 * @param {string} [opts.cancelText="取消"]
 * @param {"primary"|"danger"|"warn"} [opts.variant="primary"] 确认按钮样式；warn/danger 使用警示图标
 * @returns {Promise<boolean>} 用户点确定为 true，取消/遮罩/Esc 为 false
 */
function showConfirm(opts = {}) {
  const options = typeof opts === "string" ? { message: opts } : opts || {};
  const {
    title = "确认操作",
    message = "",
    confirmText = "确定",
    cancelText = "取消",
    variant = "primary",
  } = options;

  const modal = document.getElementById("confirmModal");
  const titleEl = document.getElementById("confirmTitle");
  const msgEl = document.getElementById("confirmMessage");
  const iconEl = document.getElementById("confirmIcon");
  const btnOk = document.getElementById("btnConfirmOk");
  const btnCancel = document.getElementById("btnConfirmCancel");
  if (!modal || !btnOk || !btnCancel) {
    return Promise.resolve(window.confirm(message || title));
  }

  // 若已有未关闭的确认，先安全收口（清理监听并 resolve false）
  if (typeof showConfirm._dismiss === "function") {
    showConfirm._dismiss(false);
  }

  titleEl.textContent = title;
  msgEl.textContent = message;
  btnOk.textContent = confirmText;
  btnCancel.textContent = cancelText;

  const isDanger = variant === "danger";
  const isWarn = variant === "warn" || isDanger;
  iconEl.classList.toggle("warn", isWarn);
  btnOk.className = isDanger ? "danger" : "primary";

  return new Promise((resolve) => {
    let settled = false;

    const onOk = () => dismiss(true);
    const onCancel = () => dismiss(false);
    const onBackdrop = (e) => {
      if (e.target === modal) dismiss(false);
    };
    const onKey = (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        dismiss(false);
      } else if (e.key === "Enter") {
        const tag = (e.target && e.target.tagName) || "";
        if (tag === "TEXTAREA" || tag === "INPUT" || tag === "SELECT") return;
        e.preventDefault();
        dismiss(true);
      }
    };

    const cleanup = () => {
      document.removeEventListener("keydown", onKey, true);
      modal.removeEventListener("click", onBackdrop);
      btnOk.removeEventListener("click", onOk);
      btnCancel.removeEventListener("click", onCancel);
      modal.hidden = true;
      modal.classList.remove("show");
      if (showConfirm._dismiss === dismiss) showConfirm._dismiss = null;
    };

    const dismiss = (ok) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(Boolean(ok));
    };

    showConfirm._dismiss = dismiss;

    btnOk.addEventListener("click", onOk);
    btnCancel.addEventListener("click", onCancel);
    modal.addEventListener("click", onBackdrop);
    document.addEventListener("keydown", onKey, true);

    modal.hidden = false;
    modal.classList.add("show");
    // 焦点落到取消，降低误确认风险
    requestAnimationFrame(() => {
      try {
        btnCancel.focus();
      } catch {
        /* ignore */
      }
    });
  });
}

function setBusy(busy, text = "处理中…") {
  uiBusy = Boolean(busy);
  overlay.hidden = !busy;
  overlay.classList.toggle("show", busy);
  overlayText.textContent = text;
  document.querySelectorAll("button, input, select").forEach((el) => {
    if (el.closest(".win-controls") || el.id === "searchInput") return;
    if (el.tagName === "BUTTON" || el.type === "checkbox" || el.tagName === "SELECT" || el.tagName === "INPUT") {
      el.disabled = busy;
    }
  });
  // 窗口控件始终可用
  ["btnWinMin", "btnWinMax", "btnWinClose"].forEach((id) => {
    const b = document.getElementById(id);
    if (b) b.disabled = false;
  });
  if (!busy) {
    // Resume adaptive host polling after long ops
    scheduleHostPoll(400);
  }
}

function friendlyError(err) {
  const msg = err?.message || String(err || "");
  if (/未找到|not found|指定客户端|选择客户端|CODEX_APP_PATH/i.test(msg)) {
    return "未自动找到 ChatGPT/Codex。请在左侧点「选择客户端」选择 ChatGPT.exe，或先打开官方客户端后再试。";
  }
  if (/调试端口|CDP|remote-debugging|未打开调试/i.test(msg)) {
    return "未能打开调试端口。请勾选「自动重启」，先完全退出 ChatGPT/Codex 再点换肤（仅窗口开着但无调试口时无法注入立绘）。";
  }
  if (/校验|verify|verification|换肤未完成/i.test(msg)) {
    return "皮肤已尝试应用，但界面校验未通过，请稍后重试。";
  }
  return msg.replace(/\s{2,}/g, " ").trim() || "操作失败，请重试。";
}

function filterSkins(skins) {
  const rule = CATEGORY_RULES[activeCategory] || CATEGORY_RULES.all;
  const q = searchQuery.trim().toLowerCase();
  return skins.filter((skin) => {
    if (!rule(skin)) return false;
    if (!q) return true;
    const hay = `${skin.name || ""} ${(skin.tags || []).join(" ")} ${skin.description || ""} ${skin.id || ""}`.toLowerCase();
    return hay.includes(q);
  });
}

function escapeHtml(s) {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Map host snapshot → pill text / class / title (single source of truth). */
function mapHostPill(h) {
  const lifecycle = h?.lifecycle || "offline";
  const needsRestart = Boolean(
    h?.needsRestartForInject ||
      (h?.processRunning && !h?.debugPortOpen && lifecycle !== "ready")
  );
  const keep = h?.keepAlive ? " · 保持注入中" : "";
  const conf = h?.confidence === "probing" ? "（检测中）" : "";

  if (lifecycle === "ready") {
    return {
      text: `ChatGPT 已就绪${conf}`,
      cls: "pill ok",
      title: `可直接热切换皮肤${keep}`,
    };
  }
  if (lifecycle === "starting") {
    if (needsRestart) {
      return {
        text: "ChatGPT 运行中，换肤需重启",
        cls: "pill warn",
        title: "未打开调试端口，请勾选「自动重启」后换肤",
      };
    }
    return {
      text: `ChatGPT 启动中…${conf}`,
      cls: "pill warn",
      title: `等待渲染页就绪${keep}`,
    };
  }
  return {
    text: conf ? "ChatGPT 检测中…" : "ChatGPT 未打开",
    cls: conf ? "pill warn" : "pill",
    title: "打开官方客户端，或点左侧「选择客户端」指定路径",
  };
}

function mergeHostFields(target, host) {
  if (!target || !host) return target;
  const keys = [
    "lifecycle",
    "lifecycleRaw",
    "lifecycleLabel",
    "confidence",
    "codexRunning",
    "debugReady",
    "debugPortOpen",
    "processRunning",
    "rendererReady",
    "canHotApply",
    "needsRestartForInject",
    "keepAlive",
    "probeAgeMs",
    "signals",
    "hostPids",
  ];
  for (const k of keys) {
    if (host[k] !== undefined) target[k] = host[k];
  }
  return target;
}

function updateHostPill(host) {
  if (!host || !pillCodex) return;
  // While probing, keep previous label if we already had a stable one (less flicker)
  if (host.confidence === "probing" && latestHost?.lifecycle && host.lifecycle === latestHost.lifecycle) {
    const mapped = mapHostPill({ ...latestHost, ...host });
    pillCodex.textContent = mapped.text;
    pillCodex.className = mapped.cls;
    pillCodex.title = mapped.title;
    latestHost = { ...(latestHost || {}), ...host };
    return;
  }
  const mapped = mapHostPill(host);
  pillCodex.textContent = mapped.text;
  pillCodex.className = mapped.cls;
  pillCodex.title = mapped.title;
  latestHost = { ...(latestHost || {}), ...host };
  if (latestStatus) mergeHostFields(latestStatus, host);
}

function updateActivePill(status) {
  const skins = Array.isArray(status?.skins) ? status.skins : [];
  const active = skins.find((s) => s.active);
  const stateSkinId = status?.state?.skinId;
  const stateName =
    active?.name ||
    (stateSkinId ? skins.find((s) => s.id === stateSkinId)?.name : null);
  let activeExtra = "";
  if (status?.paused) activeExtra = "（已暂停）";
  else if (status?.artPending && (active || stateSkinId)) activeExtra = "（立绘加载中）";
  else if (status?.shellOk && status?.artOk === false && (active || stateSkinId))
    activeExtra = "（样式已注入）";
  pillActive.textContent = stateName
    ? `当前皮肤：${stateName}${activeExtra}`
    : "当前皮肤：无";
  pillActive.className = active || stateSkinId ? "pill ok" : "pill";
  syncPauseButton(status);
}

function syncPauseButton(status) {
  const btn = document.getElementById("btnPause");
  const label = document.getElementById("btnPauseLabel");
  if (!btn || !label) return;
  const hasSession = Boolean(status?.state?.skinId || status?.skins?.some((s) => s.active));
  const paused = Boolean(status?.paused);
  btn.disabled = !hasSession && !paused;
  label.textContent = paused ? "继续显示" : "暂停皮肤";
  btn.title = paused
    ? "清除暂停并重新应用当前皮肤"
    : "写入暂停标记并即时从 ChatGPT 窗口卸下皮肤（会话可恢复）";
  btn.classList.toggle("chip-warn", paused);
}

function skinsSignature(skins) {
  return (skins || [])
    .map((s) => `${s.id}:${s.active ? 1 : 0}:${s.previewUrl ? 1 : 0}`)
    .join("|");
}

function hostPollIntervalMs(lifecycle) {
  if (lifecycle === "starting") return 1200;
  if (lifecycle === "ready") return 5000;
  return 3500;
}

function scheduleHostPoll(delayMs) {
  if (hostPollTimer) {
    clearTimeout(hostPollTimer);
    hostPollTimer = null;
  }
  const ms =
    delayMs != null
      ? delayMs
      : hostPollIntervalMs(latestHost?.lifecycle || latestStatus?.lifecycle || "offline");
  hostPollTimer = setTimeout(() => {
    hostPollTimer = null;
    pollHostStatus(false).finally(() => {
      if (!uiBusy && document.visibilityState !== "hidden") {
        scheduleHostPoll();
      }
    });
  }, ms);
}

async function pollHostStatus(force = false) {
  if (uiBusy && !force) return latestHost;
  if (hostPollInflight) return hostPollInflight;
  hostPollInflight = (async () => {
    try {
      let host;
      if (typeof window.skinAPI.hostStatus === "function") {
        host = await window.skinAPI.hostStatus({ force });
      } else {
        // Fallback: full status is heavy — only use when host_status missing
        const st = await window.skinAPI.status();
        host = st;
        if (st?.skins) {
          latestStatus = st;
        }
      }
      hostPollFailCount = 0;
      updateHostPill(host);
      // Soft-update card active badges without full grid rebuild when only host changed
      if (latestStatus?.skins?.length && host?.lifecycle) {
        const engaged = host.lifecycle !== "offline";
        const skinId = latestStatus.state?.skinId;
        const paused = Boolean(latestStatus.paused);
        let changed = false;
        for (const s of latestStatus.skins) {
          const next = Boolean(skinId && s.id === skinId && !paused && engaged);
          if (Boolean(s.active) !== next) {
            s.active = next;
            changed = true;
          }
        }
        if (changed) {
          lastSkinsSig = "";
          renderSkins(latestStatus);
          updateActivePill(latestStatus);
        }
      }
      return host;
    } catch (err) {
      hostPollFailCount += 1;
      if (hostPollFailCount >= 3 && pillCodex) {
        pillCodex.textContent = "状态刷新失败";
        pillCodex.className = "pill warn";
        pillCodex.title = friendlyError(err);
      }
      return latestHost;
    } finally {
      hostPollInflight = null;
    }
  })();
  return hostPollInflight;
}

function render(status) {
  latestStatus = status || { skins: [] };
  if (status?.lifecycle || status?.codexRunning !== undefined) {
    updateHostPill(status);
  }
  updateActivePill(latestStatus);
  renderSkins(latestStatus);
}

function renderSkins(status) {
  const skins = Array.isArray(status?.skins) ? status.skins : [];
  const sig = skinsSignature(skins) + `|${activeCategory}|${searchQuery}|${[...getFavorites()].join(",")}`;
  // Always re-filter on category/search; skip DOM rebuild only when identical
  const favorites = getFavorites();
  const filtered = filterSkins(skins);
  resultCount.textContent = `共 ${filtered.length} 套皮肤`;

  if (sig === lastSkinsSig && grid.children.length) {
    return;
  }
  lastSkinsSig = sig;

  grid.innerHTML = "";
  if (!filtered.length) {
    grid.innerHTML = `<div class="empty-state">没有匹配的皮肤，试试其他分类或关键词</div>`;
    return;
  }

  for (const skin of filtered) {
    const card = document.createElement("article");
    card.className = `card${skin.active ? " active" : ""}`;
    const previewImg = skin.previewUrl
      ? `<img class="preview-img" src="${skin.previewUrl}" alt="${escapeHtml(skin.name)}" draggable="false" loading="lazy" />`
      : "";
    const isFav = favorites.has(skin.id);
    const useLabel = skin.active ? "重新应用" : "使用皮肤";
    const sourceLabel = skin.builtin ? "内置" : "已导入";
    const skinTags = (skin.tags || []).slice(0, 3);
    const canDelete = skin.source === "user" || !skin.builtin;
    const favIcon = isFav
      ? `<svg class="fav-icon" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="currentColor" d="M12 17.27 18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z"/></svg>`
      : `<svg class="fav-icon" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" d="M12 17.27 18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z"/></svg>`;

    card.innerHTML = `
      <div class="preview" style="background:${skin.previewGradient || "#eceff6"}">
        ${previewImg}
        ${skin.active ? `<span class="badge on">使用中</span>` : ""}
        <div class="preview-actions">
          <button type="button" class="use-btn" data-apply="${escapeHtml(skin.id)}" data-name="${escapeHtml(skin.name)}">${useLabel}</button>
          <button type="button" class="export-btn" data-export="${escapeHtml(skin.id)}" data-name="${escapeHtml(skin.name)}" title="导出">导出</button>
        </div>
      </div>
      <div class="meta">
        <div class="meta-title-row">
          <h2 title="${escapeHtml(skin.name)}">${escapeHtml(skin.name)}</h2>
          <div class="meta-title-actions">
            <button type="button" class="icon-btn fav-btn${isFav ? " on" : ""}" data-fav="${escapeHtml(skin.id)}" title="${isFav ? "取消收藏" : "收藏"}" aria-label="${isFav ? "取消收藏" : "收藏"}" aria-pressed="${isFav ? "true" : "false"}">
              ${favIcon}
            </button>
            ${canDelete ? `<button type="button" class="icon-btn delete-btn" data-delete="${escapeHtml(skin.id)}" data-name="${escapeHtml(skin.name)}" title="删除" aria-label="删除">
              <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" d="M5 7h14M10 11v6M14 11v6M9 7V5h6v2M8 7l1 12h6l1-12"/></svg>
            </button>` : ""}
          </div>
        </div>
        <div class="tags">
          <span class="tag tag-source">${sourceLabel}</span>
          ${skinTags.map((t) => `<span class="tag">${escapeHtml(t)}</span>`).join("")}
        </div>
      </div>
    `;
    grid.appendChild(card);
  }

  bindCardActions();
}

function bindCardActions() {
  grid.querySelectorAll("[data-apply]").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = btn.getAttribute("data-apply");
      const name = btn.getAttribute("data-name") || "皮肤";
      const wantRestart = Boolean(chkRestart?.checked);
      let plan;
      try {
        plan = await prepareApply(wantRestart);
      } catch {
        plan = { restart: wantRestart, proceed: true, overlayHint: "hot" };
      }
      if (!plan.proceed) return;
      const restart = Boolean(plan.restart);
      const busyText =
        plan.overlayHint === "restart" || restart
          ? `正在重启客户端并换上「${name}」…`
          : plan.overlayHint === "starting"
            ? `等待客户端就绪并换上「${name}」…`
            : `正在热切换「${name}」…`;
      setBusy(true, busyText);
      try {
        const result = await window.skinAPI.apply(id, { restart });
        if (result?.lifecycle || result?.canHotApply !== undefined) {
          updateHostPill(result);
        }
        showToast(
          restart
            ? `已重启客户端并换上「${name}」`
            : result?.artPending
              ? `已换上「${name}」（立绘加载中）`
              : `已换上「${name}」`,
          "ok"
        );
        await refresh();
        await pollHostStatus(true);
      } catch (err) {
        showToast(friendlyError(err), "error");
        await pollHostStatus(true);
      } finally {
        setBusy(false);
      }
    });
  });

  grid.querySelectorAll("[data-export]").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = btn.getAttribute("data-export");
      const name = btn.getAttribute("data-name") || "皮肤";
      setBusy(true, `正在导出「${name}」…`);
      try {
        const result = await window.skinAPI.exportSkin(id);
        if (result?.canceled) return;
        showToast(`已导出「${name}」`, "ok");
        if (result?.path) await window.skinAPI.revealExport(result.path);
      } catch (err) {
        showToast(friendlyError(err), "error");
      } finally {
        setBusy(false);
      }
    });
  });

  grid.querySelectorAll("[data-delete]").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = btn.getAttribute("data-delete");
      const name = btn.getAttribute("data-name") || "皮肤";
      const ok = await showConfirm({
        title: "删除皮肤",
        message: `确定删除皮肤「${name}」吗？\n删除后不可恢复。`,
        confirmText: "删除",
        cancelText: "取消",
        variant: "danger",
      });
      if (!ok) return;
      setBusy(true, `正在删除「${name}」…`);
      try {
        await window.skinAPI.deleteSkin(id);
        showToast(`已删除「${name}」`, "ok");
        await refresh();
      } catch (err) {
        showToast(friendlyError(err), "error");
      } finally {
        setBusy(false);
      }
    });
  });

  grid.querySelectorAll("[data-fav]").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const id = btn.getAttribute("data-fav");
      toggleFavorite(id);
      if (latestStatus) render(latestStatus);
    });
  });
}

async function refresh() {
  try {
    const status = await window.skinAPI.status();
    lastSkinsSig = "";
    render(status);
    scheduleHostPoll();
    return status;
  } catch (err) {
    // Keep last good status visible if a refresh fails mid-session
    if (latestStatus?.skins?.length) {
      pillCodex.textContent = "状态刷新失败";
      pillCodex.className = "pill warn";
      showToast(friendlyError(err), "error");
      return latestStatus;
    }
    throw err;
  }
}

/**
 * Pre-check host before apply; may auto-check restart or warn.
 * @returns {{ restart: boolean, proceed: boolean }}
 */
async function prepareApply(wantRestart) {
  let host = latestHost;
  try {
    host = (await pollHostStatus(true)) || host;
  } catch {
    /* use latest */
  }
  const lifecycle = host?.lifecycle || "offline";
  const needsRestart = Boolean(
    host?.needsRestartForInject ||
      (host?.processRunning && !host?.debugPortOpen && lifecycle !== "ready")
  );
  const canHot = Boolean(host?.canHotApply || lifecycle === "ready");

  if (lifecycle === "offline" && !wantRestart) {
    // Cold start is OK without checkbox — engine will launch; just inform.
    return { restart: false, proceed: true, overlayHint: "starting" };
  }
  if (needsRestart && !wantRestart) {
    const ok = confirm(
      "ChatGPT 已在运行，但未打开调试端口，热切换无法注入。\n\n是否勾选「自动重启」并继续换肤？\n（将关闭并重开客户端）"
    );
    if (!ok) return { restart: false, proceed: false };
    if (chkRestart) {
      chkRestart.checked = true;
      try {
        localStorage.setItem(RESTART_PREF_KEY, "1");
      } catch {
        /* ignore */
      }
    }
    return { restart: true, proceed: true, overlayHint: "restart" };
  }
  if (canHot && !wantRestart) {
    return { restart: false, proceed: true, overlayHint: "hot" };
  }
  return {
    restart: wantRestart,
    proceed: true,
    overlayHint: wantRestart ? "restart" : lifecycle === "starting" ? "starting" : "hot",
  };
}

/* —— 分类 / 搜索 —— */
categoryNav?.querySelectorAll("[data-category]").forEach((btn) => {
  btn.addEventListener("click", () => {
    activeCategory = btn.getAttribute("data-category") || "all";
    categoryNav.querySelectorAll(".nav-item").forEach((el) => el.classList.remove("active"));
    btn.classList.add("active");
    if (latestStatus) render(latestStatus);
  });
});

searchInput?.addEventListener("input", () => {
  searchQuery = searchInput.value || "";
  if (latestStatus) render(latestStatus);
});

/* —— DevTools 独立窗口 —— */
async function openDevtoolsWindow() {
  try {
    await window.skinAPI.openDevtools();
  } catch (err) {
    showToast(friendlyError(err), "error");
  }
}
document.getElementById("btnDevtools")?.addEventListener("click", () => {
  openDevtoolsWindow();
});
document.addEventListener("keydown", (e) => {
  // F12 or Ctrl+Shift+I → Skin DevTools (independent window)
  const isF12 = e.key === "F12";
  const isCtrlShiftI =
    (e.ctrlKey || e.metaKey) && e.shiftKey && String(e.key || "").toLowerCase() === "i";
  if (isF12 || isCtrlShiftI) {
    e.preventDefault();
    openDevtoolsWindow();
  }
});

/* —— 帮助 / 导入 / 刷新 / 还原 —— */
const helpModal = document.getElementById("helpModal");
function openHelp() {
  helpModal.hidden = false;
  helpModal.classList.add("show");
}
function closeHelp() {
  helpModal.hidden = true;
  helpModal.classList.remove("show");
}
document.getElementById("btnHelp").addEventListener("click", openHelp);
document.getElementById("btnHelpClose").addEventListener("click", closeHelp);
helpModal.addEventListener("click", (e) => {
  if (e.target === helpModal) closeHelp();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    // 确认框有独立 keydown（capture），此处只处理其它弹层
    const confirmModal = document.getElementById("confirmModal");
    if (confirmModal?.classList.contains("show")) return;
    if (helpModal.classList.contains("show")) closeHelp();
    if (wallpaperModal.classList.contains("show")) closeWallpaper();
  }
});

document.getElementById("btnImport").addEventListener("click", async () => {
  setBusy(true, "请确认安全提示并选择皮肤包…");
  try {
    const result = await window.skinAPI.importSkin();
    if (result?.canceled) {
      showToast("已取消导入");
      return;
    }
    const hash = result.injectSha256
      ? `（脚本 ${result.injectSha256.slice(0, 10)}…）`
      : "";
    showToast(`已导入「${result.name || result.skinId}」${hash}`, "ok");
    await refresh();
  } catch (err) {
    showToast(friendlyError(err), "error");
  } finally {
    setBusy(false);
  }
});

document.getElementById("btnRefresh").addEventListener("click", async () => {
  setBusy(true, "刷新中…");
  try {
    await refresh();
    showToast("已刷新", "ok");
  } catch (err) {
    showToast(friendlyError(err), "error");
  } finally {
    setBusy(false);
  }
});

document.getElementById("btnChooseApp").addEventListener("click", async () => {
  try {
    const result = await window.skinAPI.chooseApp();
    if (result?.canceled) return;
    showToast(result?.appPath ? `已指定：${result.appPath}` : "已保存客户端路径", "ok");
  } catch (err) {
    showToast(friendlyError(err), "error");
  }
});

document.getElementById("btnPause")?.addEventListener("click", async () => {
  const paused = Boolean(latestStatus?.paused);
  if (paused) {
    setBusy(true, "正在继续显示皮肤…");
    try {
      const result = await window.skinAPI.resume({
        restart: chkRestart?.checked === true,
      });
      if (result?.ok === false) {
        showToast(result?.error || "继续显示失败", "error");
      } else {
        showToast("已继续显示皮肤", "ok");
      }
      await refresh();
    } catch (err) {
      showToast(friendlyError(err), "error");
      await refresh();
    } finally {
      setBusy(false);
    }
    return;
  }
  setBusy(true, "正在暂停皮肤（即时卸下）…");
  try {
    const result = await window.skinAPI.pause();
    if (result?.ok === false) {
      // Honest partial: flag may be set even when live remove failed
      showToast(result?.error || "已记录暂停，但即时卸下可能未完成", "error");
    } else if (result?.hostLive === false) {
      showToast("已暂停皮肤（客户端未连接，仅记录暂停标记）", "ok");
    } else {
      showToast("已暂停皮肤（ChatGPT 窗口应已卸下主题）", "ok");
    }
    await refresh();
  } catch (err) {
    // PAUSE_REMOVE_FAILED still leaves pause flag — surface clearly
    showToast(friendlyError(err), "error");
    await refresh();
  } finally {
    setBusy(false);
  }
});

document.getElementById("btnRestore").addEventListener("click", async () => {
  const ok = await showConfirm({
    title: "恢复默认界面",
    message:
      "将清除当前皮肤注入并还原 ChatGPT 官方配色。\n\n若客户端正在运行，可能会自动重启。此操作不可撤销，是否继续？",
    confirmText: "恢复默认",
    cancelText: "取消",
    variant: "warn",
  });
  if (!ok) return;

  setBusy(true, "正在恢复默认并自动重开 ChatGPT…");
  try {
    const result = await window.skinAPI.restore({ restoreTheme: true });
    if (result?.ok === false || result?.partial) {
      showToast(
        result?.error || "已清除会话，但即时卸下可能未完成；请检查 ChatGPT 窗口",
        "error"
      );
    } else if (result?.relaunched) {
      showToast("已恢复默认，ChatGPT 已自动重开并恢复官方配色", "ok");
    } else if (result?.refreshed) {
      showToast("已恢复默认，界面已刷新", "ok");
    } else {
      showToast("已恢复默认", "ok");
    }
    await refresh();
  } catch (err) {
    showToast(friendlyError(err), "error");
  } finally {
    setBusy(false);
  }
});

/* —— 壁纸设计 —— */
const wallpaperModal = document.getElementById("wallpaperModal");
const wallpaperForm = document.getElementById("wallpaperForm");
const wallpaperBase = document.getElementById("wallpaperBase");
const wallpaperPath = document.getElementById("wallpaperPath");
const wallpaperFileName = document.getElementById("wallpaperFileName");

async function openWallpaper() {
  wallpaperModal.hidden = false;
  wallpaperModal.classList.add("show");
  const status = latestStatus || (await window.skinAPI.status());
  wallpaperBase.innerHTML = status.skins
    .map((skin) => `<option value="${escapeHtml(skin.id)}">${escapeHtml(skin.name)}${skin.builtin ? "（内置）" : ""}</option>`)
    .join("");
}
function closeWallpaper() {
  wallpaperModal.hidden = true;
  wallpaperModal.classList.remove("show");
}
document.getElementById("btnWallpaper").addEventListener("click", openWallpaper);
document.getElementById("btnWallpaperClose").addEventListener("click", closeWallpaper);
document.getElementById("btnWallpaperCancel").addEventListener("click", closeWallpaper);
wallpaperModal.addEventListener("click", (e) => {
  if (e.target === wallpaperModal) closeWallpaper();
});
document.getElementById("btnChooseWallpaper").addEventListener("click", async () => {
  const picked = await window.skinAPI.chooseWallpaper();
  if (!picked?.path) return;
  wallpaperPath.value = picked.path;
  wallpaperFileName.textContent = picked.name || picked.path.split(/[\\/]/).pop();
});

["themeOverlay", "themeOpacity"].forEach((id) => {
  const input = document.getElementById(id);
  const output = document.getElementById(`${id}Value`);
  input.addEventListener("input", () => {
    output.value = `${input.value}%`;
    output.textContent = `${input.value}%`;
  });
});
wallpaperForm.addEventListener("submit", async (e) => {
  e.preventDefault();
  if (!wallpaperPath.value) {
    showToast("请先选择壁纸", "error");
    return;
  }
  setBusy(true, "正在生成自定义主题…");
  try {
    const result = await window.skinAPI.designWallpaper({
      baseSkinId: wallpaperBase.value,
      imagePath: wallpaperPath.value,
      name: document.getElementById("wallpaperName").value,
      fit: document.getElementById("wallpaperFit").value,
      position: document.getElementById("wallpaperPosition").value,
      accent: document.getElementById("themeAccent").value,
      background: document.getElementById("themeBackground").value,
      text: document.getElementById("themeText").value,
      panel: document.getElementById("themePanel").value,
      font: document.getElementById("themeFont").value,
      radius: document.getElementById("themeRadius").value,
      overlay: document.getElementById("themeOverlay").value,
      opacity: document.getElementById("themeOpacity").value,
    });
    closeWallpaper();
    wallpaperForm.reset();
    wallpaperPath.value = "";
    wallpaperFileName.textContent = "支持 PNG、JPG、WebP，最大 50 MB";
    await refresh();
    showToast(`已生成「${result.name}」`, "ok");
  } catch (err) {
    showToast(friendlyError(err), "error");
  } finally {
    setBusy(false);
  }
});

/* —— 无边框窗口控制（Tauri 2） —— */
function getCurrentWindow() {
  const api = window.__TAURI__;
  if (!api) return null;
  try {
    if (api.window?.getCurrentWindow) return api.window.getCurrentWindow();
    if (api.webviewWindow?.getCurrentWebviewWindow) {
      return api.webviewWindow.getCurrentWebviewWindow();
    }
  } catch {
    /* ignore */
  }
  return null;
}

async function waitForWindow(timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const w = getCurrentWindow();
    if (w) return w;
    await new Promise((r) => setTimeout(r, 40));
  }
  return getCurrentWindow();
}

async function setupWindowControls() {
  const appWindow = await waitForWindow();
  if (!appWindow) {
    const ctrl = document.querySelector(".win-controls");
    if (ctrl) ctrl.style.display = "none";
    return;
  }

  const btnMin = document.getElementById("btnWinMin");
  const btnMax = document.getElementById("btnWinMax");
  const btnClose = document.getElementById("btnWinClose");
  const icoMax = document.getElementById("icoMax");
  const icoRestore = document.getElementById("icoRestore");

  async function syncMaxIcon() {
    try {
      const maximized = await appWindow.isMaximized();
      icoMax?.classList.toggle("hidden", maximized);
      icoRestore?.classList.toggle("hidden", !maximized);
      if (btnMax) btnMax.title = maximized ? "还原" : "最大化";
    } catch {
      /* ignore */
    }
  }

  btnMin?.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    appWindow.minimize().catch(() => {});
  });
  btnMax?.addEventListener("click", async (e) => {
    e.preventDefault();
    e.stopPropagation();
    try {
      await appWindow.toggleMaximize();
      await syncMaxIcon();
    } catch {
      /* ignore */
    }
  });
  btnClose?.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    appWindow.close().catch(() => {});
  });

  // 双击标题栏空白区域切换最大化（排除输入/按钮）
  // 拖拽交给 data-tauri-drag-region + CSS app-region:drag，避免与 startDragging 双重触发
  document.querySelector(".titlebar")?.addEventListener("dblclick", async (e) => {
    if (e.target.closest("[data-no-drag], button, input, a, .search-wrap, .win-controls, .titlebar-actions")) {
      return;
    }
    try {
      await appWindow.toggleMaximize();
      await syncMaxIcon();
    } catch {
      /* ignore */
    }
  });

  try {
    await appWindow.onResized(() => {
      syncMaxIcon();
    });
  } catch {
    /* ignore */
  }
  await syncMaxIcon();
}

// Adaptive host polling + focus / visibility refresh
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") {
    pollHostStatus(true).finally(() => scheduleHostPoll());
  } else if (hostPollTimer) {
    clearTimeout(hostPollTimer);
    hostPollTimer = null;
  }
});
window.addEventListener("focus", () => {
  if (!uiBusy) pollHostStatus(false);
});

(async () => {
  // 先保证壳可见：即使后端失败，header / 空网格 / toast 仍应渲染
  try {
    await setupWindowControls();
  } catch {
    /* ignore */
  }

  try {
    pillCodex.textContent = "连接中…";
    pillCodex.className = "pill";
    // Light host probe first (fast pill), then full catalog
    try {
      await pollHostStatus(true);
    } catch {
      /* full refresh may still work */
    }
    await refresh();
    scheduleHostPoll();
  } catch (err) {
    pillCodex.textContent = "引擎未就绪";
    pillCodex.className = "pill warn";
    pillActive.textContent = "当前皮肤：—";
    grid.innerHTML = `<article class="card"><div class="meta"><h2>无法加载皮肤列表</h2><p>${escapeHtml(friendlyError(err))}</p><p class="muted">请确认用 <code>npm run dev</code> 启动，并已安装 Node.js 18+。</p></div></article>`;
    showToast(friendlyError(err), "error");
  }
})();
