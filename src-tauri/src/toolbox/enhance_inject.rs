//! CDP inject for force-Chinese locale + fast-startup (renderer patches).
//!
//! Pattern mirrors model unlock: inject on host ready / skin inject / keep loop.
//! Scripts are self-contained and idempotent via installation markers.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::Duration;

use super::settings;

const FORCE_CHINESE_SCRIPT: &str = include_str!("resources/force_chinese.js");
const FAST_STARTUP_SCRIPT: &str = include_str!("resources/fast_startup.js");
const PLUGIN_UNLOCK_SCRIPT: &str = include_str!("resources/plugin_unlock.js");

const KEEP_ACTIVE_SECS: u64 = 4;
const KEEP_STABLE_WATCHDOG_SECS: u64 = 90;
const KEEP_PORT_WAIT_SECS: u64 = 8;

static KEEP_STARTED: AtomicBool = AtomicBool::new(false);
static ENHANCE_DESIRED: AtomicBool = AtomicBool::new(false);
static ENHANCE_STABLE: AtomicBool = AtomicBool::new(false);
static KEEP_WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

fn shared_debug_port() -> u16 {
    std::env::var("CODEX_SKIN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|p: &u16| (1024..=65535).contains(p))
        .unwrap_or(crate::cdp::native::SHARED_PORT)
}

fn candidate_debug_ports() -> Vec<u16> {
    let primary = shared_debug_port();
    let mut ports = vec![primary];
    for p in [9335u16, 9222, 9229, 9333, 9334] {
        if !ports.contains(&p) {
            ports.push(p);
        }
    }
    ports
}

fn first_open_debug_port() -> Option<u16> {
    candidate_debug_ports()
        .into_iter()
        .find(|p| crate::cdp::http::is_debug_port_open(*p, 250))
}

/// Whether any enhancement inject is currently wanted (prefs or effective).
fn any_enhance_wanted() -> bool {
    // Always re-sync force-chinese install key (on→off when official / pref off).
    settings::force_chinese_locale()
        || super::gate::force_chinese_effective()
        || settings::fast_startup()
        || settings::plugin_marketplace_unlock()
}

fn build_evaluate_script() -> String {
    // Effective force-chinese: user pref ∧ third-party. Official always gets enabled:false
    // so managed locale can restore when switching back to OpenAI Official.
    let force_effective = super::gate::force_chinese_effective();
    let plugin_effective = super::gate::plugin_marketplace_unlock_effective();
    let third = super::gate::third_party_codex_active();
    let home = crate::sessions::default_codex_home_dir();
    let local_marketplaces = if plugin_effective {
        super::plugin_marketplace::local_plugin_marketplaces_json(&home)
    } else {
        json!([])
    };
    let force = json!({
        "enabled": force_effective,
        "locale": "zh-CN",
        "thirdPartyActive": third,
        "userPref": settings::force_chinese_locale(),
    });
    let fast = json!({
        "enabled": settings::fast_startup(),
        "statsigTimeoutMs": 800,
    });
    let plugin = json!({
        "enabled": plugin_effective,
        "autoExpand": plugin_effective,
        "userPref": settings::plugin_marketplace_unlock(),
        "thirdPartyActive": third,
    });
    format!(
        r#"(function(){{
  try {{
    window.__CHATGPT_TOOLS_FORCE_CHINESE_LOCALE__ = {force};
    window.__CHATGPT_TOOLS_FAST_STARTUP__ = {fast};
    window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACE_UNLOCK__ = {plugin};
    window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACES__ = {markets};
  }} catch (e) {{}}
  var fastResult = null;
  var localeResult = null;
  var pluginResult = null;
  try {{ fastResult = ({fast_body}); }} catch (e) {{ fastResult = {{ ok: false, error: String(e) }}; }}
  try {{ localeResult = ({locale_body}); }} catch (e) {{ localeResult = {{ ok: false, error: String(e) }}; }}
  try {{ pluginResult = ({plugin_body}); }} catch (e) {{ pluginResult = {{ ok: false, error: String(e) }}; }}
  return {{
    ok: true,
    fastStartup: fastResult,
    forceChinese: localeResult,
    pluginMarketplaceUnlock: pluginResult,
    thirdPartyActive: {third_js},
    localMarketplaceCount: Array.isArray(window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACES__)
      ? window.__CHATGPT_TOOLS_PLUGIN_MARKETPLACES__.length : 0
  }};
}})()"#,
        force = serde_json::to_string(&force).unwrap_or_else(|_| "{}".into()),
        fast = serde_json::to_string(&fast).unwrap_or_else(|_| "{}".into()),
        plugin = serde_json::to_string(&plugin).unwrap_or_else(|_| "{}".into()),
        markets = serde_json::to_string(&local_marketplaces).unwrap_or_else(|_| "[]".into()),
        third_js = if third { "true" } else { "false" },
        fast_body = FAST_STARTUP_SCRIPT.trim().trim_end_matches(';'),
        locale_body = FORCE_CHINESE_SCRIPT.trim().trim_end_matches(';'),
        plugin_body = PLUGIN_UNLOCK_SCRIPT.trim().trim_end_matches(';'),
    )
}

