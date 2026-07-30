//! Axum HTTP server for local routing.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use http_body_util::BodyExt;
use tokio::sync::oneshot;

use super::forwarder::{forward_with_failover, ForwardContext};
use super::runtime::ProxyRuntime;
use crate::providers::models::{AppKind, GlobalProxyConfig};
use crate::providers::{codex, store};

#[derive(Clone)]
struct AppState {
    runtime: Arc<ProxyRuntime>,
}

pub async fn run_server(
    runtime: Arc<ProxyRuntime>,
    cfg: GlobalProxyConfig,
    shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let addr: SocketAddr = format!("{}:{}", cfg.listen_address, cfg.listen_port)
        .parse()
        .map_err(|e| format!("无效监听地址: {e}"))?;

    let state = AppState { runtime };
    let app = Router::new()
        .route("/health", get(health))
        .route("/status", get(status_handler))
        .route("/v1/models", get(codex_models))
        .route("/v1/*path", any(codex_proxy))
        .route("/v1", any(codex_proxy))
        .route("/grok/v1/*path", any(grok_proxy))
        .route("/grok/v1", any(grok_proxy))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("绑定 {addr} 失败: {e}"))?;

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        })
        .await
        .map_err(|e| format!("代理服务退出: {e}"))
}

async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "service": "chatgpt-tools-local-routing",
    }))
}

async fn status_handler(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json(state.runtime.status_snapshot())
}

async fn codex_models() -> Response {
    // OpenAI-compatible list from projected catalog so desktop/CLI can discover
    // third-party slugs (DeepSeek / Claude / Gemini / Grok / …) over HTTP.
    let home = codex::codex_home_dir();
    let catalog_path = home.join("chatgpt-tools-model-catalog.json");
    let live = codex::read_config_text().unwrap_or_default();
    let serve = live.contains("chatgpt-tools-model-catalog.json") && catalog_path.exists();
    let body = if serve {
        let list = crate::providers::catalog::openai_models_list_from_catalog(&home);
        serde_json::to_string(&list).unwrap_or_else(|_| r#"{"object":"list","data":[]}"#.into())
    } else {
        r#"{"object":"list","data":[]}"#.into()
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn codex_proxy(State(state): State<AppState>, req: Request) -> Response {
    proxy_dispatch(state, AppKind::Codex, req).await
}

async fn grok_proxy(State(state): State<AppState>, req: Request) -> Response {
    proxy_dispatch(state, AppKind::Grok, req).await
}

async fn proxy_dispatch(state: AppState, app: AppKind, req: Request) -> Response {
    state.runtime.begin_request();

    // Guard: app must be under takeover
    let file = match store::load() {
        Ok(f) => f,
        Err(e) => {
            state.runtime.end_request(false);
            return error_json(StatusCode::INTERNAL_SERVER_ERROR, e);
        }
    };
    let app_store = file.for_kind(app);
    if !app_store.takeover_enabled {
        state.runtime.end_request(false);
        return error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "{} 未开启本地路由接管，请在 ChatGPT Tools 供应商页打开开关",
                app.display_name()
            ),
        );
    }

    let method = req.method().clone();
    if method == Method::OPTIONS {
        state.runtime.end_request(true);
        return Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header("access-control-allow-origin", "*")
            .header("access-control-allow-headers", "*")
            .header("access-control-allow-methods", "GET,POST,PUT,PATCH,DELETE,OPTIONS")
            .body(Body::empty())
            .unwrap_or_else(|_| StatusCode::NO_CONTENT.into_response());
    }

    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let headers = req.headers().clone();

    let body_bytes = match req.into_body().collect().await {
        Ok(c) => c.to_bytes(),
        Err(e) => {
            state.runtime.end_request(false);
            return error_json(StatusCode::BAD_REQUEST, format!("读取请求体失败: {e}"));
        }
    };

    let ctx = ForwardContext {
        app,
        method,
        path_and_query,
        headers,
        body: body_bytes,
    };

    match forward_with_failover(&state.runtime, ctx).await {
        Ok(outcome) => {
            state.runtime.end_request(true);
            let mut builder = Response::builder().status(outcome.status);
            {
                let headers_mut = builder.headers_mut();
                if let Some(h) = headers_mut {
                    for (k, v) in outcome.headers.iter() {
                        h.insert(k.clone(), v.clone());
                    }
                    // Identify proxy for debugging
                    if let Ok(v) = HeaderValue::from_str(&outcome.provider_id) {
                        h.insert("x-chatgpt-tools-provider", v);
                    }
                }
            }
            let body = Body::from_stream(outcome.body_stream);
            builder
                .body(body)
                .unwrap_or_else(|e| error_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        Err((status, msg)) => {
            state.runtime.end_request(false);
            error_json(status, msg)
        }
    }
}

fn error_json(status: StatusCode, message: impl AsRef<str>) -> Response {
    let body = serde_json::json!({
        "error": {
            "message": message.as_ref(),
            "type": "chatgpt_tools_proxy_error",
        }
    });
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| status.into_response())
}
