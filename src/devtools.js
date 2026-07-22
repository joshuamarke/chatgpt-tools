/**
 * Skin DevTools — host inspect (Scheme A: real-window Overlay pick) + status tabs.
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

/** @type {boolean} */
let inspectConnected = false;
/** @type {boolean} */
let picking = false;
/** @type {number|null} */
let selectedNodeId = null;
/** @type {ReturnType<typeof setInterval>|null} */
let pollTimer = null;
/** expanded nodeIds in tree */
const expanded = new Set();
/** children cache: nodeId -> summary[] */
const childrenCache = new Map();

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

/* ── Status tabs (unchanged data sources) ── */

function renderHost(host) {
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
  hostJson.textContent = JSON.stringify(host || {}, null, 2);
}

function renderRuntime(status) {
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
  const view = {
    ...pick(status || {}, keys),
    activeSkinId: active,
  };
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
  runtimeJson.textContent = JSON.stringify(view, null, 2);
}

function renderSkins(skins) {
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

    aboutJson.textContent = JSON.stringify(
      {
        engineVersion: version,
        paths,
        inspect: {
          connected: inspectConnected,
          picking,
          selectedNodeId,
        },
        note: "Elements uses Overlay.setInspectMode on the real host window via a dedicated CDP session.",
      },
      null,
      2
    );

    if (host?.error && status?.error) {
      setStatus(`刷新失败：${host.error}`, true);
    } else {
      setStatus("已刷新");
    }
  } catch (err) {
    setStatus(String(err?.message || err), true);
    hostJson.textContent = String(err?.message || err);
  }
}

/* ── Elements / inspect ── */

function nodeLabelHtml(node) {
  if (!node) return "";
  const ntype = node.nodeType;
  if (ntype === 3) {
    return `<span class="el-text-node">"${escapeHtml((node.nodeValue || node.label || "").slice(0, 80))}"</span>`;
  }
  if (ntype === 9) {
    return `<span class="el-tag">#document</span>`;
  }
  const tag = escapeHtml(node.localName || (node.nodeName || "node").toLowerCase());
  let attrs = "";
  const id = node.id;
  const cls = node.className;
  const testId = node.testId;
  if (id) {
    attrs += ` <span class="el-attr-name">id</span>=<span class="el-attr-val">"${escapeHtml(id)}"</span>`;
  }
  if (cls) {
    const short = String(cls).split(/\s+/).filter(Boolean).slice(0, 4).join(" ");
    attrs += ` <span class="el-attr-name">class</span>=<span class="el-attr-val">"${escapeHtml(short)}${String(cls).split(/\s+/).length > 4 ? "…" : ""}"</span>`;
  }
  if (testId) {
    attrs += ` <span class="el-attr-name">data-testid</span>=<span class="el-attr-val">"${escapeHtml(testId)}"</span>`;
  }
  return `<span class="el-tag">&lt;${tag}</span>${attrs}<span class="el-tag">&gt;</span>`;
}

function renderCrumbs(ancestors) {
  const list = Array.isArray(ancestors) ? ancestors : [];
  if (!list.length) {
    elCrumbs.textContent = "—";
    return;
  }
  elCrumbs.innerHTML = list
    .map((a, i) => {
      const last = i === list.length - 1;
      const label = escapeHtml(a.label || a.localName || a.nodeName || "?");
      return `${i ? '<span class="sep">›</span>' : ""}<span class="crumb${last ? " sel" : ""}">${label}</span>`;
    })
    .join("");
}

function renderSelection(selection) {
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

  // Matched rules
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
    html = '<div class="el-empty">无作者样式规则（可能仅有 user-agent 默认样式）。</div>';
  } else {
    for (const r of rules) {
      const sel = escapeHtml(r.selector || "(rule)");
      const origin = escapeHtml(r.origin || "");
      const body = escapeHtml(r.cssText || "");
      html += `<div class="el-rule">
        <div class="el-rule-sel">${sel}<span class="el-rule-origin">${origin}</span></div>
        <pre class="el-rule-body">${body || "/* empty */"}</pre>
      </div>`;
    }
  }
  viewMatched.innerHTML = html;

  // Computed
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

  // Tree: prefer path from pick
  if (Array.isArray(selection.treePath) && selection.treePath.length) {
    renderTreeFromPath(selection.treePath, selectedNodeId);
  } else if (selectedNodeId != null) {
    highlightTreeSelection(selectedNodeId);
  }
}