fn build_probe_script() -> String {
    // Probe the *effective* force-chinese key (third-party gated) + plugin unlock.
    let force_on = super::gate::force_chinese_effective();
    let plugin_on = super::gate::plugin_marketplace_unlock_effective();
    let fast_on = settings::fast_startup();
    let locale = "zh-CN";
    let force_key = format!("2:{}:{locale}", if force_on { "on" } else { "off" });
    format!(
        r#"(function(){{
  var forceKey = {force_key};
  var forceOk = window.__chatgptToolsForceChineseLocaleInstalled === forceKey;
  var fastWanted = {fast_on};
  var fastOk = !fastWanted || window.__chatgptToolsFastStartupInstalled === "1";
  var pluginWanted = {plugin_on};
  var pluginOk = !pluginWanted || window.__chatgptToolsPluginUnlockInstalled === "cgt-plugin-unlock-1:on";
  return {{
    healthy: forceOk && fastOk && pluginOk,
    forceOk: forceOk,
    fastOk: fastOk,
    pluginOk: pluginOk,
    forceEffective: {force_eff},
    pluginEffective: {plugin_on},
    thirdPartyActive: {third}
  }};
}})()"#,
        force_key = serde_json::to_string(&force_key).unwrap_or_else(|_| "\"\"".into()),
        fast_on = if fast_on { "true" } else { "false" },
        force_eff = if force_on { "true" } else { "false" },
        plugin_on = if plugin_on { "true" } else { "false" },
        third = if super::gate::third_party_codex_active() {
            "true"
        } else {
            "false"
        },
    )
}

