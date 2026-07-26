/**
 * Sessions management main view (list / multi-delete / export / undo / repair).
 * Supports two sources via Tab: Codex (ChatGPT desktop SQLite) and Grok Build.
 * Depends on: window.sessionAPI, showToast, showConfirm (from app.js).
 */
(function () {
  const PAGE_SIZE = 50;
  const SOURCE_CODEX = "codex";
  const SOURCE_GROK = "grok";
  const SOURCE_STORAGE_KEY = "chatgpt-tools.sessions.source";
  const GROUP_STORAGE_KEY = "chatgpt-tools.sessions.groupByProject";
  const UNKNOWN_PROJECT_KEY = "__unknown_project__";

  /** @type {"codex"|"grok"} */
  let source = readStoredSource();

  let offset = 0;
  let limit = PAGE_SIZE;
  let hasMore = false;
  let sessions = [];
  let selectionMode = false;
  let selectedIds = new Set();
  let loading = false;
  /** Monotonic id so overlapping list requests discard stale responses. */
  let loadSeq = 0;
  let bulkDeleting = false;
  let busyRepair = false;
  let lastPayload = null;
  let bound = false;
  /** Client-side filters (applied to the current page). */
  let searchQuery = "";
  /** @type {string} empty = all projects */
  let projectFilter = "";
  let groupByProject = readGroupByProject();
  /**
   * Expanded project group keys (only when groupByProject).
   * Default: none expanded — groups start collapsed until the user opens them.
   */
  let expandedProjects = new Set();
  /** @type {{ token: string, title: string, dbPath?: string } | null} */
  let lastUndo = null;
  /** @type {{ targets: any[], currentProvider: string } | null} */
  let providerTargets = null;
  /** @type {{ snapshotSha256: string, candidates: any[] } | null} */
  let indexPreview = null;

  function $(id) {
    return document.getElementById(id);
  }

  function readStoredSource() {
    try {
      const v = localStorage.getItem(SOURCE_STORAGE_KEY);
      if (v === SOURCE_GROK || v === SOURCE_CODEX) return v;
    } catch {
      /* ignore */
    }
    return SOURCE_CODEX;
  }

  function persistSource() {
    try {
      localStorage.setItem(SOURCE_STORAGE_KEY, source);
    } catch {
      /* ignore */
    }
  }

  function readGroupByProject() {
    try {
      const v = localStorage.getItem(GROUP_STORAGE_KEY);
      if (v === "0" || v === "false") return false;
      if (v === "1" || v === "true") return true;
    } catch {
      /* ignore */
    }
    return true;
  }

  function persistGroupByProject() {
    try {
      localStorage.setItem(GROUP_STORAGE_KEY, groupByProject ? "1" : "0");
    } catch {
      /* ignore */
    }
  }

  function isGrok() {
    return source === SOURCE_GROK;
  }

  /**
   * Strip Windows `\\?\` / `//?/` extended path prefixes for display & grouping.
   * Backend also normalizes; keep a client fallback for older payloads / mixed sources.
   */
  function normalizeDisplayPath(path) {
    const raw = String(path ?? "").trim();
    if (!raw) return "";
    if (/^\\\\\?\\UNC\\/i.test(raw)) return `\\\\${raw.slice(8)}`;
    if (/^\/\/\?\/UNC\//i.test(raw)) return `\\\\${raw.slice(8).replace(/\//g, "\\")}`;
    if (/^\\\\\?\\/i.test(raw)) return raw.slice(4);
    if (/^\/\/\?\//i.test(raw)) return raw.slice(4).replace(/\//g, "\\");
    return raw;
  }

  function sessionCwd(session) {
    return normalizeDisplayPath(session?.cwd || "");
  }

  function pathBaseName(path) {
    const normalized = normalizeDisplayPath(path).replace(/[\\/]+$/, "");
    if (!normalized) return "";
    const parts = normalized.split(/[\\/]/).filter(Boolean);
    return parts[parts.length - 1] || normalized;
  }

  function projectKeyOf(session) {
    const cwd = sessionCwd(session);
    return cwd || UNKNOWN_PROJECT_KEY;
  }

  function projectLabelOf(key, cwd) {
    if (!key || key === UNKNOWN_PROJECT_KEY) return "未记录项目";
    return pathBaseName(cwd || key) || key;
  }

  /**
   * CLI resume command (aligned with cc-switch session manager).
   * Codex: `codex resume <id>` · Grok Build: `grok --resume <id>`
   */
  function resumeCommandOf(session) {
    const id = String(session?.id || "").trim();
    if (!id) return "";
    return isGrok() ? `grok --resume ${id}` : `codex resume ${id}`;
  }

  async function copyText(text) {
    const value = String(text ?? "");
    if (!value) throw new Error("没有可复制的内容");
    if (navigator.clipboard?.writeText) {
      try {
        await navigator.clipboard.writeText(value);
        return;
      } catch {
        /* fall through to execCommand */
      }
    }
    const ta = document.createElement("textarea");
    ta.value = value;
    ta.setAttribute("readonly", "");
    ta.style.cssText = "position:fixed;left:-9999px;top:0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    if (!ok) throw new Error("复制失败");
  }

  async function copyResumeCommand(session) {
    const cmd = resumeCommandOf(session);
    if (!cmd) {
      toast("会话 ID 无效，无法生成 resume 命令", "error");
      return;
    }
    try {
      await copyText(cmd);
      toast(`已复制：${cmd}`, "ok");
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  function toast(msg, type) {
    if (typeof window.showToast === "function") window.showToast(msg, type);
    else console.log(type || "info", msg);
  }

  async function confirm(opts) {
    if (typeof window.showConfirm === "function") return window.showConfirm(opts);
    return window.confirm(opts?.message || opts?.title || "确认？");
  }

  function formatTime(ms) {
    if (!ms) return "—";
    try {
      const d = new Date(Number(ms));
      if (Number.isNaN(d.getTime())) return "—";
      const pad = (n) => String(n).padStart(2, "0");
      return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
    } catch {
      return "—";
    }
  }

  function escapeHtml(s) {
    return String(s ?? "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function currentPage() {
    return Math.floor(offset / limit) + 1;
  }

  function anyBusy() {
    return loading || bulkDeleting || busyRepair;
  }

  function applySourceChrome() {
    const grok = isGrok();

    document.querySelectorAll("[data-sess-source]").forEach((btn) => {
      const active = btn.getAttribute("data-sess-source") === source;
      btn.classList.toggle("is-active", active);
      btn.setAttribute("aria-selected", active ? "true" : "false");
    });

    const lead = $("sessLead");
    if (lead) {
      lead.textContent = grok
        ? "读取 Grok Build 本机历史会话（~/.grok/sessions）。支持导出 Markdown 与删除（删除不可撤销）。"
        : "读取 ChatGPT / Codex 本地 SQLite 会话库。支持导出 Markdown、删除（可撤销）、历史 provider 修复与 session_index 清理。";
    }

    const pathLabel = $("sessMetricPathLabel");
    if (pathLabel) pathLabel.textContent = grok ? "Grok 目录" : "数据库";

    const tools = $("sessCodexTools");
    if (tools) tools.hidden = grok;

    const undoBar = $("sessUndoBar");
    if (undoBar && grok) undoBar.hidden = true;

    const hint = $("sessHint");
    if (hint) {
      hint.innerHTML = grok
        ? "提示：Grok 会话数据位于 <code>~/.grok/sessions</code>。删除会永久移除会话目录，无撤销备份。导出不会修改本地数据。"
        : "提示：Codex 删除备份在应用状态目录 <code>session-backups</code>。索引清理前请完全退出客户端。导出不会修改本地数据。";
    }

    updateMetrics();
  }

  function filteredSessions() {
    const q = searchQuery.trim().toLowerCase();
    return sessions.filter((s) => {
      const cwd = sessionCwd(s);
      const key = projectKeyOf(s);
      if (projectFilter && key !== projectFilter) return false;
      if (!q) return true;
      const hay = [
        s.title || "",
        s.id || "",
        cwd,
        pathBaseName(cwd),
        s.modelProvider || "",
      ]
        .join("\n")
        .toLowerCase();
      return hay.includes(q);
    });
  }

  function fillProjectFilter() {
    const sel = $("sessProjectFilter");
    if (!sel) return;
    const prev = projectFilter;
    const map = new Map();
    for (const s of sessions) {
      const key = projectKeyOf(s);
      if (map.has(key)) {
        map.get(key).count += 1;
      } else {
        map.set(key, {
          key,
          cwd: sessionCwd(s),
          count: 1,
        });
      }
    }
    const options = [...map.values()].sort((a, b) => {
      if (a.key === UNKNOWN_PROJECT_KEY) return 1;
      if (b.key === UNKNOWN_PROJECT_KEY) return -1;
      const la = projectLabelOf(a.key, a.cwd);
      const lb = projectLabelOf(b.key, b.cwd);
      return la.localeCompare(lb, "zh-CN") || a.key.localeCompare(b.key);
    });
    const parts = [`<option value="">全部项目（${sessions.length}）</option>`];
    for (const opt of options) {
      const label = projectLabelOf(opt.key, opt.cwd);
      const title = opt.cwd || label;
      parts.push(
        `<option value="${escapeHtml(opt.key)}" title="${escapeHtml(title)}">${escapeHtml(
          label
        )}（${opt.count}）</option>`
      );
    }
    sel.innerHTML = parts.join("");
    if (prev && map.has(prev)) sel.value = prev;
    else {
      projectFilter = "";
      sel.value = "";
    }
  }

  function syncFilterControls() {
    const search = $("sessSearch");
    if (search && search.value !== searchQuery) search.value = searchQuery;
    const group = $("sessGroupByProject");
    if (group) group.checked = groupByProject;
    fillProjectFilter();
  }

  function updateMetrics() {
    const pageItems = sessions;
    const items = filteredSessions();
    const active = pageItems.filter((s) => !s.archived).length;
    const archived = pageItems.length - active;
    const elCount = $("sessMetricCount");
    const elActive = $("sessMetricActive");
    const elArchived = $("sessMetricArchived");
    const elDb = $("sessMetricDb");
    if (elCount) elCount.textContent = `${pageItems.length} 个`;
    if (elActive) elActive.textContent = `${active} 个`;
    if (elArchived) elArchived.textContent = `${archived} 个`;
    if (elDb) {
      let path = "未检测到";
      if (isGrok()) {
        path =
          lastPayload?.grokHome ||
          (Array.isArray(lastPayload?.sessionRoots) && lastPayload.sessionRoots[0]) ||
          "未检测到";
      } else {
        path = lastPayload?.dbPath || lastPayload?.codexHome || "未检测到";
      }
      path = normalizeDisplayPath(path) || path;
      elDb.textContent = path;
      elDb.title = path;
    }
    const pageLabel = $("sessPageLabel");
    if (pageLabel) pageLabel.textContent = `第 ${currentPage()} 页`;
    const prev = $("btnSessPrev");
    const next = $("btnSessNext");
    if (prev) prev.disabled = offset <= 0 || anyBusy();
    if (next) next.disabled = !hasMore || anyBusy();
    const sum = $("sessSelectionSummary");
    if (sum) {
      const filteredNote =
        items.length !== pageItems.length ? ` · 筛选后 ${items.length}` : "";
      sum.textContent = selectionMode
        ? `已选 ${selectedIds.size} / 可见 ${items.length}${
            pageItems.length !== items.length ? `（本页 ${pageItems.length}）` : ""
          }`
        : `本页 ${pageItems.length} 条${filteredNote}`;
    }
    const refreshBtn = $("btnSessRefresh");
    if (refreshBtn) refreshBtn.disabled = anyBusy();
    const undoBar = $("sessUndoBar");
    if (undoBar) {
      // Undo only applies to Codex deletes in the current UI session.
      undoBar.hidden = isGrok() || !lastUndo;
      const undoLabel = $("sessUndoLabel");
      if (undoLabel && lastUndo && !isGrok()) {
        undoLabel.textContent = `已删除「${lastUndo.title}」，可撤销`;
      }
    }
    const repairBtn = $("btnSessRepair");
    if (repairBtn) repairBtn.disabled = busyRepair || isGrok();
    const previewBtn = $("btnSessIndexPreview");
    if (previewBtn) previewBtn.disabled = busyRepair || isGrok();
    const applyBtn = $("btnSessIndexApply");
    if (applyBtn) {
      applyBtn.disabled =
        isGrok() ||
        busyRepair ||
        !indexPreview ||
        !(indexPreview.candidates || []).length;
    }
  }

  function fillProviderSelect() {
    const sel = $("sessProviderTarget");
    if (!sel) return;
    const prev = sel.value;
    const targets = providerTargets?.targets || [];
    const current = providerTargets?.currentProvider || "";
    /** @type {Array<{value:string,label:string}>} */
    let options;
    let selected = "";
    if (!targets.length) {
      options = [
        {
          value: current || "",
          label: current || "当前配置 provider",
        },
      ];
      selected = current || "";
    } else {
      options = targets.map((t) => ({
        value: t.id,
        label: t.isCurrentProvider ? `${t.id}（当前）` : t.id,
      }));
      if (prev && targets.some((t) => t.id === prev)) selected = prev;
      else if (current) selected = current;
      else selected = options[0]?.value || "";
    }
    if (window.UiSelect?.setOptions) {
      window.UiSelect.setOptions(sel, options, selected);
    } else {
      sel.innerHTML = options
        .map(
          (o) =>
            `<option value="${escapeHtml(o.value)}">${escapeHtml(o.label)}</option>`
        )
        .join("");
      sel.value = selected;
    }
  }

  function renderIndexPreview() {
    const box = $("sessIndexPreview");
    if (!box) return;
    if (!indexPreview || isGrok()) {
      box.hidden = true;
      box.innerHTML = "";
      return;
    }
    const list = indexPreview.candidates || [];
    box.hidden = false;
    if (!list.length) {
      box.innerHTML =
        '<div class="sess-index-empty">未发现仅存在于 session_index.jsonl 的孤儿候选。</div>';
      updateMetrics();
      return;
    }
    box.innerHTML = `
      <div class="sess-index-head">
        <strong>索引清理候选（${list.length}）</strong>
        <span class="sess-index-sha" title="snapshot">sha ${escapeHtml(
          (indexPreview.snapshotSha256 || "").slice(0, 12)
        )}…</span>
      </div>
      <div class="sess-index-list">
        ${list
          .map(
            (c) => `<label class="sess-index-row">
              <input type="checkbox" data-index-id="${escapeHtml(c.id)}" checked />
              <span class="sess-index-title">${escapeHtml(c.threadName || c.id)}</span>
              <span class="sess-index-meta">${escapeHtml(c.updatedAt || "")}</span>
            </label>`
          )
          .join("")}
      </div>`;
    updateMetrics();
  }

  /**
   * Empty / loading copy for the session list panel.
   * Tab-aware so Codex ↔ Grok switches never show the wrong source text.
   * @returns {{ kind: "loading"|"empty"|"filtered", title: string, detail: string }}
   */
  function emptyPanelCopy() {
    if (loading) {
      if (isGrok()) {
        return {
          kind: "loading",
          title: "正在加载 Grok 会话…",
          detail: "正在扫描本机 ~/.grok/sessions，请稍候",
        };
      }
      return {
        kind: "loading",
        title: "正在加载 Codex 会话…",
        detail: "正在读取 ChatGPT / Codex 本地 SQLite，请稍候",
      };
    }
    if (sessions.length && !filteredSessions().length) {
      return {
        kind: "filtered",
        title: "没有匹配的会话",
        detail: "当前搜索或项目筛选无结果，可清空搜索、选择「全部项目」，或点「刷新」。",
      };
    }
    if (isGrok()) {
      return {
        kind: "empty",
        title: "暂无 Grok 会话",
        detail:
          "未在本机找到 Grok Build 历史会话。确认已安装并产生过记录后，点列表工具栏「刷新」。",
      };
    }
    return {
      kind: "empty",
      title: "暂无 Codex 会话",
      detail:
        "未检测到本地会话库，或库中还没有记录。使用 ChatGPT / Codex 桌面端产生会话后，点「刷新」。",
    };
  }

  function renderEmptyPanel() {
    const empty = $("sessEmpty");
    if (!empty) return;
    const copy = emptyPanelCopy();
    empty.hidden = false;
    empty.dataset.state = copy.kind;
    empty.classList.toggle("is-loading", copy.kind === "loading");
    empty.setAttribute("role", copy.kind === "loading" ? "status" : "note");
    empty.setAttribute("aria-busy", copy.kind === "loading" ? "true" : "false");
    empty.innerHTML = `
      <div class="session-empty-title">${escapeHtml(copy.title)}</div>
      <div class="session-empty-detail">${escapeHtml(copy.detail)}</div>`;
  }

  /** Hide loading / empty placeholder once the list has real rows. */
  function hideEmptyPanel() {
    const empty = $("sessEmpty");
    if (!empty) return;
    empty.hidden = true;
    empty.dataset.state = "ready";
    empty.classList.remove("is-loading");
    empty.removeAttribute("aria-busy");
    empty.setAttribute("aria-hidden", "true");
  }

  function renderSessionRow(s, { hideCwd = false } = {}) {
    const selected = selectedIds.has(s.id);
    const title = s.title || "未命名会话";
    const cwd = sessionCwd(s);
    const resumeCmd = resumeCommandOf(s);
    const providerLabel = isGrok()
      ? s.modelProvider || "grok"
      : s.modelProvider || "provider 未记录";
    const shortId = s.id && s.id.length > 12 ? `${s.id.slice(0, 8)}…` : s.id || "";
    return `<div class="session-row" data-id="${escapeHtml(s.id)}" data-selection-mode="${selectionMode}" data-selected="${selected}">
          ${
            selectionMode
              ? `<label class="session-select"><input type="checkbox" data-sess-check value="${escapeHtml(s.id)}" ${selected ? "checked" : ""} aria-label="选择会话 ${escapeHtml(title)}" /></label>`
              : ""
          }
          <div class="session-main">
            <strong class="session-title" title="${escapeHtml(title)}">${escapeHtml(title)}</strong>
            <span class="session-id" title="${escapeHtml(s.id)}">${escapeHtml(shortId)}</span>
            ${
              hideCwd
                ? ""
                : `<small class="session-cwd" title="${escapeHtml(cwd)}">${escapeHtml(
                    cwd || "未记录项目路径"
                  )}</small>`
            }
          </div>
          <div class="session-meta">
            <span class="session-badge ${s.archived ? "is-archived" : "is-active"}">${s.archived ? "已归档" : "活跃"}</span>
            <span class="session-provider" title="provider">${escapeHtml(providerLabel)}</span>
            <span class="session-time">${escapeHtml(formatTime(s.updatedAtMs))}</span>
          </div>
          <div class="session-actions">
            <button type="button" class="chip-btn" data-sess-resume="${escapeHtml(s.id)}" ${anyBusy() || !resumeCmd ? "disabled" : ""} title="${escapeHtml(
              resumeCmd ? `复制 ${resumeCmd}` : "无 resume 命令"
            )}">
              <span class="chip-label">Resume</span>
            </button>
            <button type="button" class="chip-btn" data-sess-export="${escapeHtml(s.id)}" ${anyBusy() ? "disabled" : ""} title="导出 Markdown">
              <span class="chip-label">导出</span>
            </button>
            <button type="button" class="session-delete-btn chip-btn chip-danger" data-sess-delete="${escapeHtml(s.id)}" ${anyBusy() ? "disabled" : ""}>
              <span class="chip-label">删除</span>
            </button>
          </div>
        </div>`;
  }

  function buildProjectGroups(items) {
    const map = new Map();
    for (const s of items) {
      const key = projectKeyOf(s);
      if (!map.has(key)) {
        map.set(key, {
          key,
          cwd: sessionCwd(s),
          label: projectLabelOf(key, sessionCwd(s)),
          sessions: [],
        });
      }
      map.get(key).sessions.push(s);
    }
    return [...map.values()].sort((a, b) => {
      if (a.key === UNKNOWN_PROJECT_KEY) return 1;
      if (b.key === UNKNOWN_PROJECT_KEY) return -1;
      const ta = Math.max(0, ...a.sessions.map((s) => Number(s.updatedAtMs) || 0));
      const tb = Math.max(0, ...b.sessions.map((s) => Number(s.updatedAtMs) || 0));
      return tb - ta || a.label.localeCompare(b.label, "zh-CN");
    });
  }

  function renderList() {
    const list = $("sessList");
    if (!list) return;
    syncFilterControls();

    const visible = filteredSessions();

    // Loaded page has data, but filters hide everything → filtered empty state.
    if (sessions.length && !visible.length && !loading) {
      list.innerHTML = "";
      list.hidden = true;
      const empty = $("sessEmpty");
      if (empty) empty.removeAttribute("aria-hidden");
      renderEmptyPanel();
      updateMetrics();
      return;
    }

    // Successful load with visible rows: only the list (no empty/loading panel).
    if (visible.length) {
      hideEmptyPanel();
      list.hidden = false;
      if (groupByProject) {
        const groups = buildProjectGroups(visible);
        list.innerHTML = groups
          .map((g) => {
            const expanded = expandedProjects.has(g.key);
            const pathText = g.cwd || g.label;
            return `<section class="session-project-group" data-project-key="${escapeHtml(g.key)}" data-collapsed="${!expanded}">
              <button type="button" class="session-project-header" data-sess-toggle-project="${escapeHtml(g.key)}" aria-expanded="${expanded}">
                <span class="session-project-chevron" aria-hidden="true">${expanded ? "▾" : "▸"}</span>
                <span class="session-project-label" title="${escapeHtml(pathText)}">${escapeHtml(g.label)}</span>
                <span class="session-project-count">${g.sessions.length}</span>
                <span class="session-project-path" title="${escapeHtml(pathText)}">${escapeHtml(
                  g.key === UNKNOWN_PROJECT_KEY ? "无工作目录" : pathText
                )}</span>
              </button>
              <div class="session-project-body" ${expanded ? "" : "hidden"}>
                ${g.sessions.map((s) => renderSessionRow(s, { hideCwd: true })).join("")}
              </div>
            </section>`;
          })
          .join("");
      } else {
        list.innerHTML = visible.map((s) => renderSessionRow(s)).join("");
      }
      updateMetrics();
      return;
    }

    // No rows on page / still loading: hide list, show loading or true empty state.
    list.innerHTML = "";
    list.hidden = true;
    const empty = $("sessEmpty");
    if (empty) empty.removeAttribute("aria-hidden");
    renderEmptyPanel();
    updateMetrics();
  }

  async function loadProviderTargets() {
    if (isGrok() || !window.sessionAPI?.loadProviderTargets) return;
    try {
      const data = await window.sessionAPI.loadProviderTargets();
      providerTargets = {
        targets: Array.isArray(data?.targets) ? data.targets : [],
        currentProvider: data?.currentProvider || "",
      };
      fillProviderSelect();
    } catch (err) {
      console.warn("loadProviderTargets", err);
    }
  }

  function abortLoadWithMessage(msg) {
    loading = false;
    toast(msg, "error");
    renderList();
  }

  async function loadSessions(nextOffset = 0) {
    const api = window.sessionAPI;
    if (!api) {
      abortLoadWithMessage("会话 API 未就绪");
      return;
    }
    if (isGrok() && !api.listGrok) {
      abortLoadWithMessage("Grok 会话 API 未就绪");
      return;
    }
    if (!isGrok() && !api.list) {
      abortLoadWithMessage("会话 API 未就绪");
      return;
    }

    const seq = ++loadSeq;
    loading = true;
    updateMetrics();
    renderList();
    try {
      const data = isGrok()
        ? await api.listGrok({ offset: nextOffset, limit: PAGE_SIZE })
        : await api.list({ offset: nextOffset, limit: PAGE_SIZE });
      // A newer refresh / tab switch already owns the UI — drop this result.
      if (seq !== loadSeq) return;
      lastPayload = data;
      sessions = Array.isArray(data?.sessions) ? data.sessions : [];
      offset = data?.offset ?? nextOffset;
      limit = data?.limit ?? PAGE_SIZE;
      hasMore = data?.hasMore === true;
      const ids = new Set(sessions.map((s) => s.id));
      selectedIds = new Set([...selectedIds].filter((id) => ids.has(id)));
      if (data?.warnings?.length) {
        console.warn("session list warnings", data.warnings);
      }
      // Clear loading flag before paint so success path never keeps loading chrome.
      loading = false;
      renderList();
    } catch (err) {
      if (seq !== loadSeq) return;
      sessions = [];
      hasMore = false;
      loading = false;
      toast(err?.message || String(err), "error");
      renderList();
    } finally {
      // Stale requests must not flip loading off for a newer in-flight load.
      if (seq === loadSeq && loading) {
        loading = false;
        renderList();
      }
    }
  }

  async function switchSource(next) {
    if (next !== SOURCE_CODEX && next !== SOURCE_GROK) return;
    if (next === source) return;
    source = next;
    persistSource();
    offset = 0;
    hasMore = false;
    sessions = [];
    selectionMode = false;
    selectedIds = new Set();
    lastPayload = null;
    searchQuery = "";
    projectFilter = "";
    expandedProjects = new Set();
    // Invalidate in-flight list requests from the previous tab, then show
    // source-specific loading copy immediately (do not flash empty-state text).
    loadSeq += 1;
    loading = true;
    // Keep lastUndo for when user returns to Codex tab.
    applySourceChrome();
    renderList();
    if (!isGrok()) {
      void loadProviderTargets();
    }
    // Fire-and-forget: keep tab chrome responsive while the worker loads data.
    void loadSessions(0);
  }

  function rememberUndo(result, session) {
    if (isGrok()) return;
    const token = result?.undoToken || result?.undo_token;
    if (!token) return;
    lastUndo = {
      token: String(token),
      title: session?.title || session?.id || "会话",
      dbPath: session?.dbPath || null,
    };
    updateMetrics();
  }

  async function deleteOne(session) {
    const title = session.title || "未命名会话";
    const ok = await confirm({
      title: "删除会话",
      message: isGrok()
        ? `删除 Grok 会话「${title}」？\n\n将永久删除会话目录（含 chat_history 等），此操作不可撤销。`
        : `删除会话「${title}」？\n\n将删除本地数据库记录和对应 rollout 文件，并创建备份。若 Codex / ChatGPT 正在使用该会话，请先关闭对应窗口。`,
      confirmText: "删除",
      variant: "danger",
    });
    if (!ok) return;
    try {
      if (isGrok()) {
        await window.sessionAPI.deleteGrok({
          sessionId: session.id,
          title: session.title || "",
          sourcePath: session.rolloutPath || null,
        });
        toast("已删除 Grok 会话", "ok");
      } else {
        const result = await window.sessionAPI.delete({
          sessionId: session.id,
          title: session.title || "",
          dbPath: session.dbPath || null,
        });
        rememberUndo(result, session);
        toast("已删除会话（可撤销）", "ok");
      }
      selectedIds.delete(session.id);
      await loadSessions(offset);
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function exportOne(session) {
    try {
      const result = isGrok()
        ? await window.sessionAPI.exportGrokMarkdown({
            sessionId: session.id,
            title: session.title || "",
            sourcePath: session.rolloutPath || null,
          })
        : await window.sessionAPI.exportMarkdown({
            sessionId: session.id,
            title: session.title || "",
            dbPath: session.dbPath || null,
          });
      if (result?.canceled) {
        toast("已取消导出", "");
        return;
      }
      toast(result?.message || "已导出 Markdown", "ok");
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function undoLast() {
    if (isGrok() || !lastUndo?.token) return;
    try {
      await window.sessionAPI.undo({
        undoToken: lastUndo.token,
        dbPath: lastUndo.dbPath || null,
      });
      toast(`已恢复「${lastUndo.title}」`, "ok");
      lastUndo = null;
      updateMetrics();
      await loadSessions(offset);
    } catch (err) {
      toast(err?.message || String(err), "error");
    }
  }

  async function deleteSelected() {
    if (!selectionMode) {
      selectionMode = true;
      renderList();
      return;
    }
    const visibleIds = new Set(filteredSessions().map((s) => s.id));
    const picked = sessions.filter(
      (s) => selectedIds.has(s.id) && visibleIds.has(s.id)
    );
    if (!picked.length) {
      toast("请先选择要删除的会话", "error");
      return;
    }
    const preview = picked
      .slice(0, 5)
      .map((s) => `· ${s.title || s.id}`)
      .join("\n");
    const more =
      picked.length > 5 ? `\n…以及另外 ${picked.length - 5} 个会话` : "";
    const ok = await confirm({
      title: "批量删除会话",
      message: isGrok()
        ? `删除选中的 ${picked.length} 个 Grok 会话？将永久删除会话目录，不可撤销。\n\n${preview}${more}`
        : `删除选中的 ${picked.length} 个会话？将删除数据库记录与 rollout，并为每个会话创建备份。\n\n${preview}${more}`,
      confirmText: "全部删除",
      variant: "danger",
    });
    if (!ok) return;
    bulkDeleting = true;
    renderList();
    let okCount = 0;
    let failCount = 0;
    let lastResult = null;
    let lastSession = null;
    try {
      for (const s of picked) {
        try {
          if (isGrok()) {
            await window.sessionAPI.deleteGrok({
              sessionId: s.id,
              title: s.title || "",
              sourcePath: s.rolloutPath || null,
            });
          } else {
            lastResult = await window.sessionAPI.delete({
              sessionId: s.id,
              title: s.title || "",
              dbPath: s.dbPath || null,
            });
            lastSession = s;
          }
          okCount += 1;
          selectedIds.delete(s.id);
        } catch {
          failCount += 1;
        }
      }
      if (!isGrok() && lastResult && lastSession) {
        rememberUndo(lastResult, lastSession);
      }
      if (failCount === 0) toast(`已删除 ${okCount} 个会话`, "ok");
      else toast(`已删除 ${okCount} 个，失败 ${failCount} 个`, "error");
      await loadSessions(offset);
    } finally {
      bulkDeleting = false;
      renderList();
    }
  }

  async function runProviderRepair() {
    if (isGrok()) return;
    const sel = $("sessProviderTarget");
    const target = (sel?.value || "").trim();
    const ok = await confirm({
      title: "修复历史会话",
      message:
        `将历史 rollout / 数据库中的 provider 标记整理为「${target || "当前配置"}」。\n\n` +
        `会改写本机 Codex 会话元数据并创建备份。建议先完全退出 Codex / ChatGPT 客户端。`,
      confirmText: "开始修复",
      variant: "warn",
    });
    if (!ok) return;
    busyRepair = true;
    updateMetrics();
    const statusEl = $("sessRepairStatus");
    if (statusEl) {
      statusEl.hidden = false;
      statusEl.textContent = "正在修复历史会话…";
    }
    try {
      const result = await window.sessionAPI.syncProviders({
        targetProvider: target || null,
      });
      const msg =
        result?.message ||
        (result?.ok
          ? `修复完成：会话文件 ${result.changedSessionFiles ?? 0}，数据库行 ${result.sqliteRowsUpdated ?? 0}`
          : "修复已跳过或未完成");
      if (statusEl) statusEl.textContent = msg;
      toast(msg, result?.ok ? "ok" : "error");
      await loadSessions(offset);
      await loadProviderTargets();
    } catch (err) {
      if (statusEl) statusEl.textContent = err?.message || String(err);
      toast(err?.message || String(err), "error");
    } finally {
      busyRepair = false;
      updateMetrics();
    }
  }

  async function runIndexPreview() {
    if (isGrok()) return;
    busyRepair = true;
    updateMetrics();
    try {
      const data = await window.sessionAPI.previewIndexCleanup();
      indexPreview = {
        snapshotSha256: data?.snapshotSha256 || "",
        candidates: Array.isArray(data?.candidates) ? data.candidates : [],
      };
      renderIndexPreview();
      toast(
        indexPreview.candidates.length
          ? `发现 ${indexPreview.candidates.length} 条索引候选`
          : "无索引孤儿候选",
        "ok"
      );
    } catch (err) {
      toast(err?.message || String(err), "error");
    } finally {
      busyRepair = false;
      updateMetrics();
    }
  }

  async function runIndexApply() {
    if (isGrok()) return;
    if (!indexPreview?.snapshotSha256) {
      toast("请先预览索引清理", "error");
      return;
    }
    const box = $("sessIndexPreview");
    const checked = box
      ? [...box.querySelectorAll("[data-index-id]:checked")].map((el) =>
          el.getAttribute("data-index-id")
        )
      : [];
    if (!checked.length) {
      toast("请至少选择一条候选", "error");
      return;
    }
    const ok = await confirm({
      title: "清理 session_index",
      message:
        `将从 session_index.jsonl 移除 ${checked.length} 条仅索引存在的记录。\n\n` +
        `请完全退出 Codex / ChatGPT 后再继续。操作会写备份；若文件在预览后变化将自动中止。`,
      confirmText: "确认清理",
      variant: "danger",
    });
    if (!ok) return;
    busyRepair = true;
    updateMetrics();
    try {
      const result = await window.sessionAPI.applyIndexCleanup({
        snapshotSha256: indexPreview.snapshotSha256,
        threadIds: checked,
      });
      toast(`已清理 ${result?.prunedEntries ?? 0} 条索引记录`, "ok");
      indexPreview = null;
      renderIndexPreview();
    } catch (err) {
      toast(err?.message || String(err), "error");
    } finally {
      busyRepair = false;
      updateMetrics();
    }
  }

  function bind() {
    if (bound) return;
    bound = true;

    if (window.UiSelect?.mount) {
      const sel = $("sessProviderTarget");
      if (sel) window.UiSelect.mount(sel, { searchable: true, placeholder: "当前配置 provider" });
    }

    $("sessTabs")?.addEventListener("click", (e) => {
      const tab = e.target.closest?.("[data-sess-source]");
      if (!tab) return;
      const next = tab.getAttribute("data-sess-source");
      void switchSource(next);
    });

    $("btnSessRefresh")?.addEventListener("click", () => {
      void loadSessions(offset);
      if (!isGrok()) void loadProviderTargets();
    });
    $("btnSessPrev")?.addEventListener("click", () => {
      void loadSessions(Math.max(0, offset - limit));
    });
    $("btnSessNext")?.addEventListener("click", () => {
      if (hasMore) void loadSessions(offset + limit);
    });
    $("btnSessSelectAll")?.addEventListener("click", () => {
      selectionMode = true;
      selectedIds = new Set(filteredSessions().map((s) => s.id));
      renderList();
    });
    $("btnSessClearSel")?.addEventListener("click", () => {
      selectedIds = new Set();
      renderList();
    });
    $("btnSessMultiDelete")?.addEventListener("click", () => {
      void deleteSelected();
    });
    $("btnSessUndo")?.addEventListener("click", () => {
      void undoLast();
    });
    $("btnSessRepair")?.addEventListener("click", () => {
      void runProviderRepair();
    });
    $("btnSessIndexPreview")?.addEventListener("click", () => {
      void runIndexPreview();
    });
    $("btnSessIndexApply")?.addEventListener("click", () => {
      void runIndexApply();
    });

    $("sessSearch")?.addEventListener("input", (e) => {
      searchQuery = e.target?.value || "";
      renderList();
    });
    $("sessProjectFilter")?.addEventListener("change", (e) => {
      projectFilter = e.target?.value || "";
      renderList();
    });
    $("sessGroupByProject")?.addEventListener("change", (e) => {
      groupByProject = !!e.target?.checked;
      persistGroupByProject();
      renderList();
    });

    $("sessList")?.addEventListener("click", (e) => {
      const toggle = e.target.closest?.("[data-sess-toggle-project]");
      if (toggle) {
        const key = toggle.getAttribute("data-sess-toggle-project");
        if (!key) return;
        if (expandedProjects.has(key)) expandedProjects.delete(key);
        else expandedProjects.add(key);
        renderList();
        return;
      }
      const resume = e.target.closest?.("[data-sess-resume]");
      if (resume) {
        const id = resume.getAttribute("data-sess-resume");
        const session = sessions.find((s) => s.id === id);
        if (session) void copyResumeCommand(session);
        return;
      }
      const del = e.target.closest?.("[data-sess-delete]");
      if (del) {
        const id = del.getAttribute("data-sess-delete");
        const session = sessions.find((s) => s.id === id);
        if (session) void deleteOne(session);
        return;
      }
      const exp = e.target.closest?.("[data-sess-export]");
      if (exp) {
        const id = exp.getAttribute("data-sess-export");
        const session = sessions.find((s) => s.id === id);
        if (session) void exportOne(session);
      }
    });
    $("sessList")?.addEventListener("change", (e) => {
      const input = e.target.closest?.("[data-sess-check]");
      if (!input) return;
      const id = input.value;
      if (input.checked) selectedIds.add(id);
      else selectedIds.delete(id);
      updateMetrics();
      const row = input.closest(".session-row");
      if (row) row.dataset.selected = String(input.checked);
    });
  }

  window.sessionsView = {
    enter() {
      bind();
      applySourceChrome();
      if (!isGrok()) void loadProviderTargets();
      void loadSessions(offset > 0 ? offset : 0);
    },
    leave() {
      /* keep state for return */
    },
    refresh() {
      return loadSessions(offset);
    },
    /** @returns {"codex"|"grok"} */
    getSource() {
      return source;
    },
  };
})();
