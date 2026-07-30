//! Upstream selection + HTTP forward with failover.
//!
//! Success under local routing is **not** “HTTP 2xx headers only”:
//! - non-streaming: buffer body (timeout), reject 2xx error envelopes, parse usage
//! - streaming: wait for first byte (TTFB), reject early SSE semantic failures
//!
//! Request logs store input/output tokens and `first_token_ms` when available.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use reqwest::Client;

use super::circuit::CircuitRegistry;
use super::log_store::{self, NewRequestLog};
use super::runtime::ProxyRuntime;
use super::usage::{semantic_error_message, sse_early_failure_message, TokenUsage};
use super::PROXY_MANAGED;
use super::takeover;
use crate::providers::models::{AppKind, AppProviderStore, Provider};
use crate::providers::store;

#[derive(Clone)]
pub struct ForwardContext {
    pub app: AppKind,
    pub method: Method,
    pub path_and_query: String,
    pub headers: HeaderMap,
    pub body: Bytes,
}

pub struct ForwardOutcome {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body_stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + Sync>>,
    pub provider_id: String,
}

struct AttemptOk {
    outcome: ForwardOutcome,
    usage: TokenUsage,
    first_token_ms: Option<u64>,
    /// Streaming path: body still needs a logging wrapper (tokens on complete).
    needs_stream_log: bool,
    /// First chunk already consumed while priming (must be replayed).
    primed_first: Option<Bytes>,
    model_hint: String,
}

struct AttemptErr {
    status: StatusCode,
    message: String,
    retryable: bool,
}

struct AttemptTimeouts {
    non_streaming_secs: u64,
    first_byte_secs: u64,
    is_streaming: bool,
}

/// Build an outbound HTTP client.
/// `egress_proxy`: optional URL (`http://host:port`, `socks5://…`); empty = direct (no system proxy).
fn http_client(timeout_secs: u64, egress_proxy: Option<&str>) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(30)))
        .connect_timeout(Duration::from_secs(15))
        .pool_idle_timeout(Duration::from_secs(90));

    if let Some(raw) = egress_proxy.map(str::trim).filter(|s| !s.is_empty()) {
        let proxy =
            reqwest::Proxy::all(raw).map_err(|e| format!("出口代理无效（{raw}）: {e}"))?;
        builder = builder.proxy(proxy);
    } else {
        builder = builder.no_proxy();
    }

    builder
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))
}

/// Normalize / validate optional egress proxy URL stored in GlobalProxyConfig.
pub fn normalize_egress_proxy(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(String::new());
    }
    let url = if s.contains("://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    reqwest::Proxy::all(&url).map_err(|e| {
        format!(
            "出口代理格式无效：{e}。示例：http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"
        )
    })?;
    Ok(url)
}

fn select_providers(
    app: AppKind,
    app_store: &AppProviderStore,
    circuits: &CircuitRegistry,
) -> Result<Vec<Provider>, String> {
    let app_s = app.as_str();
    if app_store.auto_failover_enabled {
        let mut ordered_ids: Vec<String> = app_store.failover_order.clone();
        if ordered_ids.is_empty() {
            let mut queued: Vec<&Provider> = app_store
                .providers
                .iter()
                .filter(|p| p.in_failover_queue)
                .collect();
            queued.sort_by_key(|p| p.sort_index.unwrap_or(9999));
            ordered_ids = queued.into_iter().map(|p| p.id.clone()).collect();
        }
        if !app_store.current.is_empty() {
            if let Some(pos) = ordered_ids.iter().position(|id| id == &app_store.current) {
                let cur = ordered_ids.remove(pos);
                ordered_ids.insert(0, cur);
            } else if let Some(cur) = app_store
                .providers
                .iter()
                .find(|p| p.id == app_store.current)
            {
                ordered_ids.insert(0, cur.id.clone());
            }
        }
        if ordered_ids.is_empty() {
            if let Some(cur) = app_store.providers.iter().find(|p| p.id == app_store.current) {
                ordered_ids.push(cur.id.clone());
            }
        }
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in ordered_ids {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(p) = app_store.providers.iter().find(|x| x.id == id) else {
                continue;
            };
            if p.is_official() && app == AppKind::Grok {
                continue;
            }
            if circuits.allow(app_s, &p.id, &app_store.circuit) {
                out.push(p.clone());
            }
        }
        if out.is_empty() {
            return Err("故障转移队列中无可用供应商（可能均已熔断）".into());
        }
        Ok(out)
    } else {
        let id = if app_store.current.is_empty() {
            return Err("未设置当前供应商".into());
        } else {
            app_store.current.clone()
        };
        let p = app_store
            .providers
            .iter()
            .find(|x| x.id == id)
            .cloned()
            .ok_or_else(|| format!("当前供应商不存在: {id}"))?;
        Ok(vec![p])
    }
}

