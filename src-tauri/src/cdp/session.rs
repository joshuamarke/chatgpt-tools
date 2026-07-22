//! Single-page CDP WebSocket session (Runtime.evaluate / Page.* / events).
//!
//! One background I/O thread owns the socket (tungstenite is not duplex-safe under a single mutex).

use super::http::CdpTarget;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use tungstenite::{client::IntoClientRequest, connect, stream::MaybeTlsStream, Message, WebSocket};
use url::Url;

#[derive(Debug, Error)]
pub enum CdpSessionError {
    #[error("{0}")]
    Message(String),
}

impl CdpSessionError {
    fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

enum IoCmd {
    Request {
        id: u64,
        method: String,
        params: Value,
        reply: Sender<Result<Value, String>>,
    },
    Close,
}

/// Thread-owned WebSocket with request/response matching and optional event fan-out.
pub struct CdpSession {
    cmd_tx: Sender<IoCmd>,
    next_id: std::sync::Mutex<u64>,
    #[allow(dead_code)]
    target_id: String,
    /// Optional CDP event stream (`method` + `params` frames without `id`).
    event_rx: Option<std::sync::Mutex<Receiver<Value>>>,
    _join: Option<thread::JoinHandle<()>>,
}

pub struct OpenOptions {
    pub open_timeout_ms: u64,
    /// When true, Runtime/Page domains are enabled (inject path).
    pub enable_default_domains: bool,
    /// When true, push CDP events onto an internal channel (inspect path).
    pub with_events: bool,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            open_timeout_ms: 8000,
            enable_default_domains: true,
            with_events: false,
        }
    }
}

impl CdpSession {
    pub fn open(target: &CdpTarget, port: u16, open_timeout_ms: u64) -> Result<Self, CdpSessionError> {
        Self::open_with(
            target,
            port,
            OpenOptions {
                open_timeout_ms,
                ..Default::default()
            },
        )
    }

    pub fn open_with(
        target: &CdpTarget,
        port: u16,
        opts: OpenOptions,
    ) -> Result<Self, CdpSessionError> {
        let url = Url::parse(&target.web_socket_debugger_url)
            .map_err(|e| CdpSessionError::msg(format!("bad WS URL: {e}")))?;
        let host = url.host_str().unwrap_or("");
        if !matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1") {
            return Err(CdpSessionError::msg(format!(
                "refusing non-loopback CDP target {host}"
            )));
        }
        let url_port = url.port().unwrap_or(80);
        if url_port != port {
            return Err(CdpSessionError::msg(format!(
                "WS port mismatch expected {port} got {url_port}"
            )));
        }

