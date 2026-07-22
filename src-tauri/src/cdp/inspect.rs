//! Host element inspector (Scheme A): real-window pick via Overlay + DOM/CSS.
//!
//! Independent CDP session from inject/keep so inspect does not tear down skins.

use super::http::{list_app_targets, CdpHttpError};
use super::native::SHARED_PORT;
use super::session::{CdpSession, CdpSessionError, OpenOptions};
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InspectError {
    #[error("{0}")]
    Message(String),
}

impl InspectError {
    fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

impl From<CdpSessionError> for InspectError {
    fn from(e: CdpSessionError) -> Self {
        Self::msg(e.to_string())
    }
}

impl From<CdpHttpError> for InspectError {
    fn from(e: CdpHttpError) -> Self {
        Self::msg(e.to_string())
    }
}

fn cdp_port() -> u16 {
    std::env::var("CODEX_SKIN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|p: &u16| (1024..=65535).contains(p))
        .unwrap_or(SHARED_PORT)
}

/// Computed properties commonly useful when authoring skins.
const COMPUTED_KEYS: &[&str] = &[
    "display",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "z-index",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "box-sizing",
    "overflow",
    "overflow-x",
    "overflow-y",
    "flex",
    "flex-direction",
    "flex-wrap",
    "align-items",
    "justify-content",
    "gap",
    "grid-template-columns",
    "color",
    "background-color",
    "background-image",
    "background-size",
    "background-position",
    "border",
    "border-radius",
    "opacity",
    "transform",
    "filter",
    "backdrop-filter",
    "box-shadow",
    "font-family",
    "font-size",
    "font-weight",
    "line-height",
    "letter-spacing",
    "text-align",
    "white-space",
    "visibility",
    "pointer-events",
    "cursor",
    "color-scheme",
];

struct InspectSession {
    session: CdpSession,
    target_id: String,
    target_url: String,
    picking: bool,
    /// Last fully resolved selection payload for poll consumers.
    last_selection: Option<Value>,
    /// root document nodeId
    document_node_id: i64,
}

static INSPECT: Mutex<Option<InspectSession>> = Mutex::new(None);

fn map_err_session(e: CdpSessionError) -> InspectError {
    InspectError::msg(e.to_string())
}

/// Connect (or reuse) inspect session and enable DOM/CSS/Overlay.
pub fn connect() -> Result<Value, InspectError> {
    let mut guard = INSPECT.lock();
    if let Some(ref st) = *guard {
        return Ok(json!({
            "ok": true,
            "reused": true,
            "picking": st.picking,
            "targetId": st.target_id,
            "targetUrl": st.target_url,
            "documentNodeId": st.document_node_id,
            "hasSelection": st.last_selection.is_some(),
        }));
    }

    let port = cdp_port();
    let targets = list_app_targets(port)?;
    let target = targets
        .first()
        .ok_or_else(|| InspectError::msg("没有可用的 app:// 宿主页面（请先打开 ChatGPT/Codex）"))?
        .clone();

    let session = CdpSession::open_with(
        &target,
        port,
        OpenOptions {
            open_timeout_ms: 8000,
            enable_default_domains: true,
            with_events: true,
        },
    )?;

    // Domains for pick + styles
    session
        .send("DOM.enable", json!({}), 8000)
        .map_err(map_err_session)?;
    session
        .send("CSS.enable", json!({}), 8000)
        .map_err(map_err_session)?;
    session
        .send("Overlay.enable", json!({}), 8000)
        .map_err(map_err_session)?;

    let doc = session
        .send(
            "DOM.getDocument",
            json!({ "depth": 1, "pierce": true }),
            10000,
        )
        .map_err(map_err_session)?;
    let root = doc
        .get("root")
        .cloned()
        .ok_or_else(|| InspectError::msg("DOM.getDocument missing root"))?;
    let document_node_id = root
        .get("nodeId")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| InspectError::msg("document nodeId missing"))?;

    let st = InspectSession {
        session,
        target_id: target.id.clone(),
        target_url: target.url.clone(),
        picking: false,
        last_selection: None,
        document_node_id,
    };
    *guard = Some(st);

    Ok(json!({
        "ok": true,
        "reused": false,
        "picking": false,
        "targetId": target.id,
        "targetUrl": target.url,
        "documentNodeId": document_node_id,
        "hasSelection": false,
    }))
}

pub fn disconnect() -> Result<Value, InspectError> {
    let mut guard = INSPECT.lock();
    if let Some(st) = guard.take() {
        let _ = st.session.send(
            "Overlay.setInspectMode",
            json!({ "mode": "none", "highlightConfig": default_highlight_config() }),
            3000,
        );
        let _ = st.session.send("Overlay.hideHighlight", json!({}), 2000);
        st.session.close();
    }
    Ok(json!({ "ok": true, "disconnected": true }))
}

pub fn status() -> Result<Value, InspectError> {
    let guard = INSPECT.lock();
    match &*guard {
        None => Ok(json!({
            "ok": true,
            "connected": false,
            "picking": false,
            "hasSelection": false,
        })),
        Some(st) => Ok(json!({
            "ok": true,
            "connected": true,
            "picking": st.picking,
            "targetId": st.target_id,
            "targetUrl": st.target_url,
            "documentNodeId": st.document_node_id,
            "hasSelection": st.last_selection.is_some(),
            "selection": st.last_selection.clone(),
        })),
    }
}

fn default_highlight_config() -> Value {
    json!({
        "showInfo": true,
        "showStyles": false,
        "showRulers": false,
        "showAccessibilityInfo": false,
        "showExtensionLines": true,
        "contentColor": { "r": 111, "g": 168, "b": 220, "a": 0.45 },
        "paddingColor": { "r": 147, "g": 196, "b": 125, "a": 0.40 },
        "borderColor": { "r": 255, "g": 229, "b": 153, "a": 0.70 },
        "marginColor": { "r": 246, "g": 178, "b": 107, "a": 0.40 },
        "eventTargetColor": { "r": 255, "g": 100, "b": 100, "a": 0.35 },
        "shapeColor": { "r": 96, "g": 82, "b": 255, "a": 0.8 },
        "shapeColorMargin": { "r": 96, "g": 82, "b": 127, "a": 0.6 },
    })
}

/// Enable / disable real-window element pick (Overlay.setInspectMode).
pub fn set_picking(enabled: bool) -> Result<Value, InspectError> {
    ensure_connected()?;
    let mut guard = INSPECT.lock();
    let st = guard
        .as_mut()
        .ok_or_else(|| InspectError::msg("inspect session not connected"))?;

    if enabled {
        st.session
            .send(
                "Overlay.setInspectMode",
                json!({
                    "mode": "searchForNode",
                    "highlightConfig": default_highlight_config(),
                }),
                8000,
            )
            .map_err(map_err_session)?;
        st.picking = true;
    } else {
        st.session
            .send(
                "Overlay.setInspectMode",
                json!({
                    "mode": "none",
                    "highlightConfig": default_highlight_config(),
                }),
                8000,
            )
            .map_err(map_err_session)?;
        let _ = st.session.send("Overlay.hideHighlight", json!({}), 3000);
        st.picking = false;
    }

    Ok(json!({
        "ok": true,
        "picking": st.picking,
        "targetId": st.target_id,
    }))
}

fn ensure_connected() -> Result<(), InspectError> {
    if INSPECT.lock().is_some() {
        return Ok(());
    }
    drop(INSPECT.lock());
    connect()?;
    Ok(())
}

/// Poll CDP events; if user picked a node, resolve full Elements payload.
pub fn poll(wait_ms: Option<u64>) -> Result<Value, InspectError> {
    ensure_connected()?;
    // Optional yield so the UI poll interval can space CDP work; events are
    // buffered by the session I/O thread regardless.
    let wait_ms = wait_ms.unwrap_or(0).min(400);
    if wait_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
    }