fn probe_page_healthy(port: u16) -> bool {
    let Ok(targets) = crate::cdp::http::list_app_targets(port) else {
        return false;
    };
    if targets.is_empty() {
        return false;
    }
    let script = build_probe_script();
    let mut any = false;
    let mut all_ok = true;
    for target in &targets {
        let Ok(session) = crate::cdp::session::CdpSession::open(target, port, 4000) else {
            all_ok = false;
            continue;
        };
        match session.evaluate(&script, 6_000) {
            Ok(v) => {
                any = true;
                if !v
                    .get("healthy")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false)
                {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
        session.close();
    }
    any && all_ok
}

fn inject_on_port(port: u16) -> Result<Value, String> {
    let targets = crate::cdp::http::list_app_targets(port).map_err(|e| e.to_string())?;
    if targets.is_empty() {
        return Err("无 app:// 页面目标（Codex 可能仍在加载）".into());
    }
    let script = build_evaluate_script();
    let mut ok_count = 0u32;
    let mut last_err = String::new();
    let mut last = Value::Null;
    for target in &targets {
        match crate::cdp::session::CdpSession::open(target, port, 6000) {
            Ok(session) => {
                match session.evaluate(&script, 12_000) {
                    Ok(v) => {
                        ok_count += 1;
                        last = v;
                    }
                    Err(e) => last_err = e.to_string(),
                }
                session.close();
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    if ok_count == 0 {
        return Err(if last_err.is_empty() {
            "enhance evaluate 未成功".into()
        } else {
            last_err
        });
    }
    Ok(json!({
        "ok": true,
        "targets": ok_count,
        "result": last,
    }))
}

/// Inject into an already-open CDP session (skin inject path).
pub fn try_inject_into_session(session: &crate::cdp::session::CdpSession) -> Result<Value, String> {
    // Always inject force-chinese config: enabled follows third-party gate;
    // enabled:false restores managed locale when official / pref off.
    let _ = any_enhance_wanted();
    let script = build_evaluate_script();
    session
        .evaluate(&script, 12_000)
        .map_err(|e| e.to_string())
}

/// Best-effort inject on open debug ports (host ready / settings change).
pub fn try_inject_now() -> Result<Value, String> {
    mark_desired();
    let Some(port) = first_open_debug_port() else {
        return Ok(json!({
            "ok": false,
            "queued": true,
            "message": "未检测到 Codex 调试端口，已排队；启动客户端后自动注入",
        }));
    };
    if probe_page_healthy(port) {
        ENHANCE_STABLE.store(true, Ordering::SeqCst);
        return Ok(json!({ "ok": true, "skipped": true, "healthy": true }));
    }
    match inject_on_port(port) {
        Ok(v) => {
            ENHANCE_STABLE.store(true, Ordering::SeqCst);
            Ok(v)
        }
        Err(e) => {
            ENHANCE_STABLE.store(false, Ordering::SeqCst);
            wake_keep();
            Err(e)
        }
    }
}

fn mark_desired() {
    ENHANCE_DESIRED.store(true, Ordering::SeqCst);
    ENHANCE_STABLE.store(false, Ordering::SeqCst);
    ensure_keep_thread();
    wake_keep();
}

fn wake_keep() {
    if let Ok(mut g) = KEEP_WAKE.0.lock() {
        *g = true;
        KEEP_WAKE.1.notify_all();
    }
}

fn ensure_keep_thread() {
    if KEEP_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    thread::Builder::new()
        .name("cgt-toolbox-enhance".into())
        .spawn(|| keep_loop())
        .ok();
}

fn keep_loop() {
    loop {
        let desired = ENHANCE_DESIRED.load(Ordering::SeqCst);
        let stable = ENHANCE_STABLE.load(Ordering::SeqCst);
        // Always maintain force-chinese install key (enabled or off→restore path).
        // Fast-startup marker only required when enabled. `desired` forces active retries.

        let sleep_secs = if first_open_debug_port().is_none() {
            KEEP_PORT_WAIT_SECS
        } else if stable && !desired {
            KEEP_STABLE_WATCHDOG_SECS
        } else {
            KEEP_ACTIVE_SECS
        };

        {
            let Ok(guard) = KEEP_WAKE.0.lock() else {
                thread::sleep(Duration::from_secs(sleep_secs));
                continue;
            };
            let (mut g, _timeout) = KEEP_WAKE
                .1
                .wait_timeout(guard, Duration::from_secs(sleep_secs))
                .unwrap_or_else(|e| e.into_inner());
            *g = false;
        }

        // Always try to keep force-chinese install key current (including off).
        let Some(port) = first_open_debug_port() else {
            ENHANCE_STABLE.store(false, Ordering::SeqCst);
            continue;
        };

        if probe_page_healthy(port) {
            ENHANCE_DESIRED.store(false, Ordering::SeqCst);
            ENHANCE_STABLE.store(true, Ordering::SeqCst);
            continue;
        }

        ENHANCE_STABLE.store(false, Ordering::SeqCst);
        match inject_on_port(port) {
            Ok(_) => {
                ENHANCE_DESIRED.store(false, Ordering::SeqCst);
                ENHANCE_STABLE.store(true, Ordering::SeqCst);
            }
            Err(_) => ENHANCE_STABLE.store(false, Ordering::SeqCst),
        }
    }
}

/// Called when Codex host becomes ready (debug port up).
pub fn on_host_ready() {
    // Arm enhance keep so enabled toggles apply once the page is injectable.
    ENHANCE_DESIRED.store(true, Ordering::SeqCst);
    ENHANCE_STABLE.store(false, Ordering::SeqCst);
    ensure_keep_thread();
    wake_keep();
    // Host-ready is already off the critical GUI path; still prefer non-block.
    let _ = thread::Builder::new()
        .name("cgt-enhance-host".into())
        .spawn(|| {
            let _ = try_inject_now();
        });
}

/// Settings / provider gate changed — arm keep loop (non-blocking).
///
/// Does **not** run CDP on the caller thread: provider switch and toolbox
/// toggles must stay snappy. The enhance keep thread injects within a few
/// seconds once the debug port is up.
pub fn on_settings_changed() {
    ENHANCE_DESIRED.store(true, Ordering::SeqCst);
    ENHANCE_STABLE.store(false, Ordering::SeqCst);
    ensure_keep_thread();
    wake_keep();
    // Optional best-effort immediate inject off-thread (keep loop is the SSOT).
    let _ = thread::Builder::new()
        .name("cgt-enhance-once".into())
        .spawn(|| {
            let _ = try_inject_now();
        });
}

/// Chromium `--host-resolver-rules` for Statsig fast-fail (launch args).
pub fn statsig_fast_fail_host_resolver_rule() -> String {
    [
        "--host-resolver-rules=MAP ab.chatgpt.com 127.0.0.1",
        "MAP featureassets.org 127.0.0.1",
        "MAP prodregistryv2.org 127.0.0.1",
        "MAP api.statsigcdn.com 127.0.0.1",
        "MAP statsigapi.net 127.0.0.1",
        "MAP cloudflare-dns.com 127.0.0.1",
    ]
    .join(",")
}

/// Extra Chromium args when launching Codex (fast startup).
pub fn extra_launch_args() -> Vec<String> {
    if !settings::fast_startup() {
        return Vec::new();
    }
    vec![statsig_fast_fail_host_resolver_rule()]
}
