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
const skinsView = document.getElementById("skinsView");
const sessionsView = document.getElementById("sessionsView");
const providersView = document.getElementById("providersView");
const aboutView = document.getElementById("aboutView");
const overviewView = document.getElementById("overviewView");
const overviewGrid = document.getElementById("overviewGrid");

/** 关于页展示版本（与 package.json 对齐；版本检查走云端 catalog） */
const APP_VERSION = "2.2.0";
const APP_RELEASE_DATE = "2026-07-23";

/** @type {{ skins: any[], codexRunning?: boolean, debugReady?: boolean } | null} */
let latestStatus = null;
/** @type {Record<string, any> | null} last host_status / status host fields */
let latestHost = null;
/** @type {{ items: any[] } | null} */
let latestAnnouncements = null;
/** Active promo carousel index */
let promoIndex = 0;
let promoTimer = null;
/** @type {any[]} */
let promoItems = [];
let activeCategory = "all";
/** 当前主区域视图：overview | skins | sessions | providers | about */
let activeView = "overview";
/** @type {any | null} last env_check payload */
let latestEnv = null;
let envCheckInflight = null;
let searchQuery = "";
/** Busy overlay active — pause host polling */
let uiBusy = false;
/** In-flight host_status promise (single-flight) */
let hostPollInflight = null;
let hostPollTimer = null;
let hostPollFailCount = 0;
/** Last skins signature so host-only updates do not rebuild the grid */
let lastSkinsSig = "";
/** Follow artPending → settled without full catalog refresh (timer id) */
let artPendingWatchTimer = null;
/** Default banner copy when no cloud announcements */
const DEFAULT_PROMO = {
  id: "local-default",
  title: "",
  body: "一键换肤，随时还原 · 内置精品皮肤，支持导入导出与自定义皮肤",
  level: "info",
  dismissible: false,
};

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

/**
 * 侧栏分类注册表（src/skin-categories.json）。
 * 皮肤归属由 skin.json / catalog 的 `categories: string[]` 声明，GUI 不再关键词硬编码。
 * @type {{ id: string, label: string, kind?: string, match?: string, order?: number, icon?: string }[]}
 */
let SKIN_CATEGORIES = [
  { id: "all", label: "全部皮肤", kind: "system", match: "all", order: 0, icon: "clothes" },
  { id: "favorite", label: "我的收藏", kind: "system", match: "favorite", order: 100, icon: "favorite" },
];

/** Inline SVG paths for category nav icons (keyed by skin-categories.json `icon`) */
const CATEGORY_ICONS = {
  /** 概览菜单沿用原先「全部皮肤」的 home 图标 */
  home: `<path d="M4 10.5 12 4l8 6.5V20a1 1 0 0 1-1 1h-5v-6H10v6H5a1 1 0 0 1-1-1v-9.5z"/>`,
  /** 「全部皮肤」父级：衣服 / 换装 */
  clothes: `<path d="M9 4.5 12 7l3-2.5 3.5 2V9l-2 1v9.5a1 1 0 0 1-1 1H8.5a1 1 0 0 1-1-1V10L5.5 9V6.5L9 4.5z"/><path d="M9 4.5c0 1.2.9 2.2 2 2.5M15 4.5c0 1.2-.9 2.2-2 2.5"/>`,
  anime: `<circle cx="9" cy="10" r="1.2"/><circle cx="15" cy="10" r="1.2"/><path d="M12 3.5c-4.2 0-7.5 2.6-7.5 7.2 0 3.4 1.9 5.7 4.2 7.1.7.4 1.3-.1 1.3-.8v-1.1c-2.8-.6-3.5-2.5-3.5-5.2 0-2.9 2.4-5.2 5.5-5.2s5.5 2.3 5.5 5.2c0 2.7-.7 4.6-3.5 5.2v1.1c0 .7.6 1.2 1.3.8 2.3-1.4 4.2-3.7 4.2-7.1C19.5 6.1 16.2 3.5 12 3.5z"/><path d="M9.5 14.2c.7.8 1.6 1.2 2.5 1.2s1.8-.4 2.5-1.2"/>`,
  tech: `<rect x="7" y="7" width="10" height="10" rx="2"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4"/>`,
  nature: `<path d="M12 20V10"/><path d="M12 14c-3.2 0-5.5-1.6-6.5-4 2.5-.2 4.6.7 6.5 2.6 1.9-1.9 4-2.8 6.5-2.6-1 2.4-3.3 4-6.5 4z"/><path d="M12 10c-2.2 0-3.8-1.2-4.5-3 1.8-.2 3.2.5 4.5 1.9 1.3-1.4 2.7-2.1 4.5-1.9-.7 1.8-2.3 3-4.5 3z"/>`,
  game: `<path d="M7.5 8.5h9a4.5 4.5 0 0 1 4.4 5.4l-.7 3.2A2.8 2.8 0 0 1 17.5 19h-1.3a1.6 1.6 0 0 1-1.5-1.1l-.4-1.2H9.7l-.4 1.2A1.6 1.6 0 0 1 7.8 19H6.5a2.8 2.8 0 0 1-2.7-2l-.7-3.2A4.5 4.5 0 0 1 7.5 8.5z"/><path d="M9 12.5v3M7.5 14h3"/><circle cx="15.3" cy="13.2" r="1"/><circle cx="17.2" cy="15" r="1"/>`,
  art: `<path d="M12 4c-4.4 0-8 3.1-8 7 0 2.5 1.4 4.7 3.6 6 .5.3.9 0 .9-.5v-1.6c-2-.7-3.2-2.3-3.2-4 0-2.8 2.9-5 6.7-5s6.7 2.2 6.7 5c0 1.7-1.2 3.3-3.2 4v2.1c0 1.1-.7 2-1.7 2.3"/><circle cx="8.8" cy="10.8" r="1.1"/><circle cx="12" cy="9.2" r="1.1"/><circle cx="15.2" cy="10.8" r="1.1"/><circle cx="13.6" cy="13.8" r="1.1"/>`,
  minimal: `<rect x="4.5" y="4.5" width="15" height="15" rx="2.5"/><path d="M4.5 10h15M10 4.5v15"/>`,
  favorite: `<path d="M12 17.27 18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z"/>`,
};

/**
 * Normalize skin.categories from manifest / catalog (string[] of known filter ids).
 * @param {any} skin
 * @returns {string[]}
 */
function skinCategoryIds(skin) {
  const raw = skin?.categories;
  if (!Array.isArray(raw)) return [];
  const out = [];
  for (const c of raw) {
    const id = String(c || "")
      .trim()
      .toLowerCase();
    if (!id || id === "all" || id === "favorite" || id === "about") continue;
    if (!out.includes(id)) out.push(id);
  }
  return out;
}

/**
 * Whether skin belongs to the active sidebar filter.
 * @param {any} skin
 * @param {string} categoryId
 */
function skinMatchesCategory(skin, categoryId) {
  const cat = SKIN_CATEGORIES.find((c) => c.id === categoryId);
  const match = cat?.match || (categoryId === "all" ? "all" : categoryId === "favorite" ? "favorite" : "category");
  if (match === "all" || categoryId === "all") return true;
  if (match === "favorite" || categoryId === "favorite") {
    try {
      const fav = JSON.parse(localStorage.getItem("chatgpt-tools-favorites") || "[]");
      return Array.isArray(fav) && fav.includes(skin.id);
    } catch {
      return false;
    }
  }
  // Declared membership only — no keyword fallback
  return skinCategoryIds(skin).includes(categoryId);
}

function getFavorites() {
  try {
    return new Set(JSON.parse(localStorage.getItem("chatgpt-tools-favorites") || "[]"));
  } catch {
    return new Set();
  }
}

/** Whether the「全部皮肤」group submenu is expanded（默认概览页，收起） */
let skinsNavExpanded = false;

/**
 * Load category registry and rebuild sidebar filter buttons (keeps about + foot intact).
 * @returns {Promise<void>}
 */
async function loadSkinCategories() {
  try {
    const res = await fetch("skin-categories.json", { cache: "no-cache" });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    const list = Array.isArray(data?.categories) ? data.categories : [];
    if (list.length) {
      SKIN_CATEGORIES = list
        .filter((c) => c && c.id && c.label)
        .map((c) => ({
          id: String(c.id),
          label: String(c.label),
          kind: c.kind || "filter",
          match: c.match || (c.id === "all" ? "all" : c.id === "favorite" ? "favorite" : "category"),
          order: Number.isFinite(Number(c.order)) ? Number(c.order) : 50,
          icon: c.icon || c.id,
        }))
        .sort((a, b) => a.order - b.order || a.id.localeCompare(b.id));
    }
  } catch (err) {
    console.warn("[chatgpt-tools] skin-categories.json load failed, using defaults", err);
  }
  renderCategoryNav();
}

/**
 * Build one nav button (parent or child). Styling stays on .nav-item; structure only differs.
 * @param {{ id: string, label: string, icon?: string }} cat
 * @param {"parent"|"child"} role
 */
function createCategoryNavButton(cat, role) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = role === "parent" ? "nav-item nav-item-parent" : "nav-item nav-item-child";
  btn.dataset.category = cat.id;
  if (role === "parent") {
    btn.setAttribute("aria-expanded", skinsNavExpanded ? "true" : "false");
    btn.setAttribute("aria-controls", "skinsNavSub");
  }
  const icon = CATEGORY_ICONS[cat.icon] || CATEGORY_ICONS.clothes;
  const chevron =
    role === "parent"
      ? `<span class="nav-chevron" aria-hidden="true"><svg viewBox="0 0 24 24" width="14" height="14"><path d="M9 6.5 14.5 12 9 17.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg></span>`
      : "";
  btn.innerHTML = `
    <span class="nav-ico" aria-hidden="true">
      <svg viewBox="0 0 24 24">${icon}</svg>
    </span>
    <span class="nav-label">${escapeHtml(cat.label)}</span>
    ${chevron}
  `;
  return btn;
}

function isPinnedNavItem(el) {
  const cat = el.getAttribute?.("data-category") || el.getAttribute?.("data-view") || "";
  // Top-level feature menus must survive skins category rebuild
  return (
    cat === "about" ||
    cat === "sessions" ||
    cat === "providers" ||
    cat === "overview"
  );
}