    let mut guard = INSPECT.lock();
    let st = guard
        .as_mut()
        .ok_or_else(|| InspectError::msg("inspect session not connected"))?;

    let events = st.session.poll_events();

    let mut picked_backend: Option<i64> = None;
    let mut event_names = Vec::new();
    for ev in &events {
        let method = ev.get("method").and_then(|m| m.as_str()).unwrap_or("");
        event_names.push(method.to_string());
        if method == "Overlay.inspectNodeRequested" || method == "Overlay.inspectModeCanceled" {
            if method == "Overlay.inspectModeCanceled" {
                st.picking = false;
            }
            if let Some(backend) = ev
                .get("params")
                .and_then(|p| p.get("backendNodeId"))
                .and_then(|v| v.as_i64())
            {
                picked_backend = Some(backend);
            }
        }
    }

    let mut selection = None;
    if let Some(backend_id) = picked_backend {
        // Exit pick mode after one successful pick (Chrome-like)
        let _ = st.session.send(
            "Overlay.setInspectMode",
            json!({
                "mode": "none",
                "highlightConfig": default_highlight_config(),
            }),
            5000,
        );
        st.picking = false;

        match resolve_backend_selection(&st.session, backend_id, st.document_node_id) {
            Ok(payload) => {
                // Highlight selected node
                if let Some(node_id) = payload.get("nodeId").and_then(|v| v.as_i64()) {
                    let _ = highlight_node_inner(&st.session, node_id);
                }
                st.last_selection = Some(payload.clone());
                selection = Some(payload);
            }
            Err(e) => {
                return Ok(json!({
                    "ok": false,
                    "error": e.to_string(),
                    "picking": st.picking,
                    "events": event_names,
                    "selection": st.last_selection,
                }));
            }
        }
    }

