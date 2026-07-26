/**
 * Skin DevTools — host inspect (Scheme A) + status tabs.
 * - Closing the window disconnects CDP inspect (releases Overlay/DOM session).
 * - DOM tree: hierarchical expand/collapse, lazy children, reveal on pick.
 * - Custom undecorated titlebar; DOM|Styles resizable splitter; crumbs ↔ tree.
 */
const hostCards = document.getElementById("hostCards");
const hostJson = document.getElementById("hostJson");
const runtimeCards = document.getElementById("runtimeCards");
const runtimeJson = document.getElementById("runtimeJson");
const skinsTableBody = document.querySelector("#skinsTable tbody");
const aboutJson = document.getElementById("aboutJson");
const footerStatus = document.getElementById("footerStatus");
const footerTime = document.getElementById("footerTime");

const elTree = document.getElementById("elTree");
const elCrumbs = document.getElementById("elCrumbs");
const elSplit = document.getElementById("elSplit");
const elSplitter = document.getElementById("elSplitter");
const elTreePane = document.getElementById("elTreePane");
const elConnStatus = document.getElementById("elConnStatus");
const elSelHead = document.getElementById("elSelHead");
const elSelHint = document.getElementById("elSelHint");
const viewMatched = document.getElementById("viewMatched");
const viewComputed = document.getElementById("viewComputed");
const viewHtml = document.getElementById("viewHtml");
const elOuterHtml = document.getElementById("elOuterHtml");
const btnPick = document.getElementById("btnPick");
const btnPickLabel = document.getElementById("btnPickLabel");
const btnConnect = document.getElementById("btnConnect");
const btnReloadTree = document.getElementById("btnReloadTree");

const SPLIT_STORAGE_KEY = "devtools.el-tree-w";

/** @type {boolean} */
let inspectConnected = false;
/** @type {boolean} */
let picking = false;
/** @type {number|null} */
let selectedNodeId = null;
/** @type {ReturnType<typeof setInterval>|null} */
let pollTimer = null;
/** @type {ReturnType<typeof setInterval>|null} */
let hostPollTimer = null;
/** expanded nodeIds */
const expanded = new Set();
/** children cache: nodeId -> node summary[] */
const childrenCache = new Map();
/** node index for quick lookup: nodeId -> summary */
const nodeIndex = new Map();
/** @type {any} */
let lastDocRoot = null;
/** @type {any} */
let lastSelection = null;
/** teardown guard */
let tearingDown = false;
/** tree events bound once */
let treeEventsBound = false;
/** scroll/flash target after next tree paint */
let pendingRevealId = null;