function renderCategoryNav() {
  if (!categoryNav) return;
  const aboutBtn = categoryNav.querySelector('[data-category="about"], [data-view="about"]');
  // Remove previous skins group / flat filter items; keep pinned top-level menus
  categoryNav.querySelectorAll(".nav-group-skins, .nav-item[data-category]").forEach((el) => {
    if (isPinnedNavItem(el)) return;
    el.remove();
  });

  const parentCat =
    SKIN_CATEGORIES.find((c) => c.id === "all" || c.match === "all") || {
      id: "all",
      label: "全部皮肤",
      icon: "clothes",
    };
  const childCats = SKIN_CATEGORIES.filter((c) => c.id !== parentCat.id);

  const group = document.createElement("div");
  group.className = "nav-group nav-group-skins";
  if (skinsNavExpanded) group.classList.add("is-expanded");
  group.dataset.navGroup = "skins";

  const parentBtn = createCategoryNavButton(parentCat, "parent");
  const sub = document.createElement("div");
  sub.className = "nav-sub";
  sub.id = "skinsNavSub";
  sub.setAttribute("role", "group");
  sub.setAttribute("aria-label", "皮肤主题分类");
  if (!skinsNavExpanded) sub.hidden = true;

  for (const cat of childCats) {
    sub.appendChild(createCategoryNavButton(cat, "child"));
  }

  group.appendChild(parentBtn);
  group.appendChild(sub);

  // Order: 概览 → 会话管理 → 皮肤分组 → 关于
  if (aboutBtn) {
    categoryNav.insertBefore(group, aboutBtn);
  } else {
    categoryNav.appendChild(group);
  }

  syncCategoryNavActive();
  bindCategoryNav();
}

/** Apply expand/collapse + active highlight on the skins nav group. */
function syncCategoryNavActive() {
  if (!categoryNav) return;
  const group = categoryNav.querySelector(".nav-group-skins");
  if (group) {
    group.classList.toggle("is-expanded", skinsNavExpanded);
    const sub = group.querySelector(".nav-sub");
    if (sub) sub.hidden = !skinsNavExpanded;
    const parentBtn = group.querySelector(".nav-item-parent");
    if (parentBtn) parentBtn.setAttribute("aria-expanded", skinsNavExpanded ? "true" : "false");
  }

  categoryNav.querySelectorAll(".nav-item").forEach((el) => {
    const cat = el.getAttribute("data-category") || el.getAttribute("data-view");
    if (activeView === "about") {
      el.classList.toggle("active", cat === "about");
      el.classList.remove("is-branch");
      return;
    }
    if (activeView === "sessions") {
      el.classList.toggle("active", cat === "sessions");
      el.classList.remove("is-branch");
      return;
    }
    if (activeView === "providers") {
      el.classList.toggle("active", cat === "providers");
      el.classList.remove("is-branch");
      return;
    }
    if (activeView === "overview") {
      el.classList.toggle("active", cat === "overview");
      el.classList.remove("is-branch");
      return;
    }
    // skins view: highlight category filters; top-level feature menus inactive
    if (cat === "sessions" || cat === "providers" || cat === "about" || cat === "overview") {
      el.classList.remove("active", "is-branch");
      return;
    }
    el.classList.toggle("active", cat === activeCategory);
  });
  // Parent looks lightly selected when a child filter is active (branch context)
  const parentBtn = categoryNav.querySelector(".nav-item-parent");
  if (parentBtn) {
    if (activeView === "skins") {
      parentBtn.classList.toggle(
        "is-branch",
        skinsNavExpanded &&
          activeCategory !== "all" &&
          activeCategory !== "about" &&
          activeCategory !== "sessions" &&
          activeCategory !== "providers" &&
          activeCategory !== "overview"
      );
    } else {
      parentBtn.classList.remove("is-branch");
    }
  }
}

let categoryNavBound = false;
function bindCategoryNav() {
  if (!categoryNav || categoryNavBound) return;
  categoryNavBound = true;
  // Event delegation: survives sidebar rebuild from skin-categories.json
  categoryNav.addEventListener("click", (e) => {
    const btn = e.target.closest?.(".nav-item[data-category], .nav-item[data-view]");
    if (!btn || !categoryNav.contains(btn)) return;
    const cat = btn.getAttribute("data-category") || btn.getAttribute("data-view") || "all";

    // Top-level feature menus: leave skins view and collapse theme submenu
    if (cat === "about" || btn.getAttribute("data-view") === "about") {
      skinsNavExpanded = false;
      setMainView("about");
      return;
    }
    if (cat === "sessions" || btn.getAttribute("data-view") === "sessions") {
      skinsNavExpanded = false;
      setMainView("sessions");
      return;
    }
    if (cat === "providers" || btn.getAttribute("data-view") === "providers") {
      skinsNavExpanded = false;
      setMainView("providers");
      return;
    }
    if (cat === "overview" || btn.getAttribute("data-view") === "overview") {
      skinsNavExpanded = false;
      setMainView("overview");
      return;
    }

    // Parent「全部皮肤」: toggle expand/collapse; always show full skin list
    if (btn.classList.contains("nav-item-parent") || cat === "all") {
      const nextExpanded = !skinsNavExpanded;
      skinsNavExpanded = nextExpanded;
      activeCategory = "all";
      // setMainView must not override expand state
      setMainView("skins", { preserveSkinsExpand: true });
      syncCategoryNavActive();
      if (latestStatus) render(latestStatus);
      return;
    }

    // Child theme / favorite: expand submenu (if collapsed) and filter
    activeCategory = cat;
    skinsNavExpanded = true;
    setMainView("skins", { preserveSkinsExpand: true });
    syncCategoryNavActive();
    if (latestStatus) render(latestStatus);
  });
}

function toggleFavorite(id) {
  const fav = getFavorites();
  if (fav.has(id)) fav.delete(id);
  else fav.add(id);
  localStorage.setItem("chatgpt-tools-favorites", JSON.stringify([...fav]));
}