    let new_selection = selection.is_some();
    let selection_out = selection.or_else(|| st.last_selection.clone());
    Ok(json!({
        "ok": true,
        "picking": st.picking,
        "events": event_names,
        "selection": selection_out,
        "newSelection": new_selection,
    }))
}

fn resolve_backend_selection(
    session: &CdpSession,
    backend_node_id: i64,
    _document_node_id: i64,
) -> Result<Value, InspectError> {
    // Map backend → frontend nodeId
    let pushed = session
        .send(
            "DOM.pushNodesByBackendIdsToFrontend",
            json!({ "backendNodeIds": [backend_node_id] }),
            8000,
        )
        .map_err(map_err_session)?;
    let node_id = pushed
        .get("nodeIds")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_i64())
        .ok_or_else(|| InspectError::msg("pushNodesByBackendIdsToFrontend returned no nodeId"))?;

    build_selection(session, node_id)
}

fn build_selection(session: &CdpSession, node_id: i64) -> Result<Value, InspectError> {
    let described = session
        .send(
            "DOM.describeNode",
            json!({
                "nodeId": node_id,
                "depth": 1,
                "pierce": true,
            }),
            8000,
        )
        .map_err(map_err_session)?;
    let node = described
        .get("node")
        .cloned()
        .ok_or_else(|| InspectError::msg("describeNode missing node"))?;

    let summary = node_summary(&node);
    let ancestors = build_ancestor_chain(session, &node)?;
    let styles = collect_styles(session, node_id)?;
    let box_model = session
        .send("DOM.getBoxModel", json!({ "nodeId": node_id }), 5000)
        .ok();
    let outer = session
        .send(
            "DOM.getOuterHTML",
            json!({ "nodeId": node_id }),
            8000,
        )
        .ok()
        .and_then(|v| {
            v.get("outerHTML")
                .and_then(|s| s.as_str())
                .map(|s| {
                    if s.len() > 4000 {
                        format!("{}…", &s[..4000])
                    } else {
                        s.to_string()
                    }
                })
        });

    // Sibling-aware tree path: for each ancestor, list shallow children of its parent
    let tree_path = build_tree_path(session, &ancestors, node_id)?;

    Ok(json!({
        "nodeId": node_id,
        "backendNodeId": node.get("backendNodeId"),
        "node": summary,
        "rawNode": prune_node(&node),
        "ancestors": ancestors,
        "treePath": tree_path,
        "styles": styles,
        "boxModel": box_model,
        "outerHTML": outer,
    }))
}

fn prune_node(node: &Value) -> Value {
    // Keep a compact view for JSON dump
    let mut m = Map::new();
    for key in [
        "nodeId",
        "backendNodeId",
        "nodeType",
        "nodeName",
        "localName",
        "nodeValue",
        "childNodeCount",
        "attributes",
        "shadowRootType",
    ] {
        if let Some(v) = node.get(key) {
            m.insert(key.to_string(), v.clone());
        }
    }
    Value::Object(m)
}

fn attrs_map(node: &Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(arr) = node.get("attributes").and_then(|a| a.as_array()) {
        let mut i = 0;
        while i + 1 < arr.len() {
            let k = arr[i].as_str().unwrap_or("").to_string();
            let v = arr[i + 1].as_str().unwrap_or("").to_string();
            if !k.is_empty() {
                map.insert(k, v);
            }
            i += 2;
        }
    }
    map
}