function escapeHtml(s) {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function toneForLifecycle(lc) {
  if (lc === "ready") return "ok";
  if (lc === "starting") return "warn";
  if (lc === "offline") return "danger";
  return "muted";
}

function toneBool(v, okWhenTrue = true) {
  if (v === true) return okWhenTrue ? "ok" : "danger";
  if (v === false) return okWhenTrue ? "danger" : "ok";
  return "muted";
}

function card(label, value, tone = "") {
  const vClass = tone ? `v ${tone}` : "v";
  return `<div class="dt-card"><div class="k">${escapeHtml(label)}</div><div class="${vClass}">${escapeHtml(value)}</div></div>`;
}

function fmt(v) {
  if (v === null || v === undefined || v === "") return "—";
  if (typeof v === "boolean") return v ? "true" : "false";
  return String(v);
}

function pick(obj, keys) {
  const out = {};
  for (const k of keys) {
    if (obj && Object.prototype.hasOwnProperty.call(obj, k)) out[k] = obj[k];
  }
  return out;
}

function setStatus(text, isError = false) {
  footerStatus.textContent = text;
  footerStatus.style.color = isError ? "var(--danger)" : "var(--muted)";
  footerTime.textContent = new Date().toLocaleTimeString();
}

function setConnUi(state, text) {
  elConnStatus.textContent = text;
  elConnStatus.className = "el-conn" + (state ? ` ${state}` : "");
}

function setPickingUi(on) {
  picking = on;
  btnPick?.classList.toggle("active", on);
  if (btnPickLabel) {
    btnPickLabel.textContent = on ? "点选中…（到宿主窗口点击）" : "选择元素";
  }
}

/* ── Status tabs ── */

function renderHost(host) {
  if (!hostCards) return;
  const lc = host?.lifecycle ?? "—";
  hostCards.innerHTML = [
    card("lifecycle", fmt(lc), toneForLifecycle(lc)),
    card("processRunning", fmt(host?.processRunning), toneBool(host?.processRunning)),
    card("debugPortOpen", fmt(host?.debugPortOpen), toneBool(host?.debugPortOpen)),
    card(
      "rendererReady",
      fmt(host?.rendererReady ?? host?.debugReady),
      toneBool(host?.rendererReady ?? host?.debugReady)
    ),
    card("canHotApply", fmt(host?.canHotApply), toneBool(host?.canHotApply)),
    card("confidence", fmt(host?.confidence)),
  ].join("");
  if (hostJson) hostJson.textContent = JSON.stringify(host || {}, null, 2);
}

function renderRuntime(status) {
  if (!runtimeCards) return;
  const keys = [
    "paused",
    "shellOk",
    "artOk",
    "artPending",
    "injectorAlive",
    "protocol",
    "engineVersion",
    "activeSkinId",
    "lifecycle",
    "codexRunning",
  ];
  const active =
    status?.activeSkinId ||
    status?.state?.skinId ||
    status?.state?.activeSkinId ||
    status?.skins?.find?.((s) => s.active)?.id ||
    null;
  const view = { ...pick(status || {}, keys), activeSkinId: active };
  runtimeCards.innerHTML = [
    card("activeSkin", fmt(active)),
    card("paused", fmt(status?.paused), status?.paused ? "warn" : "ok"),
    card("shellOk", fmt(status?.shellOk), toneBool(status?.shellOk)),
    card("artOk", fmt(status?.artOk), toneBool(status?.artOk)),
    card("artPending", fmt(status?.artPending), status?.artPending ? "warn" : "muted"),
    card("injectorAlive", fmt(status?.injectorAlive), toneBool(status?.injectorAlive)),
    card("protocol", fmt(status?.protocol)),
    card("engineVersion", fmt(status?.engineVersion)),
  ].join("");
  if (runtimeJson) runtimeJson.textContent = JSON.stringify(view, null, 2);
}

function renderSkins(skins) {
  if (!skinsTableBody) return;
  const list = Array.isArray(skins) ? skins : [];
  if (!list.length) {
    skinsTableBody.innerHTML =
      '<tr><td colspan="6" style="color:var(--muted)">无皮肤数据</td></tr>';
    return;
  }
  skinsTableBody.innerHTML = list
    .map((s) => {
      const active = Boolean(s.active);
      return `<tr>
        <td><code>${escapeHtml(s.id)}</code></td>
        <td>${escapeHtml(s.name || s.id)}</td>
        <td>${escapeHtml(s.version || "—")}</td>
        <td>${s.builtin ? "是" : "否"}</td>
        <td><span class="badge ${active ? "on" : "off"}">${active ? "active" : "—"}</span></td>
        <td>${escapeHtml(s.appearance || "—")}</td>
      </tr>`;
    })
    .join("");
}

async function refresh() {
  setStatus("刷新中…");
  try {
    const [host, status, version, paths] = await Promise.all([
      window.skinAPI.hostStatus({ force: true }).catch((e) => ({ error: String(e?.message || e) })),
      window.skinAPI.status().catch((e) => ({ error: String(e?.message || e) })),
      window.skinAPI.engineVersion().catch((e) => ({ error: String(e?.message || e) })),
      window.skinAPI.enginePaths().catch((e) => ({ error: String(e?.message || e) })),
    ]);

    const hostView =
      host && !host.error
        ? host
        : pick(status || {}, [
            "lifecycle",
            "lifecycleRaw",
            "processRunning",
            "debugPortOpen",
            "rendererReady",
            "debugReady",
            "codexRunning",
            "canHotApply",
            "needsRestartForInject",
            "confidence",
            "probeAgeMs",
          ]);

    renderHost(hostView);
    renderRuntime(status?.error ? { error: status.error } : status);
    renderSkins(status?.skins);

    if (aboutJson) {
      aboutJson.textContent = JSON.stringify(
        {
          engineVersion: version,
          paths,
          inspect: { connected: inspectConnected, picking, selectedNodeId },
          note: "Closing this window disconnects the dedicated inspect CDP session.",
        },
        null,
        2
      );
    }

    if (host?.error && status?.error) setStatus(`刷新失败：${host.error}`, true);
    else setStatus("已刷新");
  } catch (err) {
    setStatus(String(err?.message || err), true);
    if (hostJson) hostJson.textContent = String(err?.message || err);
  }
}

/* ── Resource teardown ── */

async function teardownInspect(reason = "close") {
  if (tearingDown) return;
  tearingDown = true;
  stopPollLoop();
  stopHostPoll();
  try {
    if (picking) {
      await window.skinAPI.inspectSetPicking(false).catch(() => {});
    }
    if (inspectConnected) {
      await window.skinAPI.inspectDisconnect().catch(() => {});
    }
  } finally {
    inspectConnected = false;
    picking = false;
    setPickingUi(false);
    setConnUi("", "已断开");
    expanded.clear();
    childrenCache.clear();
    nodeIndex.clear();
    lastDocRoot = null;
    lastSelection = null;
    selectedNodeId = null;
    if (elTree) {
      elTree.innerHTML = `<div class="el-empty">调试会话已结束（${escapeHtml(reason)}）</div>`;
    }
    tearingDown = false;
  }
}

function stopPollLoop() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

function stopHostPoll() {
  if (hostPollTimer) {
    clearInterval(hostPollTimer);
    hostPollTimer = null;
  }
}

/* ── DOM tree ── */

function indexNode(node) {
  if (!node || node.nodeId == null) return;
  const id = Number(node.nodeId);
  nodeIndex.set(id, node);
  const kids = Array.isArray(node.children) ? node.children : childrenCache.get(id);
  if (kids) {
    for (const c of kids) indexNode(c);
  }
}

function cacheChildren(parentId, children) {
  const list = Array.isArray(children) ? children : [];
  childrenCache.set(Number(parentId), list);
  for (const c of list) {
    if (c?.nodeId != null) nodeIndex.set(Number(c.nodeId), c);
  }
}

function nodeLabelHtml(node) {
  if (!node) return "";
  const ntype = node.nodeType;
  if (ntype === 3) {
    const t = String(node.nodeValue || node.label || "").trim().slice(0, 60);
    return `<span class="el-text-node">"${escapeHtml(t)}${t.length >= 60 ? "…" : ""}"</span>`;
  }
  if (ntype === 9) {
    return `<span class="el-tag">#document</span>`;
  }
  if (ntype === 11 || node.isShadowRoot) {
    return `<span class="el-tag">#shadow-root</span>`;
  }
  const tag = escapeHtml(
    node.localName || String(node.nodeName || "node").toLowerCase()
  );
  let attrs = "";
  if (node.id) {
    attrs += ` <span class="el-attr-name">id</span>=<span class="el-attr-val">"${escapeHtml(node.id)}"</span>`;
  }
  if (node.className) {
    const parts = String(node.className).split(/\s+/).filter(Boolean);
    const short = parts.slice(0, 3).join(" ");
    attrs += ` <span class="el-attr-name">class</span>=<span class="el-attr-val">"${escapeHtml(short)}${parts.length > 3 ? "…" : ""}"</span>`;
  }
  if (node.testId) {
    attrs += ` <span class="el-attr-name">data-testid</span>=<span class="el-attr-val">"${escapeHtml(node.testId)}"</span>`;
  }
  return `<span class="el-tag">&lt;${tag}</span>${attrs}<span class="el-tag">&gt;</span>`;
}

function hasChildrenHint(node) {
  if (!node) return false;
  const id = node.nodeId != null ? Number(node.nodeId) : null;
  if (id != null && childrenCache.has(id) && childrenCache.get(id).length > 0) return true;
  if (Array.isArray(node.children) && node.children.length > 0) return true;
  if (node.hasChildren) return true;
  return (node.childNodeCount || 0) > 0;
}

function getChildrenOf(node) {
  if (!node) return [];
  const id = node.nodeId != null ? Number(node.nodeId) : null;
  if (id != null && childrenCache.has(id)) return childrenCache.get(id);
  if (Array.isArray(node.children) && node.children.length) {
    if (id != null) cacheChildren(id, node.children);
    return node.children;
  }
  return [];
}

/**
 * Build a hierarchical tree model under document, preferring element nodes.
 * Mutates childrenCache / nodeIndex from embedded children.
 */
function normalizeRoot(root) {
  if (!root) return null;
  indexNode(root);
  // Seed cache from embedded children recursively
  function seed(n) {
    if (!n || n.nodeId == null) return;
    const id = Number(n.nodeId);
    if (Array.isArray(n.children) && n.children.length) {
      cacheChildren(id, n.children);
      for (const c of n.children) seed(c);
    }
  }
  seed(root);
  return root;
}

function renderDomTree() {
  if (!elTree) return;
  if (!lastDocRoot) {
    elTree.innerHTML =
      '<div class="el-empty">连接宿主后加载 DOM 树。点选元素会自动展开并定位。</div>';
    return;
  }

  const lines = [];
  const maxDepth = 48;

  function walk(node, depth) {
    if (!node || depth > maxDepth) return;
    const id = node.nodeId != null ? Number(node.nodeId) : null;
    const kids = getChildrenOf(node);
    const canExpand = hasChildrenHint(node);
    const isOpen = id != null && expanded.has(id);
    const isSel = id != null && id === Number(selectedNodeId);
    const pad = 6 + depth * 14;
    const twistClass = canExpand ? "el-twisty" : "el-twisty empty";
    const twist = canExpand ? (isOpen ? "▼" : "▶") : "";

    lines.push(
      `<div class="el-node${isSel ? " selected" : ""}" data-node-id="${id ?? ""}" style="padding-left:${pad}px" title="${escapeHtml(node.selectorHint || node.label || "")}">
        <span class="${twistClass}" data-action="toggle" data-node-id="${id ?? ""}">${twist}</span>
        <span class="el-node-label" data-action="select" data-node-id="${id ?? ""}">${nodeLabelHtml(node)}</span>
      </div>`
    );

    if (isOpen && kids.length) {
      for (const c of kids) walk(c, depth + 1);
    }
  }

  // Prefer starting at documentElement (html) if present under #document
  const root = lastDocRoot;
  const rootKids = getChildrenOf(root);
  if (root.nodeType === 9 && rootKids.length) {
    // show #document collapsed-open with its element children
    expanded.add(Number(root.nodeId));
    walk(root, 0);
  } else {
    walk(root, 0);
  }

  elTree.innerHTML = lines.join("") || '<div class="el-empty">无节点</div>';

  // Scroll selected / pending reveal into view and flash
  const focusId = pendingRevealId ?? selectedNodeId;
  pendingRevealId = null;
  if (focusId != null) {
    const el = elTree.querySelector(`.el-node[data-node-id="${focusId}"]`);
    if (el) {
      el.scrollIntoView({ block: "center", behavior: "smooth" });
      el.classList.remove("reveal-flash");
      // reflow so animation restarts
      void el.offsetWidth;
      el.classList.add("reveal-flash");
      window.setTimeout(() => el.classList.remove("reveal-flash"), 900);
    }
  }
}

function ensureTreeEvents() {
  if (treeEventsBound || !elTree) return;
  treeEventsBound = true;
  elTree.addEventListener("click", async (e) => {
    const t = e.target.closest("[data-action]");
    if (!t || !elTree.contains(t)) return;
    const action = t.getAttribute("data-action");
    const idStr = t.getAttribute("data-node-id");
    if (!idStr) return;
    const nid = Number(idStr);
    if (Number.isNaN(nid)) return;

    if (action === "toggle") {
      e.preventDefault();
      e.stopPropagation();
      await toggleExpand(nid);
      return;
    }
    if (action === "select") {
      e.preventDefault();
      await selectNode(nid);
    }
  });
}

async function toggleExpand(nid) {
  if (expanded.has(nid)) {
    expanded.delete(nid);
    renderDomTree();
    return;
  }
  expanded.add(nid);
  if (!childrenCache.has(nid) || childrenCache.get(nid).length === 0) {
    try {
      setStatus(`展开 #${nid}…`);
      const res = await window.skinAPI.inspectGetChildren(nid);
      if (res?.children) {
        cacheChildren(nid, res.children);
        // attach onto lastDocRoot structure if present
        mergeChildrenIntoRoot(nid, res.children);
      }
    } catch (err) {
      setStatus(String(err?.message || err), true);
    }
  }
  renderDomTree();
  setStatus(`已展开 #${nid}`);
}

function mergeChildrenIntoRoot(parentId, children) {
  const node = nodeIndex.get(Number(parentId));
  if (node) {
    node.children = children;
    node.hasChildren = children.length > 0;
  }
  cacheChildren(parentId, children);
}

/**
 * After pick/select: expand ancestor chain and re-render full document tree.
 * @param {any} selection
 * @param {{ focus?: boolean }} [opts]
 */
async function revealInTree(selection, opts = {}) {
  const focus = opts.focus !== false;
  const ancestors = Array.isArray(selection?.ancestors) ? selection.ancestors : [];
  const leafId = selection?.nodeId ?? selection?.node?.nodeId ?? null;

  // Cache children from treePath levels
  const path = Array.isArray(selection?.treePath) ? selection.treePath : [];
  for (const level of path) {
    if (level?.nodeId != null && Array.isArray(level.children)) {
      cacheChildren(Number(level.nodeId), level.children);
      mergeChildrenIntoRoot(Number(level.nodeId), level.children);
      expanded.add(Number(level.nodeId));
    }
  }

  // Expand every ancestor with nodeId (not the leaf itself as "closed parent")
  for (const a of ancestors) {
    if (a?.nodeId != null) expanded.add(Number(a.nodeId));
  }
  if (leafId != null) {
    selectedNodeId = Number(leafId);
    if (focus) pendingRevealId = Number(leafId);
  }

  // Ensure we have a document tree; if not, load one
  if (!lastDocRoot) {
    await loadDocumentTree({ preserveExpand: true });
  } else {
    // Lazy-fetch children for ancestors not yet in cache (path may lack some)
    for (const a of ancestors) {
      const id = a?.nodeId != null ? Number(a.nodeId) : null;
      if (id == null) continue;
      if (!childrenCache.has(id) || childrenCache.get(id).length === 0) {
        try {
          const res = await window.skinAPI.inspectGetChildren(id);
          if (res?.children) mergeChildrenIntoRoot(id, res.children);
        } catch {
          /* ignore partial failures */
        }
      }
    }
    // Also ensure parent of leaf has children so the leaf row is visible
    if (leafId != null && ancestors.length) {
      // chain includes leaf as last entry → parent is second-to-last; else last is parent
      let parentId = null;
      const last = ancestors[ancestors.length - 1];
      if (ancestors.length >= 2 && Number(last?.nodeId) === Number(leafId)) {
        parentId = Number(ancestors[ancestors.length - 2].nodeId);
      } else if (Number(last?.nodeId) !== Number(leafId) && last?.nodeId != null) {
        parentId = Number(last.nodeId);
      }
      if (
        parentId != null &&
        (!childrenCache.has(parentId) ||
          !childrenCache.get(parentId).some((c) => Number(c.nodeId) === Number(leafId)))
      ) {
        try {
          const res = await window.skinAPI.inspectGetChildren(parentId);
          if (res?.children) mergeChildrenIntoRoot(parentId, res.children);
          expanded.add(parentId);
        } catch {
          /* ignore */
        }
      }
    }
    renderDomTree();
  }
}

function renderCrumbs(ancestors) {
  if (!elCrumbs) return;
  const list = Array.isArray(ancestors) ? ancestors : [];
  if (!list.length) {
    elCrumbs.textContent = "—";
    return;
  }
  elCrumbs.innerHTML = list
    .map((a, i) => {
      const last = i === list.length - 1;
      const label = escapeHtml(a.label || a.localName || a.nodeName || "?");
      const nid =
        a.nodeId != null && a.nodeId !== "" && !Number.isNaN(Number(a.nodeId))
          ? Number(a.nodeId)
          : "";
      const title =
        nid !== ""
          ? `点击在 DOM 树中定位并选中 #${nid}`
          : "此节点暂无 nodeId";
      return `${i ? '<span class="sep">›</span>' : ""}<span class="crumb${last ? " sel" : ""}" data-crumb-id="${nid}" title="${escapeHtml(title)}" role="${nid !== "" ? "button" : "text"}" tabindex="${nid !== "" ? "0" : "-1"}">${label}</span>`;
    })
    .join("");
}

function renderStylesOnly(selection) {
  if (!selection) {
    elSelHead.querySelector(".el-sel-label").textContent = "未选择节点";
    elSelHint.textContent = "";
    viewMatched.innerHTML =
      '<div class="el-empty">点选宿主元素后显示匹配的 CSS 规则（类似 Chrome Styles）。</div>';
    viewComputed.innerHTML =
      '<div class="el-empty">选择节点后显示关键 computed 属性。</div>';
    elOuterHtml.textContent = "—";
    selectedNodeId = null;
    return;
  }

  const node = selection.node || {};
  selectedNodeId = selection.nodeId ?? node.nodeId ?? null;
  elSelHead.querySelector(".el-sel-label").textContent =
    node.label || node.localName || String(selectedNodeId);
  elSelHint.textContent = node.selectorHint ? `selector: ${node.selectorHint}` : "";
  renderCrumbs(selection.ancestors);

  const styles = selection.styles || {};
  const rules = Array.isArray(styles.matchedRules) ? styles.matchedRules : [];
  const inline = styles.inline?.cssText || "";
  let html = "";
  if (inline) {
    html += `<div class="el-rule">
      <div class="el-rule-sel">element.style <span class="el-rule-origin">inline</span></div>
      <pre class="el-rule-body">${escapeHtml(inline)}</pre>
    </div>`;
  }
  if (!rules.length && !inline) {
    html =
      '<div class="el-empty">无作者样式规则（可能仅有 user-agent 默认样式）。</div>';
  } else {
    for (const r of rules) {
      html += `<div class="el-rule">
        <div class="el-rule-sel">${escapeHtml(r.selector || "(rule)")}<span class="el-rule-origin">${escapeHtml(r.origin || "")}</span></div>
        <pre class="el-rule-body">${escapeHtml(r.cssText || "/* empty */")}</pre>
      </div>`;
    }
  }
  viewMatched.innerHTML = html;

  const computed = styles.computed || {};
  const keys = Object.keys(computed);
  if (!keys.length) {
    viewComputed.innerHTML = '<div class="el-empty">无 computed 数据</div>';
  } else {
    viewComputed.innerHTML = `<table class="el-computed-table"><tbody>${keys
      .map(
        (k) =>
          `<tr><th>${escapeHtml(k)}</th><td>${escapeHtml(computed[k])}</td></tr>`
      )
      .join("")}</tbody></table>`;
  }

  elOuterHtml.textContent = selection.outerHTML || "—";
}

async function renderSelection(selection) {
  lastSelection = selection;
  renderStylesOnly(selection);
  if (selection) {
    await revealInTree(selection);
  }
}

async function selectNode(nodeId, opts = {}) {
  const fromCrumb = opts.fromCrumb === true;
  try {
    setStatus(fromCrumb ? `面包屑定位 #${nodeId}…` : `加载节点 ${nodeId}…`);
    // Optimistic highlight while network loads
    selectedNodeId = Number(nodeId);
    pendingRevealId = Number(nodeId);
    // Expand known ancestors locally so the node is visible ASAP
    if (lastSelection?.ancestors) {
      for (const a of lastSelection.ancestors) {
        if (a?.nodeId != null) expanded.add(Number(a.nodeId));
      }
    }
    if (lastDocRoot) renderDomTree();

    const res = await window.skinAPI.inspectSelectNode(nodeId);
    if (res?.selection) {
      pendingRevealId = Number(nodeId);
      await renderSelection(res.selection);
      // Keep Elements tab visible when jumping from crumbs
      if (fromCrumb) activateElementsTab();
      // Flash the crumb that was clicked
      if (fromCrumb && elCrumbs) {
        const crumb = elCrumbs.querySelector(`[data-crumb-id="${nodeId}"]`);
        if (crumb) {
          crumb.classList.remove("flash");
          void crumb.offsetWidth;
          crumb.classList.add("flash");
          window.setTimeout(() => crumb.classList.remove("flash"), 800);
          crumb.scrollIntoView({ block: "nearest", inline: "center", behavior: "smooth" });
        }
      }
      // Ensure tree scrolls again after styles/crumbs re-render
      requestAnimationFrame(() => {
        const el = elTree?.querySelector(`.el-node[data-node-id="${nodeId}"]`);
        if (el) {
          el.scrollIntoView({ block: "center", behavior: "smooth" });
          el.classList.add("reveal-flash");
          window.setTimeout(() => el.classList.remove("reveal-flash"), 900);
        }
      });
      setStatus(fromCrumb ? `已定位祖先 #${nodeId}` : `已选择 #${nodeId}`);
    } else {
      setStatus(res?.error || "选择失败", true);
    }
  } catch (err) {
    setStatus(String(err?.message || err), true);
  }
}

function activateElementsTab() {
  const tab = document.querySelector('.dt-tab[data-tab="elements"]');
  if (tab && !tab.classList.contains("active")) tab.click();
}

async function connectInspect() {
  try {
    setConnUi("warn", "连接中…");
    const res = await window.skinAPI.inspectConnect();
    inspectConnected = true;
    setConnUi("ok", `已连接 · ${shortUrl(res?.targetUrl)}`);
    setStatus(res?.reused ? "已复用 inspect 会话" : "已连接宿主 CDP");
    startPollLoop();
    await loadDocumentTree();
    return res;
  } catch (err) {
    inspectConnected = false;
    setConnUi("err", "连接失败");
    setStatus(String(err?.message || err), true);
    throw err;
  }
}

function shortUrl(u) {
  if (!u) return "host";
  const s = String(u);
  return s.length > 42 ? s.slice(0, 40) + "…" : s;
}

async function loadDocumentTree(opts = {}) {
  const preserveExpand = opts.preserveExpand === true;
  try {
    elTree.innerHTML = '<div class="el-empty">加载 DOM…</div>';
    const res = await window.skinAPI.inspectGetDocument({ depth: 3 });
    if (res?.root) {
      if (!preserveExpand) {
        expanded.clear();
        childrenCache.clear();
        nodeIndex.clear();
      }
      lastDocRoot = normalizeRoot(res.root);
      // Auto-expand document and html/body for a usable first view
      if (lastDocRoot?.nodeId != null) expanded.add(Number(lastDocRoot.nodeId));
      const kids = getChildrenOf(lastDocRoot);
      for (const k of kids) {
        if (k.nodeId != null) {
          const name = String(k.localName || k.nodeName || "").toLowerCase();
          if (name === "html" || name === "body" || k.nodeType === 1) {
            expanded.add(Number(k.nodeId));
            // one more level under html
            for (const c of getChildrenOf(k)) {
              const cn = String(c.localName || "").toLowerCase();
              if (cn === "body" || cn === "head") {
                if (c.nodeId != null) expanded.add(Number(c.nodeId));
              }
            }
          }
        }
      }
      ensureTreeEvents();
      renderDomTree();
    } else {
      elTree.innerHTML = '<div class="el-empty">未返回 DOM 根节点</div>';
    }
  } catch (err) {
    elTree.innerHTML = `<div class="el-empty">${escapeHtml(err?.message || err)}</div>`;
  }
}

function startPollLoop() {
  if (pollTimer) return;
  pollTimer = setInterval(async () => {
    if (!inspectConnected || tearingDown) return;
    if (document.visibilityState !== "visible") return;
    try {
      const waitMs = picking ? 200 : 0;
      const res = await window.skinAPI.inspectPoll({ waitMs });
      if (typeof res?.picking === "boolean") setPickingUi(res.picking);
      if (res?.newSelection && res.selection) {
        await renderSelection(res.selection);
        setStatus("已点选宿主元素");
      }
    } catch {
      /* host restart */
    }
  }, 450);
}

async function togglePick() {
  try {
    if (!inspectConnected) await connectInspect();
    const next = !picking;
    const res = await window.skinAPI.inspectSetPicking(next);
    setPickingUi(Boolean(res?.picking ?? next));
    setStatus(
      next
        ? "点选已开启：请切换到 ChatGPT/Codex 窗口并点击目标元素"
        : "已退出点选"
    );
  } catch (err) {
    setPickingUi(false);
    setStatus(String(err?.message || err), true);
  }
}

/* style sub-tabs */
document.querySelectorAll(".el-style-tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    const id = tab.dataset.styleTab;
    document.querySelectorAll(".el-style-tab").forEach((t) => {
      t.classList.toggle("active", t.dataset.styleTab === id);
    });
    viewMatched.hidden = id !== "matched";
    viewComputed.hidden = id !== "computed";
    viewHtml.hidden = id !== "html";
  });
});

