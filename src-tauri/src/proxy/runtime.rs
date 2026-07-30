//! Process-wide proxy runtime (start/stop, status, takeover orchestration).

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use once_cell::sync::Lazy;
use tokio::sync::oneshot;

use super::circuit::CircuitRegistry;
use super::server;
use super::takeover;
use crate::providers::models::{
    AppKind, AppProviderStore, GlobalProxyConfig, ProxyActiveTarget, ProxyRuntimeStatus, Provider,
};
use crate::providers::store;

static RUNTIME: Lazy<Arc<ProxyRuntime>> = Lazy::new(|| Arc::new(ProxyRuntime::new()));

pub fn runtime() -> Arc<ProxyRuntime> {
    RUNTIME.clone()
}

pub fn proxy_status_snapshot() -> ProxyRuntimeStatus {
    RUNTIME.status_snapshot()
}

struct ServerSlot {
    shutdown: Option<oneshot::Sender<()>>,
    cfg: GlobalProxyConfig,
    started_at: Instant,
}

pub struct ProxyRuntime {
    server: Mutex<Option<ServerSlot>>,
    circuits: CircuitRegistry,
    active_connections: AtomicUsize,
    total_requests: AtomicU64,
    success_requests: AtomicU64,
    failed_requests: AtomicU64,
    failover_count: AtomicU64,
    last_error: Mutex<Option<String>>,
    active_targets: RwLock<Vec<ProxyActiveTarget>>,
    starting: AtomicBool,
}

impl ProxyRuntime {
    fn new() -> Self {
        Self {
            server: Mutex::new(None),
            circuits: CircuitRegistry::default(),
            active_connections: AtomicUsize::new(0),
            total_requests: AtomicU64::new(0),
            success_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            failover_count: AtomicU64::new(0),
            last_error: Mutex::new(None),
            active_targets: RwLock::new(Vec::new()),
            starting: AtomicBool::new(false),
        }
    }

    pub fn circuits(&self) -> &CircuitRegistry {
        &self.circuits
    }