function showToast(message, type = "") {
  if (!toast) return;
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
  if (/多个.*包|multi.?package|多版本|Package versions|全部 ChatGPT/i.test(msg)) {
    return "检测到多个 Store 版 ChatGPT/Codex 包（常见于商店更新后）。请打开任务管理器结束全部相关进程，再勾选「自动重启」换肤。";
  }
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
  const q = searchQuery.trim().toLowerCase();
  return skins.filter((skin) => {
    if (!skinMatchesCategory(skin, activeCategory)) return false;
    if (!q) return true;
    const cats = skinCategoryIds(skin).join(" ");
    const hay = `${skin.name || ""} ${(skin.tags || []).join(" ")} ${cats} ${skin.description || ""} ${skin.id || ""}`.toLowerCase();
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
  const store = h?.storePackage;
  const multiStore = Boolean(store?.multiPackage);
  const storeHint = multiStore
    ? " · 多个 Store 包"
    : h?.storePackageStale
      ? " · 包已更新"
      : "";

  if (lifecycle === "ready") {
    return {
      text: multiStore
        ? `ChatGPT 已就绪 · 多包${conf}`
        : h?.storePackageStale
          ? `ChatGPT 已就绪 · 已更新${conf}`
          : `ChatGPT 已就绪${conf}`,
      cls: multiStore || h?.storePackageStale ? "pill warn" : "pill ok",
      title: multiStore
        ? `${store?.warning || "检测到多个 Store 包版本"}。可热切换；若异常请结束全部进程后重启换肤。${keep}`
        : h?.storePackageStale
          ? `Store 包身份已变化（可能刚更新），下次重启客户端会刷新记录。${keep}`
          : `可直接热切换皮肤${keep}${storeHint}`,
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
      title: multiStore
        ? `等待渲染页就绪。注意：多个 Store 包并存时请尽量只保留一个实例。${keep}`
        : `等待渲染页就绪${keep}`,
    };
  }
  return {
    text: conf ? "ChatGPT 检测中…" : "ChatGPT 未打开",
    cls: conf ? "pill warn" : "pill",
    title: multiStore
      ? "多个 Store 包已注册。打开客户端前建议在任务管理器结束全部 ChatGPT/Codex。"
      : "打开官方客户端，或点左侧「选择客户端」指定路径",
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
    "hostBootAppearanceTheme",
    "configAppearanceTheme",
    "storePackage",
    "storePackageStale",
    "shellMode",
    "applyMode",
    "nodeRequired",
  ];
  for (const k of keys) {
    if (host[k] !== undefined) target[k] = host[k];
  }
  return target;
}

/**
 * Build user-facing success line after apply (shellMode / art / store).
 * Prefer engine `message` when present. Used when feedback is "gui" (cold/restart)
 * or multi-package warning; host feedback path skips the long GUI success toast.
 * @param {string} name
 * @param {Record<string, any>|null|undefined} result
 * @param {boolean} restart
 */
function formatApplySuccessToast(name, result, restart) {
  if (result?.phase === "restarting" || result?.pending) {
    return result?.message?.trim()
      || (restart || result?.restarted
        ? `正在重启客户端并换上「${name}」…`
        : `正在启动客户端并换上「${name}」…`);
  }
  if (result?.message && typeof result.message === "string" && result.message.trim()) {
    // Cold path: still name the skin for clarity.
    if (restart || result?.restarted || result?.feedback === "gui") {
      const base = restart || result?.restarted
        ? `已重启客户端并换上「${name}」`
        : `已换上「${name}」`;
      if (result.artPending) return `${base}（立绘加载中）`;
      return base;
    }
    return result.message;
  }
  const parts = [];
  if (restart) parts.push(`已重启客户端并换上「${name}」`);
  else parts.push(`已换上「${name}」`);

  const mode = result?.shellMode || result?.applyMode || "";
  if (!restart && (mode === "delta" || result?.deltaPreferred || result?.deltaHits > 0)) {
    parts.push("热切换");
  } else if (!restart && mode === "full") {
    parts.push("完整注入");
  }
  if (result?.deltaHit || result?.shell?.deltaHit) {
    parts.push("缓存命中");
  }
  if (result?.artPending) {
    parts.push("立绘加载中");
  } else if (result?.artOk === false && result?.shellOk) {
    parts.push("样式已注入");
  }
  if (result?.storePackage?.multiPackage || result?.storePackage?.registeredCount > 1) {
    parts.push("注意多版本 Store 包");
  } else if (result?.storePackage?.previousStale) {
    parts.push("已刷新 Store 包记录");
  }
  return parts.length <= 1 ? parts[0] : `${parts[0]}（${parts.slice(1).join(" · ")}）`;
}

/**
 * Normalize art session flags for GUI display.
 * artPending must never stay true when artOk is true (stale state / race).
 * @param {Record<string, any>|null|undefined} src
 * @returns {{ shellOk: boolean, artOk: boolean, artPending: boolean }}
 */
function normalizeArtFlags(src) {
  const artOk = Boolean(src?.artOk);
  // Pending only while work is in flight; never when already ok.
  const artPending = Boolean(src?.artPending) && !artOk;
  // Missing shellOk: infer from art session so empty status does not show false badges.
  const shellOk =
    src?.shellOk == null || src?.shellOk === undefined
      ? artOk || artPending
      : Boolean(src.shellOk);
  return { shellOk, artOk, artPending };
}

/**
 * Merge art/shell flags from host_status / status into latestStatus + pill.
 * Does not rebuild the skin grid.
 * @param {Record<string, any>|null|undefined} host
 */
function applyArtFlagsFromHost(host) {
  if (!host || !latestStatus) return false;
  const flags = normalizeArtFlags({
    shellOk: host.shellOk ?? host.state?.shellOk ?? latestStatus.shellOk,
    artOk: host.artOk ?? host.state?.artOk ?? latestStatus.artOk,
    artPending: host.artPending ?? host.state?.artPending ?? latestStatus.artPending,
  });
  if (
    latestStatus.artPending === flags.artPending &&
    latestStatus.artOk === flags.artOk &&
    latestStatus.shellOk === flags.shellOk
  ) {
    return false;
  }
  latestStatus = {
    ...latestStatus,
    shellOk: flags.shellOk,
    artOk: flags.artOk,
    artPending: flags.artPending,
    state: {
      ...(latestStatus.state || {}),
      shellOk: flags.shellOk,
      artOk: flags.artOk,
      artPending: flags.artPending,
    },
  };
  // Pill-only update — no full catalog refresh when art settles.
  updateActivePill(latestStatus);
  return true;
}

/**
 * After apply returns artPending, poll lightweight hostStatus until art settles
 * or timeout — clears 「立绘加载中」 without waiting for a full status refresh.
 * @param {number} [timeoutMs]
 */
function watchArtPendingSettle(timeoutMs = 45_000) {
  if (artPendingWatchTimer) {
    clearInterval(artPendingWatchTimer);
    artPendingWatchTimer = null;
  }
  if (!latestStatus?.artPending) return;
  const deadline = Date.now() + timeoutMs;
  let ticks = 0;
  artPendingWatchTimer = setInterval(() => {
    ticks += 1;
    if (Date.now() > deadline || !latestStatus?.artPending) {
      if (artPendingWatchTimer) {
        clearInterval(artPendingWatchTimer);
        artPendingWatchTimer = null;
      }
      // Final clear: if still pending after timeout, drop the loading label
      // (engine may have failed silently — do not stick forever).
      if (latestStatus?.artPending) {
        latestStatus = {
          ...latestStatus,
          artPending: false,
          state: { ...(latestStatus.state || {}), artPending: false },
        };
        updateActivePill(latestStatus);
      }
      return;
    }
    void (async () => {
      try {
        // Prefer light host_status (now includes art flags); force every few ticks.
        const host = await window.skinAPI.hostStatus({ force: ticks % 3 === 0 });
        if (host) applyArtFlagsFromHost(host);
        // Fallback: full status if host poll still claims pending after ~6s.
        if (latestStatus?.artPending && ticks >= 4 && ticks % 4 === 0) {
          const st = await window.skinAPI.status();
          if (st) applyArtFlagsFromHost(st);
        }
      } catch {
        /* ignore */
      }
    })();
  }, 1400);
}

/**
 * Optimistic active badge after shell_ready (avoid waiting full status+previews).
 * @param {string} skinId
 * @param {Record<string, any>|null|undefined} result
 */
function applyOptimisticSession(skinId, result) {
  if (!skinId || !latestStatus?.skins?.length) return;
  const engaged = (result?.lifecycle || "ready") !== "offline";
  // Apply path: shell is considered ok unless engine explicitly says otherwise.
  const flags = normalizeArtFlags({
    shellOk: result?.shellOk !== false,
    artOk: result?.artOk,
    artPending: result?.artPending,
  });
  latestStatus = {
    ...latestStatus,
    state: {
      ...(latestStatus.state || {}),
      skinId,
      shellOk: flags.shellOk,
      artOk: flags.artOk,
      artPending: flags.artPending,
      applyMode: result?.applyMode,
      shellMode: result?.shellMode,
    },
    shellOk: flags.shellOk,
    artOk: flags.artOk,
    artPending: flags.artPending,
    paused: false,
    lifecycle: result?.lifecycle || latestStatus.lifecycle || "ready",
  };
  for (const s of latestStatus.skins) {
    s.active = Boolean(s.id === skinId && engaged);
  }
  lastSkinsSig = "";
  render(latestStatus);
  if (flags.artPending) watchArtPendingSettle();
  else if (artPendingWatchTimer) {
    clearInterval(artPendingWatchTimer);
    artPendingWatchTimer = null;
  }
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
    syncPauseButton(latestStatus || host);
    return;
  }
  const mapped = mapHostPill(host);
  pillCodex.textContent = mapped.text;
  pillCodex.className = mapped.cls;
  pillCodex.title = mapped.title;
  latestHost = { ...(latestHost || {}), ...host };
  if (latestStatus) mergeHostFields(latestStatus, host);
  syncPauseButton(latestStatus || host);
}

function updateActivePill(status) {
  const skins = Array.isArray(status?.skins) ? status.skins : [];
  const active = skins.find((s) => s.active);
  const stateSkinId = status?.state?.skinId;
  const stateName =
    active?.name ||
    (stateSkinId ? skins.find((s) => s.id === stateSkinId)?.name : null);
  const flags = normalizeArtFlags({
    shellOk: status?.shellOk ?? status?.state?.shellOk,
    artOk: status?.artOk ?? status?.state?.artOk,
    artPending: status?.artPending ?? status?.state?.artPending,
  });
  // Pill suffix = transient ops only. shellMode/applyMode are historical inject
  // metadata (persist in state) — never show as sticky "（热切换）".
  let activeExtra = "";
  if (status?.paused) activeExtra = "（已暂停）";
  else if (flags.artPending && (active || stateSkinId)) activeExtra = "（立绘加载中）";
  // art settled (ok or failed): no permanent badge — name alone means active skin.
  pillActive.textContent = stateName
    ? `当前皮肤：${stateName}${activeExtra}`
    : "当前皮肤：无";
  pillActive.className = active || stateSkinId ? "pill ok" : "pill";
  const modeHint =
    status?.shellMode || status?.applyMode
      ? `上次注入：${status.shellMode || "—"} / ${status.applyMode || "—"}`
      : "";
  const artHint = flags.artPending
    ? "立绘加载中"
    : flags.artOk
      ? "立绘就绪"
      : flags.shellOk
        ? "仅样式（立绘未就绪）"
        : "";
  const store = status?.storePackage;
  const storeHint = store?.multiPackage
    ? store.warning || "多个 Store 包"
    : status?.storePackageStale
      ? "Store 包已更新"
      : store?.version
        ? `Store ${store.version}`
        : "";
  pillActive.title = [stateName || "无活动皮肤", modeHint, artHint, storeHint]
    .filter(Boolean)
    .join(" · ");
  syncPauseButton(status);
}

/**
 * Titlebar action modes for #btnPause:
 * - start: host offline → launch ChatGPT (+ last skin if any)
 * - resume: paused session → continue skin
 * - pause: host running with session → pause skin
 * - idle: host running, no session → disabled pause label
 */
function pauseButtonMode(status) {
  const host = latestHost || status || {};
  const lifecycle = host.lifecycle || status?.lifecycle || "offline";
  const offline =
    lifecycle === "offline" &&
    !host.processRunning &&
    !host.debugPortOpen &&
    !host.rendererReady &&
    !status?.codexRunning;
  if (offline) return "start";
  const paused = Boolean(status?.paused);
  if (paused) return "resume";
  const hasSession = Boolean(status?.state?.skinId || status?.skins?.some((s) => s.active));
  if (hasSession) return "pause";
  return "idle";
}

function syncPauseButton(status) {
  const btn = document.getElementById("btnPause");
  const label = document.getElementById("btnPauseLabel");
  if (!btn || !label) return;
  const mode = pauseButtonMode(status);
  btn.dataset.mode = mode;
  btn.disabled = mode === "idle";
  btn.classList.toggle("chip-warn", mode === "resume");
  btn.classList.toggle("chip-primary", mode === "start");
  if (mode === "start") {
    label.textContent = "启动 ChatGPT";
    btn.title = "启动 ChatGPT 客户端；若有上次使用的皮肤将自动应用";
  } else if (mode === "resume") {
    label.textContent = "继续显示";
    btn.title = "清除暂停并重新应用当前皮肤";
  } else if (mode === "pause") {
    label.textContent = "暂停皮肤";
    btn.title = "写入暂停标记并即时从 ChatGPT 窗口卸下皮肤（会话可恢复）";
  } else {
    label.textContent = "暂停皮肤";
    btn.title = "当前没有可暂停的皮肤会话";
  }
}

function skinsSignature(skins) {
  return (skins || [])
    .map((s) => {
      const cats = Array.isArray(s.categories) ? s.categories.join(",") : "";
      return `${s.id}:${s.active ? 1 : 0}:${s.previewUrl ? 1 : 0}:${cats}`;
    })
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
      // host_status now carries shellOk/artOk/artPending — clear stuck 「立绘加载中」.
      applyArtFlagsFromHost(host);
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
  const raw = status || { skins: [] };
  // Normalize art flags at the source so pills never see inconsistent pairs.
  const flags = normalizeArtFlags({
    shellOk: raw.shellOk ?? raw.state?.shellOk,
    artOk: raw.artOk ?? raw.state?.artOk,
    artPending: raw.artPending ?? raw.state?.artPending,
  });
  latestStatus = {
    ...raw,
    shellOk: flags.shellOk,
    artOk: flags.artOk,
    artPending: flags.artPending,
    state: raw.state
      ? { ...raw.state, shellOk: flags.shellOk, artOk: flags.artOk, artPending: flags.artPending }
      : raw.state,
  };
  // Keep Store / inject mode on host pill even after light host_status polls
  if (latestHost && latestStatus) {
    for (const k of ["storePackage", "storePackageStale", "shellMode", "applyMode", "nodeRequired"]) {
      if (latestStatus[k] !== undefined) latestHost[k] = latestStatus[k];
    }
  }
  if (latestStatus?.lifecycle || latestStatus?.codexRunning !== undefined) {
    updateHostPill(latestStatus);
  }
  updateActivePill(latestStatus);
  if (flags.artPending) watchArtPendingSettle();
  else if (artPendingWatchTimer) {
    clearInterval(artPendingWatchTimer);
    artPendingWatchTimer = null;
  }
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
    const isRemote = skin.source === "remote" || skin.installState === "remote";
    const needUpdate = Boolean(skin.updateAvailable || skin.installState === "updateAvailable");
    const useLabel = isRemote
      ? "下载皮肤"
      : needUpdate
        ? "更新皮肤"
        : skin.active
          ? "重新应用"
          : "使用皮肤";
    const sourceMeta = skinSourceLabel(skin);
    const skinTags = (skin.tags || []).slice(0, 3);
    const canDelete =
      skin.source === "user" ||
      skin.source === "cache" ||
      (!skin.builtin && skin.source !== "remote" && skin.source !== "bundled");
    const canExport = !isRemote && skin.dir;
    const favIcon = isFav
      ? `<svg class="fav-icon" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="currentColor" d="M12 17.27 18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z"/></svg>`
      : `<svg class="fav-icon" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" d="M12 17.27 18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z"/></svg>`;

    const primaryAction =
      isRemote || needUpdate
        ? `<button type="button" class="use-btn is-download" data-download="${escapeHtml(skin.id)}" data-name="${escapeHtml(skin.name)}">${useLabel}</button>`
        : `<button type="button" class="use-btn" data-apply="${escapeHtml(skin.id)}" data-name="${escapeHtml(skin.name)}">${useLabel}</button>`;

    card.innerHTML = `
      <div class="preview" style="background:${skin.previewGradient || "#eceff6"}">
        ${previewImg}
        ${skin.active ? `<span class="badge on">使用中</span>` : ""}
        <div class="preview-actions">
          ${primaryAction}
          ${canExport ? `<button type="button" class="export-btn" data-export="${escapeHtml(skin.id)}" data-name="${escapeHtml(skin.name)}" title="导出">导出</button>` : ""}
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
          <span class="tag tag-source ${sourceMeta.cls}">${escapeHtml(sourceMeta.label)}</span>
          ${needUpdate && !isRemote ? `<span class="tag tag-source tag-update">可更新</span>` : ""}
          ${skinTags.map((t) => `<span class="tag">${escapeHtml(t)}</span>`).join("")}
        </div>
      </div>
    `;
    grid.appendChild(card);
  }

  bindCardActions();
}