        let req = target
            .web_socket_debugger_url
            .as_str()
            .into_client_request()
            .map_err(|e| CdpSessionError::msg(format!("CDP request: {e}")))?;
        let (socket, _resp) =
            connect(req).map_err(|e| CdpSessionError::msg(format!("CDP WS connect: {e}")))?;

        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(
                opts.open_timeout_ms.max(5000),
            )));
        }

        let (cmd_tx, cmd_rx) = mpsc::channel::<IoCmd>();
        let (event_tx, event_rx) = if opts.with_events {
            let (tx, rx) = mpsc::channel::<Value>();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let target_id = target.id.clone();

        let join = thread::spawn(move || io_loop(socket, cmd_rx, event_tx));

        let session = Self {
            cmd_tx,
            next_id: std::sync::Mutex::new(1),
            target_id,
            event_rx: event_rx.map(std::sync::Mutex::new),
            _join: Some(join),
        };

        if opts.enable_default_domains {
            session.send("Runtime.enable", json!({}), 8000)?;
            session.send("Page.enable", json!({}), 8000)?;
        }
        Ok(session)
    }

    #[allow(dead_code)]
    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    /// Drain buffered CDP events (non-blocking). Empty if events were not enabled.
    pub fn poll_events(&self) -> Vec<Value> {
        let Some(rx_lock) = self.event_rx.as_ref() else {
            return Vec::new();
        };
        let Ok(rx) = rx_lock.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            out.push(ev);
            if out.len() >= 64 {
                break;
            }
        }
        out
    }

    /// Block until an event arrives or timeout. Returns None on timeout / no event channel.
    #[allow(dead_code)]
    pub fn recv_event_timeout(&self, timeout_ms: u64) -> Option<Value> {
        let rx_lock = self.event_rx.as_ref()?;
        let rx = rx_lock.lock().ok()?;
        rx.recv_timeout(Duration::from_millis(timeout_ms)).ok()
    }

    pub fn send(
        &self,
        method: &str,
        params: Value,
        timeout_ms: u64,
    ) -> Result<Value, CdpSessionError> {
        let id = {
            let mut n = self.next_id.lock().unwrap_or_else(|e| e.into_inner());
            let id = *n;
            *n += 1;
            id
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        self.cmd_tx
            .send(IoCmd::Request {
                id,
                method: method.to_string(),
                params,
                reply: reply_tx,
            })
            .map_err(|_| CdpSessionError::msg("CDP I/O thread stopped"))?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CdpSessionError::msg(format!(
                    "CDP command timed out: {method}"
                )));
            }
            match reply_rx.recv_timeout(remaining) {
                Ok(Ok(v)) => return Ok(v),
                Ok(Err(e)) => return Err(CdpSessionError::msg(e)),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(CdpSessionError::msg(format!(
                        "CDP command timed out: {method}"
                    )));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(CdpSessionError::msg("CDP waiter disconnected"));
                }
            }
        }
    }

    pub fn evaluate(&self, expression: &str, timeout_ms: u64) -> Result<Value, CdpSessionError> {
        let result = self.send(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "awaitPromise": true,
                "returnByValue": true,
                "userGesture": false,
            }),
            timeout_ms,
        )?;
        if let Some(details) = result.get("exceptionDetails") {
            let detail = details
                .get("exception")
                .and_then(|e| e.get("description"))
                .and_then(|d| d.as_str())
                .or_else(|| details.get("text").and_then(|t| t.as_str()))
                .unwrap_or("evaluate failed");
            return Err(CdpSessionError::msg(format!(
                "Renderer evaluation failed: {detail}"
            )));
        }
        Ok(result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub fn close(&self) {
        let _ = self.cmd_tx.send(IoCmd::Close);
    }
}

impl Drop for CdpSession {
    fn drop(&mut self) {
        self.close();
    }
}

fn io_loop(mut socket: Ws, cmd_rx: Receiver<IoCmd>, event_tx: Option<Sender<Value>>) {
    let mut pending: HashMap<u64, Sender<Result<Value, String>>> = HashMap::new();
    let mut running = true;

    while running {
        // Non-blocking command pump
        loop {
            match cmd_rx.try_recv() {
                Ok(IoCmd::Close) => {
                    running = false;
                    break;
                }
                Ok(IoCmd::Request {
                    id,
                    method,
                    params,
                    reply,
                }) => {
                    let frame = json!({ "id": id, "method": method, "params": params });
                    match socket.send(Message::Text(frame.to_string())) {
                        Ok(()) => {
                            pending.insert(id, reply);
                        }
                        Err(e) => {
                            let _ = reply.send(Err(format!("CDP send: {e}")));
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    running = false;
                    break;
                }
            }
        }
        if !running {
            break;
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
                        let result = if let Some(err) = v.get("error") {
                            let m = err
                                .get("message")
                                .and_then(|x| x.as_str())
                                .unwrap_or("CDP error");
                            let code = err.get("code").and_then(|x| x.as_i64()).unwrap_or(0);
                            Err(format!("{m} ({code})"))
                        } else {
                            Ok(v.get("result").cloned().unwrap_or(Value::Null))
                        };
                        if let Some(tx) = pending.remove(&id) {
                            let _ = tx.send(result);
                        }
                    } else if v.get("method").and_then(|m| m.as_str()).is_some() {
                        // CDP event (no id)
                        if let Some(ref tx) = event_tx {
                            let _ = tx.send(v);
                        }
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = socket.send(Message::Pong(data));
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // idle — loop to pump commands
            }
            Err(_) => break,
        }
    }

    for (_, tx) in pending.drain() {
        let _ = tx.send(Err("CDP socket closed".into()));
    }
    let _ = socket.close(None);
}