fn node_summary(node: &Value) -> Value {
    let attrs = attrs_map(node);
    let name = node
        .get("nodeName")
        .and_then(|v| v.as_str())
        .unwrap_or("#node")
        .to_lowercase();
    let id_attr = attrs.get("id").cloned();
    let class_attr = attrs.get("class").cloned();
    let testid = attrs
        .get("data-testid")
        .cloned()
        .or_else(|| attrs.get("data-test-id").cloned());
    let role = attrs.get("role").cloned();
    let label = format_node_label(&name, &attrs);

    json!({
        "nodeId": node.get("nodeId"),
        "backendNodeId": node.get("backendNodeId"),
        "nodeType": node.get("nodeType"),
        "nodeName": node.get("nodeName"),
        "localName": name,
        "childNodeCount": node.get("childNodeCount").and_then(|v| v.as_i64()).unwrap_or(0),
        "id": id_attr,
        "className": class_attr,
        "testId": testid,
        "role": role,
        "label": label,
        "attributes": attrs_as_object(&attrs),
        "selectorHint": selector_hint(&name, &attrs),
    })
}

fn attrs_as_object(attrs: &HashMap<String, String>) -> Value {
    let mut m = Map::new();
    for (k, v) in attrs {
        // Cap very long attributes
        let val = if v.len() > 200 {
            format!("{}…", &v[..200])
        } else {
            v.clone()
        };
        m.insert(k.clone(), Value::String(val));
    }
    Value::Object(m)
}

fn format_node_label(name: &str, attrs: &HashMap<String, String>) -> String {
    let mut s = name.to_string();
    if let Some(id) = attrs.get("id") {
        if !id.is_empty() {
            s.push('#');
            s.push_str(id);
        }
    }
    if let Some(class) = attrs.get("class") {
        for part in class.split_whitespace().take(3) {
            if !part.is_empty() {
                s.push('.');
                s.push_str(part);
            }
        }
        if class.split_whitespace().count() > 3 {
            s.push_str("…");
        }
    }
    s
}

fn selector_hint(name: &str, attrs: &HashMap<String, String>) -> String {
    if let Some(id) = attrs.get("id") {
        if !id.is_empty() && !id.contains(' ') {
            return format!("#{id}");
        }
    }
    if let Some(tid) = attrs
        .get("data-testid")
        .or_else(|| attrs.get("data-test-id"))
    {
        if !tid.is_empty() {
            return format!("[data-testid=\"{tid}\"]");
        }
    }
    if let Some(class) = attrs.get("class") {
        let parts: Vec<&str> = class
            .split_whitespace()
            .filter(|c| !c.is_empty() && !c.contains(':') && !c.contains('/'))
            .take(2)
            .collect();
        if !parts.is_empty() {
            return format!("{name}.{}", parts.join("."));
        }
    }
    name.to_string()
}

fn build_ancestor_chain(session: &CdpSession, node: &Value) -> Result<Vec<Value>, InspectError> {
    // Prefer JS parentElement walk (describeNode often omits parentId).
    if let Some(node_id) = node.get("nodeId").and_then(|v| v.as_i64()) {
        if let Ok(chain) = ancestor_chain_via_js(session, node_id) {
            if !chain.is_empty() {
                return Ok(chain);
            }
        }
    }

    // Fallback: walk parentId if present
    let mut chain = Vec::new();
    let mut current = node.clone();
    for _ in 0..40 {
        chain.push(node_summary(&current));
        let parent_id = current.get("parentId").and_then(|v| v.as_i64());
        let Some(pid) = parent_id else { break };
        if pid == 0 {
            break;
        }
        let described = session
            .send(
                "DOM.describeNode",
                json!({ "nodeId": pid, "depth": 0 }),
                5000,
            )
            .map_err(map_err_session)?;
        let Some(parent) = described.get("node").cloned() else {
            break;
        };
        let ptype = parent
            .get("nodeType")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if ptype == 9 {
            chain.push(node_summary(&parent));
            break;
        }
        current = parent;
    }
    chain.reverse();
    Ok(chain)
}

