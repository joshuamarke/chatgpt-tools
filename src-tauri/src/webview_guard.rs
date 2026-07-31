//! WebView inspect policy:
//! - **Debug** (`tauri dev` / debug binary): allow right-click menus + F12 / DevTools
//! - **Release** (packaged): block browser-like right-click and DevTools hotkeys
//!
//! Does **not** touch Skin DevTools / host CDP inject — only this app's GUI WebView.
//! Release-only and debug-only symbols are `cfg`-gated so each profile compiles cleanly.

use tauri::Manager;

/// Early init script: runs before page scripts (plugin `js_init_script`).
/// Capture-phase listeners so app handlers still work; we only suppress defaults.
#[cfg(not(debug_assertions))]
const RELEASE_WEBVIEW_GUARD_JS: &str = r#"
try {
  document.addEventListener('contextmenu', function (e) {
    e.preventDefault();
  }, true);

  document.addEventListener('keydown', function (e) {
    var key = e.key;
    var code = e.code;
    // F12 → Chromium / WebView2 DevTools
    if (key === 'F12' || code === 'F12') {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    // Ctrl+Shift+I / J / C (Windows · Linux)
    if (e.ctrlKey && e.shiftKey && (
      code === 'KeyI' || code === 'KeyJ' || code === 'KeyC' ||
      key === 'I' || key === 'i' || key === 'J' || key === 'j' || key === 'C' || key === 'c'
    )) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    // Cmd+Option+I (macOS DevTools)
    if (e.metaKey && e.altKey && (code === 'KeyI' || key === 'I' || key === 'i')) {
      e.preventDefault();
      e.stopPropagation();
    }
  }, true);
} catch (_) {}
"#;

/// Plugin that injects the **release** guard into every WebView (main + secondary).
#[cfg(not(debug_assertions))]
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("release-webview-guard")
        .js_init_script(RELEASE_WEBVIEW_GUARD_JS)
        .build()
}

/// Apply native WebView2 settings (Windows). No-op on other platforms.
#[allow(unused_variables)]
fn set_webview2_inspect(window: &tauri::WebviewWindow, enabled: bool) {
    #[cfg(windows)]
    {
        let _ = window.with_webview(move |webview| {
            unsafe {
                let controller = webview.controller();
                if let Ok(core) = controller.CoreWebView2() {
                    if let Ok(settings) = core.Settings() {
                        let _ = settings.SetAreDefaultContextMenusEnabled(enabled);
                        let _ = settings.SetAreDevToolsEnabled(enabled);
                    }
                }
            }
        });
    }
}

/// Release: native off for default menus + DevTools.
#[cfg(not(debug_assertions))]
pub fn harden_window(window: &tauri::WebviewWindow) {
    set_webview2_inspect(window, false);
}

/// Debug: native on so right-click + F12 work in `tauri dev`.
#[cfg(debug_assertions)]
pub fn enable_dev_inspect_window(window: &tauri::WebviewWindow) {
    set_webview2_inspect(window, true);
}

/// Harden every currently open WebView window (release).
#[cfg(not(debug_assertions))]
pub fn harden_all(app: &tauri::AppHandle) {
    for (_, window) in app.webview_windows() {
        harden_window(&window);
    }
}

/// Enable inspect on every open WebView (debug / `tauri dev`).
#[cfg(debug_assertions)]
pub fn enable_dev_inspect_all(app: &tauri::AppHandle) {
    for (_, window) in app.webview_windows() {
        enable_dev_inspect_window(&window);
    }
}