    pub fn is_running(&self) -> bool {
        self.server
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    pub fn status_snapshot(&self) -> ProxyRuntimeStatus {
        let guard = self.server.lock().unwrap_or_else(|e| e.into_inner());
        let (running, address, port, uptime) = if let Some(slot) = guard.as_ref() {
            (
                true,
                takeover::proxy_connect_host(&slot.cfg),
                slot.cfg.listen_port,
                slot.started_at.elapsed().as_secs(),
            )
        } else {
            let cfg = store::load().map(|f| f.proxy).unwrap_or_default();
            (false, takeover::proxy_connect_host(&cfg), cfg.listen_port, 0)
        };
        drop(guard);

        let total = self.total_requests.load(Ordering::Relaxed);
        let ok = self.success_requests.load(Ordering::Relaxed);
        let success_rate = if total == 0 {
            100.0
        } else {
            (ok as f32 / total as f32) * 100.0
        };
        let targets = self
            .active_targets
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let last_error = self
            .last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        ProxyRuntimeStatus {
            running,
            address,
            port,
            active_connections: self.active_connections.load(Ordering::Relaxed),
            total_requests: total,
            success_requests: ok,
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
            success_rate,
            uptime_seconds: uptime,
            failover_count: self.failover_count.load(Ordering::Relaxed),
            active_targets: targets,
            last_error,
        }
    }

    pub fn begin_request(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn end_request(&self, success: bool) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
        if success {
            self.success_requests.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_requests.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn note_success(&self, app: &AppKind, provider: &Provider) {
        let mut targets = self
            .active_targets
            .write()
            .unwrap_or_else(|e| e.into_inner());
        targets.retain(|t| t.app_type != app.as_str());
        targets.push(ProxyActiveTarget {
            app_type: app.as_str().into(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
        });
    }

    pub fn note_failure(&self, msg: &str) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(msg.chars().take(400).collect());
    }

    pub fn note_failover(&self) {
        self.failover_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn start_server_blocking(&self) -> Result<GlobalProxyConfig, String> {
        if self.is_running() {
            let cfg = store::load()?.proxy;
            return Ok(cfg);
        }
        if self
            .starting
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            // Another start in progress
            std::thread::sleep(std::time::Duration::from_millis(200));
            if self.is_running() {
                return Ok(store::load()?.proxy);
            }
        }
        let result = self.start_server_inner();
        self.starting.store(false, Ordering::SeqCst);
        result
    }

    fn start_server_inner(&self) -> Result<GlobalProxyConfig, String> {
        let cfg = store::load()?.proxy;
        let (tx, rx) = oneshot::channel::<()>();
        let runtime = RUNTIME.clone();
        let cfg_clone = cfg.clone();

        tauri::async_runtime::spawn(async move {
            if let Err(e) = server::run_server(runtime.clone(), cfg_clone, rx).await {
                eprintln!("[proxy] server error: {e}");
                *runtime
                    .last_error
                    .lock()
                    .unwrap_or_else(|err| err.into_inner()) = Some(e);
            }
            let mut slot = runtime.server.lock().unwrap_or_else(|e| e.into_inner());
            *slot = None;
        });

        // Brief wait for bind
        std::thread::sleep(std::time::Duration::from_millis(120));

        let mut guard = self.server.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(ServerSlot {
            shutdown: Some(tx),
            cfg: cfg.clone(),
            started_at: Instant::now(),
        });
        Ok(cfg)
    }

    pub fn stop_server_blocking(&self) -> Result<(), String> {
        let mut guard = self.server.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut slot) = guard.take() {
            if let Some(tx) = slot.shutdown.take() {
                let _ = tx.send(());
            }
        }
        // Give the accept loop a moment to exit
        std::thread::sleep(std::time::Duration::from_millis(80));
        Ok(())
    }

    /// Enable/disable takeover for one app.
    pub fn set_takeover(&self, kind: AppKind, enabled: bool) -> Result<Vec<String>, String> {
        let mut warnings = Vec::new();
        let mut file = store::load()?;
        let cfg = file.proxy.clone();
        let other_on = match kind {
            AppKind::Codex => file.grok.takeover_enabled,
            AppKind::Grok => file.codex.takeover_enabled,
        };
        let (current_id, already_on, auto_fo) = {
            let s = file.for_kind(kind);
            (s.current.clone(), s.takeover_enabled, s.auto_failover_enabled)
        };

        if enabled {
            if current_id.is_empty() {
                return Err("请先启用一个供应商，再打开本地路由".into());
            }
            let provider = file
                .for_kind(kind)
                .providers
                .iter()
                .find(|p| p.id == current_id)
                .cloned()
                .ok_or_else(|| "当前供应商不存在".to_string())?;

            if kind == AppKind::Grok && provider.is_official() {
                return Err("请先切换到第三方 Grok 供应商，再开启本地路由".into());
            }

            self.start_server_blocking()?;

            if !already_on {
                takeover::backup_live(kind)?;
            }
            // Apply live BEFORE setting flag (transactional).
            let w = takeover::apply_takeover_live(kind, &provider, &cfg).map_err(|e| {
                if !other_on {
                    let _ = self.stop_server_blocking();
                }
                e
            })?;
            warnings.extend(w);
            {
                let app_store = file.for_kind_mut(kind);
                app_store.takeover_enabled = true;
                app_store.normalize_failover_order();
                if auto_fo {
                    ensure_in_failover_order(app_store, &provider.id);
                }
            }
            self.note_success(&kind, &provider);
            store::save(&file)?;
        } else {
            if !already_on {
                return Ok(vec!["该应用未处于本地路由接管状态".into()]);
            }
            let provider = file
                .for_kind(kind)
                .providers
                .iter()
                .find(|p| p.id == current_id)
                .cloned();

            // Restore FIRST; only clear flag on success.
            if let Some(ref p) = provider {
                let w = takeover::restore_direct_live(kind, p)?;
                warnings.extend(w);
            } else {
                return Err("无当前供应商，无法安全关闭路由；请先启用一个供应商".into());
            }

            file.for_kind_mut(kind).takeover_enabled = false;
            self.circuits.clear_app(kind.as_str());
            store::save(&file)?;

            {
                let mut targets = self
                    .active_targets
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                targets.retain(|t| t.app_type != kind.as_str());
            }

            let file2 = store::load()?;
            if !file2.codex.takeover_enabled && !file2.grok.takeover_enabled {
                let _ = self.stop_server_blocking();
            }
            warnings.push(format!("已关闭 {} 本地路由并恢复直连配置", kind.display_name()));
        }
        Ok(warnings)
    }

    /// Hot-switch logical current under takeover.
    ///
    /// - **Codex**: minimal live rewrite (model + catalog only; no `model_providers`).
    /// - **Grok**: no live rewrite when already on local proxy — only `current` changes;
    ///   upstream is chosen inside the proxy. Falls back to full takeover if live is
    ///   not yet proxy-shaped.
    pub fn hot_switch_current(&self, kind: AppKind, id: &str) -> Result<Vec<String>, String> {
        let mut file = store::load()?;
        let cfg = file.proxy.clone();
        let app_store = file.for_kind_mut(kind);
        if !app_store.takeover_enabled {
            return Err("未开启本地路由，请使用普通启用".into());
        }
        let provider = app_store
            .providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| format!("供应商不存在: {id}"))?;

        if kind == AppKind::Grok && provider.is_official() {
            return Err("路由模式下不能启用 Grok Official".into());
        }
        if !provider.is_official() {
            let _ = takeover::upstream_from_provider(kind, &provider)?;
        }

        // Live first — only then advance current pointer.
        let w = takeover::hot_switch_live(kind, &provider, &cfg)?;
        app_store.current = id.to_string();
        self.note_success(&kind, &provider);
        store::save(&file)?;
        Ok(w)
    }

    /// Re-assert takeover live for current (fix half-broken state).
    /// Always full [`takeover::apply_takeover_live`] so a missing proxy shell is rebuilt.
    pub fn repair_takeover(&self, kind: AppKind) -> Result<Vec<String>, String> {
        let file = store::load()?;
        let cfg = file.proxy.clone();
        let app = file.for_kind(kind);
        if !app.takeover_enabled {
            return Err("该应用未开启本地路由".into());
        }
        let id = app.current.clone();
        if id.is_empty() {
            return Err("无当前供应商".into());
        }
        let provider = app
            .providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| format!("当前供应商不存在: {id}"))?;
        if kind == AppKind::Grok && provider.is_official() {
            return Err("路由模式下不能启用 Grok Official".into());
        }
        if !provider.is_official() {
            let _ = takeover::upstream_from_provider(kind, &provider)?;
        }
        let w = takeover::apply_takeover_live(kind, &provider, &cfg)?;
        self.note_success(&kind, &provider);
        Ok(w)
    }