fn ancestor_chain_via_js(session: &CdpSession, node_id: i64) -> Result<Vec<Value>, InspectError> {
    let resolved = session
        .send("DOM.resolveNode", json!({ "nodeId": node_id }), 8000)
        .map_err(map_err_session)?;
    let object_id = resolved
        .get("object")
        .and_then(|o| o.get("objectId"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| InspectError::msg("resolveNode missing objectId"))?
        .to_string();

    // r## so JS string fragments like "#" + n.id do not terminate the raw string.
    let function_declaration = r##"function() {
  const out = [];
  let n = this;
  let guard = 0;
  while (n && guard++ < 48) {
    if (n.nodeType === 1) {
      const attrs = {};
      if (n.attributes) {
        for (const a of n.attributes) attrs[a.name] = a.value;
      }
      const tag = (n.tagName || "").toLowerCase();
      let label = tag;
      if (n.id) label += "#" + n.id;
      if (n.classList && n.classList.length) {
        const cls = Array.from(n.classList).slice(0, 3);
        label += "." + cls.join(".");
        if (n.classList.length > 3) label += "...";
      }
      let selectorHint = tag;
      if (n.id) selectorHint = "#" + CSS.escape(n.id);
      else if (n.getAttribute && n.getAttribute("data-testid"))
        selectorHint = '[data-testid="' + n.getAttribute("data-testid") + '"]';
      else if (n.classList && n.classList.length) {
        const safe = Array.from(n.classList).filter(c => c && !c.includes(":") && !c.includes("/")).slice(0, 2);
        if (safe.length) selectorHint = tag + "." + safe.map(c => CSS.escape(c)).join(".");
      }
      out.push({
        nodeType: 1,
        localName: tag,
        nodeName: n.tagName,
        id: n.id || null,
        className: n.className && typeof n.className === "string" ? n.className : null,
        testId: n.getAttribute ? n.getAttribute("data-testid") : null,
        role: n.getAttribute ? n.getAttribute("role") : null,
        label: label,
        selectorHint: selectorHint,
        attributes: attrs,
        childNodeCount: n.childElementCount || 0,
      });
    } else if (n.nodeType === 9) {
      out.push({
        nodeType: 9,
        localName: "#document",
        nodeName: "#document",
        label: "#document",
        selectorHint: "document",
        attributes: {},
        childNodeCount: 1,
      });
      break;
    }
    n = n.parentNode;
  }
  return out.reverse();
}"##;

    let result = session
        .send(
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": function_declaration,
                "returnByValue": true,
                "awaitPromise": false,
            }),
            8000,
        )
        .map_err(map_err_session)?;

    if let Some(details) = result.get("exceptionDetails") {
        let t = details
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("callFunctionOn failed");
        return Err(InspectError::msg(t.to_string()));
    }

    let arr = result
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let len = arr.len();
    let mut out = Vec::with_capacity(len);
    for (i, item) in arr.into_iter().enumerate() {
        let mut obj = item.as_object().cloned().unwrap_or_default();
        // Only the selected leaf is guaranteed to have a frontend nodeId
        if i + 1 == len {
            obj.insert("nodeId".into(), json!(node_id));
        }
        out.push(Value::Object(obj));
    }
    Ok(out)
}

fn build_tree_path(
    session: &CdpSession,
    ancestors: &[Value],
    selected_id: i64,
) -> Result<Vec<Value>, InspectError> {
    // For each ancestor that has a nodeId, fetch shallow children so the UI can render a path tree
    let mut levels = Vec::new();
    for anc in ancestors {
        let Some(nid) = anc.get("nodeId").and_then(|v| v.as_i64()) else {
            continue;
        };
        let children = request_children(session, nid, 1)?;
        levels.push(json!({
            "nodeId": nid,
            "label": anc.get("label"),
            "selected": nid == selected_id,
            "children": children,
        }));
    }
    Ok(levels)
}