/** @param {any} skin */
function skinSourceLabel(skin) {
  const src = skin.source || (skin.builtin ? "bundled" : "user");
  if (src === "remote" || skin.installState === "remote") {
    return { label: "云端", cls: "tag-remote" };
  }
  if (src === "cache") {
    return { label: "已缓存", cls: "tag-cache" };
  }
  if (src === "bundled" || skin.builtin) {
    return { label: "内置", cls: "" };
  }
  return { label: "已导入", cls: "" };
}

function bindCardActions() {
  grid.querySelectorAll("[data-download]").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = btn.getAttribute("data-download");
      const name = btn.getAttribute("data-name") || "皮肤";
      if (!id) return;
      setBusy(true, `正在从云端下载「${name}」…`);
      try {
        const result = await window.skinAPI.cloudDownloadSkin(id);
        if (result?.cached) {
          showToast(`「${name}」已在本地缓存`, "ok");
        } else {
          showToast(`已下载并缓存「${result?.name || name}」`, "ok");
        }
        await refresh();
      } catch (err) {
        showToast(friendlyError(err), "error");
      } finally {
        setBusy(false);
      }
    });
  });

  grid.querySelectorAll("[data-apply]").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = btn.getAttribute("data-apply");
      const name = btn.getAttribute("data-name") || "皮肤";
      const skin = (latestStatus?.skins || []).find((s) => s.id === id) || null;
      const wantRestart = Boolean(chkRestart?.checked);
      // Immediate feedback: never wait for host_status / dialogs before overlay.
      setBusy(true, `正在准备切换「${name}」…`);
      let plan;
      try {
        plan = await prepareApply(wantRestart, skin);
      } catch {
        plan = { restart: wantRestart, proceed: true, overlayHint: "hot" };
      }
      if (!plan.proceed) {
        setBusy(false);
        return;
      }
      const restart = Boolean(plan.restart);
      // Full-screen busy only for cold start / restart; hot switch uses short busy until shell_ready.
      const heavy =
        plan.overlayHint === "restart" ||
        plan.overlayHint === "starting" ||
        restart;
      const busyText = heavy
        ? plan.overlayHint === "starting"
          ? `等待客户端就绪并换上「${name}」…`
          : `正在重启客户端并换上「${name}」…`
        : `正在热切换「${name}」…`;
      setBusy(true, busyText);
      try {
        const result = await window.skinAPI.apply(id, { restart });
        if (result?.lifecycle || result?.canHotApply !== undefined) {
          updateHostPill(result);
        }
        applyOptimisticSession(result?.skinId || id, result);

        const multi =
          result?.storePackage?.multiPackage || result?.storePackage?.registeredCount > 1;
        const pendingRestart =
          result?.phase === "restarting" || result?.pending === true;
        // feedback=host: page toast already announced success — avoid double "已换上".
        // Still warn on multi-package / always toast on cold (feedback=gui).
        const feedback = result?.feedback || (heavy ? "gui" : "host");
        if (multi) {
          showToast(formatApplySuccessToast(name, result, restart), "error");
        } else if (pendingRestart) {
          showToast(formatApplySuccessToast(name, result, restart), "ok");
        } else if (feedback === "gui" || heavy) {
          showToast(formatApplySuccessToast(name, result, restart), "ok");
        } else if (result?.artPending) {
          // Brief GUI note only when wallpaper is still loading (optional soft sync).
          // Keep quiet for pure hot shell success — host corner toast is enough.
        }

        // Unlock UI before heavy catalog refresh (previews can be multi-MB).
        setBusy(false);
        // Soft host refresh first; force only if lifecycle looks stale.
        void pollHostStatus(false).catch(() => {});
        void refresh().catch(() => {});
        // Fire-and-forget cold/restart: poll until shell ready or timeout (~90s).
        if (pendingRestart) {
          void (async () => {
            const deadline = Date.now() + 90_000;
            while (Date.now() < deadline) {
              await new Promise((r) => setTimeout(r, 1200));
              try {
                await pollHostStatus(true);
                const st = await window.skinAPI.status();
                if (st) {
                  latestStatus = { ...latestStatus, ...st, skins: st.skins || latestStatus?.skins };
                  if (st.shellOk || st.state?.shellOk || st.phase === "active") {
                    applyOptimisticSession(result?.skinId || id, {
                      ...result,
                      shellOk: true,
                      phase: "shell_ready",
                      lifecycle: st.lifecycle || "ready",
                      artPending: st.artPending ?? st.state?.artPending,
                      artOk: st.artOk ?? st.state?.artOk,
                    });
                    showToast(`已换上「${name}」`, "ok");
                    void refresh().catch(() => {});
                    return;
                  }
                  if (st.phase === "error" || st.state?.phase === "error") {
                    showToast(
                      st.lastError || st.state?.lastError || "换肤未完成，请重试",
                      "error"
                    );
                    return;
                  }
                }
              } catch {
                /* keep polling */
              }
            }
          })();
        }
      } catch (err) {
        showToast(friendlyError(err), "error");
        try {
          await pollHostStatus(true);
        } catch {
          /* ignore */
        }
        setBusy(false);
      } finally {
        if (uiBusy) setBusy(false);
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
 * Normalize appearance theme tokens from skin / host / config.
 * @param {unknown} raw
 * @returns {"light"|"dark"|null}
 */
function normalizeAppearanceTheme(raw) {
  if (raw == null) return null;
  const s = String(raw).trim().toLowerCase();
  if (s === "light" || s === "dark") return s;
  return null;
}

/**
 * Skin-declared desktop appearance (config.toml [desktop].appearanceTheme target).
 * Prefer desktopTheme.appearanceTheme; fall back to top-level appearance.
 * @param {any} skin
 * @returns {"light"|"dark"|null}
 */
function skinAppearanceTheme(skin) {
  if (!skin || typeof skin !== "object") return null;
  return (
    normalizeAppearanceTheme(skin.desktopTheme?.appearanceTheme) ||
    normalizeAppearanceTheme(skin.appearance) ||
    null
  );
}

/**
 * Appearance mode the running ChatGPT/Codex process actually booted with.
 * Prefer hostBootAppearanceTheme (frozen at process engage); fall back to config file.
 * When the host is running but config has no appearanceTheme, treat as light (Codex default).
 * @param {Record<string, any>|null|undefined} host
 * @returns {"light"|"dark"|null}
 */
function hostBootAppearanceTheme(host) {
  const explicit =
    normalizeAppearanceTheme(host?.hostBootAppearanceTheme) ||
    normalizeAppearanceTheme(host?.configAppearanceTheme);
  if (explicit) return explicit;
  const running =
    host?.lifecycle === "ready" ||
    host?.lifecycle === "starting" ||
    Boolean(host?.processRunning || host?.codexRunning);
  return running ? "light" : null;
}

/**
 * Pre-check host before apply; may auto-check restart or warn.
 *
 * Order:
 * 1) If「换肤时自动重启客户端」checked → restart apply (no appearance dialog).
 * 2) Else if appearanceTheme mismatches host boot mode → prompt:
 *    confirm → restart apply; cancel / close dialog → abort (no hot apply).
 * 3) Else keep existing hot / cold flow (no dialog when restart not required).
 *
 * @param {boolean} wantRestart
 * @param {any} [skin]
 * @returns {Promise<{ restart: boolean, proceed: boolean, overlayHint?: string }>}
 */
async function prepareApply(wantRestart, skin = null) {
  // Prefer in-memory host pill; only force-refresh when we lack a usable snapshot.
  // (Previously always force-polled → 1–3s silent gap before setBusy.)
  let host = latestHost;
  const lifecycleHint = host?.lifecycle || "";
  const hostFreshEnough =
    host &&
    (lifecycleHint === "ready" ||
      lifecycleHint === "starting" ||
      lifecycleHint === "offline" ||
      host?.canHotApply !== undefined);
  if (!hostFreshEnough) {
    try {
      // Soft poll first (uses server-side CDP cache); force only if still empty.
      host = (await pollHostStatus(false)) || host;
      if (!host?.lifecycle) {
        host = (await pollHostStatus(true)) || host;
      }
    } catch {
      /* use latest */
    }
  }

  const lifecycle = host?.lifecycle || "offline";
  const needsRestart = Boolean(
    host?.needsRestartForInject ||
      (host?.processRunning && !host?.debugPortOpen && lifecycle !== "ready")
  );
  const canHot = Boolean(host?.canHotApply || lifecycle === "ready");

  // User already opted into auto-restart: skip appearance prompt, force restart path.
  if (wantRestart) {
    return { restart: true, proceed: true, overlayHint: "restart" };
  }

  if (lifecycle === "offline") {
    // Cold start is OK without checkbox — engine will launch with new config.
    return { restart: false, proceed: true, overlayHint: "starting" };
  }

  // Multiple Store package versions (post-update residue): warn before inject.
  // storePackage comes from full status/detect (not every host_status poll).
  const store = host?.storePackage || latestStatus?.storePackage;
  if (store?.multiPackage && lifecycle !== "offline") {
    // Allow reading confirm dialog — temporarily clear busy so modal is interactive.
    setBusy(false);
    const ok = await showConfirm({
      title: "检测到多个 Store 包",
      message:
        (store.warning ||
          "Microsoft Store 更新后可能同时注册了多个 ChatGPT/Codex 包版本。") +
        "\n\n建议先打开任务管理器，结束全部 ChatGPT/Codex 进程，再继续换肤。" +
        "\n\n选择「仍要继续」将尝试注入；选择「取消」可先手动清理。",
      confirmText: "仍要继续",
      cancelText: "取消",
      variant: "warn",
    });
    if (!ok) return { restart: false, proceed: false };
    setBusy(true, "正在继续换肤…");
  }

  if (needsRestart) {
    setBusy(false);
    const ok = await showConfirm({
      title: "需要重启客户端",
      message:
        "ChatGPT 已在运行，但未打开调试端口，热切换无法注入。\n\n是否勾选「自动重启」并继续换肤？\n（将关闭并重开客户端）",
      confirmText: "重启并换肤",
      cancelText: "取消",
      variant: "warn",
    });
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

  // Unchecked auto-restart: only prompt when skin appearance differs from host boot mode.
  // Confirm → restart; cancel / close → abort apply entirely (do not hot-switch).
  if (canHot) {
    const skinTheme = skinAppearanceTheme(skin);
    const bootTheme = hostBootAppearanceTheme(host);
    if (skinTheme && bootTheme && skinTheme !== bootTheme) {
      setBusy(false);
      const themeLabel = (t) => (t === "dark" ? "深色（dark）" : "浅色（light）");
      const ok = await showConfirm({
        title: "外观模式不一致",
        message:
          `当前客户端启动时为 ${themeLabel(bootTheme)} 模式，` +
          `而皮肤「${skin?.name || skin?.id || ""}」配置为 ${themeLabel(skinTheme)}。\n\n` +
          `桌面标题栏/对话框等外观需重启客户端后才会切换。\n\n` +
          `选择「重启并换肤」将关闭并重开客户端后应用皮肤；选择「取消」或关闭对话框则不执行换肤。`,
        confirmText: "重启并换肤",
        cancelText: "取消",
        variant: "warn",
      });
      if (ok) {
        return { restart: true, proceed: true, overlayHint: "restart" };
      }
      return { restart: false, proceed: false };
    }
    // Same appearance (or unknown): existing hot path, no dialog.
    return { restart: false, proceed: true, overlayHint: "hot" };
  }

  return {
    restart: false,
    proceed: true,
    overlayHint: lifecycle === "starting" ? "starting" : "hot",
  };
}

/**
 * 主视图切换（皮肤列表 / 会话管理 / 关于）
 * @param {"skins"|"sessions"|"about"} view
 * @param {{ preserveSkinsExpand?: boolean }} [opts]
 *   preserveSkinsExpand: 侧栏点击已设好展开态时勿覆盖；
 *   切到「关于」「会话管理」等其它主菜单时始终收起皮肤子菜单。
 */
function setMainView(view, opts = {}) {
  const next =
    view === "about"
      ? "about"
      : view === "sessions"
        ? "sessions"
        : view === "providers"
          ? "providers"
          : view === "overview"
            ? "overview"
            : "skins";
  const prev = activeView;
  activeView = next;
  if (overviewView) overviewView.hidden = activeView !== "overview";
  if (skinsView) skinsView.hidden = activeView !== "skins";
  if (sessionsView) sessionsView.hidden = activeView !== "sessions";
  if (providersView) providersView.hidden = activeView !== "providers";
  if (aboutView) aboutView.hidden = activeView !== "about";
  if (searchInput) {
    const disableSearch = activeView !== "skins";
    searchInput.disabled = disableSearch;
    searchInput.parentElement?.classList.toggle("is-disabled", disableSearch);
  }
  if (
    activeView === "about" ||
    activeView === "sessions" ||
    activeView === "providers" ||
    activeView === "overview"
  ) {
    // 离开皮肤主分支：收起主题子菜单
    skinsNavExpanded = false;
  } else if (!opts.preserveSkinsExpand) {
    // 非侧栏显式控制的进入皮肤列表（如其它入口）默认展开
    skinsNavExpanded = true;
  }
  syncCategoryNavActive();
  if (activeView === "sessions" && prev !== "sessions") {
    try {
      window.sessionsView?.enter?.();
    } catch (err) {
      console.warn("sessionsView.enter failed", err);
    }
  } else if (prev === "sessions" && activeView !== "sessions") {
    try {
      window.sessionsView?.leave?.();
    } catch {
      /* ignore */
    }
  }
  if (activeView === "providers" && prev !== "providers") {
    try {
      window.providersView?.enter?.();
    } catch (err) {
      console.warn("providersView.enter failed", err);
    }
  } else if (prev === "providers" && activeView !== "providers") {
    try {
      window.providersView?.leave?.();
    } catch {
      /* ignore */
    }
  }
  if (activeView === "overview") {
    loadEnvironment({ force: prev !== "overview" }).catch((err) => {
      console.warn("env check failed", err);
    });
  }
}

// Expose shell helpers for feature modules (sessions, future tools)
window.showToast = showToast;
window.showConfirm = showConfirm;

/* ── 环境概览 ─────────────────────────────────────────── */

const OV_TOOL_ICONS = {
  "chatgpt-desktop": {
    className: "ov-card-ico",
    svg: `<rect x="3.5" y="4.5" width="17" height="12.5" rx="2"/><path d="M8 20.5h8M12 17v3.5"/>`,
  },
  "codex-cli": {
    className: "ov-card-ico ico-cli",
    svg: `<path d="M5 8.5 9 12l-4 3.5"/><path d="M11.5 16.5h7.5"/><rect x="3.5" y="4.5" width="17" height="15" rx="2"/>`,
  },
  "grok-build": {
    className: "ov-card-ico ico-grok",
    svg: `<circle cx="12" cy="12" r="8.5"/><path d="M8.5 12.5c1.2 1.8 3 2.8 5 2.8s3.8-1 5-2.8"/><circle cx="9.2" cy="10" r="1"/><circle cx="14.8" cy="10" r="1"/>`,
  },
  node: {
    className: "ov-card-ico ico-runtime",
    svg: `<path d="M12 3.5 19.5 8v8L12 20.5 4.5 16V8L12 3.5z"/><path d="M12 8.5v7M9 10.2l6 3.6M15 10.2l-6 3.6"/>`,
  },
  npm: {
    className: "ov-card-ico ico-runtime ico-npm",
    svg: `<rect x="4" y="5.5" width="16" height="13" rx="2"/><path d="M8 9.5v5M12 9.5v5M16 9.5v3.2"/>`,
  },
};

const OV_FALLBACK_INSTALL = {
  "chatgpt-desktop": {
    type: "url",
    url: "https://openai.com/zh-Hans-CN/codex/",
    label: "前往下载安装",
  },
  "codex-cli": {
    type: "npm",
    command: "npm i -g @openai/codex@latest",
    label: "安装 Codex CLI",
  },
  "grok-build": {
    type: "npm",
    command: "npm i -g @xai-official/grok@latest",
    label: "安装 Grok Build",
  },
};

function kindLabel(kind) {
  if (kind === "desktop") return "桌面应用";
  if (kind === "cli") return "命令行工具";
  if (kind === "app") return "本地应用";
  if (kind === "runtime") return "运行环境";
  return kind || "组件";
}

function sourceLabel(source) {
  const map = {
    "microsoft-store": "Microsoft Store",
    configured: "用户指定",
    applications: "Applications",
    local: "本机安装",
    path: "PATH",
    homebrew: "Homebrew",
    npm: "npm",
    volta: "Volta",
    pnpm: "pnpm",
    "grok-native": "Grok 原生",
    "grok-home": "~/.grok",
    system: "系统",
  };
  return map[source] || source || "—";
}

/**
 * @param {{ force?: boolean }} [opts]
 */
async function loadEnvironment(opts = {}) {
  const force = opts.force === true;
  if (envCheckInflight) return envCheckInflight;
  if (!force && latestEnv) {
    renderOverview(latestEnv);
    return latestEnv;
  }
  if (overviewGrid && !latestEnv) {
    overviewGrid.innerHTML = `<div class="overview-loading muted">正在检测本地环境…</div>`;
  }
  envCheckInflight = (async () => {
    try {
      if (typeof window.skinAPI?.envCheck !== "function") {
        throw new Error("envCheck API 不可用");
      }
      const data = await window.skinAPI.envCheck({ force });
      latestEnv = data;
      renderOverview(data);
      return data;
    } catch (err) {
      const msg = err?.message || String(err);
      if (overviewGrid) {
        overviewGrid.innerHTML = `<div class="overview-loading" style="color:#b45309">环境检测失败：${escapeHtml(msg)}</div>`;
      }
      const runtimesEl = document.getElementById("overviewRuntimes");
      if (runtimesEl) {
        runtimesEl.hidden = true;
        runtimesEl.innerHTML = "";
      }
      throw err;
    } finally {
      envCheckInflight = null;
    }
  })();
  return envCheckInflight;
}

/**
 * @param {any} tool
 * @param {{ npmInstalled?: boolean }} [ctx]
 */
function buildInstallActionHtml(tool, ctx = {}) {
  if (tool?.installed) return "";
  const install = tool?.install && typeof tool.install === "object"
    ? tool.install
    : OV_FALLBACK_INSTALL[tool?.id] || null;
  if (!install) return "";

  const type = String(install.type || "");
  const label = escapeHtml(String(install.label || "安装"));
  const hint = install.hint ? escapeHtml(String(install.hint)) : "";

  if (type === "url" && install.url) {
    return `
      <div class="ov-card-actions">
        <button type="button" class="chip-btn chip-primary ov-install-btn"
          data-install-type="url"
          data-install-url="${escapeHtml(String(install.url))}"
          title="${hint || "打开官网下载页"}">
          <span class="chip-label">${label}</span>
        </button>
        <span class="ov-install-hint muted">跳转官网，由用户自行下载安装</span>
      </div>`;
  }

  if (type === "npm" && install.command) {
    const cmd = String(install.command);
    const npmOk = ctx.npmInstalled !== false;
    const disabled = npmOk ? "" : "disabled";
    const title = !npmOk
      ? "未检测到 npm，请先安装 Node.js / npm"
      : hint || `在系统终端执行：${cmd}`;
    return `
      <div class="ov-card-actions">
        <button type="button" class="chip-btn chip-primary ov-install-btn"
          data-install-type="npm"
          data-install-cmd="${escapeHtml(cmd)}"
          ${disabled}
          title="${escapeHtml(title)}">
          <span class="chip-label">${label}</span>
        </button>
        <code class="ov-install-cmd" title="安装命令">${escapeHtml(cmd)}</code>
        ${
          npmOk
            ? `<span class="ov-install-hint muted">将拉起系统终端执行（Windows / macOS 自动适配）</span>`
            : `<span class="ov-install-hint ov-install-warn">需要先安装 npm / Node.js</span>`
        }
      </div>`;
  }

  return "";
}

/**
 * @param {any} data
 */
function renderOverviewRuntimes(data) {
  const el = document.getElementById("overviewRuntimes");
  if (!el) return;
  const runtimes = Array.isArray(data?.runtimes) ? data.runtimes : [];
  if (!runtimes.length) {
    el.hidden = true;
    el.innerHTML = "";
    return;
  }
  el.hidden = false;
  el.innerHTML = `
    <div class="overview-runtimes-head">
      <span class="overview-runtimes-title">运行环境</span>
      <span class="overview-runtimes-sub muted">CLI 安装依赖 Node.js 与 npm</span>
    </div>
    <div class="overview-runtimes-list">
      ${runtimes
        .map((rt) => {
          const id = rt.id || "";
          const ico = OV_TOOL_ICONS[id] || OV_TOOL_ICONS.node;
          const installed = Boolean(rt.installed);
          const version = rt.version ? escapeHtml(String(rt.version)) : "—";
          const path = rt.path ? escapeHtml(String(rt.path)) : "";
          return `
            <div class="ov-runtime-chip ${installed ? "is-ok" : "is-miss"}" data-runtime-id="${escapeHtml(id)}" title="${path || version}">
              <span class="${ico.className}" aria-hidden="true">
                <svg viewBox="0 0 24 24">${ico.svg}</svg>
              </span>
              <div class="ov-runtime-meta">
                <strong>${escapeHtml(rt.name || id)}</strong>
                <span>${installed ? `已安装 · ${version}` : "未检测到"}</span>
              </div>
            </div>`;
        })
        .join("")}
    </div>`;
}

/**
 * @param {any} data
 */
function renderOverview(data) {
  if (!data) return;

  renderOverviewRuntimes(data);

  const summary = data.summary || {};
  const npmInstalled = summary.npmInstalled !== false;
  // Prefer explicit runtime probe when present
  const npmRt = Array.isArray(data.runtimes)
    ? data.runtimes.find((r) => r.id === "npm")
    : null;
  const npmOk = npmRt ? Boolean(npmRt.installed) : npmInstalled;

  if (!overviewGrid) return;
  const tools = Array.isArray(data.tools) ? data.tools : [];
  if (!tools.length) {
    overviewGrid.innerHTML = `<div class="overview-loading muted">未返回组件信息</div>`;
    return;
  }

  overviewGrid.innerHTML = tools
    .map((tool) => {
      const id = tool.id || "";
      const ico = OV_TOOL_ICONS[id] || OV_TOOL_ICONS["chatgpt-desktop"];
      const installed = Boolean(tool.installed);
      const skinOk = Boolean(tool.skinSupported);
      const badges = [
        `<span class="ov-badge ${installed ? "is-ok" : "is-miss"}">${installed ? "已安装" : "未安装"}</span>`,
        skinOk
          ? `<span class="ov-badge is-skin">支持皮肤</span>`
          : `<span class="ov-badge is-noskin">不支持皮肤</span>`,
      ].join("");
      const version = tool.version ? escapeHtml(String(tool.version)) : "—";
      const path = tool.path ? escapeHtml(String(tool.path)) : "—";
      const source = tool.source ? escapeHtml(sourceLabel(tool.source)) : "—";
      const note = tool.note ? `<p class="ov-card-note">${escapeHtml(String(tool.note))}</p>` : "";
      const err = tool.error
        ? `<p class="ov-card-err">诊断：${escapeHtml(String(tool.error))}</p>`
        : "";
      const actions = buildInstallActionHtml(tool, { npmInstalled: npmOk });
      return `
        <article class="ov-card" role="listitem" data-tool-id="${escapeHtml(id)}">
          <div class="ov-card-head">
            <span class="${ico.className}" aria-hidden="true">
              <svg viewBox="0 0 24 24">${ico.svg}</svg>
            </span>
            <div class="ov-card-titles">
              <h2>${escapeHtml(tool.name || id)}</h2>
              <div class="ov-card-kind">${escapeHtml(kindLabel(tool.kind))}</div>
              <div class="ov-badges">${badges}</div>
            </div>
          </div>
          <dl class="ov-meta-list">
            <div class="ov-meta-row"><dt>版本</dt><dd>${version}</dd></div>
            <div class="ov-meta-row"><dt>路径</dt><dd title="${path}">${path}</dd></div>
            <div class="ov-meta-row"><dt>来源</dt><dd>${source}</dd></div>
          </dl>
          ${note}
          ${err}
          ${actions}
        </article>`;
    })
    .join("");
}

/**
 * Handle install buttons on overview cards (event delegation).
 * @param {Event} ev
 */
async function onOverviewInstallClick(ev) {
  const btn = ev.target?.closest?.(".ov-install-btn");
  if (!btn || btn.disabled) return;
  const type = btn.getAttribute("data-install-type") || "";
  if (type === "url") {
    const url = btn.getAttribute("data-install-url") || "";
    if (!url) return;
    btn.disabled = true;
    try {
      await openExternalUrl(url);
      showToast("已打开下载页，安装完成后请刷新检测");
    } catch (err) {
      showToast(err?.message || "无法打开下载页", "error");
    } finally {
      btn.disabled = false;
    }
    return;
  }
  if (type === "npm") {
    const command = btn.getAttribute("data-install-cmd") || "";
    if (!command) return;
    btn.disabled = true;
    try {
      if (typeof window.skinAPI?.openInstallTerminal !== "function") {
        throw new Error("安装终端 API 不可用");
      }
      const res = await window.skinAPI.openInstallTerminal(command);
      showToast(res?.message || "已打开系统终端执行安装命令");
    } catch (err) {
      showToast(err?.message || "无法打开系统终端", "error");
    } finally {
      btn.disabled = false;
    }
  }
}

overviewGrid?.addEventListener("click", (ev) => {
  onOverviewInstallClick(ev).catch((err) => {
    console.warn("install action failed", err);
  });
});

document.getElementById("btnEnvRefresh")?.addEventListener("click", async () => {
  const btn = document.getElementById("btnEnvRefresh");
  if (btn) btn.disabled = true;
  try {
    await loadEnvironment({ force: true });
    showToast("环境信息已刷新");
  } catch (err) {
    showToast(err?.message || "刷新失败", "error");
  } finally {
    if (btn) btn.disabled = false;
  }
});

function syncAboutVersionUi() {
  const badge = document.getElementById("aboutVersionBadge");
  const text = document.getElementById("aboutVersionText");
  const dateEl = document.getElementById("aboutReleaseDate");
  const ver = `v${APP_VERSION}`;
  if (badge) badge.textContent = ver;
  if (text) text.textContent = ver;
  if (dateEl) dateEl.textContent = APP_RELEASE_DATE;
}

/**
 * 打开外链（Tauri open_external，失败则 window.open）
 * @param {string} url
 */
async function openExternalUrl(url) {
  if (!url) return;
  try {
    if (window.skinAPI?.openExternal) {
      await window.skinAPI.openExternal(url);
      return;
    }
  } catch {
    /* fall through */
  }
  try {
    window.open(url, "_blank", "noopener,noreferrer");
  } catch {
    showToast("无法打开链接", "error");
  }
}

/* —— 分类 / 搜索 / 关于 —— */
// Category filter buttons are built from skin-categories.json (see loadSkinCategories).
bindCategoryNav();

searchInput?.addEventListener("input", () => {
  searchQuery = searchInput.value || "";
  if (activeView !== "skins") return;
  if (latestStatus) render(latestStatus);
});

/* 关于页：检查更新（云端 catalog / version） */
const btnCheckUpdate = document.getElementById("btnCheckUpdate");
const aboutUpdateStatus = document.getElementById("aboutUpdateStatus");
/** @type {any|null} last result that truly has an update (for reopening dialog) */
let lastUpdateResult = null;

/** Loose semver compare: a < b → -1, a==b → 0, a > b → 1 */
function compareAppVersions(a, b) {
  const parse = (s) =>
    String(s || "")
      .trim()
      .replace(/^v/i, "")
      .split(/[.+-]/)
      .slice(0, 3)
      .map((p) => parseInt(String(p).replace(/\D/g, ""), 10) || 0);
  const pa = parse(a);
  const pb = parse(b);
  for (let i = 0; i < 3; i++) {
    const x = pa[i] || 0;
    const y = pb[i] || 0;
    if (x < y) return -1;
    if (x > y) return 1;
  }
  return 0;
}

/**
 * Whether the check result means the user should update.
 * Trusts server flags, but rejects "updateAvailable" when latest ≤ current.
 * Cloud admin `message` alone must never imply an update.
 */
function hasActualUpdate(result) {
  if (!result || typeof result !== "object") return false;
  const current = String(result.current || APP_VERSION).replace(/^v/i, "");
  const latest = result.latest != null && result.latest !== ""
    ? String(result.latest).replace(/^v/i, "")
    : "";

  // Force update by minAppVersion (server)
  if (result.updateRequired === true) {
    if (result.minAppVersion) {
      return compareAppVersions(current, result.minAppVersion) < 0;
    }
    return true;
  }

  // Optional update: latest must be strictly newer
  if (latest) {
    if (compareAppVersions(current, latest) < 0) return true;
    return false;
  }

  // No latest to compare — only honor explicit true if not contradicted
  if (result.updateAvailable === true) return false;
  return false;
}

function clearUpdateStatusInteractive() {
  if (!aboutUpdateStatus) return;
  aboutUpdateStatus.removeAttribute("title");
  aboutUpdateStatus.setAttribute("role", "status");
  aboutUpdateStatus.removeAttribute("tabindex");
  aboutUpdateStatus.classList.remove("is-update");
}

/**
 * 发现新版本弹窗：仅在 hasActualUpdate 时调用
 * @param {object} result cloudCheckUpdate 结果
 * @returns {Promise<"open"|"later">}
 */
function showUpdateDialog(result = {}) {
  // Hard guard: never open update UI when there is no real update
  if (!hasActualUpdate(result)) {
    return Promise.resolve("later");
  }

  const modal = document.getElementById("updateModal");
  const titleEl = document.getElementById("updateTitle");
  const versionsEl = document.getElementById("updateVersions");
  const msgEl = document.getElementById("updateMessage");
  const notesWrap = document.getElementById("updateNotesWrap");
  const notesEl = document.getElementById("updateNotes");
  const btnLater = document.getElementById("btnUpdateLater");
  const btnOpen = document.getElementById("btnUpdateOpen");

  const latest = result?.latest ? String(result.latest) : "";
  const current = result?.current ? String(result.current) : APP_VERSION;
  const required = Boolean(result?.updateRequired) && hasActualUpdate(result);
  // Prompt only for update path (backend already scopes message; re-assert here)
  const msg =
    (result?.message && String(result.message).trim()) ||
    (latest ? `发现新版本 ${latest}` : "发现新版本");
  const notes = String(result?.releaseNotes || result?.notes || "").trim();
  const downloadUrl = String(result?.downloadUrl || "").trim();

  if (!modal || !btnLater) {
    const body = notes ? `${msg}\n\n更新说明：\n${notes}` : msg;
    if (downloadUrl) {
      return showConfirm({
        title: required ? "需要更新" : "发现新版本",
        message: `${body}\n\n是否打开下载页？`,
        confirmText: "打开下载页",
        cancelText: "稍后",
        variant: required ? "warn" : "primary",
      }).then((ok) => (ok ? "open" : "later"));
    }
    return showConfirm({
      title: required ? "需要更新" : "发现新版本",
      message: body,
      confirmText: "知道了",
      cancelText: "关闭",
      variant: required ? "warn" : "primary",
    }).then(() => "later");
  }

  if (typeof showUpdateDialog._dismiss === "function") {
    showUpdateDialog._dismiss("later");
  }

  if (titleEl) titleEl.textContent = required ? "需要更新" : "发现新版本";
  if (versionsEl) {
    versionsEl.textContent = latest
      ? `当前 v${current.replace(/^v/i, "")}  →  最新 v${latest.replace(/^v/i, "")}`
      : `当前 v${current.replace(/^v/i, "")}`;
  }
  if (msgEl) msgEl.textContent = msg;

  if (notesWrap && notesEl) {
    if (notes) {
      notesWrap.hidden = false;
      notesEl.textContent = notes;
    } else {
      notesWrap.hidden = true;
      notesEl.textContent = "";
    }
  }

  if (btnOpen) {
    if (downloadUrl) {
      btnOpen.hidden = false;
      btnOpen.textContent = "打开下载页";
    } else {
      btnOpen.hidden = true;
    }
  }
  btnLater.textContent = downloadUrl ? "稍后" : "知道了";

  return new Promise((resolve) => {
    let settled = false;
    const dismiss = (action) => {
      if (settled) return;
      settled = true;
      modal.hidden = true;
      modal.classList.remove("show");
      btnLater.removeEventListener("click", onLater);
      btnOpen?.removeEventListener("click", onOpen);
      modal.removeEventListener("click", onBackdrop);
      document.removeEventListener("keydown", onKey, true);
      if (showUpdateDialog._dismiss === dismiss) showUpdateDialog._dismiss = null;
      resolve(action === "open" ? "open" : "later");
    };
    const onLater = () => dismiss("later");
    const onOpen = () => dismiss("open");
    const onBackdrop = (e) => {
      if (e.target === modal) dismiss("later");
    };
    const onKey = (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        dismiss("later");
      }
    };

    btnLater.addEventListener("click", onLater);
    btnOpen?.addEventListener("click", onOpen);
    modal.addEventListener("click", onBackdrop);
    document.addEventListener("keydown", onKey, true);
    showUpdateDialog._dismiss = dismiss;

    modal.hidden = false;
    modal.classList.add("show");
    (downloadUrl && btnOpen ? btnOpen : btnLater).focus?.();
  });
}

/**
 * 展示更新结果：状态行固定文案（有更新 / 已是最新），不展示云端 message。
 * 弹窗内仍可用云端提示与更新说明。
 * @param {object} result
 * @param {{ silent?: boolean }} [opts]
 */
async function presentUpdateResult(result, opts = {}) {
  const silent = opts.silent === true;
  const hasUpdate = hasActualUpdate(result);

  if (hasUpdate) {
    lastUpdateResult = result;
    // 状态行固定文案，不同步云端「检查更新提示文案」
    if (aboutUpdateStatus) {
      aboutUpdateStatus.className = "about-update-status is-update";
      aboutUpdateStatus.textContent = "发现新版本，点击查看";
      aboutUpdateStatus.title = "点击查看更新详情";
      aboutUpdateStatus.setAttribute("role", "button");
      aboutUpdateStatus.tabIndex = 0;
    }
    if (!silent) {
      const action = await showUpdateDialog(result);
      if (action === "open" && result?.downloadUrl) {
        await openExternalUrl(result.downloadUrl);
      }
    }
    return;
  }

  // 无更新：固定文案，不弹窗
  lastUpdateResult = null;
  if (aboutUpdateStatus) {
    aboutUpdateStatus.className = "about-update-status is-latest";
    aboutUpdateStatus.textContent = "已是最新版本";
    clearUpdateStatusInteractive();
  }
}

async function runCheckUpdate({ reopenOnly = false } = {}) {
  // 状态行点击：仅当上次结果确实有更新时复开弹窗
  if (reopenOnly) {
    if (lastUpdateResult && hasActualUpdate(lastUpdateResult)) {
      const action = await showUpdateDialog(lastUpdateResult);
      if (action === "open" && lastUpdateResult.downloadUrl) {
        await openExternalUrl(lastUpdateResult.downloadUrl);
      }
      return;
    }
    // 无有效缓存：不强制重检，避免误弹
    return;
  }

  if (btnCheckUpdate?.disabled) return;
  if (btnCheckUpdate) {
    btnCheckUpdate.disabled = true;
    btnCheckUpdate.textContent = "检查中…";
  }
  if (aboutUpdateStatus) {
    aboutUpdateStatus.className = "about-update-status is-checking";
    aboutUpdateStatus.textContent = "正在检查更新…";
    clearUpdateStatusInteractive();
  }
  try {
    if (typeof window.skinAPI?.cloudCheckUpdate !== "function") {
      throw new Error("云端版本检查不可用");
    }
    const result = await window.skinAPI.cloudCheckUpdate();
    const hasUpdate = hasActualUpdate(result);
    await presentUpdateResult(result);
    if (!hasUpdate) {
      showToast("已是最新版本", "ok");
    } else if (result?.updateRequired) {
      showToast("需要更新", "error");
    }
  } catch (err) {
    lastUpdateResult = null;
    if (aboutUpdateStatus) {
      aboutUpdateStatus.className = "about-update-status is-error";
      aboutUpdateStatus.textContent = friendlyError(err);
      clearUpdateStatusInteractive();
    }
    showToast(friendlyError(err), "error");
  } finally {
    if (btnCheckUpdate) {
      btnCheckUpdate.disabled = false;
      btnCheckUpdate.textContent = "检查更新";
    }
  }
}

btnCheckUpdate?.addEventListener("click", () => {
  runCheckUpdate({ reopenOnly: false });
});

// 仅「有更新」状态可点开弹窗
aboutUpdateStatus?.addEventListener("click", () => {
  if (!aboutUpdateStatus.classList.contains("is-update")) return;
  if (!lastUpdateResult || !hasActualUpdate(lastUpdateResult)) return;
  runCheckUpdate({ reopenOnly: true });
});
aboutUpdateStatus?.addEventListener("keydown", (e) => {
  if (!aboutUpdateStatus.classList.contains("is-update")) return;
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    if (!lastUpdateResult || !hasActualUpdate(lastUpdateResult)) return;
    runCheckUpdate({ reopenOnly: true });
  }
});

