//! One loopback connection per desktop instance. No reconnect or request retries.
#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use lightcraft_engine::companion::RoundIdentity;
use serde_json::{Value, json};

pub struct Request {
    pub path: String,
    pub body: Value,
    pub submission: u64,
    pub round: String,
    pub identity: RoundIdentity,
}
pub enum Event {
    Connected { client: String, connection: String },
    Server { kind: String, data: Value },
    Response { submission: u64, round: String, path: String, identity: RoundIdentity, result: Result<Value, RequestFailure> },
    Closed(String),
}
pub struct RequestFailure {
    pub rejected: bool,
    message: String,
}
impl From<String> for RequestFailure {
    fn from(message: String) -> Self {
        Self { rejected: false, message }
    }
}
impl From<&str> for RequestFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
impl std::fmt::Display for RequestFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
pub struct Connection {
    pub requests: mpsc::SyncSender<Request>,
    pub events: mpsc::Receiver<Event>,
    pub alive: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}
struct RequestWorker {
    thread: Option<std::thread::JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for RequestWorker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
pub fn connect(address: &str) -> Result<Connection, String> {
    let address: SocketAddr = address.parse().map_err(|_| "本地 AI 服务地址无效。")?;
    if !address.ip().is_loopback() {
        return Err("开发模式仅连接本机 AI 服务。".into());
    }
    let (requests, rx) = mpsc::sync_channel::<Request>(8);
    let (tx, events) = mpsc::sync_channel::<Event>(32);
    let cancel = Arc::new(AtomicBool::new(false));
    let alive = Arc::new(AtomicBool::new(true));
    let worker_cancel = cancel.clone();
    let worker_alive = alive.clone();
    std::thread::Builder::new()
        .name("photo-companion-events".into())
        .spawn(move || {
            let result = receive(address, rx, &tx, &worker_cancel);
            worker_cancel.store(true, Ordering::Relaxed);
            let _ = tx.try_send(Event::Closed(result.err().unwrap_or_else(|| "AI 连接已结束。".into())));
            worker_alive.store(false, Ordering::Relaxed);
        })
        .map_err(|e| format!("无法启动 AI 连接：{e}"))?;
    Ok(Connection { requests, events, alive, cancel })
}
fn emit(tx: &mpsc::SyncSender<Event>, mut event: Event, cancel: &AtomicBool) -> Result<(), String> {
    let start = Instant::now();
    loop {
        cancelled(cancel, start, Duration::from_secs(20))?;
        match tx.try_send(event) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(value)) => {
                event = value;
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return Err("AI 消息接收已结束。".into()),
        }
    }
}
fn socket(address: SocketAddr) -> Result<TcpStream, String> {
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|_| "无法连接本地 AI 服务，请先启动 Go 后端。")?;
    stream.set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    Ok(stream)
}
fn cancelled(cancel: &AtomicBool, start: Instant, timeout: Duration) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("AI 连接已结束。".into());
    }
    if start.elapsed() > timeout {
        return Err("AI 网络请求超时。".into());
    }
    Ok(())
}
fn timed_out(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
}
fn line(reader: &mut BufReader<TcpStream>, cancel: &AtomicBool, timeout: Duration, limit: usize) -> Result<String, String> {
    let start = Instant::now();
    let mut bytes = Vec::new();
    loop {
        cancelled(cancel, start, timeout)?;
        // take() bounds a hostile line even when no newline ever arrives.
        match (&mut *reader).take(limit.saturating_add(1).saturating_sub(bytes.len()) as u64).read_until(b'\n', &mut bytes) {
            Ok(0) => return Err("AI 事件连接已断开。".into()),
            Ok(_) if bytes.len() > limit => return Err("AI 事件超过大小限制。".into()),
            Ok(_) if bytes.last() == Some(&b'\n') => return String::from_utf8(bytes).map_err(|_| "AI 事件编码无效。".into()),
            Ok(_) => {}
            Err(e) if timed_out(&e) => {}
            Err(_) => return Err("AI 事件连接已断开。".into()),
        }
    }
}
fn headers(reader: &mut BufReader<TcpStream>, cancel: &AtomicBool) -> Result<(u16, Option<usize>), String> {
    let status = line(reader, cancel, Duration::from_secs(10), 8192)?;
    let code = status.split_whitespace().nth(1).and_then(|v| v.parse().ok()).ok_or("后端 HTTP 状态无效。")?;
    let mut size = None;
    let mut bytes = status.len();
    loop {
        let value = line(reader, cancel, Duration::from_secs(10), 8192)?;
        bytes += value.len();
        if bytes > 8192 {
            return Err("后端 HTTP 头过大。".into());
        }
        if value.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = value.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                size = Some(value.trim().parse::<usize>().map_err(|_| "HTTP 长度无效。")?);
            }
            if name.eq_ignore_ascii_case("transfer-encoding") {
                return Err("后端传输格式不匹配。".into());
            }
        }
    }
    Ok((code, size))
}
fn post(address: SocketAddr, path: &str, token: &str, body: &Value, cancel: &AtomicBool) -> Result<Value, String> {
    post_request(address, path, token, body, cancel).map_err(|e| e.to_string())
}
fn post_request(address: SocketAddr, path: &str, token: &str, body: &Value, cancel: &AtomicBool) -> Result<Value, RequestFailure> {
    cancelled(cancel, Instant::now(), Duration::from_secs(15))?;
    if !path.starts_with("/v1/") || path.chars().any(char::is_whitespace) || (!token.is_empty() && !token.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err("AI 请求身份无效。".into());
    }
    let data = serde_json::to_vec(body).map_err(|_| "AI 输入无效。")?;
    if data.len() > 32 * 1024 * 1024 {
        return Err("AI 输入超过服务大小限制，未删减历史。".into());
    }
    let mut stream = socket(address)?;
    let header = format!(
        "POST {path} HTTP/1.0\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        data.len()
    );
    stream.write_all(header.as_bytes()).and_then(|_| stream.write_all(&data)).map_err(|_| "AI 请求发送失败；未自动重试。")?;
    let mut reader = BufReader::new(stream);
    let (status, length) = headers(&mut reader, cancel)?;
    if length.is_some_and(|n| n > 96 * 1024) {
        return Err("后端回复过大。".into());
    }
    let mut bytes = Vec::new();
    let start = Instant::now();
    let mut buffer = [0u8; 4096];
    loop {
        cancelled(cancel, start, Duration::from_secs(15))?;
        if length == Some(bytes.len()) {
            break;
        }
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(buffer.get(..n).ok_or("HTTP 回复无效。")?);
                if bytes.len() > 96 * 1024 {
                    return Err("后端回复过大。".into());
                }
            }
            Err(e) if timed_out(&e) => {}
            Err(_) => return Err("AI 请求中断；未自动重试。".into()),
        }
    }
    if length.is_some_and(|n| n != bytes.len()) {
        return Err("后端回复不完整。".into());
    }
    let result: Value = serde_json::from_slice(&bytes).map_err(|_| "后端回复无效。")?;
    if !(200..300).contains(&status) {
        return Err(RequestFailure {
            rejected: status < 500 && result.get("code").and_then(Value::as_str).is_some(),
            message: format!(
                "{}; traceId={}",
                result.get("code").and_then(Value::as_str).unwrap_or("request_failed"),
                result.get("trace_id").and_then(Value::as_str).unwrap_or("-")
            ),
        });
    }
    if (path.ends_with("/execution") || path.ends_with("/input")) && (status != 202 || result.get("accepted").and_then(Value::as_bool) != Some(true))
    {
        return Err(RequestFailure { rejected: false, message: "后端回复无效。".into() });
    }
    Ok(result)
}
fn receive(address: SocketAddr, requests: mpsc::Receiver<Request>, tx: &mpsc::SyncSender<Event>, cancel: &Arc<AtomicBool>) -> Result<(), String> {
    let registration = post(address, "/v1/clients", "", &json!({"protocol_version":"v1"}), cancel)?;
    let client = registration.get("client_id").and_then(Value::as_str).ok_or("AI 客户端身份无效。")?.to_owned();
    let token = registration.get("client_token").and_then(Value::as_str).ok_or("AI 客户端令牌无效。")?.to_owned();
    if client.len() != 32 || token.len() != 32 || !client.bytes().all(|b| b.is_ascii_hexdigit()) || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("AI 运行身份无效。".into());
    }
    let mut stream = socket(address)?;
    write!(stream, "GET /v1/clients/{client}/events HTTP/1.0\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n")
        .map_err(|_| "AI 事件连接发送失败。")?;
    let mut reader = BufReader::new(stream);
    let (status, _) = headers(&mut reader, cancel)?;
    if status != 200 {
        return Err("无法建立 AI 事件连接。".into());
    }
    let mut kind = String::new();
    let mut data = String::new();
    let mut connection = String::new();
    let mut last_seq = 0u64;
    let mut requests = Some(requests);
    let mut worker = RequestWorker { thread: None, cancel: cancel.clone() };
    loop {
        let value = line(&mut reader, cancel, Duration::from_secs(35), 96 * 1024)?;
        if value.trim().is_empty() {
            if data.is_empty() {
                continue;
            }
            let event: Value = serde_json::from_str(&data).map_err(|_| "AI 事件 JSON 无效。")?;
            let seq = event.get("event_seq").and_then(Value::as_u64).ok_or("AI 事件编号无效。")?;
            let owner = event.get("connection_id").and_then(Value::as_str).ok_or("AI 事件归属无效。")?;
            if event.get("protocol_version").and_then(Value::as_str) != Some("v1") || (!connection.is_empty() && owner != connection) {
                return Err("AI 事件顺序或归属不匹配。".into());
            }
            if seq <= last_seq {
                kind.clear();
                data.clear();
                continue;
            }
            last_seq = seq;
            if kind == "connected" {
                if !connection.is_empty() {
                    return Err("重复 AI 连接。".into());
                }
                connection = owner.to_owned();
                emit(tx, Event::Connected { client: client.clone(), connection: connection.clone() }, cancel)?;
                let token = token.clone();
                let client = client.clone();
                let connection = connection.clone();
                let tx = tx.clone();
                let cancel = cancel.clone();
                let requests = requests.take().ok_or("重复 AI 请求线程。")?;
                // One ordered submission/request outlet; heartbeat uses the reader thread.
                worker.thread = Some(
                    std::thread::Builder::new()
                        .name("photo-companion-requests".into())
                        .spawn(move || {
                            while !cancel.load(Ordering::Relaxed) {
                                let mut request = match requests.recv_timeout(Duration::from_millis(100)) {
                                    Ok(r) => r,
                                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                    Err(_) => break,
                                };
                                if request.path == "/v1/rounds" {
                                    request.body["client_id"] = json!(client);
                                    request.body["connection_id"] = json!(connection);
                                }
                                let mut result = post_request(address, &request.path, &token, &request.body, &cancel);
                                // Local receipt correlation is never part of the backend request/model history.
                                if request.path.ends_with("/execution")
                                    && let Ok(response) = &mut result
                                    && let Some(object) = response.as_object_mut()
                                {
                                    object.insert("_client_execution_plan_id".into(), request.body.get("plan_id").cloned().unwrap_or(Value::Null));
                                }
                                if emit(
                                    &tx,
                                    Event::Response {
                                        submission: request.submission,
                                        round: request.round,
                                        path: request.path,
                                        identity: request.identity,
                                        result,
                                    },
                                    &cancel,
                                )
                                .is_err()
                                {
                                    cancel.store(true, Ordering::Relaxed);
                                    break;
                                }
                            }
                        })
                        .map_err(|_| "无法启动 AI 请求线程。")?,
                );
            } else if connection.is_empty() {
                return Err("AI 连接尚未确认。".into());
            } else if kind == "ping" {
                let ping = event.pointer("/payload/ping_seq").and_then(Value::as_u64).ok_or("心跳编号无效。")?;
                post(address, &format!("/v1/clients/{client}/heartbeat"), &token, &json!({"connection_id":connection,"ping_seq":ping}), cancel)?;
            } else {
                emit(tx, Event::Server { kind: kind.clone(), data: event }, cancel)?;
            }
            kind.clear();
            data.clear();
        } else if let Some(value) = value.strip_prefix("event:") {
            kind = value.trim().to_owned();
        } else if let Some(value) = value.strip_prefix("data:") {
            data.push_str(value.trim());
            if data.len() > 96 * 1024 {
                return Err("AI 事件过大。".into());
            }
        }
    }
}

#[cfg(test)]
pub(super) fn test_connection() -> (Connection, mpsc::Receiver<Request>, mpsc::SyncSender<Event>) {
    let (requests, receiver) = mpsc::sync_channel(8);
    let (sender, events) = mpsc::sync_channel(32);
    (Connection { requests, events, alive: Arc::new(AtomicBool::new(true)), cancel: Arc::new(AtomicBool::new(false)) }, receiver, sender)
}
