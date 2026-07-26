/**
 * Page-local operation toast (apply / pause / switch feedback).
 *
 * Placement: bottom-right, near the viewport edge (does not block chat).
 * Used by:
 *   - renderer-core (host.showOperation / finishOperation)
 *   - native inject preflight (standalone evaluate before host exists)
 *
 * Scheme B: optional `token` on show/finish — stale ops are ignored.
 * Assembly: payload may embed this file as a string; keep it dependency-free.
 * Selectors / skin CSS do NOT belong here.
 */
(() => {
  const HOST_ID = "chatgpt-tools-skin-operation";
  const REG_KEY = "__CHATGPT_TOOLS_SKIN_OPERATION_UI__";
  const TOKEN_KEY = "__CHATGPT_TOOLS_OP_TOKEN__";
  const CSS = `
    :host {
      all: initial;
      position: fixed;
      inset: auto 12px 12px auto;
      z-index: 2147483646;
      pointer-events: none;
      opacity: 0;
      display: block;
      max-width: min(280px, calc(100vw - 20px));
      transition: opacity 140ms cubic-bezier(0.16, 1, 0.3, 1);
      font-family: system-ui, "Segoe UI", "PingFang SC", "Microsoft YaHei UI", sans-serif;
    }
    :host([data-visible="true"]) { opacity: 1; }
    .card {
      box-sizing: border-box;
      min-width: 140px;
      max-width: min(280px, calc(100vw - 20px));
      padding: 10px 12px;
      border-radius: 10px;
      border: 1px solid rgba(238, 239, 244, 0.16);
      background: rgba(32, 33, 38, 0.94);
      color: #f3f3f6;
      box-shadow: 0 8px 22px rgba(12, 14, 19, 0.28);
      text-align: left;
      font-size: 12.5px;
      font-weight: 550;
      line-height: 1.35;
      display: flex;
      align-items: center;
      gap: 10px;
      transform: translateY(6px);
      transition: transform 140ms cubic-bezier(0.16, 1, 0.3, 1);
    }
    :host([data-visible="true"]) .card { transform: none; }
    :host([data-tone="light"]) .card {
      border-color: #d9dbe3;
      background: rgba(248, 248, 251, 0.96);
      color: #25262c;
      box-shadow: 0 8px 22px rgba(31, 35, 48, 0.12);
    }
    .spin {
      flex: 0 0 auto;
      width: 16px; height: 16px;
      border: 2px solid currentColor;
      border-right-color: transparent;
      border-radius: 50%;
      animation: cg-op-spin 720ms linear infinite;
    }
    :host([data-state="success"]) .spin,
    :host([data-state="error"]) .spin,
    :host([data-state="cancelled"]) .spin { display: none; }
    .msg { word-break: break-word; flex: 1 1 auto; }
    @keyframes cg-op-spin { to { transform: rotate(360deg); } }
  `;

  function toneFromDoc() {
    try {
      const root = document.documentElement;
      if (root?.classList?.contains("electron-light")) return "light";
      if (root?.classList?.contains("electron-dark")) return "dark";
      if (window.matchMedia?.("(prefers-color-scheme: light)")?.matches) return "light";
    } catch {
      /* ignore */
    }
    return "dark";
  }

  function ensureHost() {
    let el = document.getElementById(HOST_ID);
    if (el?.shadowRoot) return el;
    el?.remove();
    el = document.createElement("div");
    el.id = HOST_ID;
    el.setAttribute("aria-live", "polite");
    const shadow = el.attachShadow({ mode: "open" });
    const style = document.createElement("style");
    style.textContent = CSS;
    const card = document.createElement("div");
    card.className = "card";
    card.innerHTML = `<div class="spin" aria-hidden="true"></div><div class="msg"></div>`;
    shadow.append(style, card);
    try {
      (document.documentElement || document.body)?.appendChild(el);
    } catch {
      /* host page may be mid-navigation */
    }
    return el;
  }

  function isStale(token, el) {
    if (token == null || token === 0) return false;
    const t = Number(token);
    const live = Number(window[TOKEN_KEY] || 0);
    const elTok = el ? Number(el.getAttribute("data-op-token") || 0) : 0;
    if (live && live !== t && elTok && elTok !== t) return true;
    if (elTok && elTok !== t) return true;
    return false;
  }

  function show(kind, message, token) {
    try {
      const el = ensureHost();
      if (!el?.shadowRoot) return { ok: false };
      if (token != null && token !== 0) {
        try {
          window[TOKEN_KEY] = Number(token);
          el.setAttribute("data-op-token", String(token));
        } catch {
          /* ignore */
        }
      }
      const msg =
        message ||
        (kind === "pause"
          ? "正在暂停皮肤…"
          : kind === "switch"
            ? "正在切换皮肤…"
            : "正在应用皮肤…");
      el.dataset.state = "loading";
      el.dataset.tone = toneFromDoc();
      el.dataset.visible = "true";
      const text = el.shadowRoot.querySelector(".msg");
      if (text) text.textContent = msg;
      window[REG_KEY] = {
        kind: kind || "apply",
        token: token != null ? Number(token) : `${Date.now()}:${Math.random().toString(36).slice(2, 8)}`,
        startedAt: Date.now(),
      };
      return window[REG_KEY];
    } catch {
      return { ok: false };
    }
  }

  function finish(state, message, token) {
    try {
      let el = document.getElementById(HOST_ID);
      if (isStale(token, el)) {
        return { ok: false, reason: "stale", token };
      }
      // Native restamp may leave a plain bootstrap node (no shadowRoot).
      if (el && !el.shadowRoot) {
        try {
          el.remove();
        } catch {
          /* ignore */
        }
        el = ensureHost();
      }
      if (!el?.shadowRoot) {
        try {
          delete window[REG_KEY];
        } catch {
          /* ignore */
        }
        return { ok: true, state: "cleared" };
      }
      if (token != null && token !== 0) {
        try {
          el.setAttribute("data-op-token", String(token));
          window[TOKEN_KEY] = Number(token);
        } catch {
          /* ignore */
        }
      }
      const st = state === "error" || state === "cancelled" ? state : "success";
      el.dataset.state = st;
      el.dataset.tone = toneFromDoc();
      el.dataset.visible = "true";
      const text = el.shadowRoot.querySelector(".msg");
      if (text) {
        text.textContent =
          message ||
          (st === "success" ? "完成" : st === "cancelled" ? "已取消" : "失败");
      }
      const hideMs = st === "error" ? 2000 : 1100;
      const tok = token != null ? Number(token) : 0;
      setTimeout(() => {
        try {
          const cur = document.getElementById(HOST_ID);
          if (!cur) return;
          if (tok && Number(cur.getAttribute("data-op-token") || 0) !== tok) return;
          if (tok && Number(window[TOKEN_KEY] || 0) !== tok) return;
          el.dataset.visible = "false";
          setTimeout(() => {
            try {
              const n = document.getElementById(HOST_ID);
              if (!n) return;
              if (tok && Number(n.getAttribute("data-op-token") || 0) !== tok) return;
              n.remove();
            } catch {
              /* ignore */
            }
          }, 160);
        } catch {
          /* ignore */
        }
      }, hideMs);
      try {
        delete window[REG_KEY];
      } catch {
        /* ignore */
      }
      return { ok: true, state: st, token: tok || null };
    } catch {
      return { ok: false };
    }
  }

  function dismiss(token) {
    try {
      const el = document.getElementById(HOST_ID);
      if (el) {
        const elTok = Number(el.getAttribute("data-op-token") || 0);
        if (!token || !elTok || elTok === Number(token)) el.remove();
      }
      if (!token || Number(window[TOKEN_KEY] || 0) === Number(token)) {
        try {
          delete window[TOKEN_KEY];
        } catch {
          /* ignore */
        }
      }
      delete window[REG_KEY];
    } catch {
      /* ignore */
    }
    return { ok: true };
  }

  const api = { show, finish, dismiss, HOST_ID, REG_KEY, TOKEN_KEY };
  try {
    window.__CHATGPT_TOOLS_SKIN_OP__ = api;
  } catch {
    /* ignore */
  }
  return api;
})();