/* crumbs → select ancestor + reveal in DOM tree */
function onCrumbActivate(c) {
  if (!c) return;
  const id = c.getAttribute("data-crumb-id");
  if (!id) return;
  const nid = Number(id);
  if (Number.isNaN(nid)) return;
  if (selectedNodeId === nid && lastSelection?.nodeId === nid) {
    // Re-reveal already selected node in tree
    pendingRevealId = nid;
    renderDomTree();
    setStatus(`已定位 #${nid}`);
    return;
  }
  selectNode(nid, { fromCrumb: true });
}

elCrumbs?.addEventListener("click", (e) => {
  const c = e.target.closest("[data-crumb-id]");
  if (!c || !elCrumbs.contains(c)) return;
  e.preventDefault();
  onCrumbActivate(c);
});
elCrumbs?.addEventListener("keydown", (e) => {
  if (e.key !== "Enter" && e.key !== " ") return;
  const c = e.target.closest("[data-crumb-id]");
  if (!c || !elCrumbs.contains(c)) return;
  e.preventDefault();
  onCrumbActivate(c);
});

btnPick?.addEventListener("click", () => togglePick());
btnConnect?.addEventListener("click", () => connectInspect());
btnReloadTree?.addEventListener("click", async () => {
  try {
    if (!inspectConnected) await connectInspect();
    else await loadDocumentTree();
    setStatus("DOM 已刷新");
  } catch (err) {
    setStatus(String(err?.message || err), true);
  }
});

