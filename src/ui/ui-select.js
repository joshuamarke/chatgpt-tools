/**
 * Shared custom select (ui-select).
 * Wraps a native <select> for accessibility / form values while rendering a
 * consistent dropdown used by sessions + providers.
 *
 * Usage:
 *   UiSelect.mount(selectEl, { placeholder?, search?, className? })
 *   UiSelect.setOptions(selectEl, [{ value, label, meta? }, ...], selected?)
 *   UiSelect.refresh(selectEl)
 *   UiSelect.destroy(selectEl)
 */
(function () {
  const OPEN_CLASS = "is-open";
  const instances = new WeakMap();

  function $(sel, root) {
    return (root || document).querySelector(sel);
  }

  function escapeHtml(s) {
    return String(s ?? "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function closeAll(except) {
    document.querySelectorAll(`.ui-select.${OPEN_CLASS}`).forEach((wrap) => {
      if (except && wrap === except) return;
      wrap.classList.remove(OPEN_CLASS);
      const btn = $(".ui-select-trigger", wrap);
      if (btn) btn.setAttribute("aria-expanded", "false");
    });
  }

  function optionList(select) {
    return Array.from(select.options || []).map((opt, index) => ({
      value: opt.value,
      label: opt.textContent || opt.label || opt.value,
      disabled: !!opt.disabled,
      hidden: opt.hidden || opt.value === "" && opt.disabled,
      index,
    }));
  }

  function syncTriggerLabel(inst) {
    const { select, triggerLabel, placeholder } = inst;
    const opt = select.options[select.selectedIndex];
    const text =
      opt && (opt.value !== "" || select.value !== "")
        ? opt.textContent || opt.label || opt.value
        : placeholder || "请选择";
    triggerLabel.textContent = text;
    triggerLabel.classList.toggle("is-placeholder", !opt || (opt.value === "" && !select.value));
  }

  function renderMenu(inst, filterText) {
    const { select, menu, opts } = inst;
    const q = (filterText || "").trim().toLowerCase();
    const items = optionList(select).filter((o) => {
      if (o.hidden && !o.value) return !q; // keep empty placeholder when not searching
      if (!q) return true;
      return (
        o.label.toLowerCase().includes(q) ||
        String(o.value).toLowerCase().includes(q)
      );
    });

    if (!items.length) {
      menu.innerHTML = `<div class="ui-select-empty">无匹配项</div>`;
      return;
    }

    const selected = select.value;
    menu.innerHTML = items
      .map((o) => {
        const isSel = o.value === selected;
        const dis = o.disabled ? " disabled" : "";
        const sel = isSel ? " aria-selected=\"true\"" : " aria-selected=\"false\"";
        return `<button type="button" class="ui-select-option${isSel ? " is-selected" : ""}${o.disabled ? " is-disabled" : ""}" data-value="${escapeHtml(o.value)}" role="option"${sel}${dis}>
          <span class="ui-select-option-label">${escapeHtml(o.label)}</span>
        </button>`;
      })
      .join("");

    menu.querySelectorAll(".ui-select-option:not(.is-disabled)").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.preventDefault();
        e.stopPropagation();
        const value = btn.getAttribute("data-value") ?? "";
        const prev = select.value;
        select.value = value;
        // Fire native change so existing listeners keep working.
        if (prev !== value) {
          select.dispatchEvent(new Event("change", { bubbles: true }));
        }
        syncTriggerLabel(inst);
        closeMenu(inst);
        inst.trigger?.focus?.();
      });
    });

    // Focus selected or first
    const focusEl =
      menu.querySelector(".ui-select-option.is-selected") ||
      menu.querySelector(".ui-select-option:not(.is-disabled)");
    if (focusEl && opts?.focusOnOpen !== false) {
      requestAnimationFrame(() => focusEl.focus?.());
    }
  }

  function openMenu(inst) {
    if (inst.select.disabled) return;
    closeAll(inst.wrap);
    inst.wrap.classList.add(OPEN_CLASS);
    inst.trigger.setAttribute("aria-expanded", "true");
    if (inst.search) {
      inst.search.value = "";
      inst.search.hidden = false;
      renderMenu(inst, "");
      requestAnimationFrame(() => inst.search.focus());
    } else {
      renderMenu(inst, "");
    }
  }

  function closeMenu(inst) {
    inst.wrap.classList.remove(OPEN_CLASS);
    inst.trigger.setAttribute("aria-expanded", "false");
    if (inst.search) inst.search.value = "";
  }

  function onTriggerKey(inst, e) {
    if (inst.select.disabled) return;
    if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (!inst.wrap.classList.contains(OPEN_CLASS)) openMenu(inst);
    } else if (e.key === "Escape") {
      if (inst.wrap.classList.contains(OPEN_CLASS)) {
        e.preventDefault();
        closeMenu(inst);
      }
    }
  }

  /**
   * @param {HTMLSelectElement} select
   * @param {{ placeholder?: string, searchable?: boolean, className?: string }} [opts]
   */
  function mount(select, opts) {
    if (!select || select.tagName !== "SELECT") return null;
    if (instances.has(select)) {
      refresh(select);
      return instances.get(select);
    }

    const placeholder =
      opts?.placeholder ||
      select.getAttribute("data-placeholder") ||
      (select.options[0] && select.options[0].value === ""
        ? select.options[0].textContent
        : "请选择");

    const wrap = document.createElement("div");
    wrap.className = "ui-select" + (opts?.className ? ` ${opts.className}` : "");
    // Toolbar density: data attr, legacy sess-select, or sessions tools field
    const toolbarVariant =
      select.getAttribute("data-ui-select-variant") === "toolbar" ||
      select.classList.contains("sess-select") ||
      !!select.closest(".sess-field, .sessions-tools-row");
    if (toolbarVariant) wrap.classList.add("ui-select--toolbar");
    if (select.disabled) wrap.classList.add("is-disabled");

    const trigger = document.createElement("button");
    trigger.type = "button";
    trigger.className = "ui-select-trigger";
    trigger.setAttribute("aria-haspopup", "listbox");
    trigger.setAttribute("aria-expanded", "false");
    trigger.disabled = !!select.disabled;

    const triggerLabel = document.createElement("span");
    triggerLabel.className = "ui-select-value";
    const chevron = document.createElement("span");
    chevron.className = "ui-select-chevron";
    chevron.setAttribute("aria-hidden", "true");
    chevron.innerHTML =
      '<svg viewBox="0 0 20 20" width="16" height="16" fill="none"><path d="M5 7.5 10 12.5 15 7.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>';
    trigger.appendChild(triggerLabel);
    trigger.appendChild(chevron);

    const panel = document.createElement("div");
    panel.className = "ui-select-panel";
    panel.setAttribute("role", "listbox");

    let search = null;
    const searchable =
      opts?.searchable === true ||
      select.getAttribute("data-searchable") === "true" ||
      (select.options && select.options.length > 8);
    if (searchable) {
      search = document.createElement("input");
      search.type = "search";
      search.className = "ui-select-search";
      search.placeholder = "搜索…";
      search.autocomplete = "off";
      search.spellcheck = false;
      panel.appendChild(search);
    }

    const menu = document.createElement("div");
    menu.className = "ui-select-menu";
    panel.appendChild(menu);

    // Hide native select but keep it in the form tree for value/change.
    select.classList.add("ui-select-native");
    select.tabIndex = -1;
    select.setAttribute("aria-hidden", "true");

    const parent = select.parentNode;
    parent.insertBefore(wrap, select);
    wrap.appendChild(trigger);
    wrap.appendChild(panel);
    wrap.appendChild(select);

    const inst = {
      select,
      wrap,
      trigger,
      triggerLabel,
      panel,
      menu,
      search,
      placeholder,
      opts: opts || {},
    };
    instances.set(select, inst);
    wrap._uiSelect = inst;

    trigger.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (inst.wrap.classList.contains(OPEN_CLASS)) closeMenu(inst);
      else openMenu(inst);
    });
    trigger.addEventListener("keydown", (e) => onTriggerKey(inst, e));

    if (search) {
      search.addEventListener("input", () => renderMenu(inst, search.value));
      search.addEventListener("keydown", (e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          closeMenu(inst);
          trigger.focus();
        } else if (e.key === "ArrowDown") {
          e.preventDefault();
          menu.querySelector(".ui-select-option:not(.is-disabled)")?.focus?.();
        }
      });
    }

    select.addEventListener("change", () => syncTriggerLabel(inst));

    // Observe disabled attribute changes
    const mo = new MutationObserver(() => {
      const dis = !!select.disabled;
      trigger.disabled = dis;
      wrap.classList.toggle("is-disabled", dis);
    });
    mo.observe(select, { attributes: true, attributeFilter: ["disabled"] });
    inst._mo = mo;

    syncTriggerLabel(inst);
    return inst;
  }

  function refresh(select) {
    const inst = instances.get(select);
    if (!inst) return;
    syncTriggerLabel(inst);
    if (inst.wrap.classList.contains(OPEN_CLASS)) {
      renderMenu(inst, inst.search?.value || "");
    }
  }

  /**
   * Replace <select> options and refresh UI.
   * @param {HTMLSelectElement} select
   * @param {Array<{value:string,label:string,disabled?:boolean}>} options
   * @param {string} [selected]
   */
  function setOptions(select, options, selected) {
    if (!select) return;
    const list = Array.isArray(options) ? options : [];
    select.innerHTML = list
      .map((o) => {
        const dis = o.disabled ? " disabled" : "";
        return `<option value="${escapeHtml(o.value)}"${dis}>${escapeHtml(o.label)}</option>`;
      })
      .join("");
    if (selected != null) select.value = selected;
    else if (list.length && !select.value) select.value = list[0].value;
    if (!instances.has(select)) mount(select);
    else refresh(select);
  }

  function destroy(select) {
    const inst = instances.get(select);
    if (!inst) return;
    inst._mo?.disconnect?.();
    const parent = inst.wrap.parentNode;
    if (parent) {
      parent.insertBefore(select, inst.wrap);
      parent.removeChild(inst.wrap);
    }
    select.classList.remove("ui-select-native");
    select.removeAttribute("aria-hidden");
    select.tabIndex = 0;
    instances.delete(select);
  }

  function mountAll(root, selector) {
    const scope = root || document;
    const sel = selector || "select[data-ui-select], select.ui-select-source";
    scope.querySelectorAll(sel).forEach((el) => {
      if (el.tagName === "SELECT") mount(el);
    });
  }

  // Global outside click / Escape
  if (!window.__uiSelectDocBound) {
    window.__uiSelectDocBound = true;
    document.addEventListener(
      "click",
      (e) => {
        const wrap = e.target?.closest?.(".ui-select");
        if (!wrap) closeAll(null);
      },
      true
    );
    document.addEventListener(
      "keydown",
      (e) => {
        if (e.key === "Escape") closeAll(null);
      },
      true
    );
  }

  window.UiSelect = {
    mount,
    mountAll,
    refresh,
    setOptions,
    destroy,
    closeAll,
  };
})();