fn build_upstream_url(base: &str, request_path: &str, app: AppKind) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    let path = match app {
        AppKind::Codex => request_path,
        AppKind::Grok => request_path
            .strip_prefix("/grok")
            .unwrap_or(request_path),
    };
    let path = path.split('?').next().unwrap_or(path);
    if let Some(rest) = path.strip_prefix("/v1") {
        Ok(format!("{base}{rest}"))
    } else if path.starts_with('/') {
        Ok(format!("{base}{path}"))
    } else {
        Ok(format!("{base}/{path}"))
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status.as_u16(),
        408 | 425 | 429 | 500 | 502 | 503 | 504 | 520 | 521 | 522 | 523 | 524
    )
}

fn apply_meta_headers(headers: &mut HeaderMap, provider: &Provider) {
    if let Some(meta) = provider.meta.as_ref() {
        if let Some(ua) = meta
            .custom_user_agent
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if let Ok(v) = HeaderValue::from_str(ua) {
                headers.insert(http::header::USER_AGENT, v);
            }
        }
        if let Some(map) = meta
            .local_proxy_request_overrides
            .as_ref()
            .and_then(|o| o.headers.as_ref())
        {
            for (k, v) in map {
                if let (Ok(name), Some(val)) = (
                    HeaderName::from_bytes(k.as_bytes()),
                    v.as_str().and_then(|s| HeaderValue::from_str(s).ok()),
                ) {
                    headers.insert(name, val);
                }
            }
        }
    }
}

fn apply_meta_body(body: Bytes, provider: &Provider) -> Bytes {
    let Some(override_body) = provider
        .meta
        .as_ref()
        .and_then(|m| m.local_proxy_request_overrides.as_ref())
        .and_then(|o| o.body.as_ref())
    else {
        return body;
    };
    let Ok(mut original) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };
    if let (Some(base), Some(over)) = (original.as_object_mut(), override_body.as_object()) {
        for (k, v) in over {
            base.insert(k.clone(), v.clone());
        }
        Bytes::from(serde_json::to_vec(&original).unwrap_or_else(|_| body.to_vec()))
    } else {
        body
    }
}

fn inject_auth(
    headers: &mut HeaderMap,
    api_key: Option<&str>,
    oauth_passthrough: bool,
    incoming: &HeaderMap,
) -> Result<(), String> {
    let auth_is_placeholder = incoming
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains(PROXY_MANAGED))
        .unwrap_or(false);
    if auth_is_placeholder {
        headers.remove(http::header::AUTHORIZATION);
    }

    if oauth_passthrough {
        if let Some(auth) = incoming.get(http::header::AUTHORIZATION) {
            if let Ok(s) = auth.to_str() {
                if !s.contains(PROXY_MANAGED) && !s.trim().is_empty() {
                    headers.insert(http::header::AUTHORIZATION, auth.clone());
                    return Ok(());
                }
            }
        }
        return Err(
            "官方路由需要 Codex 登录态：请先在 Codex 完成 ChatGPT 登录后重试".into(),
        );
    }

    let key = api_key
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "上游 API Key 为空".to_string())?;
    let value = format!("Bearer {key}");
    headers.insert(
        http::header::AUTHORIZATION,
        HeaderValue::from_str(&value).map_err(|e| e.to_string())?,
    );
    Ok(())
}

