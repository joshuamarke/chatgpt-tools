/**
 * Frontend bridge for local chat session management
 * (Codex / ChatGPT desktop SQLite + Grok Build ~/.grok).
 * Independent of window.skinAPI (skin engine).
 */
(function () {
  function getInvoke() {
    const core = window.__TAURI__?.core;
    if (core?.invoke) return core.invoke.bind(core);
    throw new Error(
      "Tauri API 不可用。请通过 `npm run dev` 或安装包启动 ChatGPT Tools。"
    );
  }

  async function waitForInvoke(timeoutMs = 8000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      try {
        return getInvoke();
      } catch {
        await new Promise((r) => setTimeout(r, 50));
      }
    }
    return getInvoke();
  }

  async function invoke(cmd, args) {
    try {
      const inv = await waitForInvoke();
      return await inv(cmd, args || {});
    } catch (err) {
      const msg =
        typeof err === "string"
          ? err
          : err?.message || err?.toString?.() || JSON.stringify(err);
      throw new Error(msg);
    }
  }

  window.sessionAPI = {
    /** @param {{ offset?: number, limit?: number }} [opts] */
    list: (opts = {}) =>
      invoke("list_local_sessions", {
        request: {
          offset: opts.offset ?? 0,
          limit: opts.limit ?? 50,
        },
      }),
    /**
     * @param {{ sessionId: string, title?: string, dbPath?: string }} opts
     */
    delete: (opts) =>
      invoke("delete_local_session", {
        request: {
          sessionId: opts.sessionId,
          title: opts.title || "",
          dbPath: opts.dbPath || null,
        },
      }),
    /**
     * @param {{ undoToken: string, dbPath?: string }} opts
     */
    undo: (opts) =>
      invoke("undo_local_session", {
        request: {
          undoToken: opts.undoToken,
          dbPath: opts.dbPath || null,
        },
      }),
    /**
     * @param {{ sessionId: string, title?: string, dbPath?: string }} opts
     */
    exportMarkdown: (opts) =>
      invoke("export_local_session_markdown", {
        request: {
          sessionId: opts.sessionId,
          title: opts.title || "",
          dbPath: opts.dbPath || null,
        },
      }),
    loadProviderTargets: () => invoke("load_provider_sync_targets"),
    /**
     * @param {{ targetProvider?: string }} [opts]
     */
    syncProviders: (opts = {}) =>
      invoke("sync_providers_now", {
        request: {
          targetProvider: opts.targetProvider || null,
        },
      }),
    previewIndexCleanup: () => invoke("preview_session_index_cleanup"),
    /**
     * @param {{ snapshotSha256: string, threadIds: string[] }} opts
     */
    applyIndexCleanup: (opts) =>
      invoke("apply_session_index_cleanup_cmd", {
        request: {
          snapshotSha256: opts.snapshotSha256,
          threadIds: opts.threadIds || [],
        },
      }),
    paths: () => invoke("session_paths_info"),

    // ── Grok Build ────────────────────────────────────────────────────────
    /** @param {{ offset?: number, limit?: number }} [opts] */
    listGrok: (opts = {}) =>
      invoke("list_grok_sessions", {
        request: {
          offset: opts.offset ?? 0,
          limit: opts.limit ?? 50,
        },
      }),
    /**
     * @param {{ sessionId: string, title?: string, sourcePath?: string }} opts
     */
    deleteGrok: (opts) =>
      invoke("delete_grok_session", {
        request: {
          sessionId: opts.sessionId,
          title: opts.title || "",
          sourcePath: opts.sourcePath || opts.rolloutPath || null,
        },
      }),
    /**
     * @param {{ sessionId: string, title?: string, sourcePath?: string }} opts
     */
    exportGrokMarkdown: (opts) =>
      invoke("export_grok_session_markdown", {
        request: {
          sessionId: opts.sessionId,
          title: opts.title || "",
          sourcePath: opts.sourcePath || opts.rolloutPath || null,
        },
      }),
  };
})();