function renderTreeFromPath(treePath, selectedId) {
  // Flatten path levels into a simple indented list of ancestors + siblings at each level
  // Simpler UX: show ancestor chain as expandable path + children of selected
  const lines = [];
  for (let depth = 0; depth < treePath.length; depth++) {
    const level = treePath[depth];
    const nid = level.nodeId;
    const kids = Array.isArray(level.children) ? level.children : [];
    if (nid != null) childrenCache.set(Number(nid), kids);
    if (nid != null) expanded.add(Number(nid));
  }

  // Root-ish: first level's children or the first node
  const first = treePath[0];
  if (!first) {
    elTree.innerHTML = '<div class="el-empty">无 DOM 路径</div>';
    return;
  }

  // Render recursive from first ancestor if it has nodeId; else list all path labels
  function renderNode(node, depth) {
    const id = node.nodeId != null ? Number(node.nodeId) : null;
    const hasKids =
      (id != null && childrenCache.has(id) && childrenCache.get(id).length > 0) ||
      node.hasChildren ||
      (node.childNodeCount || 0) > 0;
    const isOpen = id != null && expanded.has(id);
    const isSel = id != null && id === Number(selectedId);
    const pad = 8 + depth * 14;
    const twist = hasKids ? (isOpen ? "▼" : "▶") : "";
    lines.push(
      `<div class="el-node${isSel ? " selected" : ""}" data-node-id="${id ?? ""}" style="padding-left:${pad}px">
        <span class="el-twisty${hasKids ? "" : " empty"}" data-toggle="${id ?? ""}">${twist}</span>
        <span class="el-node-label" data-select="${id ?? ""}">${nodeLabelHtml(node)}</span>
      </div>`
    );
    if (isOpen && id != null) {
      const ch = childrenCache.get(id) || [];
      for (const c of ch) {
        // If child is the next selected path node without nodeId on ancestors, still recurse if we have kids cached
        renderNode(c, depth + 1);
      }
    }
  }

  // Start from first path entry as root node summary
  const rootNode = {
    nodeId: first.nodeId,
    label: first.label,
    localName: String(first.label || "").split(/[#.]/)[0],
    nodeName: first.label,
    nodeType: 1,
    childNodeCount: (first.children || []).length,
    hasChildren: true,
  };
  // If first has children list, treat those as top-level under document
  if (Array.isArray(first.children) && first.children.length && first.nodeId == null) {
    for (const c of first.children) renderNode(c, 0);
  } else {
    renderNode(rootNode, 0);
    // Also ensure path nodes appear selected: open along path
    for (const level of treePath) {
      if (level.nodeId != null) expanded.add(Number(level.nodeId));
    }
    // Re-render more carefully: use only treePath levels as backbone
    lines.length = 0;
    for (let i = 0; i < treePath.length; i++) {
      const level = treePath[i];
      const node = {
        nodeId: level.nodeId,
        label: level.label,
        localName: String(level.label || "").split(/[#.]/)[0],
        nodeName: level.label,
        nodeType: 1,
        childNodeCount: (level.children || []).length,
        hasChildren: (level.children || []).length > 0,
      };
      const id = node.nodeId != null ? Number(node.nodeId) : null;
      const isSel = id != null && id === Number(selectedId);
      const pad = 8 + i * 14;
      const hasKids = (level.children || []).length > 0;
      lines.push(
        `<div class="el-node${isSel ? " selected" : ""}" data-node-id="${id ?? ""}" style="padding-left:${pad}px">
          <span class="el-twisty${hasKids ? "" : " empty"}" data-toggle="${id ?? ""}">${hasKids ? "▼" : ""}</span>
          <span class="el-node-label" data-select="${id ?? ""}">${nodeLabelHtml(node)}</span>
        </div>`
      );
      // siblings / children under this level
      if (hasKids && id != null) {
        for (const c of level.children) {
          const cid = c.nodeId != null ? Number(c.nodeId) : null;
          // skip if it's the next path node (will show as next level)
          const nextId = treePath[i + 1]?.nodeId;
          if (nextId != null && cid === Number(nextId)) continue;
          const cSel = cid != null && cid === Number(selectedId);
          const cPad = 8 + (i + 1) * 14;
          const cHas =
            (c.childNodeCount || 0) > 0 || c.hasChildren || (childrenCache.get(cid) || []).length > 0;
          lines.push(
            `<div class="el-node${cSel ? " selected" : ""}" data-node-id="${cid ?? ""}" style="padding-left:${cPad}px">
              <span class="el-twisty${cHas ? "" : " empty"}" data-toggle="${cid ?? ""}">${cHas && expanded.has(cid) ? "▼" : cHas ? "▶" : ""}</span>
              <span class="el-node-label" data-select="${cid ?? ""}">${nodeLabelHtml(c)}</span>
            </div>`
          );
        }
      }
    }
  }

  elTree.innerHTML = lines.join("") || '<div class="el-empty">无节点</div>';
  bindTreeEvents();
}

function highlightTreeSelection(nodeId) {
  elTree.querySelectorAll(".el-node").forEach((el) => {
    el.classList.toggle("selected", String(el.dataset.nodeId) === String(nodeId));
  });
}

function renderDocumentTree(root) {
  if (!root) {
    elTree.innerHTML = '<div class="el-empty">无文档</div>';
    return;
  }
  const lines = [];
  function walk(node, depth) {
    const id = node.nodeId != null ? Number(node.nodeId) : null;
    const kids = Array.isArray(node.children) ? node.children : [];
    if (id != null && kids.length) childrenCache.set(id, kids);
    const hasKids = kids.length > 0 || node.hasChildren || (node.childNodeCount || 0) > 0;
    const isOpen = id != null && (expanded.has(id) || depth < 2);
    if (id != null && depth < 2) expanded.add(id);
    const isSel = id != null && id === Number(selectedNodeId);
    const pad = 8 + depth * 14;
    lines.push(
      `<div class="el-node${isSel ? " selected" : ""}" data-node-id="${id ?? ""}" style="padding-left:${pad}px">
        <span class="el-twisty${hasKids ? "" : " empty"}" data-toggle="${id ?? ""}">${hasKids ? (isOpen ? "▼" : "▶") : ""}</span>
        <span class="el-node-label" data-select="${id ?? ""}">${nodeLabelHtml(node)}</span>
      </div>`
    );
    if (isOpen && kids.length) {
      for (const c of kids) walk(c, depth + 1);
    } else if (isOpen && id != null && childrenCache.has(id)) {
      for (const c of childrenCache.get(id)) walk(c, depth + 1);
    }
  }
  walk(root, 0);
  elTree.innerHTML = lines.join("");
  bindTreeEvents();
}

function bindTreeEvents() {
  elTree.querySelectorAll("[data-select]").forEach((el) => {
    el.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = el.getAttribute("data-select");
      if (!id) return;
      await selectNode(Number(id));
    });
  });
  elTree.querySelectorAll("[data-toggle]").forEach((el) => {
    el.addEventListener("click", async (e) => {
      e.stopPropagation();
      const id = el.getAttribute("data-toggle");
      if (!id) return;
      const nid = Number(id);
      if (expanded.has(nid)) {
        expanded.delete(nid);
      } else {
        expanded.add(nid);
        if (!childrenCache.has(nid)) {
          try {
            const res = await window.skinAPI.inspectGetChildren(nid);
            if (res?.children) childrenCache.set(nid, res.children);
          } catch (err) {
            setStatus(String(err?.message || err), true);
          }
        }
      }
      // re-render from document if we have root cache, else path
      if (lastDocRoot) renderDocumentTree(lastDocRoot);
      else if (lastSelection?.treePath) renderTreeFromPath(lastSelection.treePath, selectedNodeId);
    });
  });
}

/** @type {any} */
let lastDocRoot = null;
/** @type {any} */
let lastSelection = null;

async function selectNode(nodeId) {
  try {
    setStatus(`加载节点 ${nodeId}…`);
    const res = await window.skinAPI.inspectSelectNode(nodeId);
    if (res?.selection) {
      lastSelection = res.selection;
      renderSelection(res.selection);
      setStatus(`已选择 #${nodeId}`);
    } else {
      setStatus(res?.error || "选择失败", true);
    }
  } catch (err) {
    setStatus(String(err?.message || err), true);
  }
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

async function loadDocumentTree() {
  try {
    const res = await window.skinAPI.inspectGetDocument({ depth: 2 });
    if (res?.root) {
      lastDocRoot = res.root;
      expanded.clear();
      childrenCache.clear();
      renderDocumentTree(res.root);
    }
  } catch (err) {
    elTree.innerHTML = `<div class="el-empty">${escapeHtml(err?.message || err)}</div>`;
  }
}

function startPollLoop() {
  if (pollTimer) return;
  pollTimer = setInterval(async () => {
    if (!inspectConnected) return;
    if (document.visibilityState !== "visible") return;
    try {
      // Short wait only while picking so we catch Overlay.inspectNodeRequested
      const waitMs = picking ? 400 : 0;
      const res = await window.skinAPI.inspectPoll({ waitMs });
      if (typeof res?.picking === "boolean") {
        setPickingUi(res.picking);
      }
      if (res?.newSelection && res.selection) {
        lastSelection = res.selection;
        renderSelection(res.selection);
        setStatus("已点选宿主元素");
        // focus Elements tab is already default
      }
    } catch {
      /* session may drop when host restarts */
    }
  }, 500);
}

function stopPollLoop() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

async function togglePick() {
  try {
    if (!inspectConnected) {
      await connectInspect();
    }
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
    viewMatched.classList.toggle("active", id === "matched");
    viewComputed.classList.toggle("active", id === "computed");
    viewHtml.classList.toggle("active", id === "html");
  });
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
document.getElementById("btnClose")?.addEventListener("click", async () => {
  try {
    if (picking) {
      await window.skinAPI.inspectSetPicking(false).catch(() => {});
    }
    // keep session for next open; only stop poll
    stopPollLoop();
    const api = window.__TAURI__;
    const win =
      api?.webviewWindow?.getCurrentWebviewWindow?.() ||
      api?.window?.getCurrentWindow?.();
    if (win?.close) await win.close();
    else window.close();
  } catch {
    window.close();
  }
});

// Auto-connect when Elements opens (best-effort)
(async () => {
  await refresh();
  try {
    await connectInspect();
  } catch {
    setConnUi("err", "宿主未就绪 — 打开 ChatGPT/Codex 后点「连接宿主」");
  }
})();

setInterval(() => {
  if (document.visibilityState === "visible") {
    window.skinAPI
      .hostStatus({ force: false })
      .then((host) => {
        if (host && !host.error) renderHost(host);
      })
      .catch(() => {});
  }
}, 4000);