fn filter_request_headers(incoming: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in incoming.iter() {
        let name = k.as_str();
        if matches!(
            name,
            "host"
                | "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailers"
                | "transfer-encoding"
                | "upgrade"
                | "content-length"
        ) {
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    out
}

fn filter_response_headers(incoming: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in incoming.iter() {
        let name = k.as_str();
        if matches!(
            name,
            "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailers"
                | "transfer-encoding"
                | "upgrade"
                | "content-length"
        ) {
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    out
}

fn make_log(
    app: &str,
    provider: &Provider,
    model: &str,
    method: &str,
    path: &str,
    status: u16,
    latency_ms: u64,
    is_streaming: bool,
    attempt: u32,
    error_message: Option<String>,
    usage: &TokenUsage,
    first_token_ms: Option<u64>,
) -> NewRequestLog {
    let model = if model.is_empty() {
        usage.model.clone().unwrap_or_default()
    } else {
        model.to_string()
    };
    NewRequestLog {
        app: app.to_string(),
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        model,
        method: method.to_string(),
        path: path.to_string(),
        status_code: status,
        latency_ms,
        is_streaming,
        attempt,
        error_message,
        input_tokens: u64::from(usage.input_tokens),
        output_tokens: u64::from(usage.output_tokens),
        first_token_ms,
    }
}

// ── Streaming log collector (finish on Drop = client finished reading) ────

struct StreamLogState {
    app: String,
    provider_id: String,
    provider_name: String,
    model: String,
    method: String,
    path: String,
    status_code: u16,
    attempt: u32,
    prior_failures: Option<String>,
    first_token_ms: Option<u64>,
    started: Instant,
    sse_buf: String,
    finished: bool,
    logging_enabled: bool,
}

impl StreamLogState {
    fn feed(&mut self, chunk: &[u8]) {
        const MAX: usize = 512 * 1024;
        if let Ok(s) = std::str::from_utf8(chunk) {
            self.sse_buf.push_str(s);
            if self.sse_buf.len() > MAX {
                let keep = MAX / 2;
                self.sse_buf = self.sse_buf[self.sse_buf.len() - keep..].to_string();
            }
        }
    }

    fn finish_log(&mut self) {
        if self.finished || !self.logging_enabled {
            return;
        }
        self.finished = true;
        let usage = TokenUsage::from_sse_text(&self.sse_buf).unwrap_or_default();
        let model = if self.model.is_empty() {
            usage.model.clone().unwrap_or_default()
        } else {
            self.model.clone()
        };
        log_store::try_insert(NewRequestLog {
            app: self.app.clone(),
            provider_id: self.provider_id.clone(),
            provider_name: self.provider_name.clone(),
            model,
            method: self.method.clone(),
            path: self.path.clone(),
            status_code: self.status_code,
            latency_ms: self.started.elapsed().as_millis() as u64,
            is_streaming: true,
            attempt: self.attempt,
            error_message: self.prior_failures.clone(),
            input_tokens: u64::from(usage.input_tokens),
            output_tokens: u64::from(usage.output_tokens),
            first_token_ms: self.first_token_ms,
        });
    }
}

pub async fn forward_with_failover(
    runtime: &ProxyRuntime,
    ctx: ForwardContext,
) -> Result<ForwardOutcome, (StatusCode, String)> {
    let file = store::load().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let logging_enabled = file.proxy.enable_logging;
    let app_store = file.for_kind(ctx.app).clone();
    let circuits = runtime.circuits();
    let providers = select_providers(ctx.app, &app_store, &circuits)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    let max_attempts = if app_store.auto_failover_enabled {
        (app_store.max_retries as usize).saturating_add(1).max(1)
    } else {
        1
    };
    let non_streaming_timeout = app_store.non_streaming_timeout.max(60);
    // Always honor configured first-byte timeout under local routing so a bare
    // 200 with no body cannot be treated as success.
    let first_byte_timeout = app_store.streaming_first_byte_timeout;

    let egress = file.proxy.egress_proxy.trim();
    let client = http_client(
        non_streaming_timeout,
        if egress.is_empty() {
            None
        } else {
            Some(egress)
        },
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let start_current = app_store.current.clone();
    let mut last_err = "无可用上游".to_string();
    let mut last_provider: Option<Provider> = None;
    let mut last_status = StatusCode::BAD_GATEWAY;
    let mut fail_chain: Vec<String> = Vec::new();
    let started = Instant::now();
    let model = log_store::extract_model_from_body(&ctx.body);
    let is_streaming = log_store::body_requests_stream(&ctx.body)
        || log_store::looks_streaming(&ctx.path_and_query, &ctx.headers);
    let method = ctx.method.as_str().to_string();
    let path = ctx.path_and_query.clone();
    let app_str = ctx.app.as_str().to_string();

    let timeouts = AttemptTimeouts {
        non_streaming_secs: non_streaming_timeout,
        first_byte_secs: first_byte_timeout,
        is_streaming,
    };

    for (idx, provider) in providers.into_iter().take(max_attempts).enumerate() {
        let attempt = (idx + 1) as u32;
        match forward_one(&client, &ctx, &provider, &timeouts, started).await {
            Ok(mut ok) => {
                // Circuit + FO only after real success (first byte / full body).
                circuits.record(ctx.app.as_str(), &provider.id, true, &app_store.circuit);
                runtime.note_success(&ctx.app, &provider);
                if app_store.auto_failover_enabled && provider.id != start_current && idx > 0 {
                    runtime.note_failover();
                    let _ = runtime.hot_switch_current(ctx.app, &provider.id);
                }

                let err_note = if fail_chain.is_empty() {
                    None
                } else {
                    Some(format!("先前失败: {}", fail_chain.join(" → ")))
                };

                if ok.needs_stream_log {
                    if let Some(first) = ok.primed_first.take() {
                        let state = Arc::new(Mutex::new(StreamLogState {
                            app: app_str.clone(),
                            provider_id: provider.id.clone(),
                            provider_name: provider.name.clone(),
                            model: if ok.model_hint.is_empty() {
                                model.clone()
                            } else {
                                ok.model_hint.clone()
                            },
                            method: method.clone(),
                            path: path.clone(),
                            status_code: ok.outcome.status.as_u16(),
                            attempt,
                            prior_failures: err_note,
                            first_token_ms: ok.first_token_ms,
                            started,
                            sse_buf: String::new(),
                            finished: false,
                            logging_enabled,
                        }));
                        // Replace body with logging wrapper (rest was already the raw stream).
                        let rest = ok.outcome.body_stream;
                        // body_stream is !Unpin boxed — re-pin via LoggingBodyStream needs Unpin rest.
                        // Use a channel-free approach: map side-effect stream.
                        ok.outcome.body_stream =
                            wrap_stream_with_log(first, rest, state);
                    }
                } else if logging_enabled {
                    log_store::try_insert(make_log(
                        &app_str,
                        &provider,
                        if ok.model_hint.is_empty() {
                            &model
                        } else {
                            &ok.model_hint
                        },
                        &method,
                        &path,
                        ok.outcome.status.as_u16(),
                        started.elapsed().as_millis() as u64,
                        is_streaming,
                        attempt,
                        err_note,
                        &ok.usage,
                        ok.first_token_ms,
                    ));
                }

                return Ok(ok.outcome);
            }
            Err(err) => {
                circuits.record(ctx.app.as_str(), &provider.id, false, &app_store.circuit);
                runtime.note_failure(&err.message);
                last_err = format!("{}: {}", provider.name, err.message);
                fail_chain.push(last_err.clone());
                last_status = err.status;
                last_provider = Some(provider);
                let retryable = err.retryable || is_retryable_status(err.status);
                if !retryable {
                    if logging_enabled {
                        let p = last_provider.as_ref();
                        log_store::try_insert(NewRequestLog {
                            app: app_str.clone(),
                            provider_id: p.map(|x| x.id.clone()).unwrap_or_default(),
                            provider_name: p.map(|x| x.name.clone()).unwrap_or_default(),
                            model: model.clone(),
                            method: method.clone(),
                            path: path.clone(),
                            status_code: err.status.as_u16(),
                            latency_ms: started.elapsed().as_millis() as u64,
                            is_streaming,
                            attempt,
                            error_message: Some(last_err.clone()),
                            input_tokens: 0,
                            output_tokens: 0,
                            first_token_ms: None,
                        });
                    }
                    return Err((err.status, last_err));
                }
            }
        }
    }
    if logging_enabled {
        let p = last_provider.as_ref();
        log_store::try_insert(NewRequestLog {
            app: app_str,
            provider_id: p.map(|x| x.id.clone()).unwrap_or_default(),
            provider_name: p.map(|x| x.name.clone()).unwrap_or_default(),
            model,
            method,
            path,
            status_code: last_status.as_u16(),
            latency_ms: started.elapsed().as_millis() as u64,
            is_streaming,
            attempt: max_attempts.min(u32::MAX as usize) as u32,
            error_message: Some(last_err.clone()),
            input_tokens: 0,
            output_tokens: 0,
            first_token_ms: None,
        });
    }
    Err((StatusCode::BAD_GATEWAY, last_err))
}

/// Logging wrapper that does not require the inner stream to be Unpin.
fn wrap_stream_with_log(
    first: Bytes,
    rest: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + Sync>>,
    state: Arc<Mutex<StreamLogState>>,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + Sync>> {
    Box::pin(LoggingBodyStreamPinned {
        inner: rest,
        pending_first: Some(first),
        state,
    })
}

struct LoggingBodyStreamPinned {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + Sync>>,
    pending_first: Option<Bytes>,
    state: Arc<Mutex<StreamLogState>>,
}

impl Stream for LoggingBodyStreamPinned {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(first) = self.pending_first.take() {
            if let Ok(mut st) = self.state.lock() {
                st.feed(&first);
            }
            return Poll::Ready(Some(Ok(first)));
        }
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                if let Ok(mut st) = self.state.lock() {
                    st.feed(&chunk);
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            other => other,
        }
    }
}

impl Drop for LoggingBodyStreamPinned {
    fn drop(&mut self) {
        if let Ok(mut st) = self.state.lock() {
            st.finish_log();
        }
    }
}

async fn forward_one(
    client: &Client,
    ctx: &ForwardContext,
    provider: &Provider,
    timeouts: &AttemptTimeouts,
    started: Instant,
) -> Result<AttemptOk, AttemptErr> {
    let (base, api_key, oauth) = takeover::upstream_from_provider(ctx.app, provider).map_err(
        |e| AttemptErr {
            status: StatusCode::BAD_GATEWAY,
            message: e,
            retryable: false,
        },
    )?;

    let base = if oauth {
        crate::providers::codex::OFFICIAL_API_BASE_URL.to_string()
    } else {
        base
    };

    let url = build_upstream_url(&base, &ctx.path_and_query, ctx.app).map_err(|e| AttemptErr {
        status: StatusCode::BAD_REQUEST,
        message: e,
        retryable: false,
    })?;

    let mut headers = filter_request_headers(&ctx.headers);
    inject_auth(
        &mut headers,
        api_key.as_deref(),
        oauth,
        &ctx.headers,
    )
    .map_err(|e| AttemptErr {
        status: StatusCode::UNAUTHORIZED,
        message: e,
        retryable: false,
    })?;
    apply_meta_headers(&mut headers, provider);
    let body = apply_meta_body(ctx.body.clone(), provider);

    let mut builder = client.request(ctx.method.clone(), &url);
    for (k, v) in headers.iter() {
        builder = builder.header(k, v);
    }
    builder = builder.body(body);

    // Streaming must not use the full non-streaming wall-clock timeout.
    if timeouts.is_streaming {
        builder = builder.timeout(Duration::from_secs(24 * 60 * 60));
    } else {
        builder = builder.timeout(Duration::from_secs(timeouts.non_streaming_secs.max(30)));
    }

    let send_fut = builder.send();
    let resp = if timeouts.is_streaming && timeouts.first_byte_secs > 0 {
        match tokio::time::timeout(Duration::from_secs(timeouts.first_byte_secs), send_fut).await {
            Ok(r) => r,
            Err(_) => {
                return Err(AttemptErr {
                    status: StatusCode::GATEWAY_TIMEOUT,
                    message: format!(
                        "流式响应首包超时: {}s（上游未返回响应头）",
                        timeouts.first_byte_secs
                    ),
                    retryable: true,
                });
            }
        }
    } else {
        send_fut.await
    }
    .map_err(|e| {
        let retry = e.is_timeout() || e.is_connect() || e.is_request();
        AttemptErr {
            status: StatusCode::BAD_GATEWAY,
            message: format!("上游请求失败: {e}"),
            retryable: retry,
        }
    })?;

    let status =
        StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let preview = if text.is_empty() {
            format!("上游返回 {status}")
        } else {
            text.chars().take(600).collect()
        };
        return Err(AttemptErr {
            status,
            message: preview,
            retryable: is_retryable_status(status),
        });
    }

    let out_headers = filter_response_headers(resp.headers());
    let model_hint = log_store::extract_model_from_body(&ctx.body);

    // Non-streaming: buffer body, validate semantic success, parse usage.
    if !timeouts.is_streaming {
        let body_timeout = Duration::from_secs(timeouts.non_streaming_secs.max(30));
        let body_bytes = tokio::time::timeout(body_timeout, resp.bytes())
            .await
            .map_err(|_| AttemptErr {
                status: StatusCode::GATEWAY_TIMEOUT,
                message: format!(
                    "响应体读取超时: {}s（上游发完响应头后 body 未到达）",
                    body_timeout.as_secs()
                ),
                retryable: true,
            })?
            .map_err(|e| AttemptErr {
                status: StatusCode::BAD_GATEWAY,
                message: format!("读取响应体失败: {e}"),
                retryable: true,
            })?;

        if let Some(msg) = semantic_error_message(&body_bytes) {
            return Err(AttemptErr {
                status: StatusCode::BAD_GATEWAY,
                message: format!("上游 2xx 语义失败: {msg}"),
                retryable: true,
            });
        }

        let parsed = serde_json::from_slice::<serde_json::Value>(&body_bytes).ok();
        let usage = parsed
            .as_ref()
            .and_then(TokenUsage::from_json_body)
            .unwrap_or_default();
        let first_token_ms = Some(started.elapsed().as_millis() as u64);
        let model_hint = usage
            .model
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or(model_hint);

        let stream = futures::stream::once(async move { Ok(body_bytes) });
        return Ok(AttemptOk {
            outcome: ForwardOutcome {
                status,
                headers: out_headers,
                body_stream: Box::pin(stream),
                provider_id: provider.id.clone(),
            },
            usage,
            first_token_ms,
            needs_stream_log: false,
            primed_first: None,
            model_hint,
        });
    }

    // Streaming: prime first byte before committing success.
    let mut byte_stream = resp.bytes_stream();
    let first_chunk = if timeouts.first_byte_secs > 0 {
        match tokio::time::timeout(
            Duration::from_secs(timeouts.first_byte_secs),
            byte_stream.next(),
        )
        .await
        {
            Ok(v) => v,
            Err(_) => {
                return Err(AttemptErr {
                    status: StatusCode::GATEWAY_TIMEOUT,
                    message: format!(
                        "流式响应首包超时: {}s（上游已返回响应头但未返回数据）",
                        timeouts.first_byte_secs
                    ),
                    retryable: true,
                });
            }
        }
    } else {
        byte_stream.next().await
    };

    let first = match first_chunk {
        None => {
            return Err(AttemptErr {
                status: StatusCode::BAD_GATEWAY,
                message: "流式响应在首包到达前结束".into(),
                retryable: true,
            });
        }
        Some(Err(e)) => {
            return Err(AttemptErr {
                status: StatusCode::BAD_GATEWAY,
                message: format!("读取流式响应首包失败: {e}"),
                retryable: true,
            });
        }
        Some(Ok(b)) => Bytes::from(b),
    };

    let first_token_ms = Some(started.elapsed().as_millis() as u64);
    let first_text = String::from_utf8_lossy(&first);
    if let Some(msg) = sse_early_failure_message(&first_text) {
        return Err(AttemptErr {
            status: StatusCode::BAD_GATEWAY,
            message: format!("上游流式 2xx 语义失败: {msg}"),
            retryable: true,
        });
    }

    let early_usage = TokenUsage::from_sse_text(&first_text).unwrap_or_default();
    let rest = byte_stream.map(|chunk| {
        chunk
            .map(Bytes::from)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    });

    Ok(AttemptOk {
        outcome: ForwardOutcome {
            status,
            headers: out_headers,
            // Temporary rest stream; caller rewraps with first + logger.
            body_stream: Box::pin(rest),
            provider_id: provider.id.clone(),
        },
        usage: early_usage,
        first_token_ms,
        needs_stream_log: true,
        primed_first: Some(first),
        model_hint,
    })
}
