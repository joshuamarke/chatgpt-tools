//! Release-only WebView hardening: block browser-like right-click menus and
//! DevTools hotkeys (F12 / Ctrl+Shift+I|J|C / Cmd+Option+I).
//!
//! Enabled only when `not(debug_assertions)` so `tauri dev` keeps full inspect.
//! This does **not** touch Skin DevTools / CDP inject — only the GUI WebView chrome.
//!
//! Layers:
//! 1. Early JS init (all platforms) — suppress `contextmenu` + DevTools shortcuts
//! 2. Windows WebView2 settings — native off for default menus + DevTools

use tauri::Manager;

/// Early init script: runs before page scripts (plugin `js_init_script`).
/// Capture-phase listeners so app handlers still work; we only suppress defaults.
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

/// Plugin that injects the guard into every WebView (main + secondary).
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("release-webview-guard")
        .js_init_script(RELEASE_WEBVIEW_GUARD_JS)
        .build()
}

/// Apply native WebView2 settings (Windows). No-op on other platforms.
/// Call after a window exists (setup + any late-created window).
#[allow(unused_variables)]
pub fn harden_window(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        let _ = window.with_webview(|webview| {
            // PlatformWebview::controller is WebView2-only; disable menus + inspector natively.
            unsafe {
                let controller = webview.controller();
                if let Ok(core) = controller.CoreWebView2() {
                    if let Ok(settings) = core.Settings() {
                        let _ = settings.SetAreDefaultContextMenusEnabled(false);
                        let _ = settings.SetAreDevToolsEnabled(false);
                    }
                }
            }
        });
    }
}

/// Harden every currently open WebView window.
pub fn harden_all(app: &tauri::AppHandle) {
    for (_, window) in app.webview_windows() {
        harden_window(&window);
    }
}