    pub fn update_listen_config(&self, cfg: GlobalProxyConfig) -> Result<(), String> {
        if cfg.listen_port == 0 {
            return Err("端口不能为 0".into());
        }
        let egress = super::forwarder::normalize_egress_proxy(&cfg.egress_proxy)?;
        let mut next = cfg;
        next.egress_proxy = egress;

        let mut file = store::load()?;
        if self.is_running() {
            // Listen address/port are bound at server start; egress + logging can hot-update.
            let cur = &file.proxy;
            if cur.listen_address != next.listen_address || cur.listen_port != next.listen_port {
                return Err("请先关闭所有应用的本地路由后再修改监听地址/端口".into());
            }
            file.proxy.enable_logging = next.enable_logging;
            file.proxy.log_retention_days = next.log_retention_days.max(1).min(365);
            file.proxy.egress_proxy = next.egress_proxy;
            // Keep in-memory slot cfg in sync for status / debugging.
            if let Ok(mut guard) = self.server.lock() {
                if let Some(slot) = guard.as_mut() {
                    slot.cfg.enable_logging = file.proxy.enable_logging;
                    slot.cfg.log_retention_days = file.proxy.log_retention_days;
                    slot.cfg.egress_proxy = file.proxy.egress_proxy.clone();
                }
            }
            // Mirror retention into the log DB so prune uses the latest value.
            let _ = super::log_store::set_retention_days(file.proxy.log_retention_days);
        } else {
            next.log_retention_days = next.log_retention_days.max(1).min(365);
            file.proxy = next;
        }
        store::save(&file)?;
        let _ = super::log_store::set_retention_days(file.proxy.log_retention_days);
        Ok(())
    }

    pub fn restore_on_startup(&self) -> Result<(), String> {
        let mut file = store::load()?;
        file.codex.normalize_failover_order();
        file.grok.normalize_failover_order();
        let _ = store::save(&file);

        let need = file.codex.takeover_enabled || file.grok.takeover_enabled;
        if !need {
            return Ok(());
        }
        let _ = self.start_server_blocking()?;
        let cfg = file.proxy.clone();
        for kind in [AppKind::Codex, AppKind::Grok] {
            let app = file.for_kind(kind);
            if !app.takeover_enabled {
                continue;
            }
            if let Some(p) = app.providers.iter().find(|p| p.id == app.current) {
                match takeover::apply_takeover_live(kind, p, &cfg) {
                    Ok(_) => self.note_success(&kind, p),
                    Err(e) => eprintln!("[proxy] re-takeover {} failed: {e}", kind.as_str()),
                }
            }
        }
        Ok(())
    }

    pub fn shutdown_all(&self) -> Result<(), String> {
        let file = store::load()?;
        for kind in [AppKind::Codex, AppKind::Grok] {
            let app = file.for_kind(kind);
            if !app.takeover_enabled {
                continue;
            }
            if let Some(p) = app.providers.iter().find(|x| x.id == app.current) {
                if let Err(e) = takeover::restore_direct_live(kind, p) {
                    eprintln!("[proxy] exit restore {}: {e}", kind.as_str());
                    // Still clear flag on exit to avoid permanent half-state after restart
                }
            }
        }
        if let Ok(mut f) = store::load() {
            f.codex.takeover_enabled = false;
            f.grok.takeover_enabled = false;
            let _ = store::save(&f);
        }
        self.stop_server_blocking()
    }
}

fn ensure_in_failover_order(app_store: &mut AppProviderStore, id: &str) {
    app_store.normalize_failover_order();
    if !app_store.failover_order.iter().any(|x| x == id) {
        app_store.failover_order.push(id.to_string());
    }
    if let Some(p) = app_store.providers.iter_mut().find(|p| p.id == id) {
        p.in_failover_queue = true;
    }
}
