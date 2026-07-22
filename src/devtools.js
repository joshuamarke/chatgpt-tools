/**
 * Skin DevTools — independent window UI (P0).
 * Uses the same window.skinAPI surface as the main manager.
 */
const hostCards = document.getElementById("hostCards");
const hostJson = document.getElementById("hostJson");
const runtimeCards = document.getElementById("runtimeCards");
const runtimeJson = document.getElementById("runtimeJson");
const skinsTableBody = document.querySelector("#skinsTable tbody");
const aboutJson = document.getElementById("aboutJson");
const footerStatus = document.getElementById("footerStatus");
const footerTime = document.getElementById("footerTime");

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

function renderHost(host) {
  const lc = host?.lifecycle ?? "—";
  hostCards.innerHTML = [
    card("lifecycle", fmt(lc), toneForLifecycle(lc)),
    card("processRunning", fmt(host?.processRunning), toneBool(host?.processRunning)),
    card("debugPortOpen", fmt(host?.debugPortOpen), toneBool(host?.debugPortOpen)),
    card("rendererReady", fmt(host?.rendererReady ?? host?.debugReady), toneBool(host?.rendererReady ?? host?.debugReady)),
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
  // active may live under state or top-level depending on engine revision
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

    // Prefer dedicated hostStatus; fall back to status host fields
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
        note: "Skin DevTools P0 — use main window to apply skins; this window inspects host + session.",
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

/* tabs */
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

// F12 inside DevTools just refreshes (window already open)
document.addEventListener("keydown", (e) => {
  if (e.key === "F12" || ((e.ctrlKey || e.metaKey) && e.shiftKey && String(e.key).toLowerCase() === "i")) {
    e.preventDefault();
    refresh();
  }
});

refresh();
// Light poll while focused (host lifecycle)
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
