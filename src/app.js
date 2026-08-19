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
const toolboxView = document.getElementById("toolboxView");
const aboutView = document.getElementById("aboutView");
const overviewView = document.getElementById("overviewView");
const overviewGrid = document.getElementById("overviewGrid");

/**
 * Runtime product version from the Tauri package (Cargo / tauri.conf / package.json).
 * Never hardcode — about UI + update compare fall back only when Tauri is unavailable.
 * App updates / release notes use GitHub via tauri-plugin-updater (`checkAppUpdate`).
 */
let APP_VERSION = "";

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
/** 当前主区域视图：overview | skins | sessions | providers | toolbox | about */
let activeView = "overview";
/** @type {any | null} last env_check payload */
let latestEnv = null;
let envCheckInflight = null;
/** Per-card single-tool env probes currently in flight (tool id → Promise) */
const envToolInflight = new Map();
/** Tool ids currently showing card-level probing UI */
const probingToolIds = new Set();
let searchQuery = "";
/** Busy overlay active — pause host polling */
let uiBusy = false;
/** In-flight host_status promise (single-flight) */
let hostPollInflight = null;
let hostPollTimer = null;
let hostPollFailCount = 0;
/** Last skins signature so host-only updates do not rebuild the grid */
let lastSkinsSig = "";
/** In-flight catalog preview ensure (single-flight progressive fill) */
let previewEnsureInflight = null;
/** Another ensure requested while one is running */
let previewEnsureQueued = false;
/** Ids that failed last ensure pass — avoid tight retry loops */
let previewEnsureCooldown = new Map();
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
    cat === "toolbox" ||
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

  // Order: 概览 → 会话 → 供应商 → 全部皮肤 → 设置 → 关于
  // Insert skins group just before 设置 (fallback: 关于 / end).
  const toolboxBtn = categoryNav.querySelector(
    '[data-category="toolbox"], [data-view="toolbox"]'
  );
  const insertBefore = toolboxBtn || aboutBtn;
  if (insertBefore) {
    categoryNav.insertBefore(group, insertBefore);
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
    if (activeView === "toolbox") {
      el.classList.toggle("active", cat === "toolbox");
      el.classList.remove("is-branch");
      return;
    }
    if (activeView === "overview") {
      el.classList.toggle("active", cat === "overview");
      el.classList.remove("is-branch");
      return;
    }
    // skins view: highlight category filters; top-level feature menus inactive
    if (
      cat === "sessions" ||
      cat === "providers" ||
      cat === "toolbox" ||
      cat === "about" ||
      cat === "overview"
    ) {
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
          activeCategory !== "toolbox" &&
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
    if (cat === "toolbox" || btn.getAttribute("data-view") === "toolbox") {
      skinsNavExpanded = false;
      setMainView("toolbox");
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
    syncTitlebarHostActions(latestStatus || host);
    return;
  }
  const mapped = mapHostPill(host);
  pillCodex.textContent = mapped.text;
  pillCodex.className = mapped.cls;
  pillCodex.title = mapped.title;
  latestHost = { ...(latestHost || {}), ...host };
  if (latestStatus) mergeHostFields(latestStatus, host);
  syncTitlebarHostActions(latestStatus || host);
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
  syncTitlebarHostActions(status);
}

/** Host offline (no process / debug / renderer). */
function isHostOffline(status) {
  const host = latestHost || status || {};
  const lifecycle = host.lifecycle || status?.lifecycle || "offline";
  return (
    lifecycle === "offline" &&
    !host.processRunning &&
    !host.debugPortOpen &&
    !host.rendererReady &&
    !status?.codexRunning &&
    !host.codexRunning
  );
}

/**
 * Host client is running (process up and/or debug/renderer path alive).
 * Distinct from "starting but not yet injectable".
 */
function isHostClientRunning(status) {
  if (isHostOffline(status)) return false;
  const host = latestHost || status || {};
  const lifecycle = host.lifecycle || status?.lifecycle || "offline";
  return (
    lifecycle === "ready" ||
    lifecycle === "starting" ||
    host.processRunning === true ||
    host.codexRunning === true ||
    status?.codexRunning === true ||
    host.debugPortOpen === true ||
    host.rendererReady === true ||
    host.canHotApply === true
  );
}

/**
 * Inject path is healthy: host ready for live pause/resume (not merely process up).
 * Mid-start without renderer does not count as「皮肤启用正常」.
 */
function isSkinInjectionHealthy(status) {
  const host = latestHost || status || {};
  const lifecycle = host.lifecycle || status?.lifecycle || "offline";
  if (lifecycle === "ready") return true;
  if (host.canHotApply === true && (host.rendererReady === true || host.debugPortOpen === true)) {
    return true;
  }
  if (host.rendererReady === true) return true;
  return false;
}

/** Active skin card or session skinId (including paused). */
function hasSkinSession(status) {
  return Boolean(
    status?.state?.skinId ||
      status?.paused ||
      (status?.skins || []).some((s) => s.active)
  );
}

/**
 * Skin is engaged on a running host: active badge, or keep-alive holding inject,
 * or paused session that can be resumed.
 */
function isSkinEnabledOnHost(status) {
  if (!hasSkinSession(status)) return false;
  if (status?.paused) return Boolean(status?.state?.skinId);
  if ((status?.skins || []).some((s) => s.active)) return true;
  const host = latestHost || status || {};
  // Session + keep-alive means inject loop is holding the skin live.
  if (status?.state?.skinId && (status?.keepAlive || host.keepAlive)) return true;
  return false;
}

/**
 * Titlebar host button (#btnHost): start when offline, restart when running.
 * @returns {"start"|"restart"}
 */
function hostButtonMode(status) {
  return isHostOffline(status) ? "start" : "restart";
}

/**
 * Titlebar skin control (#btnSkinPause).
 * Visible only when:
 *   1) host client is running, and
 *   2) skin injection is healthy, and
 *   3) a skin is enabled (active / keep-alive) or paused on that host.
 * - pause: live skin engaged
 * - resume: session paused on a still-running host
 * - hidden: otherwise
 * @returns {"pause"|"resume"|"hidden"}
 */
function skinControlMode(status) {
  // Gate 1+2: running host + healthy inject path
  if (!isHostClientRunning(status) || !isSkinInjectionHealthy(status)) {
    return "hidden";
  }
  // Gate 3: skin actually enabled (or paused with restorable session)
  if (!isSkinEnabledOnHost(status)) return "hidden";

  if (status?.paused) return "resume";
  return "pause";
}

const HOST_ICO_START =
  '<svg viewBox="0 0 24 24"><path d="M8 5.5v13l11-6.5z"/></svg>';
const HOST_ICO_RESTART =
  '<svg viewBox="0 0 24 24"><path d="M4.5 12a7.5 7.5 0 0 1 12.7-5.4"/><path d="M17.5 4.5v3.2h-3.2"/><path d="M19.5 12a7.5 7.5 0 0 1-12.7 5.4"/><path d="M6.5 19.5v-3.2h3.2"/></svg>';
const SKIN_ICO_PAUSE =
  '<svg viewBox="0 0 24 24"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>';
const SKIN_ICO_RESUME =
  '<svg viewBox="0 0 24 24"><path d="M8 5.5v13l11-6.5z"/></svg>';

function syncHostButton(status) {
  const btn = document.getElementById("btnHost");
  const label = document.getElementById("btnHostLabel");
  const ico = document.getElementById("btnHostIco");
  if (!btn || !label) return;
  const mode = hostButtonMode(status);
  btn.dataset.mode = mode;
  btn.disabled = false;
  btn.classList.add("chip-primary");
  btn.classList.remove("chip-warn");
  if (mode === "start") {
    label.textContent = "启动 ChatGPT";
    btn.title = "启动 ChatGPT 客户端；若有上次使用的皮肤将自动应用";
    if (ico) ico.innerHTML = HOST_ICO_START;
  } else {
    label.textContent = "重启 ChatGPT";
    btn.title = "强制重启 ChatGPT 客户端；若有上次皮肤将在重启后重新应用";
    if (ico) ico.innerHTML = HOST_ICO_RESTART;
  }
}

function syncSkinPauseButton(status) {
  const btn = document.getElementById("btnSkinPause");
  const label = document.getElementById("btnSkinPauseLabel");
  const ico = document.getElementById("btnSkinPauseIco");
  if (!btn || !label) return;
  const mode = skinControlMode(status);
  btn.dataset.mode = mode;
  if (mode === "hidden") {
    btn.hidden = true;
    btn.disabled = true;
    return;
  }
  btn.hidden = false;
  btn.disabled = false;
  btn.classList.toggle("chip-warn", mode === "resume");
  if (mode === "resume") {
    label.textContent = "继续显示";
    btn.title = "清除暂停并重新应用当前皮肤";
    if (ico) ico.innerHTML = SKIN_ICO_RESUME;
  } else {
    label.textContent = "暂停皮肤";
    btn.title = "写入暂停标记并即时从 ChatGPT 窗口卸下皮肤（会话可恢复）";
    if (ico) ico.innerHTML = SKIN_ICO_PAUSE;
  }
}

/** Keep titlebar host + skin actions in sync with status / host polls. */
function syncTitlebarHostActions(status) {
  syncHostButton(status);
  syncSkinPauseButton(status);
}

/** @deprecated use syncTitlebarHostActions */
function syncPauseButton(status) {
  syncTitlebarHostActions(status);
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
    if (skin.id) card.dataset.skinId = String(skin.id);
    const hasPreview = Boolean(skin.previewUrl);
    const isRemote = skin.source === "remote" || skin.installState === "remote";
    const expectCloudPreview =
      !hasPreview &&
      (isRemote ||
        Boolean(skin.remotePreviewUrl) ||
        Boolean(skin.remotePreview?.url) ||
        skin.installState === "updateAvailable");
    const previewImg = hasPreview
      ? `<img class="preview-img" src="${skin.previewUrl}" alt="${escapeHtml(skin.name)}" draggable="false" loading="lazy" />`
      : "";
    const isFav = favorites.has(skin.id);
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

    const previewCls = expectCloudPreview ? "preview is-loading-preview" : "preview";
    card.innerHTML = `
      <div class="${previewCls}" style="background:${skin.previewGradient || "#eceff6"}">
        ${previewImg}
        ${expectCloudPreview ? `<span class="preview-loading-hint" aria-hidden="true">预览加载中</span>` : ""}
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
  // Progressive fill: only visible (filtered) cards that still lack previewUrl.
  scheduleEnsureCloudPreviews(filtered);
}

/** @param {any} skin */
function skinSourceLabel(skin) {
  const origin = skin.origin || "";
  const src = skin.source || (skin.builtin ? "bundled" : "user");
  if (src === "remote" || skin.installState === "remote") {
    return { label: "云端", cls: "tag-remote" };
  }
  if (origin === "cloud" || src === "cache") {
    return { label: "已安装", cls: "tag-cache" };
  }
  if (origin === "design") {
    return { label: "自定义", cls: "" };
  }
  if (origin === "workspace") {
    return { label: "工作区", cls: "" };
  }
  if (src === "bundled" || skin.builtin || origin === "seed") {
    return { label: "内置", cls: "" };
  }
  return { label: "已导入", cls: "" };
}

/** Safe attribute selector for skin id (ids are sanitized server-side). */
function skinCardSelector(id) {
  const safe = String(id || "").replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  return `[data-skin-id="${safe}"]`;
}

/**
 * @param {any[]} skins
 * @returns {string[]}
 */
function skinsNeedingCloudPreview(skins) {
  const now = Date.now();
  return (skins || [])
    .filter((s) => {
      if (!s?.id || s.previewUrl) return false;
      const coolUntil = previewEnsureCooldown.get(String(s.id)) || 0;
      if (coolUntil > now) return false;
      return (
        s.source === "remote" ||
        s.installState === "remote" ||
        s.remotePreviewUrl ||
        s.remotePreview?.url ||
        s.installState === "updateAvailable"
      );
    })
    .map((s) => String(s.id));
}

/** Clear loading shimmer on a card when preview is done or failed. */
function clearPreviewLoading(card) {
  if (!card) return;
  const box = card.querySelector(".preview");
  if (!box) return;
  box.classList.remove("is-loading-preview");
  box.querySelector(".preview-loading-hint")?.remove();
}

/**
 * Patch DOM + latestStatus when preview data-URLs arrive (avoid full grid rebuild).
 * @param {Record<string, string>} map
 */
function applyCloudPreviewMap(map) {
  if (!map || typeof map !== "object") return false;
  let changed = false;
  const skins = latestStatus?.skins;
  for (const [id, url] of Object.entries(map)) {
    if (!id || typeof url !== "string" || !url) continue;
    previewEnsureCooldown.delete(id);
    if (Array.isArray(skins)) {
      const skin = skins.find((s) => s.id === id);
      if (skin && skin.previewUrl !== url) {
        skin.previewUrl = url;
        skin.previewKind = skin.previewKind || "cloud-cache";
        changed = true;
      }
    }
    const card = grid?.querySelector?.(skinCardSelector(id));
    if (!card) continue;
    const box = card.querySelector(".preview");
    if (!box) continue;
    clearPreviewLoading(card);
    let img = box.querySelector("img.preview-img");
    if (!img) {
      img = document.createElement("img");
      img.className = "preview-img";
      img.draggable = false;
      img.loading = "lazy";
      img.alt = card.querySelector("h2")?.textContent || id;
      box.insertBefore(img, box.firstChild);
    }
    if (img.getAttribute("src") !== url) {
      img.setAttribute("src", url);
      changed = true;
    }
  }
  if (changed) {
    // Allow a later full render to pick up previewUrl in signature.
    lastSkinsSig = "";
  }
  return changed;
}

/**
 * After list paint: ensure missing catalog previews are fetched/cached in Rust,
 * then progressively fill card thumbnails (CSP-safe data URLs).
 * @param {any[]} [skins]
 */
function scheduleEnsureCloudPreviews(skins) {
  if (typeof window.skinAPI?.cloudEnsurePreviews !== "function") return;
  const pool = skins || latestStatus?.skins || [];
  const need = skinsNeedingCloudPreview(pool);
  if (!need.length) return;

  if (previewEnsureInflight) {
    previewEnsureQueued = true;
    return;
  }

  previewEnsureInflight = (async () => {
    let pending = [];
    try {
      const res = await window.skinAPI.cloudEnsurePreviews(need);
      const map =
        res?.previews && typeof res.previews === "object" ? res.previews : null;
      if (map && Object.keys(map).length) {
        applyCloudPreviewMap(map);
      }
      // Failures: cool down so we do not hammer CDN / host allowlist misses.
      const failed = Array.isArray(res?.failed) ? res.failed : [];
      const coolMs = 45_000;
      const until = Date.now() + coolMs;
      for (const f of failed) {
        const id = f?.id != null ? String(f.id) : "";
        if (!id) continue;
        previewEnsureCooldown.set(id, until);
        clearPreviewLoading(grid?.querySelector?.(skinCardSelector(id)));
      }
      // Network budget leftover → retry later without cooldown.
      pending = Array.isArray(res?.pending)
        ? res.pending.map(String).filter(Boolean)
        : [];
      // Cards still missing and not failed: drop shimmer if nothing more coming this pass.
      for (const id of need) {
        if (map?.[id]) continue;
        if (pending.includes(id)) continue;
        if (failed.some((f) => String(f?.id) === id)) continue;
        // No result and not pending — treat as soft fail (e.g. no catalog entry).
        if (!map?.[id]) {
          clearPreviewLoading(grid?.querySelector?.(skinCardSelector(id)));
        }
      }
    } catch (err) {
      console.warn("cloudEnsurePreviews failed", err);
      const until = Date.now() + 60_000;
      for (const id of need) {
        previewEnsureCooldown.set(id, until);
        clearPreviewLoading(grid?.querySelector?.(skinCardSelector(id)));
      }
    } finally {
      previewEnsureInflight = null;
      const queued = previewEnsureQueued;
      previewEnsureQueued = false;
      // Drain queue (list changed mid-flight) or continue pending network budget.
      if (queued) {
        window.setTimeout(() => scheduleEnsureCloudPreviews(latestStatus?.skins), 200);
      } else if (pending.length) {
        window.setTimeout(() => scheduleEnsureCloudPreviews(latestStatus?.skins), 1200);
      }
    }
  })();
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
          : view === "toolbox"
            ? "toolbox"
            : view === "overview"
              ? "overview"
              : "skins";
  const prev = activeView;
  activeView = next;
  if (overviewView) overviewView.hidden = activeView !== "overview";
  if (skinsView) skinsView.hidden = activeView !== "skins";
  if (sessionsView) sessionsView.hidden = activeView !== "sessions";
  if (providersView) providersView.hidden = activeView !== "providers";
  if (toolboxView) toolboxView.hidden = activeView !== "toolbox";
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
    activeView === "toolbox" ||
    activeView === "overview"
  ) {
    // 离开皮肤主分支：收起主题子菜单
    skinsNavExpanded = false;
  } else if (!opts.preserveSkinsExpand) {
    // 非侧栏显式控制的进入皮肤列表（如其它入口）默认展开
    skinsNavExpanded = true;
  }
  syncCategoryNavActive();
  if (activeView === "about") {
    const now = Date.now();
    if (now - lastAboutSyncTime > ABOUT_TAB_SYNC_COOLDOWN_MS) {
      loadAboutContactFromCloud({ refresh: true }).catch(() => {});
    }
  }
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
  if (activeView === "toolbox" && prev !== "toolbox") {
    try {
      window.toolboxView?.enter?.();
    } catch (err) {
      console.warn("toolboxView.enter failed", err);
    }
  } else if (prev === "toolbox" && activeView !== "toolbox") {
    try {
      window.toolboxView?.leave?.();
    } catch {
      /* ignore */
    }
  }
  // Overview: only re-paint cached env on switch — never re-probe.
  // Full env_check runs only when user clicks「刷新检测」(manual).
  if (activeView === "overview") {
    try {
      if (latestEnv) renderOverview(latestEnv);
      else ensureOverviewScaffold();
    } catch (err) {
      console.warn("overview repaint failed", err);
    }
  }
}

// Expose shell helpers for feature modules (sessions, future tools)
window.showToast = showToast;
window.showConfirm = showConfirm;

/* ── 环境概览 ─────────────────────────────────────────── */

/** Brand marks shared with providers / sessions tabs (filled paths). */
function ovLogoSvgChatgpt() {
  return `<svg viewBox="0 0 180 180" aria-hidden="true" focusable="false"><path fill="currentColor" d="M101.228 164.247C96.2776 164.247 91.5751 163.307 87.1201 161.426C82.6651 159.545 78.7051 156.921 75.2401 153.555C71.4781 154.842 67.5676 155.486 63.5086 155.486C56.8756 155.486 50.7376 153.852 45.0946 150.585C39.4516 147.318 34.8976 142.863 31.4326 137.22C28.0666 131.577 26.3836 125.291 26.3836 118.361C26.3836 115.49 26.7796 112.371 27.5716 109.005C23.6116 105.342 20.5426 101.135 18.3646 96.3828C16.1866 91.5318 15.0976 86.4828 15.0976 81.2358C15.0976 75.8898 16.2361 70.7418 18.5131 65.7918C20.7901 60.8418 23.9581 56.5848 28.0171 53.0208C32.1751 49.3578 36.9766 46.8333 42.4216 45.4473C43.5106 39.8043 45.7876 34.7553 49.2526 30.3003C52.8166 25.7463 57.1726 22.1823 62.3206 19.6083C67.4686 17.0343 72.9631 15.7473 78.8041 15.7473C83.7541 15.7473 88.4566 16.6878 92.9116 18.5688C97.3666 20.4498 101.327 23.0733 104.792 26.4393C108.554 25.1523 112.464 24.5088 116.523 24.5088C123.156 24.5088 129.294 26.1423 134.937 29.4093C140.58 32.6763 145.085 37.1313 148.451 42.7743C151.916 48.4173 153.648 54.7038 153.648 61.6338C153.648 64.5048 153.252 67.6233 152.46 70.9893C156.42 74.6523 159.489 78.9093 161.667 83.7603C163.845 88.5123 164.934 93.5118 164.934 98.7588C164.934 104.105 163.796 109.253 161.519 114.203C159.242 119.153 156.024 123.459 151.866 127.122C147.807 130.686 143.055 133.161 137.61 134.547C136.521 140.19 134.195 145.239 130.631 149.694C127.166 154.248 122.859 157.812 117.711 160.386C112.563 162.96 107.069 164.247 101.228 164.247ZM64.5481 145.685C69.4981 145.685 73.8046 144.645 77.4676 142.566L105.386 126.528C106.376 125.835 106.871 124.895 106.871 123.707V110.936L70.9336 131.577C68.7556 132.864 66.5776 132.864 64.3996 131.577L36.3331 115.391C36.3331 115.688 36.2836 116.034 36.1846 116.43C36.1846 116.826 36.1846 117.42 36.1846 118.212C36.1846 123.261 37.3726 127.914 39.7486 132.171C42.2236 136.329 45.6391 139.596 49.9951 141.972C54.3511 144.447 59.2021 145.685 64.5481 145.685ZM66.0331 121.479C66.6271 121.776 67.1716 121.925 67.6666 121.925C68.1616 121.925 68.6566 121.776 69.1516 121.479L80.2891 115.094L44.5006 94.3038C42.3226 93.0168 41.2336 91.0863 41.2336 88.5123V56.2878C36.2836 58.4658 32.3236 61.8318 29.3536 66.3858C26.3836 70.8408 24.8986 75.7908 24.8986 81.2358C24.8986 86.0868 26.1361 90.7398 28.6111 95.1948C31.0861 99.6498 34.3036 103.016 38.2636 105.293L66.0331 121.479ZM101.228 154.446C106.475 154.446 111.227 153.258 115.484 150.882C119.741 148.506 123.107 145.239 125.582 141.081C128.057 136.923 129.294 132.27 129.294 127.122V95.0463C129.294 93.8583 128.799 92.9673 127.809 92.3733L116.523 85.8393V127.271C116.523 129.845 115.434 131.775 113.256 133.062L85.1896 149.249C90.0406 152.714 95.3866 154.446 101.228 154.446ZM106.871 100.095V79.8993L90.09 70.3953L73.1611 79.8993V100.095L90.09 109.599L106.871 100.095ZM63.5086 52.7238C63.5086 50.1498 64.5976 48.2193 66.7756 46.9323L94.8421 30.7458C89.9911 27.2808 84.6451 25.5483 78.8041 25.5483C73.5571 25.5483 68.8051 26.7363 64.5481 29.1123C60.2911 31.4883 56.9251 34.7553 54.4501 38.9133C52.0741 43.0713 50.8861 47.7243 50.8861 52.8723V84.7998C50.8861 85.9878 51.3811 86.9283 52.3711 87.6213L63.5086 94.1553V52.7238ZM138.947 123.707C143.897 121.529 147.807 118.163 150.678 113.609C153.648 109.055 155.133 104.105 155.133 98.7588C155.133 93.9078 153.896 89.2548 151.421 84.7998C148.946 80.3448 145.728 76.9788 141.768 74.7018L113.999 58.6638C113.405 58.2678 112.86 58.1193 112.365 58.2183C111.87 58.2183 111.375 58.3668 110.88 58.6638L99.7426 64.9008L135.68 85.8393C136.769 86.4333 137.561 87.2253 138.056 88.2153C138.65 89.1063 138.947 90.1953 138.947 91.4823V123.707ZM109.098 48.2688C111.276 46.8828 113.454 46.8828 115.632 48.2688L143.847 64.7523C143.847 64.0593 143.847 63.1683 143.847 62.0793C143.847 57.3273 142.659 52.8228 140.283 48.5658C138.006 44.2098 134.69 40.7448 130.334 38.1708C126.077 35.5968 121.127 34.3098 115.484 34.3098C110.534 34.3098 106.227 35.3493 102.564 37.4283L74.6461 53.4663C73.6561 54.1593 73.1611 55.0998 73.1611 56.2878V69.0588L109.098 48.2688Z"/></svg>`;
}

function ovLogoSvgGrok() {
  return `<svg viewBox="0 0 34 33" aria-hidden="true" focusable="false"><path fill="currentColor" d="M13.2371 21.0407L24.3186 12.8506C24.8619 12.4491 25.6384 12.6057 25.8973 13.2294C27.2597 16.5185 26.651 20.4712 23.9403 23.1851C21.2297 25.8989 17.4581 26.4941 14.0108 25.1386L10.2449 26.8843C15.6463 30.5806 22.2053 29.6665 26.304 25.5601C29.5551 22.3051 30.562 17.8683 29.6205 13.8673L29.629 13.8758C28.2637 7.99809 29.9647 5.64871 33.449 0.844576C33.5314 0.730667 33.6139 0.616757 33.6964 0.5L29.1113 5.09055V5.07631L13.2343 21.0436"/><path fill="currentColor" d="M10.9503 23.0313C7.07343 19.3235 7.74185 13.5853 11.0498 10.2763C13.4959 7.82722 17.5036 6.82767 21.0021 8.2971L24.7595 6.55998C24.0826 6.07017 23.215 5.54334 22.2195 5.17313C17.7198 3.31926 12.3326 4.24192 8.67479 7.90126C5.15635 11.4239 4.0499 16.8403 5.94992 21.4622C7.36924 24.9165 5.04257 27.3598 2.69884 29.826C1.86829 30.7002 1.0349 31.5745 0.36364 32.5L10.9474 23.0341"/></svg>`;
}

/** Codex CLI mark (OpenAI Codex glyph) — brand color is pink, not black. */
function ovLogoSvgCodex() {
  return `<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path fill="currentColor" fill-rule="evenodd" clip-rule="evenodd" d="M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z"/></svg>`;
}

/**
 * @typedef {{ className: string, brand?: boolean, svg?: string, markup?: string }} OvIcon
 * brand icons: filled path via currentColor; stroke icons: outline glyphs.
 */
const OV_TOOL_ICONS = {
  "chatgpt-desktop": {
    className: "ov-card-ico ico-brand-logo ico-logo-chatgpt",
    brand: true,
    markup: () => ovLogoSvgChatgpt(),
  },
  "codex-cli": {
    className: "ov-card-ico ico-brand-logo ico-logo-codex",
    brand: true,
    markup: () => ovLogoSvgCodex(),
  },
  "grok-build": {
    className: "ov-card-ico ico-brand-logo ico-logo-grok",
    brand: true,
    markup: () => ovLogoSvgGrok(),
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

/** Always-visible tool cards before / while env_check runs. */
const OV_SCAFFOLD_TOOLS = [
  {
    id: "chatgpt-desktop",
    name: "ChatGPT / Codex 桌面端",
    kind: "desktop",
    skinSupported: true,
    installed: null,
    note: "支持本工具皮肤注入",
  },
  {
    id: "codex-cli",
    name: "Codex CLI",
    kind: "cli",
    skinSupported: false,
    installed: null,
    note: "命令行 Codex 不支持皮肤注入（仅桌面端可换肤）",
  },
  {
    id: "grok-build",
    name: "Grok Build",
    kind: "cli",
    skinSupported: false,
    installed: null,
    note: "CLI / 本地 Grok Build 环境",
  },
];

const OV_SCAFFOLD_RUNTIMES = [
  { id: "node", name: "Node.js", installed: null, kind: "runtime" },
  { id: "npm", name: "npm", installed: null, kind: "runtime" },
];

function ovScaffoldPayload(extra = {}) {
  return {
    ok: false,
    // Idle until user clicks「刷新检测」— no auto probe on boot.
    probing: false,
    tools: OV_SCAFFOLD_TOOLS.map((t) => ({ ...t })),
    runtimes: OV_SCAFFOLD_RUNTIMES.map((r) => ({ ...r })),
    summary: { npmInstalled: true, nodeInstalled: true },
    ...extra,
  };
}

/**
 * @param {string} id
 * @returns {string}
 */
function ovIconHtml(id) {
  const ico = OV_TOOL_ICONS[id] || OV_TOOL_ICONS["chatgpt-desktop"];
  if (ico.brand && typeof ico.markup === "function") {
    return `<span class="${ico.className}" aria-hidden="true">${ico.markup()}</span>`;
  }
  return `<span class="${ico.className}" aria-hidden="true"><svg viewBox="0 0 24 24">${ico.svg || ""}</svg></span>`;
}

/**
 * Full-environment probing chrome (runtimes strip「刷新检测」).
 * Does not show a page-level status paragraph — only button + card states.
 * @param {boolean} on
 * @param {string} [label]
 */
function setOverviewProbing(on, label) {
  overviewView?.classList.toggle("is-probing", !!on);
  const btn = document.getElementById("btnEnvRefresh");
  if (btn) {
    btn.classList.toggle("is-busy", !!on);
    if (on) {
      btn.setAttribute("aria-busy", "true");
      btn.dataset.label = btn.dataset.label || btn.textContent || "刷新检测";
      btn.textContent = label || "检测中…";
      btn.disabled = true;
    } else if (btn.dataset.label) {
      btn.textContent = btn.dataset.label;
      btn.removeAttribute("aria-busy");
      btn.disabled = false;
      delete btn.dataset.label;
    } else {
      btn.disabled = false;
    }
  }
  // When full refresh is running, mark every card refresh as busy.
  // Per-card-only probes use setCardProbing instead and leave other cards free.
  if (on) {
    overviewGrid?.querySelectorAll(".ov-card-refresh").forEach((el) => {
      el.classList.add("is-busy");
      el.setAttribute("aria-busy", "true");
      el.setAttribute("disabled", "");
      el.setAttribute("title", "正在检测…");
      el.setAttribute("aria-label", "正在检测…");
    });
  }
}

/**
 * Card-level probing chrome for a single tool id.
 * @param {string} toolId
 * @param {boolean} on
 */
function setCardProbing(toolId, on) {
  const id = String(toolId || "").trim();
  if (!id) return;
  if (on) probingToolIds.add(id);
  else probingToolIds.delete(id);

  // Prefer live DOM tweak before next full renderOverview; ids are allow-listed.
  const card = overviewGrid?.querySelector(`.ov-card[data-tool-id="${id}"]`);
  if (!card) return;
  const btn = card.querySelector(".ov-card-refresh");
  if (on) {
    card.classList.add("is-probing");
    card.classList.remove("is-idle");
    if (btn) {
      btn.classList.add("is-busy");
      btn.setAttribute("aria-busy", "true");
      btn.setAttribute("disabled", "");
      btn.setAttribute("title", "正在检测…");
      btn.setAttribute("aria-label", "正在检测…");
    }
  } else if (btn && !envCheckInflight) {
    btn.classList.remove("is-busy");
    btn.removeAttribute("aria-busy");
    btn.removeAttribute("disabled");
    btn.setAttribute("title", "仅刷新检测此项");
    btn.setAttribute("aria-label", "仅刷新检测此项");
  }
}

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
 * Paint scaffold immediately so overview never waits on env_check.
 * Safe to call multiple times; keeps latestEnv when already known.
 */
function ensureOverviewScaffold() {
  if (latestEnv) {
    renderOverview(latestEnv);
    return;
  }
  renderOverview(ovScaffoldPayload());
}

/**
 * @param {{ force?: boolean }} [opts]
 * Non-blocking: shows scaffold / stale data first, then fills in asynchronously.
 */
async function loadEnvironment(opts = {}) {
  const force = opts.force === true;
  if (envCheckInflight) return envCheckInflight;
  if (!force && latestEnv) {
    renderOverview(latestEnv);
    return latestEnv;
  }

  // Always keep cards visible — never replace the grid with a full-page spinner.
  if (!latestEnv) {
    // Manual trigger: flip idle scaffold into probing UI while env_check runs.
    renderOverview(ovScaffoldPayload({ probing: true }));
  } else {
    // Stale-while-revalidate: keep last result while re-probing.
    renderOverview({ ...latestEnv, probing: true });
  }
  setOverviewProbing(true, force && latestEnv ? "刷新中…" : "检测中…");

  envCheckInflight = (async () => {
    try {
      if (typeof window.skinAPI?.envCheck !== "function") {
        throw new Error("envCheck API 不可用");
      }
      const data = await window.skinAPI.envCheck({ force });
      latestEnv = data;
      probingToolIds.clear();
      renderOverview(data);
      return data;
    } catch (err) {
      const msg = err?.message || String(err);
      // Keep scaffold / last good data — surface error via toast (no status bar).
      if (!latestEnv) {
        renderOverview(
          ovScaffoldPayload({
            probeError: msg,
            note: `环境检测失败：${msg}`,
          })
        );
      }
      throw err;
    } finally {
      envCheckInflight = null;
      setOverviewProbing(false);
    }
  })();
  return envCheckInflight;
}

/**
 * Merge a single-tool probe result into latestEnv (or scaffold).
 * @param {string} toolId
 * @param {any} toolPayload tool object from env_check_tool
 * @param {"tool"|"runtime"} [kind]
 */
function mergeEnvToolResult(toolId, toolPayload, kind = "tool") {
  const base = latestEnv
    ? { ...latestEnv, tools: [...(latestEnv.tools || [])], runtimes: [...(latestEnv.runtimes || [])] }
    : ovScaffoldPayload();
  const listKey = kind === "runtime" ? "runtimes" : "tools";
  const list = Array.isArray(base[listKey]) ? [...base[listKey]] : [];
  const idx = list.findIndex((t) => t?.id === toolId);
  if (idx >= 0) list[idx] = { ...list[idx], ...toolPayload };
  else list.push(toolPayload);
  base[listKey] = list;
  base.ok = true;
  base.probing = false;
  delete base.probeError;
  // Light summary sync for npm gate on install CTAs.
  if (kind === "runtime" || listKey === "runtimes") {
    const npmRt = list.find((r) => r.id === "npm");
    const nodeRt = list.find((r) => r.id === "node");
    base.summary = {
      ...(base.summary || {}),
      npmInstalled: npmRt?.installed === true,
      nodeInstalled: nodeRt?.installed === true,
      runtimeReady: npmRt?.installed === true && nodeRt?.installed === true,
    };
  } else {
    const tools = base.tools || [];
    base.summary = {
      ...(base.summary || {}),
      installedCount: tools.filter((t) => t?.installed === true).length,
      toolCount: tools.length,
      skinCapable: tools.some((t) => t?.installed === true && t?.skinSupported === true),
    };
  }
  latestEnv = base;
  return base;
}

/**
 * Probe one Overview tool card (does not re-scan all environments).
 * @param {string} toolId
 */
async function loadEnvironmentTool(toolId) {
  const id = String(toolId || "").trim();
  if (!id) return null;
  if (envToolInflight.has(id)) return envToolInflight.get(id);
  // Full refresh already covers this card — wait for it instead of double work.
  if (envCheckInflight) return envCheckInflight;

  setCardProbing(id, true);
  // Re-paint so this card shows probing meta without touching others.
  if (latestEnv) {
    renderOverview({ ...latestEnv, probing: false });
  } else {
    renderOverview(ovScaffoldPayload({ probing: false }));
  }

  const job = (async () => {
    try {
      if (typeof window.skinAPI?.envCheckTool !== "function") {
        throw new Error("envCheckTool API 不可用");
      }
      const data = await window.skinAPI.envCheckTool(id);
      const tool = data?.tool;
      if (!tool || typeof tool !== "object") {
        throw new Error("单环境检测返回无效");
      }
      const kind = data?.kind === "runtime" ? "runtime" : "tool";
      const merged = mergeEnvToolResult(id, tool, kind);
      // Drop probing flag before final paint so the card settles immediately.
      probingToolIds.delete(id);
      renderOverview(merged);
      return merged;
    } catch (err) {
      probingToolIds.delete(id);
      // Keep last paint; caller toasts.
      if (latestEnv) renderOverview(latestEnv);
      else renderOverview(ovScaffoldPayload());
      throw err;
    } finally {
      envToolInflight.delete(id);
      probingToolIds.delete(id);
    }
  })();

  envToolInflight.set(id, job);
  return job;
}

/** Circular-arrow glyph for per-card env refresh (top-right of ov-card). */
const OV_REFRESH_SVG =
  `<svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true" focusable="false"><path fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" d="M20.5 12a8.5 8.5 0 1 1-2.48-6.02"/><path fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" d="M20.5 3.5V9h-5.5"/></svg>`;

/**
 * Top-right refresh control on each tool card (single-tool probe).
 * @param {{ probing?: boolean, known?: boolean, toolId?: string }} [opts]
 */
function buildCardRefreshHtml(opts = {}) {
  const probing = opts.probing === true;
  const toolId = opts.toolId ? escapeHtml(String(opts.toolId)) : "";
  const busy = probing ? " is-busy" : "";
  const title = probing ? "正在检测…" : "仅刷新检测此项";
  const dataAttr = toolId ? ` data-tool-id="${toolId}"` : "";
  return `<button type="button" class="icon-btn ov-card-refresh${busy}" title="${title}" aria-label="${title}"${dataAttr} ${probing ? 'aria-busy="true" disabled' : ""}>${OV_REFRESH_SVG}</button>`;
}

/**
 * @param {any} tool
 * @param {{ npmInstalled?: boolean, probing?: boolean }} [ctx]
 */
function buildInstallActionHtml(tool, ctx = {}) {
  // Only after a successful probe that confirms missing — never on idle / probing.
  if (tool?.installed !== false) return "";
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
/** Static copy under「运行环境」— Node is optional for this app itself. */
const OV_RUNTIMES_SUB_IDLE =
  "CLI 安装依赖 Node.js 与 npm 本工具可不依赖 node 运行";
const OV_RUNTIMES_SUB_PROBING = "正在后台检测 Node.js / npm…";

function renderOverviewRuntimes(data) {
  const el = document.getElementById("overviewRuntimes");
  if (!el) return;
  const runtimes = Array.isArray(data?.runtimes) ? data.runtimes : OV_SCAFFOLD_RUNTIMES;
  // Always visible — never hide the runtime strip while waiting on probe.
  el.hidden = false;
  const probing = data?.probing === true;

  // Only refresh list + subtitle. Keep head button / 窗口与托盘 block intact
  // so #btnEnvRefresh and #chkMinimizeToTray stay bound.
  const subEl = document.getElementById("overviewRuntimesSub");
  if (subEl) {
    subEl.textContent = probing ? OV_RUNTIMES_SUB_PROBING : OV_RUNTIMES_SUB_IDLE;
  }

  let listEl = document.getElementById("overviewRuntimesList");
  if (!listEl) {
    listEl = document.createElement("div");
    listEl.className = "overview-runtimes-list";
    listEl.id = "overviewRuntimesList";
    const head = el.querySelector(".overview-runtimes-head");
    if (head?.nextSibling) el.insertBefore(listEl, head.nextSibling);
    else el.appendChild(listEl);
  }

  listEl.innerHTML = runtimes
    .map((rt) => {
      const id = rt.id || "";
      const known = rt.installed === true || rt.installed === false;
      const installed = rt.installed === true;
      const version = rt.version ? escapeHtml(String(rt.version)) : "—";
      const path = rt.path ? escapeHtml(String(rt.path)) : "";
      const chipState = !known
        ? probing
          ? "is-probing"
          : "is-idle"
        : installed
          ? "is-ok"
          : "is-miss";
      const statusText = !known
        ? probing
          ? "检测中…"
          : "未检测"
        : installed
          ? `已安装 · ${version}`
          : "未检测到";
      return `
            <div class="ov-runtime-chip ${chipState}" data-runtime-id="${escapeHtml(id)}" title="${path || version}">
              ${ovIconHtml(id)}
              <div class="ov-runtime-meta">
                <strong>${escapeHtml(rt.name || id)}</strong>
                <span>${statusText}</span>
              </div>
            </div>`;
    })
    .join("");
}

/**
 * @param {any} data
 */
function renderOverview(data) {
  if (!data) return;

  renderOverviewRuntimes(data);

  const summary = data.summary || {};
  const globalProbing = data.probing === true;
  const npmRt = Array.isArray(data.runtimes)
    ? data.runtimes.find((r) => r.id === "npm")
    : null;
  let npmOk;
  if (globalProbing && (!npmRt || npmRt.installed == null)) {
    npmOk = undefined; // unknown — install buttons stay enabled
  } else if (npmRt && (npmRt.installed === true || npmRt.installed === false)) {
    npmOk = Boolean(npmRt.installed);
  } else {
    npmOk = summary.npmInstalled !== false;
  }

  if (!overviewGrid) return;
  const tools = Array.isArray(data.tools) && data.tools.length
    ? data.tools
    : OV_SCAFFOLD_TOOLS;

  overviewGrid.innerHTML = tools
    .map((tool) => {
      const id = tool.id || "";
      const probing = globalProbing || probingToolIds.has(id);
      const known = tool.installed === true || tool.installed === false;
      const installed = tool.installed === true;
      const skinOk = Boolean(tool.skinSupported);
      const statusBadge = !known
        ? probing
          ? `<span class="ov-badge is-probing">检测中</span>`
          : `<span class="ov-badge is-idle">未检测</span>`
        : probing
          ? `<span class="ov-badge is-probing">刷新中</span>`
          : `<span class="ov-badge ${installed ? "is-ok" : "is-miss"}">${installed ? "已安装" : "未安装"}</span>`;
      const badges = [
        statusBadge,
        skinOk
          ? `<span class="ov-badge is-skin">支持皮肤</span>`
          : `<span class="ov-badge is-noskin">不支持皮肤</span>`,
      ].join("");
      // Idle (not yet refreshed): keep card minimal — no meta / note / install.
      // After probe: show details; install CTA only when confirmed missing.
      let bodyHtml = "";
      if (!known && !probing) {
        bodyHtml = `<p class="ov-card-idle-hint muted">点击右上角刷新，仅检测本项是否已安装</p>`;
      } else if (probing && !known) {
        bodyHtml = `
          <dl class="ov-meta-list">
            <div class="ov-meta-row"><dt>版本</dt><dd>检测中…</dd></div>
            <div class="ov-meta-row"><dt>路径</dt><dd>检测中…</dd></div>
            <div class="ov-meta-row"><dt>来源</dt><dd>—</dd></div>
          </dl>`;
      } else if (probing && known) {
        // Stale-while-revalidate: keep last known details while this card re-probes.
        const version = tool.version ? escapeHtml(String(tool.version)) : "—";
        const path = tool.path ? escapeHtml(String(tool.path)) : "—";
        const source = tool.source ? escapeHtml(sourceLabel(tool.source)) : "—";
        bodyHtml = `
          <dl class="ov-meta-list">
            <div class="ov-meta-row"><dt>版本</dt><dd>${version}</dd></div>
            <div class="ov-meta-row"><dt>路径</dt><dd title="${path}">${path}</dd></div>
            <div class="ov-meta-row"><dt>来源</dt><dd>${source}</dd></div>
          </dl>
          <p class="ov-card-idle-hint muted">正在刷新本项…</p>`;
      } else {
        const version = tool.version ? escapeHtml(String(tool.version)) : "—";
        const path = tool.path ? escapeHtml(String(tool.path)) : "—";
        const source = tool.source ? escapeHtml(sourceLabel(tool.source)) : "—";
        const note = tool.note
          ? `<p class="ov-card-note">${escapeHtml(String(tool.note))}</p>`
          : "";
        const err = tool.error
          ? `<p class="ov-card-err">诊断：${escapeHtml(String(tool.error))}</p>`
          : "";
        const actions = buildInstallActionHtml(tool, { npmInstalled: npmOk });
        bodyHtml = `
          <dl class="ov-meta-list">
            <div class="ov-meta-row"><dt>版本</dt><dd>${version}</dd></div>
            <div class="ov-meta-row"><dt>路径</dt><dd title="${path}">${path}</dd></div>
            <div class="ov-meta-row"><dt>来源</dt><dd>${source}</dd></div>
          </dl>
          ${note}
          ${err}
          ${actions}`;
      }
      const cardCls = probing
        ? "ov-card is-probing"
        : !known
          ? "ov-card is-idle"
          : "ov-card";
      return `
        <article class="${cardCls}" role="listitem" data-tool-id="${escapeHtml(id)}">
          ${buildCardRefreshHtml({ probing, known, toolId: id })}
          <div class="ov-card-head">
            ${ovIconHtml(id)}
            <div class="ov-card-titles">
              <h2>${escapeHtml(tool.name || id)}</h2>
              <div class="ov-card-kind">${escapeHtml(kindLabel(tool.kind))}</div>
              <div class="ov-badges">${badges}</div>
            </div>
          </div>
          ${bodyHtml}
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

/**
 * Full env refresh from the runtimes strip「刷新检测」button.
 * @param {HTMLElement | null} [triggerBtn]
 */
function triggerOverviewRefresh(triggerBtn) {
  const headBtn = document.getElementById("btnEnvRefresh");
  if (triggerBtn) triggerBtn.disabled = true;
  if (headBtn) headBtn.disabled = true;
  loadEnvironment({ force: true })
    .then(() => {
      showToast("环境信息已刷新");
    })
    .catch((err) => {
      showToast(err?.message || "刷新失败", "error");
    })
    .finally(() => {
      if (headBtn && !envCheckInflight) headBtn.disabled = false;
      // Card refresh buttons are re-rendered by renderOverview; no need to re-enable.
    });
}

/**
 * Per-card single-tool refresh (does not call full env_check).
 * @param {string} toolId
 * @param {HTMLElement | null} [triggerBtn]
 */
function triggerOverviewToolRefresh(toolId, triggerBtn) {
  const id = String(toolId || "").trim();
  if (!id) return;
  if (triggerBtn) triggerBtn.disabled = true;
  const names = {
    "chatgpt-desktop": "ChatGPT / Codex 桌面端",
    "codex-cli": "Codex CLI",
    "grok-build": "Grok Build",
  };
  const label = names[id] || id;
  loadEnvironmentTool(id)
    .then(() => {
      showToast(`${label} 已刷新`);
    })
    .catch((err) => {
      showToast(err?.message || `${label} 刷新失败`, "error");
    });
}

overviewGrid?.addEventListener("click", (ev) => {
  const refreshBtn = ev.target?.closest?.(".ov-card-refresh");
  if (refreshBtn) {
    if (refreshBtn.disabled || refreshBtn.classList.contains("is-busy")) return;
    const card = refreshBtn.closest?.("[data-tool-id]");
    const toolId =
      refreshBtn.getAttribute("data-tool-id") ||
      card?.getAttribute("data-tool-id") ||
      "";
    if (toolId) {
      triggerOverviewToolRefresh(toolId, refreshBtn);
    } else {
      // Fallback: full refresh only if card id is missing.
      triggerOverviewRefresh(refreshBtn);
    }
    return;
  }
  onOverviewInstallClick(ev).catch((err) => {
    console.warn("install action failed", err);
  });
});

document.getElementById("btnEnvRefresh")?.addEventListener("click", () => {
  // Full scan: Node/npm + all tool cards. Card icons use env_check_tool instead.
  triggerOverviewRefresh(document.getElementById("btnEnvRefresh"));
});

function formatAboutVersion(ver) {
  const v = String(ver || "").trim().replace(/^v/i, "");
  return v ? `v${v}` : "…";
}

function syncAboutVersionUi() {
  const badge = document.getElementById("aboutVersionBadge");
  const text = document.getElementById("aboutVersionText");
  const ver = formatAboutVersion(APP_VERSION);
  if (badge) badge.textContent = ver;
  if (text) text.textContent = ver;
}

/**
 * Resolve installed app version from Tauri package metadata (single source of truth).
 * Release notes / newer versions come from GitHub `latest.json` on check-update.
 */
async function loadAppVersionFromPackage() {
  try {
    let ver = "";
    if (typeof window.skinAPI?.getAppVersion === "function") {
      ver = await window.skinAPI.getAppVersion();
    } else {
      const getVersion = window.__TAURI__?.app?.getVersion;
      if (typeof getVersion === "function") {
        ver = await getVersion();
      }
    }
    const normalized = String(ver || "")
      .trim()
      .replace(/^v/i, "");
    if (normalized) {
      APP_VERSION = normalized;
      syncAboutVersionUi();
    }
  } catch (err) {
    console.warn("loadAppVersionFromPackage failed", err);
  }
}

/** @type {boolean} */
let minimizeToTrayOnClose = true;

async function getInvokeFn() {
  const core = window.__TAURI__?.core;
  if (core?.invoke) return core.invoke.bind(core);
  throw new Error("Tauri API 不可用");
}

async function loadTrayUiSettings() {
  try {
    const inv = await getInvokeFn();
    const s = await inv("get_app_ui_settings");
    minimizeToTrayOnClose = s?.minimizeToTrayOnClose !== false;
  } catch {
    minimizeToTrayOnClose = true;
  }
  const chk = document.getElementById("chkMinimizeToTray");
  if (chk) chk.checked = minimizeToTrayOnClose;
  syncCloseButtonTitle();
}

function syncCloseButtonTitle() {
  const btn = document.getElementById("btnWinClose");
  if (!btn) return;
  btn.title = minimizeToTrayOnClose
    ? "关闭到系统托盘（本地路由保持运行）"
    : "退出应用";
  btn.setAttribute(
    "aria-label",
    minimizeToTrayOnClose ? "关闭到系统托盘" : "退出应用"
  );
}

function bindTraySettingsUi() {
  const chk = document.getElementById("chkMinimizeToTray");
  if (!chk || chk.dataset.bound === "1") return;
  chk.dataset.bound = "1";
  chk.addEventListener("change", async () => {
    const enabled = !!chk.checked;
    chk.disabled = true;
    try {
      const inv = await getInvokeFn();
      const s = await inv("set_minimize_to_tray_on_close_cmd", { enabled });
      minimizeToTrayOnClose = s?.minimizeToTrayOnClose !== false;
      chk.checked = minimizeToTrayOnClose;
      syncCloseButtonTitle();
      showToast(
        minimizeToTrayOnClose
          ? "已开启：关闭窗口将最小化到托盘"
          : "已关闭：关闭窗口将退出应用",
        "ok"
      );
    } catch (err) {
      chk.checked = minimizeToTrayOnClose;
      showToast(err?.message || String(err), "error");
    } finally {
      chk.disabled = false;
    }
  });
}

/** Listen for tray / backend provider switches while the window is open or hidden. */
function bindProviderTrayEvents() {
  const eventApi = window.__TAURI__?.event;
  if (!eventApi?.listen || bindProviderTrayEvents._done) return;
  bindProviderTrayEvents._done = true;

  eventApi
    .listen("provider-switched", (ev) => {
      const p = ev?.payload || {};
      const msg = p.message || (p.providerId ? `已切换供应商：${p.providerId}` : "供应商已切换");
      // Only toast when the main window is likely visible enough for feedback;
      // providers view reloads on next enter either way.
      showToast(msg, "ok");
      try {
        if (activeView === "providers" && window.providersView?.reload) {
          window.providersView.reload();
        }
      } catch {
        /* ignore */
      }
    })
    .catch(() => {});

  eventApi
    .listen("provider-switch-failed", (ev) => {
      const err = ev?.payload?.error || "托盘切换供应商失败";
      showToast(String(err), "error");
    })
    .catch(() => {});
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

/* 关于页：检查更新（tauri-plugin-updater → latest.json，多 endpoint fallback） */
const btnCheckUpdate = document.getElementById("btnCheckUpdate");
const aboutUpdateStatus = document.getElementById("aboutUpdateStatus");
/** @type {any|null} last updater metadata that has a real update (for reopening dialog) */
let lastUpdateResult = null;
/** @type {boolean} prevent double install */
let updateInstallInFlight = false;

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
 * Normalize tauri-plugin-updater check() payload into a stable UI shape.
 * `null` / empty → no update.
 */
function normalizeUpdaterResult(raw) {
  if (!raw || typeof raw !== "object") return null;
  const latest = String(raw.version || raw.latest || "").replace(/^v/i, "");
  const current = String(raw.currentVersion || raw.current || APP_VERSION).replace(
    /^v/i,
    ""
  );
  if (!latest) return null;
  if (compareAppVersions(current, latest) >= 0) return null;
  return {
    rid: raw.rid,
    current,
    latest,
    notes: String(raw.body || raw.notes || raw.releaseNotes || "").trim(),
    date: raw.date || null,
    updateAvailable: true,
    raw,
  };
}

function hasActualUpdate(result) {
  if (!result || typeof result !== "object") return false;
  if (result.rid == null && !result.latest) return false;
  const current = String(result.current || APP_VERSION).replace(/^v/i, "");
  const latest = String(result.latest || "").replace(/^v/i, "");
  if (!latest) return false;
  return compareAppVersions(current, latest) < 0;
}

function clearUpdateStatusInteractive() {
  if (!aboutUpdateStatus) return;
  aboutUpdateStatus.removeAttribute("title");
  aboutUpdateStatus.setAttribute("role", "status");
  aboutUpdateStatus.removeAttribute("tabindex");
  aboutUpdateStatus.classList.remove("is-update");
}

/**
 * 发现新版本弹窗（官方 updater）
 * @returns {Promise<"install"|"later">}
 */
function showUpdateDialog(result = {}) {
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
  const msg = latest ? `发现新版本 ${latest}，可在应用内直接安装。` : "发现新版本";
  const notes = String(result?.notes || result?.releaseNotes || "").trim();
  const canInstall = result?.rid != null;

  if (!modal || !btnLater) {
    return showConfirm({
      title: "发现新版本",
      message: notes ? `${msg}\n\n更新说明：\n${notes}` : msg,
      confirmText: canInstall ? "立即更新" : "知道了",
      cancelText: "稍后",
      variant: "primary",
    }).then((ok) => (ok && canInstall ? "install" : "later"));
  }

  if (typeof showUpdateDialog._dismiss === "function") {
    showUpdateDialog._dismiss("later");
  }

  if (titleEl) titleEl.textContent = "发现新版本";
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
    btnOpen.hidden = !canInstall;
    btnOpen.textContent = "立即更新";
    btnOpen.disabled = false;
  }
  btnLater.textContent = "稍后";
  btnLater.disabled = false;

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
      resolve(action === "install" ? "install" : "later");
    };
    const onLater = () => dismiss("later");
    const onOpen = () => dismiss("install");
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
    (canInstall && btnOpen ? btnOpen : btnLater).focus?.();
  });
}

/**
 * Download + install via tauri-plugin-updater, then relaunch.
 */
async function performAppUpdateInstall(result) {
  if (!result || result.rid == null) {
    throw new Error("没有可安装的更新包");
  }
  if (updateInstallInFlight) return;
  updateInstallInFlight = true;
  const btnOpen = document.getElementById("btnUpdateOpen");
  const btnLater = document.getElementById("btnUpdateLater");
  try {
    if (aboutUpdateStatus) {
      aboutUpdateStatus.className = "about-update-status is-checking";
      aboutUpdateStatus.textContent = "正在下载更新…";
      clearUpdateStatusInteractive();
    }
    if (btnOpen) {
      btnOpen.disabled = true;
      btnOpen.textContent = "下载中…";
    }
    if (btnLater) btnLater.disabled = true;
    showToast("开始下载更新…", "ok");

    await window.skinAPI.installAppUpdate(result.rid, (ev) => {
      // Progress events: Started | Progress { chunkLength, contentLength } | Finished
      try {
        const event = ev?.event || ev;
        if (event === "Started" || event?.Started != null) {
          if (aboutUpdateStatus) aboutUpdateStatus.textContent = "开始下载…";
          return;
        }
        const prog = event?.Progress || (event === "Progress" ? ev?.data : null);
        if (prog && aboutUpdateStatus) {
          const total = Number(prog.contentLength || 0);
          const chunk = Number(prog.chunkLength || 0);
          if (total > 0 && chunk >= 0) {
            // chunkLength is per-chunk; keep generic text if we lack cumulative
            aboutUpdateStatus.textContent = "正在下载更新…";
          } else {
            aboutUpdateStatus.textContent = "正在下载更新…";
          }
        }
        if (event === "Finished" || event?.Finished != null) {
          if (aboutUpdateStatus) aboutUpdateStatus.textContent = "正在安装…";
          if (btnOpen) btnOpen.textContent = "安装中…";
        }
      } catch {
        /* ignore progress parse errors */
      }
    });

    if (aboutUpdateStatus) {
      aboutUpdateStatus.className = "about-update-status is-latest";
      aboutUpdateStatus.textContent = "更新完成，即将重启…";
    }
    showToast("更新完成，正在重启…", "ok");
    await window.skinAPI.relaunchApp();
  } finally {
    updateInstallInFlight = false;
    if (btnOpen) {
      btnOpen.disabled = false;
      btnOpen.textContent = "立即更新";
    }
    if (btnLater) btnLater.disabled = false;
  }
}

/**
 * @param {object|null} result normalized updater result
 * @param {{ silent?: boolean }} [opts]
 */
async function presentUpdateResult(result, opts = {}) {
  const silent = opts.silent === true;
  const hasUpdate = hasActualUpdate(result);

  if (hasUpdate) {
    lastUpdateResult = result;
    if (aboutUpdateStatus) {
      aboutUpdateStatus.className = "about-update-status is-update";
      aboutUpdateStatus.textContent = "发现新版本，点击查看";
      aboutUpdateStatus.title = "点击查看更新详情";
      aboutUpdateStatus.setAttribute("role", "button");
      aboutUpdateStatus.tabIndex = 0;
    }
    if (!silent) {
      const action = await showUpdateDialog(result);
      if (action === "install") {
        await performAppUpdateInstall(result);
      }
    }
    return;
  }

  lastUpdateResult = null;
  if (aboutUpdateStatus) {
    aboutUpdateStatus.className = "about-update-status is-latest";
    aboutUpdateStatus.textContent = "已是最新版本";
    clearUpdateStatusInteractive();
  }
}

async function runCheckUpdate({ reopenOnly = false } = {}) {
  if (reopenOnly) {
    if (lastUpdateResult && hasActualUpdate(lastUpdateResult)) {
      const action = await showUpdateDialog(lastUpdateResult);
      if (action === "install") {
        await performAppUpdateInstall(lastUpdateResult);
      }
      return;
    }
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
    if (typeof window.skinAPI?.checkAppUpdate !== "function") {
      throw new Error("应用更新检查不可用");
    }
    const raw = await window.skinAPI.checkAppUpdate();
    const result = normalizeUpdaterResult(raw);
    const hasUpdate = hasActualUpdate(result);
    await presentUpdateResult(result);
    if (!hasUpdate) {
      showToast("已是最新版本", "ok");
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
    return;
  }
  // Cloud HTML may ship copy buttons: data-copy="556251300"
  const copyEl = t.closest("[data-copy]");
  if (copyEl && copyEl.id !== "btnCopyContact") {
    e.preventDefault();
    const text = String(copyEl.getAttribute("data-copy") || "").trim();
    if (!text) return;
    (async () => {
      try {
        if (navigator.clipboard?.writeText) {
          await navigator.clipboard.writeText(text);
          showToast(`已复制：${text}`, "ok");
        } else {
          showToast("复制失败，请手动复制", "error");
        }
      } catch {
        showToast("复制失败，请手动复制", "error");
      }
    })();
  }
});

syncAboutVersionUi();
loadAppVersionFromPackage();

/**
 * Wire About → GitHub button from src/repo-meta.json
 * (stamped by scripts/stamp-repo-meta.mjs / CI from GITHUB_REPOSITORY).
 */
async function applyRepoMetaUi() {
  const btn = document.getElementById("btnAboutGithub");
  if (!btn) return;
  try {
    const res = await fetch("repo-meta.json", { cache: "no-store" });
    if (!res.ok) return;
    const meta = await res.json();
    const url =
      (meta?.url && String(meta.url).trim()) ||
      (meta?.owner && meta?.name
        ? `https://github.com/${meta.owner}/${meta.name}`
        : "");
    if (!url || !/^https:\/\/github\.com\//i.test(url)) {
      btn.hidden = true;
      btn.removeAttribute("data-external");
      return;
    }
    btn.hidden = false;
    btn.setAttribute("data-external", url);
    btn.title = `在 GitHub 上查看源码（${meta.repository || meta.owner + "/" + meta.name}）`;
  } catch {
    btn.hidden = true;
  }
}
applyRepoMetaUi();

/**
 * Contact + ad are cloud-only (`/v1/about.json`).
 * Never invent QQ / email / ad copy in the client — empty → hide the block.
 */

function sanitizeAboutContactHtml(html) {
  return String(html || "")
    .replace(/<\s*(script|iframe|object|embed|link|meta|base)[\s\S]*?>/gi, "")
    .replace(/<\s*\/\s*(script|iframe|object|embed|link|meta|base)\s*>/gi, "")
    .replace(/\son[a-z]+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, "")
    .replace(/(href|src)\s*=\s*(['"])\s*javascript:[^'"]*\2/gi, '$1="#"');
}

function sanitizeAboutContactCss(css) {
  return String(css || "")
    .replace(/@import\b[^;]*;?/gi, "")
    .replace(/expression\s*\(/gi, "/* blocked */(")
    .replace(/javascript\s*:/gi, "blocked:")
    .replace(/-moz-binding\s*:/gi, "blocked:");
}

function escapeAboutText(s) {
  return String(s || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * Show/hide contact block inside the left panel (GitHub stays in the same box).
 * Also toggles the divider between contact and GitHub.
 */
function setAboutContactSectionVisible(visible) {
  const section = document.getElementById("aboutContactCard");
  const divider = document.getElementById("aboutLeftDivider");
  const panel = document.getElementById("aboutLeftPanel");
  if (section) section.hidden = !visible;
  if (divider) divider.hidden = !visible;
  panel?.classList.toggle("has-contact", !!visible);
}

/** Show/hide the ad slot shell (content only from cloud). */
function setAboutAdSlotVisible(visible) {
  const slot = document.getElementById("aboutAdSlot");
  if (slot) slot.hidden = !visible;
}

/**
 * Clear contact UI only (GitHub row in the same left panel is untouched).
 */
function clearAboutContactUi() {
  const remote = document.getElementById("aboutContactRemote");
  const body = document.getElementById("aboutContactRemoteBody");
  const fallback = document.getElementById("aboutContactFallback");
  const media = document.getElementById("aboutContactMedia");
  const list = document.getElementById("aboutContactList");
  const titleEl = document.getElementById("aboutContactTitle");
  const noteEl = document.getElementById("aboutContactNote");
  const copyBtn = document.getElementById("btnCopyContact");
  const panel = document.getElementById("aboutLeftPanel");

  if (remote) {
    remote.hidden = true;
    remote.querySelector("style[data-about-contact-css]")?.remove();
  }
  if (body) body.innerHTML = "";
  if (fallback) fallback.hidden = true;
  if (media) media.hidden = true;
  if (list) {
    list.hidden = true;
    list.innerHTML = "";
  }
  if (titleEl) {
    titleEl.textContent = "";
    titleEl.hidden = true;
  }
  if (noteEl) {
    noteEl.textContent = "";
    noteEl.hidden = true;
  }
  if (copyBtn) {
    copyBtn.hidden = true;
    delete copyBtn.dataset.copy;
  }
  setAboutContactMedia("");
  panel?.classList.remove("is-remote-contact");
  setAboutContactSectionVisible(false);
}

/**
 * Mount cloud HTML+CSS into the contact host (full layout freedom for ops).
 * Fields fallback is hidden; GitHub remains below the divider in the same box.
 * @param {string} html
 * @param {string} css
 * @returns {boolean}
 */
function mountAboutContactRemote(html, css) {
  const remote = document.getElementById("aboutContactRemote");
  const body = document.getElementById("aboutContactRemoteBody");
  const fallback = document.getElementById("aboutContactFallback");
  const panel = document.getElementById("aboutLeftPanel");
  if (!remote || !body) return false;

  const htmlTrim = String(html || "").trim();
  if (!htmlTrim) {
    clearAboutContactUi();
    return false;
  }

  let styleEl = remote.querySelector("style[data-about-contact-css]");
  if (!styleEl) {
    styleEl = document.createElement("style");
    styleEl.setAttribute("data-about-contact-css", "1");
    remote.insertBefore(styleEl, body);
  }
  // Cloud CSS is injected as-is (sanitized); client does not lock contact layout.
  styleEl.textContent = sanitizeAboutContactCss(css || "");
  body.innerHTML = sanitizeAboutContactHtml(htmlTrim);

  remote.hidden = false;
  if (fallback) fallback.hidden = true;
  panel?.classList.add("is-remote-contact");
  setAboutContactSectionVisible(true);
  return true;
}

/**
 * Set contact media image from a cloud image field (optional).
 * @param {string} [src]
 */
function setAboutContactMedia(src) {
  const media = document.getElementById("aboutContactMedia");
  const img = document.getElementById("aboutContactQrImg");
  const placeholder = document.getElementById("aboutContactQrPlaceholder");
  const url = String(src || "").trim();
  if (img && url) {
    if (media) media.hidden = false;
    img.src = url;
    img.alt = "";
    img.hidden = false;
    if (placeholder) placeholder.hidden = true;
  } else {
    if (img) {
      img.removeAttribute("src");
      img.alt = "";
      img.hidden = true;
    }
    if (placeholder) placeholder.hidden = false;
    if (media) media.hidden = true;
  }
}

/**
 * Render cloud contact in fields mode (framework only; values all from cloud).
 * Prefer compact media+title layout when one primary text + optional image.
 * @param {{ intro?: string, fields?: object[] }} contact
 */
function renderAboutContactFields(contact) {
  const noteEl = document.getElementById("aboutContactNote");
  const titleEl = document.getElementById("aboutContactTitle");
  const list = document.getElementById("aboutContactList");
  const copyBtn = document.getElementById("btnCopyContact");
  const fallback = document.getElementById("aboutContactFallback");
  const remote = document.getElementById("aboutContactRemote");
  const panel = document.getElementById("aboutLeftPanel");

  if (remote) {
    remote.hidden = true;
    remote.querySelector("style[data-about-contact-css]")?.remove();
    const body = document.getElementById("aboutContactRemoteBody");
    if (body) body.innerHTML = "";
  }
  panel?.classList.remove("is-remote-contact");

  const fields = Array.isArray(contact?.fields) ? contact.fields.filter(Boolean) : [];
  const intro = String(contact?.intro || "").trim();

  if (!fields.length && !intro) {
    clearAboutContactUi();
    return;
  }

  if (fallback) fallback.hidden = false;
  setAboutContactSectionVisible(true);

  const imageField = fields.find((f) => String(f.type || "").toLowerCase() === "image" && f.value);
  const textFields = fields.filter((f) => String(f.type || "").toLowerCase() !== "image");
  const primary =
    textFields.find((f) => f.copyable === true || /qq|群/i.test(String(f.label || ""))) ||
    textFields[0] ||
    null;

  // Compact card: optional image + primary line + intro + copy (matches design when cloud sends QQ-style fields)
  const useCompact =
    !!primary &&
    textFields.length <= 2 &&
    !textFields.some((f) => {
      const t = String(f.type || "").toLowerCase();
      return t === "email" || t === "link";
    });

  if (useCompact) {
    if (list) {
      list.hidden = true;
      list.innerHTML = "";
    }
    setAboutContactMedia(imageField?.value || "");

    const label = String(primary.label || "").trim();
    const value = String(primary.value || "").trim();
    const title =
      label && value
        ? /[:：]\s*$/.test(label)
          ? `${label}${value}`
          : `${label}: ${value}`
        : value || label;
    if (titleEl) {
      titleEl.textContent = title;
      titleEl.hidden = !title;
    }
    if (noteEl) {
      if (intro) {
        noteEl.hidden = false;
        noteEl.innerHTML = escapeAboutText(intro).replace(/\n/g, "<br />");
      } else {
        noteEl.hidden = true;
        noteEl.textContent = "";
      }
    }

    const copyValue =
      String(primary.copyValue || "").trim() ||
      value ||
      String(primary.href || "").replace(/^mailto:/i, "");
    if (copyBtn) {
      if (copyValue) {
        copyBtn.hidden = false;
        copyBtn.dataset.copy = copyValue;
        const isQq = /qq|群/i.test(label) || /^\d{5,}$/.test(copyValue);
        copyBtn.textContent = isQq ? "复制群号" : "复制";
      } else {
        copyBtn.hidden = true;
        delete copyBtn.dataset.copy;
      }
    }
    return;
  }

  // Generic multi-field list — still 100% cloud values; layout is a thin framework
  setAboutContactMedia(imageField?.value || "");
  if (copyBtn) {
    copyBtn.hidden = true;
    delete copyBtn.dataset.copy;
  }
  if (titleEl) {
    titleEl.textContent = "";
    titleEl.hidden = true;
  }
  if (noteEl) {
    if (intro) {
      noteEl.hidden = false;
      noteEl.innerHTML = escapeAboutText(intro).replace(/\n/g, "<br />");
    } else {
      noteEl.hidden = true;
      noteEl.textContent = "";
    }
  }
  const listFields = imageField ? textFields : fields;
  if (list) {
    list.hidden = !listFields.length;
    list.innerHTML = listFields
      .map((f) => {
        const label = escapeAboutText(f.label || "");
        const value = escapeAboutText(f.value || "");
        const type = String(f.type || "text").toLowerCase();
        const href = String(f.href || "").trim();

        if (type === "image" && f.value) {
          return `<li class="about-contact-field about-contact-field-image">
            ${label ? `<span class="about-contact-label">${label}：</span>` : ""}
            <img class="about-contact-field-img" src="${escapeAboutText(f.value)}" alt="${label || ""}" loading="lazy" decoding="async" />
          </li>`;
        }

        const icon =
          type === "email"
            ? `<svg viewBox="0 0 24 24"><rect x="3.5" y="5.5" width="17" height="13" rx="2"/><path d="m4.5 7.5 7.5 6 7.5-6"/></svg>`
            : type === "link"
              ? `<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5" fill="none"/><path d="M3.5 12h17M12 3.5c2.4 2.6 3.6 5.4 3.6 8.5s-1.2 5.9-3.6 8.5M12 3.5C9.6 6.1 8.4 8.9 8.4 12s1.2 5.9 3.6 8.5"/></svg>`
              : `<svg viewBox="0 0 24 24"><path d="M5 12h14M12 5v14" fill="none"/></svg>`;

        let actionAttr = "";
        if (type === "email" || href.startsWith("mailto:")) {
          const mail = href.replace(/^mailto:/i, "") || f.value || "";
          if (mail) actionAttr = ` href="#" data-mailto="${escapeAboutText(mail)}"`;
        } else if (href && /^https?:\/\//i.test(href)) {
          actionAttr = ` href="#" data-external="${escapeAboutText(href)}"`;
        } else if (type === "link" && f.value && /^https?:\/\//i.test(f.value)) {
          actionAttr = ` href="#" data-external="${escapeAboutText(f.value)}"`;
        }

        const display = value || escapeAboutText(href) || label;
        const content = actionAttr
          ? `<a class="about-contact-link"${actionAttr}>${display}</a>`
          : `<span class="about-contact-text">${display}</span>`;

        return `<li class="about-contact-field">
          <span class="about-contact-ico" aria-hidden="true">${icon}</span>
          ${label ? `<span class="about-contact-label">${label}：</span>` : ""}
          ${content}
        </li>`;
      })
      .join("");
  }
}

/**
 * Apply cloud about.contact — mode html | fields.
 * html: inject cloud HTML+CSS into host (not client-hardcoded layout).
 * fields: thin framework render of cloud field values.
 * @param {object|null|undefined} contact
 */
function applyAboutContact(contact) {
  const c = contact && typeof contact === "object" ? contact : null;
  if (!c) {
    clearAboutContactUi();
    return;
  }

  let mode = String(c.mode || "").toLowerCase();
  const html = String(c.html || "").trim();
  if (mode !== "html" && mode !== "fields") {
    mode = html ? "html" : "fields";
  }

  // HTML mode first: full structure/style from cloud
  if (mode === "html") {
    if (html) mountAboutContactRemote(html, c.css || "");
    else clearAboutContactUi();
    return;
  }

  let fields = Array.isArray(c.fields) ? c.fields.slice() : [];
  // Legacy cloud keys → fields (still cloud data, not local invent)
  if (!fields.length && (c.email || c.website || c.imageUrl || c.qq || c.qqGroup)) {
    if (c.qq || c.qqGroup) {
      fields.push({
        id: "legacy_qq",
        label: "QQ群",
        value: c.qq || c.qqGroup,
        type: "text",
        href: "",
        copyable: true,
      });
    }
    if (c.email) {
      fields.push({
        id: "legacy_email",
        label: "邮箱",
        value: c.email,
        type: "email",
        href: `mailto:${c.email}`,
      });
    }
    if (c.website) {
      fields.push({
        id: "legacy_website",
        label: "网站",
        value: c.websiteLabel || c.website,
        type: "link",
        href: c.website,
      });
    }
    if (c.imageUrl) {
      fields.push({
        id: "legacy_image",
        label: c.imageAlt || "图片",
        value: c.imageUrl,
        type: "image",
        href: "",
      });
    }
  }

  renderAboutContactFields({
    intro: c.intro || c.note || "",
    fields,
  });
}

/**
 * Clear ad slot UI (no cloud ad / disabled / empty).
 */
function clearAboutAdUi() {
  const slot = document.getElementById("aboutAdSlot");
  const placeholder = document.getElementById("aboutAdPlaceholder");
  const imageLink = document.getElementById("aboutAdImageLink");
  const imageEl = document.getElementById("aboutAdImage");
  const remote = document.getElementById("aboutAdRemote");
  const remoteBody = document.getElementById("aboutAdRemoteBody");
  const titleEl = document.getElementById("aboutAdTitle");
  const subEl = document.getElementById("aboutAdSub");

  if (slot) {
    slot.querySelectorAll("style[data-about-ad-css]").forEach((el) => el.remove());
    slot.classList.remove("is-html-mode");
  }
  document.getElementById("aboutAdInjectedCss")?.remove();
  if (placeholder) placeholder.hidden = true;
  if (imageLink) {
    imageLink.hidden = true;
    imageLink.removeAttribute("data-external");
    imageLink.removeAttribute("href");
    imageLink.classList.add("is-static");
  }
  if (imageEl) {
    imageEl.removeAttribute("src");
    imageEl.alt = "";
  }
  if (remote) {
    remote.hidden = true;
    remote.classList.remove("is-html-mode");
    remote.querySelector("style[data-about-ad-css]")?.remove();
  }
  if (remoteBody) remoteBody.innerHTML = "";
  if (titleEl) titleEl.textContent = "";
  if (subEl) subEl.textContent = "";
  lastRenderedAdJson = null;
  setAboutAdSlotVisible(false);
}

let lastRenderedAdJson = null;
let lastRenderedAnnouncementsJson = null;
let lastAboutSyncTime = 0;
let lastCloudSoftSyncTime = 0;
const ABOUT_TAB_SYNC_COOLDOWN_MS = 5 * 60 * 1000; // 5 minutes
const FOCUS_SYNC_COOLDOWN_MS = 15 * 60 * 1000; // 15 minutes
const CLOUD_HEARTBEAT_INTERVAL_MS = 20 * 60 * 1000; // 20 minutes

/**
 * Apply cloud about.ad. Modes: placeholder | image | html.
 * Empty / missing / disabled → hide slot (never invent local ad copy).
 * @param {object|null|undefined} ad
 * @param {{ force?: boolean }} [opts]
 */
function applyAboutAd(ad, opts = {}) {
  const conf = ad && typeof ad === "object" ? ad : null;
  const stateKey = JSON.stringify(conf || null);
  if (!opts.force && stateKey === lastRenderedAdJson) {
    return; // Content unchanged, avoid unnecessary DOM reflow / image flicker
  }
  lastRenderedAdJson = stateKey;

  const slot = document.getElementById("aboutAdSlot");
  const placeholder = document.getElementById("aboutAdPlaceholder");
  const imageLink = document.getElementById("aboutAdImageLink");
  const imageEl = document.getElementById("aboutAdImage");
  const remote = document.getElementById("aboutAdRemote");
  const remoteBody = document.getElementById("aboutAdRemoteBody");
  const titleEl = document.getElementById("aboutAdTitle");
  const subEl = document.getElementById("aboutAdSub");

  if (!conf || conf.enabled === false) {
    clearAboutAdUi();
    return;
  }

  let mode = String(conf.mode || "").toLowerCase();
  const html = String(conf.html || "").trim();
  const imageUrl = String(conf.imageUrl || conf.image || "").trim();
  const title = String(conf.title || "").trim();
  const subtitle = String(conf.subtitle || conf.body || "").trim();
  if (mode !== "html" && mode !== "image" && mode !== "placeholder") {
    mode = html ? "html" : imageUrl ? "image" : "placeholder";
  }

  // Ensure cloud CSS for ad slot is injected and updated
  let styleEl = document.getElementById("aboutAdInjectedCss");
  const cssText = sanitizeAboutContactCss(conf.css || "");
  if (cssText) {
    if (!styleEl) {
      styleEl = document.createElement("style");
      styleEl.id = "aboutAdInjectedCss";
      styleEl.setAttribute("data-about-ad-css", "1");
      document.head.appendChild(styleEl);
    }
    styleEl.textContent = cssText;
  } else if (styleEl) {
    styleEl.remove();
  }

  const hideLayers = () => {
    if (placeholder) placeholder.hidden = true;
    if (imageLink) imageLink.hidden = true;
    if (remote) {
      remote.hidden = true;
      if (remoteBody) remoteBody.innerHTML = "";
    }
  };

  if (mode === "html" && html && remote && remoteBody) {
    hideLayers();
    remote.classList.add("is-html-mode");
    if (slot) slot.classList.add("is-html-mode");
    remoteBody.innerHTML = sanitizeAboutContactHtml(html);
    remote.hidden = false;
    setAboutAdSlotVisible(true);
    return;
  }

  if (remote) remote.classList.remove("is-html-mode");
  if (slot) slot.classList.remove("is-html-mode");

  if (mode === "image" && imageUrl && imageLink && imageEl) {
    hideLayers();
    imageEl.src = imageUrl;
    imageEl.alt = title || String(conf.alt || "").trim() || "广告";
    const href = String(conf.href || conf.link || "").trim();
    if (href && /^https?:\/\//i.test(href)) {
      imageLink.href = "#";
      imageLink.setAttribute("data-external", href);
      imageLink.classList.remove("is-static");
      imageLink.removeAttribute("aria-disabled");
    } else {
      imageLink.removeAttribute("data-external");
      imageLink.removeAttribute("href");
      imageLink.classList.add("is-static");
      imageLink.setAttribute("aria-disabled", "true");
    }
    imageLink.hidden = false;
    setAboutAdSlotVisible(true);
    return;
  }

  // placeholder: only show when cloud actually provided title and/or subtitle
  if (mode === "placeholder" && (title || subtitle) && placeholder) {
    hideLayers();
    if (titleEl) titleEl.textContent = title;
    if (subEl) subEl.textContent = subtitle;
    placeholder.hidden = false;
    setAboutAdSlotVisible(true);
    return;
  }

  clearAboutAdUi();
}

/**
 * Load about/contact/ad from cloud (disk cache or network). No local content invent.
 * @param {{ refresh?: boolean }} [opts]
 */
async function loadAboutContactFromCloud(opts = {}) {
  if (typeof window.skinAPI?.cloudAbout !== "function") {
    clearAboutContactUi();
    clearAboutAdUi();
    return;
  }
  try {
    const res = await window.skinAPI.cloudAbout({ refresh: opts.refresh === true });
    lastAboutSyncTime = Date.now();
    if (res?.ok === false && !res?.contact && !res?.ad) {
      // Temporary network error / offline without new data: keep existing cached UI
      return;
    }
    if (res?.contact && typeof res.contact === "object") {
      applyAboutContact(res.contact);
    } else if (res?.contact === null) {
      clearAboutContactUi();
    }
    if (res && Object.prototype.hasOwnProperty.call(res, "ad")) {
      if (res.ad) {
        applyAboutAd(res.ad);
      } else if (res.ok !== false) {
        clearAboutAdUi();
      }
    }
  } catch {
    /* offline without cache: leave empty (already cleared on first paint) */
  }
}

/** Copy cloud-provided contact value (e.g. QQ group number). */
document.getElementById("btnCopyContact")?.addEventListener("click", async () => {
  const btn = document.getElementById("btnCopyContact");
  const text = String(btn?.dataset?.copy || "").trim();
  if (!text) return;
  try {
    let ok = false;
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      ok = true;
    } else {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.setAttribute("readonly", "");
      ta.style.position = "fixed";
      ta.style.left = "-9999px";
      document.body.appendChild(ta);
      ta.select();
      ok = document.execCommand("copy");
      ta.remove();
    }
    if (ok) showToast(`已复制：${text}`, "ok");
    else showToast("复制失败，请手动复制", "error");
  } catch {
    showToast("复制失败，请手动复制", "error");
  }
});

// Empty until cloud/disk about arrives — never paint invented contact/ad copy
clearAboutContactUi();
clearAboutAdUi();
loadAboutContactFromCloud({ refresh: false });

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

function applyAnnouncementsToBanner(payload, opts = {}) {
  const items = Array.isArray(payload?.items) ? payload.items : [];
  const stateKey = JSON.stringify(items);
  if (!opts.force && stateKey === lastRenderedAnnouncementsJson) {
    return; // Content unchanged, keep current promo carousel running smoothly
  }
  lastRenderedAnnouncementsJson = stateKey;
  latestAnnouncements = payload || null;
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
      if (snap?.about?.contact || snap?.about?.ad) {
        if (snap.about.contact) applyAboutContact(snap.about.contact);
        if (Object.prototype.hasOwnProperty.call(snap.about, "ad")) applyAboutAd(snap.about.ad);
      } else {
        await loadAboutContactFromCloud({ refresh: true });
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
    await loadAboutContactFromCloud({ refresh: false });
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
      if (snap?.about?.contact) applyAboutContact(snap.about.contact);
      if (snap?.about?.ad) {
        applyAboutAd(snap.about.ad);
      }
    }
  } catch {
    paintPromo(0);
  }

  // Deferred soft network: skin update flags + announcements + about after GUI is ready
  if (typeof window.skinAPI?.cloudRefresh !== "function") return;
  window.setTimeout(() => {
    if (document.visibilityState === "hidden") return;
    // About is independent of catalog soft-TTL: always try network once on boot
    // so CODEX_SKIN_CLOUD_URL / new CDN contact is not stuck on stale disk cache.
    loadAboutContactFromCloud({ refresh: true });
    window.skinAPI
      .cloudRefresh({ force: false })
      .then((res) => {
        const snap = res?.snapshot || res;
        if (snap?.announcements) applyAnnouncementsToBanner(snap.announcements);
        if (snap?.about?.contact) {
          applyAboutContact(snap.about.contact);
        }
        if (snap?.about?.ad) {
          applyAboutAd(snap.about.ad);
        }
        // Soft sync may be skipped (cache-fresh) — only rebuild list when catalog may change
        const skipped = res?.sync?.skipped === true || snap?.sync?.skipped === true;
        startCloudHeartbeat();
        if (!skipped) return refresh();
        return null;
      })
      .catch(() => {
        startCloudHeartbeat();
        /* offline: keep disk cache */
      });
  }, CLOUD_BOOT_DELAY_MS);
}

let isSyncingBackground = false;
/**
 * Gentle background sync for ads, announcements, and catalog.
 * Uses HTTP ETag / 304 and soft TTL to keep zero unnecessary network/server load.
 */
async function syncCloudSoftBackground() {
  if (isSyncingBackground) return;
  if (typeof window.skinAPI?.cloudRefresh !== "function") return;
  isSyncingBackground = true;
  lastCloudSoftSyncTime = Date.now();
  try {
    // 1. Check about / ad (ETag protected, 0-byte 304 if unchanged)
    await loadAboutContactFromCloud({ refresh: true });
    // 2. Soft sync for catalog & announcements
    const res = await window.skinAPI.cloudRefresh({ force: false });
    const snap = res?.snapshot || res;
    if (snap?.announcements) {
      applyAnnouncementsToBanner(snap.announcements);
    }
    if (snap?.about?.contact) {
      applyAboutContact(snap.about.contact);
    }
    if (snap?.about?.ad) {
      applyAboutAd(snap.about.ad);
    }
  } catch {
    /* offline / network error: keep current cache state silently */
  } finally {
    isSyncingBackground = false;
  }
}

let cloudHeartbeatTimer = null;
function startCloudHeartbeat() {
  if (cloudHeartbeatTimer) clearInterval(cloudHeartbeatTimer);
  // 20 min interval + small random jitter (±1 min) to disperse server requests across clients
  const jitter = Math.floor((Math.random() - 0.5) * 120000);
  const interval = Math.max(10 * 60 * 1000, CLOUD_HEARTBEAT_INTERVAL_MS + jitter);
  cloudHeartbeatTimer = setInterval(() => {
    if (document.visibilityState === "hidden") return;
    syncCloudSoftBackground();
  }, interval);
}

// Window focus listener for gentle soft wake-up refresh
window.addEventListener("focus", () => {
  const now = Date.now();
  if (now - lastCloudSoftSyncTime > FOCUS_SYNC_COOLDOWN_MS) {
    syncCloudSoftBackground();
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

/**
 * Shared post-start/restart toast + status refresh.
 * @param {any} result
 * @param {"start"|"restart"} action
 */
async function finishHostLaunchResult(result, action) {
  const lastId = latestStatus?.state?.skinId || result?.skinId;
  const lastName =
    (lastId && (latestStatus?.skins || []).find((s) => s.id === lastId)?.name) ||
    lastId;
  const verb = action === "restart" ? "重启" : "启动";
  if (result?.ok === false) {
    showToast(result?.error || `${verb}失败`, "error");
  } else if (result?.mode === "apply-last-skin" || result?.skinId) {
    const name =
      result?.name ||
      (latestStatus?.skins || []).find((s) => s.id === result.skinId)?.name ||
      result.skinId ||
      lastName ||
      "上次皮肤";
    showToast(
      result?.artPending
        ? `已${verb}并换上「${name}」（立绘加载中）`
        : `已${verb}并换上「${name}」`,
      "ok"
    );
  } else {
    showToast(
      action === "restart"
        ? "已重启 ChatGPT（可直接换肤）"
        : "已启动 ChatGPT（可直接换肤）",
      "ok"
    );
  }
  if (result?.lifecycle || result?.canHotApply !== undefined) {
    updateHostPill(result);
  }
  await refresh();
  await pollHostStatus(true);
}

document.getElementById("btnHost")?.addEventListener("click", async () => {
  const mode = hostButtonMode(latestStatus);
  const lastId = latestStatus?.state?.skinId;
  const lastName =
    (lastId && (latestStatus?.skins || []).find((s) => s.id === lastId)?.name) ||
    lastId;
  const isRestart = mode === "restart";
  setBusy(
    true,
    isRestart
      ? lastId
        ? `正在重启 ChatGPT 并应用「${lastName}」…`
        : "正在重启 ChatGPT…"
      : lastId
        ? `正在启动 ChatGPT 并应用「${lastName}」…`
        : "正在启动 ChatGPT…"
  );
  try {
    if (isRestart) {
      if (typeof window.skinAPI?.restartHost !== "function") {
        throw new Error("当前版本不支持重启客户端，请更新后重试");
      }
      const result = await window.skinAPI.restartHost();
      await finishHostLaunchResult(result, "restart");
    } else {
      if (typeof window.skinAPI?.startHost !== "function") {
        throw new Error("当前版本不支持启动客户端，请更新后重试");
      }
      const result = await window.skinAPI.startHost();
      await finishHostLaunchResult(result, "start");
    }
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
});

document.getElementById("btnSkinPause")?.addEventListener("click", async () => {
  const mode = skinControlMode(latestStatus);
  if (mode === "hidden") return;

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

function refreshWallpaperSelects() {
  if (!window.UiSelect?.refresh) return;
  ["wallpaperBase", "themeFont", "themeRadius", "wallpaperFit", "wallpaperPosition"].forEach((id) => {
    const el = document.getElementById(id);
    if (el) window.UiSelect.refresh(el);
  });
}

function fillWallpaperBase(skins, activeId) {
  const options = (skins || []).map((skin) => ({
    value: skin.id,
    label: `${skin.name}${skin.builtin ? "（内置）" : ""}`,
  }));
  const selected =
    (activeId && options.some((o) => o.value === activeId) && activeId) ||
    options[0]?.value ||
    "";
  if (window.UiSelect?.setOptions) {
    window.UiSelect.setOptions(wallpaperBase, options, selected);
    return;
  }
  wallpaperBase.innerHTML = options
    .map((o) => `<option value="${escapeHtml(o.value)}">${escapeHtml(o.label)}</option>`)
    .join("");
  if (selected) wallpaperBase.value = selected;
}

function prefillWallpaperTheme(skins) {
  const selected = (skins || []).find((s) => s.id === wallpaperBase.value) || skins?.[0];
  if (selected?.accent && /^#[0-9a-fA-F]{6}$/.test(selected.accent)) {
    document.getElementById("themeAccent").value = selected.accent;
  }
}

async function openWallpaper() {
  wallpaperModal.hidden = false;
  wallpaperModal.classList.add("show");
  if (window.UiSelect?.mountAll) window.UiSelect.mountAll(wallpaperModal);
  const status = latestStatus || (await window.skinAPI.status());
  const skins = status.skins || [];
  // Prefer currently applied skin as template when available
  fillWallpaperBase(skins, status.activeSkinId || null);
  prefillWallpaperTheme(skins);
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
    refreshWallpaperSelects();
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
    // CloseRequested is handled in Rust: hide-to-tray (default) or real exit.
    appWindow.close().catch(() => {});
  });

  // Keep close-button tooltip aligned with tray setting.
  syncCloseButtonTitle();

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
  // ── First paint (sync-ish): shell + overview scaffold before any heavy invoke ──
  // Do not await force host probe / full status here — those were the multi-second
  // white-screen stall (CDP timeouts when ChatGPT is offline + base64 previews).
  try {
    ensureOverviewScaffold();
    setMainView("overview");
    paintPromo(0);
    if (pillCodex) {
      pillCodex.textContent = "连接中…";
      pillCodex.className = "pill";
    }
  } catch {
    /* ignore */
  }

  // Window chrome + tray prefs: non-critical for first content paint
  try {
    await setupWindowControls();
  } catch {
    /* ignore */
  }
  try {
    bindTraySettingsUi();
    bindProviderTrayEvents();
    // Fire-and-forget: do not block overview on settings round-trip
    void loadTrayUiSettings().catch(() => {});
  } catch {
    /* ignore */
  }

  // Sidebar categories (local JSON) — cheap; still don't block host/status
  void loadSkinCategories().catch(() => {
    try {
      renderCategoryNav();
    } catch {
      /* ignore */
    }
  });

  // ── Background data (parallel, non-blocking for shell) ──
  // Soft host poll first (uses TTL cache, short CDP timeouts). Avoid force on boot.
  void pollHostStatus(false)
    .catch(() => null)
    .finally(() => {
      scheduleHostPoll();
    });

  void refresh()
    .catch((err) => {
      if (pillCodex && !latestHost) {
        pillCodex.textContent = "引擎未就绪";
        pillCodex.className = "pill warn";
      }
      if (pillActive) pillActive.textContent = "当前皮肤：—";
      if (grid && !latestStatus?.skins?.length) {
        grid.innerHTML = `<article class="card"><div class="meta"><h2>无法加载皮肤列表</h2><p>${escapeHtml(friendlyError(err))}</p><p class="muted">请确认用 <code>npm run dev</code> 或安装包启动本应用。</p></div></article>`;
      }
      showToast(friendlyError(err), "error");
    });

  // Cloud: announcements + catalog merge (already delayed internally)
  try {
    bootCloud();
  } catch {
    /* ignore */
  }
})();