document.getElementById("btnAboutHelp")?.addEventListener("click", () => {
  openHelp();
});

aboutView?.addEventListener("click", (e) => {
  const t = e.target;
  if (!(t instanceof Element)) return;
  const external = t.closest("[data-external]");
  if (external) {
    e.preventDefault();
    const url = external.getAttribute("data-external");
    if (url) openExternalUrl(url);
    return;
  }
  const mail = t.closest("[data-mailto]");
  if (mail) {
    e.preventDefault();
    const addr = mail.getAttribute("data-mailto");
    if (addr) openExternalUrl(`mailto:${addr}`);
  }
});

syncAboutVersionUi();

/* —— DevTools 独立窗口（自定义皮肤弹窗顶部；不绑定 F12，避免占用系统/WebView 调试快捷键） —— */
/** 防止连点并发 invoke；后端也会单例复用 `devtools` 窗口 */
let openingDevtools = false;
async function openDevtoolsWindow() {
  if (openingDevtools) return;
  openingDevtools = true;
  const btn = document.getElementById("btnDevtools");
  if (btn) btn.disabled = true;
  try {
    await window.skinAPI.openDevtools();
  } catch (err) {
    showToast(friendlyError(err), "error");
  } finally {
    openingDevtools = false;
    if (btn) btn.disabled = false;
  }
}
document.getElementById("btnDevtools")?.addEventListener("click", () => {
  openDevtoolsWindow();
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
    // 确认框 / 更新弹窗有独立 keydown（capture），此处只处理其它弹层
    const confirmModal = document.getElementById("confirmModal");
    if (confirmModal?.classList.contains("show")) return;
    const updateModal = document.getElementById("updateModal");
    if (updateModal?.classList.contains("show")) return;
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

document.getElementById("btnRefresh")?.addEventListener("click", async () => {
  setBusy(true, "刷新中…");
  try {
    await refreshCloudAndSkins(true);
    showToast("已刷新", "ok");
  } catch (err) {
    showToast(friendlyError(err), "error");
  } finally {
    setBusy(false);
  }
});

/* —— 云端公告 banner —— */
function stopPromoTimer() {
  if (promoTimer) {
    clearInterval(promoTimer);
    promoTimer = null;
  }
}

function startPromoTimer() {
  stopPromoTimer();
  if (promoItems.length <= 1) return;
  promoTimer = setInterval(() => {
    promoIndex = (promoIndex + 1) % promoItems.length;
    paintPromo(promoIndex);
  }, 8000);
}

function paintPromo(index) {
  const banner = document.getElementById("promoBanner");
  if (!banner || !promoItems.length) return;
  const item = promoItems[index % promoItems.length] || DEFAULT_PROMO;
  promoIndex = index % promoItems.length;
  const level = item.level || "info";
  banner.dataset.level = level;
  banner.dataset.announceId = item.id || "";
  const ico = document.getElementById("promoBannerIco");
  const titleEl = document.getElementById("promoBannerTitle");
  const textEl = document.getElementById("promoBannerText");
  const linkEl = document.getElementById("promoBannerLink");
  const dismissEl = document.getElementById("promoBannerDismiss");
  const dotsEl = document.getElementById("promoBannerDots");
  if (ico) {
    ico.textContent =
      level === "critical" ? "❗" : level === "warn" ? "⚠️" : "📢";
  }
  // 客户端公告位只展示正文，不显示标题（标题仅管理端/检索用）
  if (titleEl) {
    titleEl.hidden = true;
    titleEl.textContent = "";
  }
  if (textEl) {
    textEl.textContent = item.body || item.title || DEFAULT_PROMO.body;
  }
  if (linkEl) {
    const href = item.link || "";
    if (href && /^https?:\/\//i.test(href)) {
      linkEl.hidden = false;
      linkEl.textContent = item.linkLabel || "了解更多";
      linkEl.href = href;
      linkEl.onclick = (e) => {
        e.preventDefault();
        openExternalUrl(href);
      };
    } else {
      linkEl.hidden = true;
      linkEl.removeAttribute("href");
      linkEl.onclick = null;
    }
  }
  if (dismissEl) {
    const canDismiss = item.dismissible !== false && item.id && item.id !== "local-default";
    dismissEl.hidden = !canDismiss;
    banner.classList.toggle("is-dismissible", canDismiss);
  } else {
    banner.classList.remove("is-dismissible");
  }
  if (dotsEl) {
    if (promoItems.length <= 1) {
      dotsEl.innerHTML = `<span class="dot on"></span>`;
    } else {
      dotsEl.innerHTML = promoItems
        .map(
          (_, i) =>
            `<span class="dot${i === promoIndex ? " on" : ""}" data-promo-dot="${i}" role="button" tabindex="0"></span>`
        )
        .join("");
      dotsEl.querySelectorAll("[data-promo-dot]").forEach((dot) => {
        dot.addEventListener("click", () => {
          const i = Number(dot.getAttribute("data-promo-dot") || 0);
          paintPromo(i);
          startPromoTimer();
        });
      });
    }
  }
}

function applyAnnouncementsToBanner(payload) {
  latestAnnouncements = payload || null;
  const items = Array.isArray(payload?.items) ? payload.items : [];
  // Prefer unread; if all read, still show active items once
  const unread = items.filter((it) => !it.read);
  const pool = (unread.length ? unread : items).slice(0, 8);
  promoItems = pool.length
    ? pool.map((it) => ({
        id: it.id,
        title: it.title || "",
        body: it.body || it.title || "",
        level: it.level || "info",
        link: it.link || "",
        linkLabel: it.linkLabel || "了解更多",
        dismissible: it.dismissible !== false,
      }))
    : [DEFAULT_PROMO];
  paintPromo(0);
  startPromoTimer();
}

document.getElementById("promoBannerDismiss")?.addEventListener("click", async () => {
  const id = document.getElementById("promoBanner")?.dataset?.announceId;
  if (!id || id === "local-default") return;
  try {
    if (typeof window.skinAPI?.cloudMarkAnnouncementRead === "function") {
      await window.skinAPI.cloudMarkAnnouncementRead(id);
    }
  } catch {
    /* ignore mark errors */
  }
  promoItems = promoItems.filter((it) => it.id !== id);
  if (!promoItems.length) promoItems = [DEFAULT_PROMO];
  paintPromo(0);
  startPromoTimer();
});

/**
 * Refresh cloud catalog/announcements then local status list.
 * @param {boolean} forceNetwork user-initiated: bypass soft TTL
 */
async function refreshCloudAndSkins(forceNetwork) {
  if (typeof window.skinAPI?.cloudRefresh === "function" && forceNetwork) {
    try {
      const res = await window.skinAPI.cloudRefresh({ force: true });
      const snap = res?.snapshot || res;
      if (snap?.announcements) {
        applyAnnouncementsToBanner(snap.announcements);
      }
    } catch {
      /* offline: still refresh local skins from disk merge */
    }
  } else if (typeof window.skinAPI?.cloudAnnouncements === "function") {
    try {
      const ann = await window.skinAPI.cloudAnnouncements({ refresh: false });
      applyAnnouncementsToBanner(ann);
    } catch {
      /* ignore */
    }
  }
  return refresh();
}

/** Delay before background CDN soft-sync so first paint stays snappy. */
const CLOUD_BOOT_DELAY_MS = 2800;

/**
 * Soft boot: disk cache first (no network), then delayed soft CDN sync.
 * Soft sync respects TTL; offline keeps cache without hammering the network.
 */
async function bootCloud() {
  try {
    // Disk-only snapshot (force:false never hits CDN in soft path)
    if (typeof window.skinAPI?.cloudStatus === "function") {
      const snap = await window.skinAPI.cloudStatus({ force: false });
      if (snap?.announcements) applyAnnouncementsToBanner(snap.announcements);
    }
  } catch {
    paintPromo(0);
  }

  // Deferred soft network: skin update flags + announcements after GUI is ready
  if (typeof window.skinAPI?.cloudRefresh !== "function") return;
  window.setTimeout(() => {
    if (document.visibilityState === "hidden") return;
    window.skinAPI
      .cloudRefresh({ force: false })
      .then((res) => {
        const snap = res?.snapshot || res;
        if (snap?.announcements) applyAnnouncementsToBanner(snap.announcements);
        // Soft sync may be skipped (cache-fresh) — only rebuild list when catalog may change
        const skipped = res?.sync?.skipped === true || snap?.sync?.skipped === true;
        if (!skipped) return refresh();
        return null;
      })
      .catch(() => {
        /* offline: keep disk cache */
      });
  }, CLOUD_BOOT_DELAY_MS);
}

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
  const mode = pauseButtonMode(latestStatus);
  if (mode === "idle") return;

  // Offline → launch ChatGPT; re-apply last skin when session exists
  if (mode === "start") {
    const lastId = latestStatus?.state?.skinId;
    const lastName =
      (lastId && (latestStatus?.skins || []).find((s) => s.id === lastId)?.name) || lastId;
    setBusy(
      true,
      lastId
        ? `正在启动 ChatGPT 并应用「${lastName}」…`
        : "正在启动 ChatGPT…"
    );
    try {
      if (typeof window.skinAPI?.startHost !== "function") {
        throw new Error("当前版本不支持启动客户端，请更新后重试");
      }
      const result = await window.skinAPI.startHost();
      if (result?.ok === false) {
        showToast(result?.error || "启动失败", "error");
      } else if (result?.mode === "apply-last-skin" || result?.skinId) {
        const name =
          result?.name ||
          (latestStatus?.skins || []).find((s) => s.id === result.skinId)?.name ||
          result.skinId ||
          lastName ||
          "上次皮肤";
        showToast(
          result?.artPending
            ? `已启动并换上「${name}」（立绘加载中）`
            : `已启动并换上「${name}」`,
          "ok"
        );
      } else {
        showToast("已启动 ChatGPT（可直接换肤）", "ok");
      }
      if (result?.lifecycle || result?.canHotApply !== undefined) {
        updateHostPill(result);
      }
      await refresh();
      await pollHostStatus(true);
    } catch (err) {
      showToast(friendlyError(err), "error");
      await pollHostStatus(true);
      try {
        await refresh();
      } catch {
        /* ignore */
      }
    } finally {
      setBusy(false);
    }
    return;
  }

  if (mode === "resume") {
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
      await pollHostStatus(true);
    } catch (err) {
      showToast(friendlyError(err), "error");
      await refresh();
    } finally {
      setBusy(false);
    }
    return;
  }

  // mode === "pause"
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
    await pollHostStatus(true);
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

/* —— 自定义皮肤（基于目标模板） —— */
const wallpaperModal = document.getElementById("wallpaperModal");
const wallpaperForm = document.getElementById("wallpaperForm");
const wallpaperBase = document.getElementById("wallpaperBase");
const wallpaperPath = document.getElementById("wallpaperPath");
const wallpaperFileName = document.getElementById("wallpaperFileName");
/** 与引擎 MAX_ART_BYTES 一致：壁纸选择硬上限 16 MB */
const WALLPAPER_MAX_BYTES = 16 * 1024 * 1024;

async function openWallpaper() {
  wallpaperModal.hidden = false;
  wallpaperModal.classList.add("show");
  const status = latestStatus || (await window.skinAPI.status());
  const skins = status.skins || [];
  // Prefer currently applied skin as template when available
  const activeId = status.activeSkinId || null;
  wallpaperBase.innerHTML = skins
    .map((skin) => {
      const selected = activeId && skin.id === activeId ? " selected" : "";
      return `<option value="${escapeHtml(skin.id)}"${selected}>${escapeHtml(skin.name)}${skin.builtin ? "（内置）" : ""}</option>`;
    })
    .join("");
  // Prefill color tokens from selected template when possible
  const selected = skins.find((s) => s.id === wallpaperBase.value) || skins[0];
  if (selected?.accent && /^#[0-9a-fA-F]{6}$/.test(selected.accent)) {
    document.getElementById("themeAccent").value = selected.accent;
  }
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
wallpaperBase?.addEventListener("change", async () => {
  try {
    const status = latestStatus || (await window.skinAPI.status());
    const skin = (status.skins || []).find((s) => s.id === wallpaperBase.value);
    if (skin?.accent && /^#[0-9a-fA-F]{6}$/.test(skin.accent)) {
      document.getElementById("themeAccent").value = skin.accent;
    }
  } catch {
    /* ignore */
  }
});
document.getElementById("btnChooseWallpaper").addEventListener("click", async () => {
  const picked = await window.skinAPI.chooseWallpaper();
  if (!picked?.path) return;
  if (picked.canceled) return;
  if (picked.error) {
    showToast(picked.error, "error");
    return;
  }
  if (typeof picked.size === "number" && picked.size > WALLPAPER_MAX_BYTES) {
    showToast("壁纸必须不超过 16 MB", "error");
    return;
  }
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
  if (!wallpaperBase.value) {
    showToast("请选择目标皮肤模板", "error");
    return;
  }
  setBusy(true, "正在基于模板生成自定义皮肤…");
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
    wallpaperFileName.textContent = "支持 PNG、JPG、WebP，最大 16 MB";
    await refresh();
    showToast(`已生成自定义皮肤「${result.name}」`, "ok");
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

  // Sidebar categories from skin-categories.json (not hardcoded skin membership)
  try {
    await loadSkinCategories();
  } catch {
    renderCategoryNav();
  }

  // Default promo until cloud responds
  paintPromo(0);

  // 默认进入概览：检测本机环境（与皮肤列表并行）
  try {
    setMainView("overview");
  } catch {
    /* ignore */
  }
  loadEnvironment({ force: true }).catch(() => {});

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
    // Cloud: announcements + catalog merge (async, non-fatal)
    bootCloud();
  } catch (err) {
    pillCodex.textContent = "引擎未就绪";
    pillCodex.className = "pill warn";
    pillActive.textContent = "当前皮肤：—";
    grid.innerHTML = `<article class="card"><div class="meta"><h2>无法加载皮肤列表</h2><p>${escapeHtml(friendlyError(err))}</p><p class="muted">请确认用 <code>npm run dev</code> 启动，并已安装 Node.js 18+。</p></div></article>`;
    showToast(friendlyError(err), "error");
  }
})();