/* ── DOM | Styles resizable splitter ── */
function applyTreeWidth(pxOrPercent) {
  if (!elSplit) return;
  const splitW = elSplit.getBoundingClientRect().width;
  if (splitW <= 0) return;
  let px;
  if (typeof pxOrPercent === "string" && pxOrPercent.endsWith("%")) {
    px = (parseFloat(pxOrPercent) / 100) * splitW;
  } else {
    px = Number(pxOrPercent);
  }
  if (!Number.isFinite(px)) return;
  const minL = 200;
  const minR = 220;
  const splitterW = elSplitter?.getBoundingClientRect().width || 5;
  const maxL = Math.max(minL, splitW - minR - splitterW);
  px = Math.min(maxL, Math.max(minL, px));
  const pct = `${((px / splitW) * 100).toFixed(2)}%`;
  elSplit.style.setProperty("--el-tree-w", pct);
  document.documentElement.style.setProperty("--el-tree-w", pct);
  if (elTreePane) {
    elTreePane.style.flexBasis = pct;
    elTreePane.style.width = pct;
  }
  return pct;
}

function setupSplitter() {
  if (!elSplitter || !elSplit) return;

  // Restore saved width
  try {
    const saved = localStorage.getItem(SPLIT_STORAGE_KEY);
    if (saved) applyTreeWidth(saved);
  } catch {
    /* ignore */
  }

  let dragging = false;

  const onMove = (clientX) => {
    if (!dragging) return;
    const rect = elSplit.getBoundingClientRect();
    const x = clientX - rect.left;
    const pct = applyTreeWidth(x);
    if (pct) {
      try {
        localStorage.setItem(SPLIT_STORAGE_KEY, pct);
      } catch {
        /* ignore */
      }
    }
  };

  const endDrag = () => {
    if (!dragging) return;
    dragging = false;
    elSplit.classList.remove("resizing");
    document.body.classList.remove("el-resizing");
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", endDrag);
    window.removeEventListener("pointercancel", endDrag);
  };

  const onPointerMove = (e) => {
    e.preventDefault();
    onMove(e.clientX);
  };

  elSplitter.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    elSplit.classList.add("resizing");
    document.body.classList.add("el-resizing");
    try {
      elSplitter.setPointerCapture(e.pointerId);
    } catch {
      /* ignore */
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", endDrag);
    window.addEventListener("pointercancel", endDrag);
  });

  // Keyboard: left/right arrows nudge width
  elSplitter.addEventListener("keydown", (e) => {
    const rect = elSplit.getBoundingClientRect();
    const current =
      elTreePane?.getBoundingClientRect().width || rect.width * 0.42;
    if (e.key === "ArrowLeft") {
      e.preventDefault();
      applyTreeWidth(current - 24);
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      applyTreeWidth(current + 24);
    } else if (e.key === "Home") {
      e.preventDefault();
      applyTreeWidth(200);
    } else if (e.key === "End") {
      e.preventDefault();
      applyTreeWidth(rect.width - 220);
    }
  });

  // Double-click reset to default 42%
  elSplitter.addEventListener("dblclick", () => {
    const pct = applyTreeWidth("42%");
    if (pct) {
      try {
        localStorage.setItem(SPLIT_STORAGE_KEY, pct);
      } catch {
        /* ignore */
      }
    }
  });
}

