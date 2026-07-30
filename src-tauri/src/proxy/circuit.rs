//! Per-provider circuit breaker (Closed / Open / HalfOpen).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::providers::models::CircuitConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone)]
struct Breaker {
    state: CircuitState,
    consecutive_failures: u32,
    consecutive_successes: u32,
    total: u32,
    failures: u32,
    opened_at: Option<Instant>,
    half_open_in_flight: bool,
    config: CircuitConfig,
}

impl Breaker {
    fn new(config: CircuitConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            consecutive_successes: 0,
            total: 0,
            failures: 0,
            opened_at: None,
            half_open_in_flight: false,
            config,
        }
    }

    fn health_label(&self) -> &'static str {
        match self.state {
            CircuitState::Open => "open",
            CircuitState::HalfOpen => "degraded",
            CircuitState::Closed if self.consecutive_failures == 0 => "healthy",
            CircuitState::Closed => "degraded",
        }
    }

    fn maybe_transition_from_open(&mut self) {
        if self.state != CircuitState::Open {
            return;
        }
        let Some(opened) = self.opened_at else {
            self.state = CircuitState::HalfOpen;
            return;
        };
        if opened.elapsed() >= Duration::from_secs(self.config.timeout_seconds.max(1)) {
            self.state = CircuitState::HalfOpen;
            self.half_open_in_flight = false;
            self.consecutive_successes = 0;
        }
    }

    fn allow(&mut self) -> bool {
        self.maybe_transition_from_open();
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => false,
            CircuitState::HalfOpen => {
                if self.half_open_in_flight {
                    false
                } else {
                    self.half_open_in_flight = true;
                    true
                }
            }
        }
    }

    fn record_success(&mut self) {
        self.total = self.total.saturating_add(1);
        self.consecutive_failures = 0;
        match self.state {
            CircuitState::HalfOpen => {
                self.half_open_in_flight = false;
                self.consecutive_successes = self.consecutive_successes.saturating_add(1);
                if self.consecutive_successes >= self.config.success_threshold.max(1) {
                    self.state = CircuitState::Closed;
                    self.consecutive_successes = 0;
                    self.failures = 0;
                    self.total = 0;
                    self.opened_at = None;
                }
            }
            _ => {
                self.consecutive_successes = 0;
            }
        }
    }

    fn record_failure(&mut self) {
        self.total = self.total.saturating_add(1);
        self.failures = self.failures.saturating_add(1);
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.consecutive_successes = 0;
        if self.state == CircuitState::HalfOpen {
            self.half_open_in_flight = false;
            self.trip_open();
            return;
        }
        let thresh = self.config.failure_threshold.max(1);
        if self.consecutive_failures >= thresh {
            self.trip_open();
            return;
        }
        if self.total >= self.config.min_requests.max(1) {
            let rate = self.failures as f64 / self.total as f64;
            if rate >= self.config.error_rate_threshold {
                self.trip_open();
            }
        }
    }

    fn trip_open(&mut self) {
        self.state = CircuitState::Open;
        self.opened_at = Some(Instant::now());
        self.half_open_in_flight = false;
    }

    fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.consecutive_failures = 0;
        self.consecutive_successes = 0;
        self.total = 0;
        self.failures = 0;
        self.opened_at = None;
        self.half_open_in_flight = false;
    }

    fn update_config(&mut self, config: CircuitConfig) {
        self.config = config;
    }
}

#[derive(Default)]
pub struct CircuitRegistry {
    inner: Mutex<HashMap<String, Breaker>>,
}

impl CircuitRegistry {
    pub fn key(app: &str, provider_id: &str) -> String {
        format!("{app}:{provider_id}")
    }

    pub fn allow(&self, app: &str, provider_id: &str, config: &CircuitConfig) -> bool {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let b = map
            .entry(Self::key(app, provider_id))
            .or_insert_with(|| Breaker::new(config.clone()));
        b.update_config(config.clone());
        b.allow()
    }

    pub fn record(&self, app: &str, provider_id: &str, success: bool, config: &CircuitConfig) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let b = map
            .entry(Self::key(app, provider_id))
            .or_insert_with(|| Breaker::new(config.clone()));
        b.update_config(config.clone());
        if success {
            b.record_success();
        } else {
            b.record_failure();
        }
    }

    pub fn health(&self, app: &str, provider_id: &str) -> String {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&Self::key(app, provider_id))
            .map(|b| b.health_label().to_string())
            .unwrap_or_else(|| "unknown".into())
    }

    pub fn reset_provider(&self, app: &str, provider_id: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(b) = map.get_mut(&Self::key(app, provider_id)) {
            b.reset();
        }
    }

    pub fn clear_app(&self, app: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|k, _| !k.starts_with(&format!("{app}:")));
    }
}