fn request_children(session: &CdpSession, node_id: i64, depth: i64) -> Result<Vec<Value>, InspectError> {
    // describeNode with depth returns children embedded
    let described = session
        .send(
            "DOM.describeNode",
            json!({
                "nodeId": node_id,
                "depth": depth,
                "pierce": true,
            }),
            8000,
        )
        .map_err(map_err_session)?;
    let node = described
        .get("node")
        .cloned()
        .ok_or_else(|| InspectError::msg("describeNode missing node for children"))?;
    let mut out = Vec::new();
    if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
        for ch in children {
            let ntype = ch.get("nodeType").and_then(|v| v.as_i64()).unwrap_or(0);
            // 1 element, 3 text (only non-empty), 9 document, 11 document fragment
            if ntype == 3 {
                let text = ch.get("nodeValue").and_then(|v| v.as_str()).unwrap_or("");
                if text.trim().is_empty() {
                    continue;
                }
            }
            if ntype == 1 || ntype == 3 || ntype == 9 || ntype == 11 {
                out.push(node_summary(ch));
            }
            // include open shadow roots lightly
            if let Some(shadow) = ch.get("shadowRoots").and_then(|s| s.as_array()) {
                for sr in shadow {
                    let mut sum = node_summary(sr);
                    if let Some(obj) = sum.as_object_mut() {
                        obj.insert("isShadowRoot".into(), Value::Bool(true));
                    }
                    out.push(sum);
                }
            }
        }
    }
    // Cap siblings for UI performance
    if out.len() > 80 {
        out.truncate(80);
    }
    Ok(out)
}

/// Expand children of a tree node (for Elements panel).
pub fn get_children(node_id: i64) -> Result<Value, InspectError> {
    ensure_connected()?;
    let guard = INSPECT.lock();
    let st = guard
        .as_ref()
        .ok_or_else(|| InspectError::msg("inspect session not connected"))?;
    let children = request_children(&st.session, node_id, 1)?;
    Ok(json!({
        "ok": true,
        "nodeId": node_id,
        "children": children,
    }))
}

/// Select a node from the tree (highlight + full styles).
pub fn select_node(node_id: i64) -> Result<Value, InspectError> {
    ensure_connected()?;
    let mut guard = INSPECT.lock();
    let st = guard
        .as_mut()
        .ok_or_else(|| InspectError::msg("inspect session not connected"))?;
    let payload = build_selection(&st.session, node_id)?;
    let _ = highlight_node_inner(&st.session, node_id);
    st.last_selection = Some(payload.clone());
    Ok(json!({
        "ok": true,
        "selection": payload,
    }))
}

pub fn highlight(node_id: i64) -> Result<Value, InspectError> {
    ensure_connected()?;
    let guard = INSPECT.lock();
    let st = guard
        .as_ref()
        .ok_or_else(|| InspectError::msg("inspect session not connected"))?;
    highlight_node_inner(&st.session, node_id)?;
    Ok(json!({ "ok": true, "nodeId": node_id }))
}

fn highlight_node_inner(session: &CdpSession, node_id: i64) -> Result<(), InspectError> {
    session
        .send(
            "Overlay.highlightNode",
            json!({
                "nodeId": node_id,
                "highlightConfig": default_highlight_config(),
            }),
            5000,
        )
        .map_err(map_err_session)?;
    Ok(())
}

fn collect_styles(session: &CdpSession, node_id: i64) -> Result<Value, InspectError> {
    let matched = session
        .send(
            "CSS.getMatchedStylesForNode",
            json!({ "nodeId": node_id }),
            10000,
        )
        .map_err(map_err_session)?;

    let computed_raw = session
        .send(
            "CSS.getComputedStyleForNode",
            json!({ "nodeId": node_id }),
            10000,
        )
        .map_err(map_err_session)?;

    let computed_all = computed_raw
        .get("computedStyle")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();

    let mut computed_map = Map::new();
    let mut computed_key_set: HashMap<String, String> = HashMap::new();
    for item in &computed_all {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let value = item
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !name.is_empty() {
            computed_key_set.insert(name, value);
        }
    }
    for key in COMPUTED_KEYS {
        if let Some(v) = computed_key_set.get(*key) {
            computed_map.insert((*key).to_string(), Value::String(v.clone()));
        }
    }

    // Matched rules (author stylesheets)
    let mut rules_out = Vec::new();
    if let Some(rules) = matched
        .get("matchedCSSRules")
        .and_then(|a| a.as_array())
    {
        for rule_match in rules.iter().rev().take(40) {
            // reverse: most specific-ish last in CDP often; show cascade end first
            if let Some(formatted) = format_rule_match(rule_match) {
                rules_out.push(formatted);
            }
        }
        rules_out.reverse();
    }

    // Inline style
    let inline = matched.get("inlineStyle").cloned().unwrap_or(Value::Null);
    let inline_fmt = format_style_body(&inline);

    // Attribute style (style="")
    let attr_style = matched
        .get("attributesStyle")
        .cloned()
        .unwrap_or(Value::Null);

    Ok(json!({
        "inline": inline_fmt,
        "attributesStyle": format_style_body(&attr_style),
        "matchedRules": rules_out,
        "computed": Value::Object(computed_map),
        "computedCount": computed_all.len(),
    }))
}