/* main tabs */
document.querySelectorAll(".dt-tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    const id = tab.dataset.tab;
    document.querySelectorAll(".dt-tab").forEach((t) => {
      const on = t.dataset.tab === id;
      t.classList.toggle("active", on);
      t.setAttribute("aria-selected", on ? "true" : "false");
    });
    document.querySelectorAll(".dt-panel").forEach((p) => {
      const on = p.dataset.panel === id;
      p.classList.toggle("active", on);
      p.hidden = !on;
    });
  });
});

document.getElementById("btnRefresh")?.addEventListener("click", () => refresh());

/* ── Undecorated window controls (match main window) ── */
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

async function closeDevtoolsWindow() {
  await teardownInspect("close");
  try {
    const win = getCurrentWindow();
    if (win?.close) await win.close();
    else window.close();
  } catch {
    window.close();
  }
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
    closeDevtoolsWindow();
  });

  document.querySelector(".dt-titlebar")?.addEventListener("dblclick", async (e) => {
    if (e.target.closest("[data-no-drag], button, .win-controls, .dt-titlebar-actions")) {
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

// OS / navigate away teardown (Rust also hooks Destroyed)
window.addEventListener("beforeunload", () => {
  try {
    stopPollLoop();
    stopHostPoll();
    if (inspectConnected) {
      window.skinAPI.inspectDisconnect().catch(() => {});
    }
  } catch {
    /* ignore */
  }
});

// Boot
(async () => {
  ensureTreeEvents();
  setupSplitter();
  setupWindowControls();
  await refresh();
  try {
    await connectInspect();
  } catch {
    setConnUi("err", "宿主未就绪 — 打开 ChatGPT/Codex 后点「连接宿主」");
  }
})();

hostPollTimer = setInterval(() => {
  if (document.visibilityState !== "visible" || tearingDown) return;
  window.skinAPI
    .hostStatus({ force: false })
    .then((host) => {
      if (host && !host.error) renderHost(host);
    })
    .catch(() => {});
}, 4000);
