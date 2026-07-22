//! Single-page CDP WebSocket session (Runtime.evaluate / Page.*).
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

/// Thread-owned WebSocket with request/response matching.
pub struct CdpSession {
    cmd_tx: Sender<IoCmd>,
    next_id: std::sync::Mutex<u64>,
    #[allow(dead_code)]
    target_id: String,
    _join: Option<thread::JoinHandle<()>>,
}

impl CdpSession {
    pub fn open(target: &CdpTarget, port: u16, open_timeout_ms: u64) -> Result<Self, CdpSessionError> {
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
            let _ = stream.set_write_timeout(Some(Duration::from_millis(open_timeout_ms.max(5000))));
        }

        let (cmd_tx, cmd_rx) = mpsc::channel::<IoCmd>();
        let target_id = target.id.clone();

        let join = thread::spawn(move || io_loop(socket, cmd_rx));

        let session = Self {
            cmd_tx,
            next_id: std::sync::Mutex::new(1),
            target_id,
            _join: Some(join),
        };

        // Enable domains (same as Node injector)
        session.send("Runtime.enable", json!({}), 8000)?;
        session.send("Page.enable", json!({}), 8000)?;
        Ok(session)
    }

    #[allow(dead_code)]
    pub fn target_id(&self) -> &str {
        &self.target_id
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

fn io_loop(mut socket: Ws, cmd_rx: Receiver<IoCmd>) {
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