fn format_rule_match(rule_match: &Value) -> Option<Value> {
    let rule = rule_match.get("rule")?;
    let style_sheet_id = rule
        .get("styleSheetId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let origin = rule.get("origin").and_then(|v| v.as_str()).unwrap_or("");
    // Skip user-agent bulk unless needed
    if origin == "user-agent" {
        return None;
    }
    let selector = rule
        .get("selectorList")
        .and_then(|s| s.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("(unknown)")
        .to_string();
    let style = rule.get("style").cloned().unwrap_or(Value::Null);
    let body = format_style_body(&style);
    let source = rule
        .get("style")
        .and_then(|s| s.get("range"))
        .cloned()
        .unwrap_or(Value::Null);
    Some(json!({
        "selector": selector,
        "origin": origin,
        "styleSheetId": style_sheet_id,
        "cssText": body.get("cssText"),
        "properties": body.get("properties"),
        "range": source,
    }))
}

fn format_style_body(style: &Value) -> Value {
    if style.is_null() {
        return json!({ "cssText": "", "properties": [] });
    }
    let css_text = style
        .get("cssText")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut properties = Vec::new();
    if let Some(props) = style.get("cssProperties").and_then(|a| a.as_array()) {
        for p in props {
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() || name.starts_with("--") && name.len() > 40 {
                // keep CSS vars but skip empty
            }
            if name.is_empty() {
                continue;
            }
            let value = p.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let disabled = p
                .get("disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let implicit = p
                .get("implicit")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if implicit {
                continue;
            }
            properties.push(json!({
                "name": name,
                "value": value,
                "disabled": disabled,
            }));
        }
    }
    let display_text = if !css_text.is_empty() {
        css_text
    } else {
        properties
            .iter()
            .filter_map(|p| {
                let n = p.get("name")?.as_str()?;
                let v = p.get("value")?.as_str()?;
                if p.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false) {
                    return None;
                }
                Some(format!("{n}: {v};"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    json!({
        "cssText": display_text,
        "properties": properties,
    })
}

/// Document root + first-level children for initial Elements tree.
pub fn get_document_tree(depth: Option<i64>) -> Result<Value, InspectError> {
    ensure_connected()?;
    let depth = depth.unwrap_or(2).clamp(1, 4);
    let mut guard = INSPECT.lock();
    let st = guard
        .as_mut()
        .ok_or_else(|| InspectError::msg("inspect session not connected"))?;

    let doc = st
        .session
        .send(
            "DOM.getDocument",
            json!({ "depth": depth, "pierce": true }),
            12000,
        )
        .map_err(map_err_session)?;
    let root = doc
        .get("root")
        .cloned()
        .ok_or_else(|| InspectError::msg("DOM.getDocument missing root"))?;
    if let Some(id) = root.get("nodeId").and_then(|v| v.as_i64()) {
        st.document_node_id = id;
    }
    let tree = summarize_tree_node(&root, 0, depth as usize);
    Ok(json!({
        "ok": true,
        "documentNodeId": st.document_node_id,
        "root": tree,
    }))
}

fn summarize_tree_node(node: &Value, depth: usize, max_depth: usize) -> Value {
    let mut summary = node_summary(node);
    let mut children_out = Vec::new();
    if depth < max_depth {
        if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
            for ch in children {
                let ntype = ch.get("nodeType").and_then(|v| v.as_i64()).unwrap_or(0);
                if ntype == 3 {
                    let text = ch.get("nodeValue").and_then(|v| v.as_str()).unwrap_or("");
                    if text.trim().is_empty() {
                        continue;
                    }
                }
                if ntype == 1 || ntype == 3 || ntype == 9 || ntype == 11 {
                    children_out.push(summarize_tree_node(ch, depth + 1, max_depth));
                }
            }
        }
    }
    if let Some(obj) = summary.as_object_mut() {
        obj.insert("children".into(), Value::Array(children_out));
        obj.insert(
            "hasChildren".into(),
            Value::Bool(
                node.get("childNodeCount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
                    > 0
                    || node
                        .get("children")
                        .and_then(|c| c.as_array())
                        .map(|a| !a.is_empty())
                        .unwrap_or(false),
            ),
        );
    }
    summary
}
