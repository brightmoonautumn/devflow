//! Transport only: terminal authorization, state and mutations live in core.
use super::*;
use axum::{extract::ws::{Message, WebSocket, WebSocketUpgrade}, response::{IntoResponse, Response}};
use devflow_core::workspace::{TerminalOperation, TerminalRequest};
use serde::Deserialize;
use std::{collections::VecDeque, time::Duration};
use tokio::sync::Semaphore;

static CONNECTIONS: Semaphore = Semaphore::const_new(24);
const WINDOW: u64 = 256 * 1024;
const PAGE: u64 = 64 * 1024;

#[derive(Default)]
struct OutputWindow {
    frames: VecDeque<(u64, u64)>,
    bytes: u64,
}
impl OutputWindow {
    fn writable(&self) -> bool { self.bytes <= WINDOW - PAGE }
    fn sent(&mut self, cursor: u64, length: u64) {
        self.frames.push_back((cursor, length));
        self.bytes += length;
    }
    fn acknowledge(&mut self, cursor: u64) -> Result<(), WorkspaceError> {
        let index = self.frames.iter().position(|(end, _)| *end == cursor)
            .ok_or_else(|| failure("invalid_query", "终端输出确认游标无效"))?;
        for (_, bytes) in self.frames.drain(..=index) { self.bytes -= bytes; }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Attach {
    token: String,
    expected_revision: u64,
    id: String,
    after: u64,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Control {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
    Ack { cursor: u64 },
}

pub async fn terminal_socket(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    let Ok(permit) = CONNECTIONS.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    ws.max_message_size(64 * 1024).max_frame_size(64 * 1024)
        .write_buffer_size(0).max_write_buffer_size(512 * 1024)
        .on_upgrade(move |mut socket| async move {
            let _permit = permit;
            if let Err(error) = run(&mut socket, state).await {
                let _ = send(&mut socket, Message::Text(json!({ "type": "error", "code": error.code, "message": error.message }).to_string().into())).await;
            }
            let _ = send(&mut socket, Message::Close(None)).await;
        })
}

fn failure(code: &'static str, message: impl Into<String>) -> WorkspaceError {
    WorkspaceError { code, message: message.into() }
}

async fn send(socket: &mut WebSocket, message: Message) -> Result<(), WorkspaceError> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message)).await
        .map_err(|_| failure("unavailable", "终端连接发送超时"))?
        .map_err(|_| failure("unavailable", "终端连接已断开"))
}

async fn run(socket: &mut WebSocket, state: Arc<AppState>) -> Result<(), WorkspaceError> {
    // Authenticate in the first frame, never put the owner token in a URL/log.
    let message = tokio::time::timeout(Duration::from_secs(5), socket.recv()).await
        .map_err(|_| failure("unauthorized", "终端连接认证超时"))?;
    let Some(Ok(Message::Text(text))) = message else { return Err(failure("unauthorized", "终端连接需要认证")); };
    let attach: Attach = serde_json::from_str(&text).map_err(|_| failure("invalid_query", "终端连接参数无效"))?;
    if attach.token != state.api_token { return Err(failure("unauthorized", "需要有效的访问令牌")); }
    let revision = attach.expected_revision;
    let id = attach.id;
    let subscribe_id = id.clone();
    let mut changed = call_core(state.clone(), move |engine| engine.workspace.terminal_subscribe(revision, subscribe_id, true)).await?;
    send(socket, Message::Text(json!({ "type": "ready" }).to_string().into())).await?;
    let mut cursor = attach.after;
    let mut window = OutputWindow::default();
    let mut readable = true;
    let mut exit_sent = false;
    let mut check = tokio::time::interval(Duration::from_secs(1));
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            message = socket.recv() => {
                let Some(Ok(message)) = message else { return Ok(()); };
                let control = match message {
                    Message::Text(text) => serde_json::from_str::<Control>(&text).map_err(|_| failure("invalid_query", "终端控制消息无效"))?,
                    Message::Close(_) => return Ok(()),
                    Message::Ping(_) | Message::Pong(_) => continue,
                    _ => return Err(failure("invalid_query", "终端输入需要控制消息")),
                };
                let operation = match control {
                    Control::Ack { cursor: acknowledged } => {
                        // Only acknowledge complete frames we actually sent.
                        window.acknowledge(acknowledged)?;
                        readable = true;
                        continue;
                    },
                    Control::Input { data } => TerminalOperation::Input { id: id.clone(), data },
                    Control::Resize { cols, rows } => TerminalOperation::Resize { id: id.clone(), cols, rows },
                };
                let input = matches!(&operation, TerminalOperation::Input { .. });
                let result = call_core(state.clone(), move |engine| engine.workspace.terminal_stream_execute(TerminalRequest { expected_revision: revision, operation }, true)).await;
                match result {
                    Ok(()) => {
                        if input { send(socket, Message::Text(json!({ "type": "input_ack" }).to_string().into())).await?; }
                    },
                    Err(error) if error.code == "conflict" && error.message.contains("输入队列") => {
                        // Do not replay potentially executed input on reconnect.
                        send(socket, Message::Text(json!({ "type": "input_error", "message": error.message }).to_string().into())).await?;
                    },
                    Err(error) => return Err(error),
                }
            },
            _ = async {}, if readable && window.writable() => {
                let read_id = id.clone();
                let page = call_core(state.clone(), move |engine| engine.workspace.terminal_stream_read(revision, &read_id, cursor, true)).await?;
                if page.truncated {
                    send(socket, Message::Text(json!({ "type": "reset" }).to_string().into())).await?;
                }
                let length = page.data.len() as u64;
                cursor = page.next_cursor;
                readable = length > 0;
                if length > 0 {
                    // Eight-byte big-endian nextCursor followed by raw PTY bytes.
                    let mut frame = Vec::with_capacity(8 + page.data.len());
                    frame.extend_from_slice(&cursor.to_be_bytes());
                    frame.extend_from_slice(&page.data);
                    send(socket, Message::Binary(frame.into())).await?;
                    window.sent(cursor, length);
                } else if page.exited {
                    if !exit_sent {
                        send(socket, Message::Text(json!({ "type": "exit", "exitCode": page.exit_code }).to_string().into())).await?;
                        exit_sent = true;
                    }
                    // ConPTY can keep the output pipe open after shell exit.
                    // Show exit promptly, but continue draining late PTY output.
                    if page.eof { return Ok(()); }
                }
            },
            result = changed.changed() => {
                if result.is_err() { return Err(failure("not_found", "终端已关闭")); }
                readable = true;
            },
            _ = check.tick() => {
                // Check revision/closure even when a hidden client stops ACKing.
                // This is a lifecycle check, output itself wakes via watch.
                let check_id = id.clone();
                call_core(state.clone(), move |engine| engine.workspace.terminal_stream_read(revision, &check_id, cursor, true)).await?;
                readable = true;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn handshake(address: std::net::SocketAddr, origin: &str) -> (tokio::net::TcpStream, String) {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let request = format!("GET /api/workspace/terminal/socket HTTP/1.1\r\nHost: localhost:5173\r\n{origin}Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(stream.read_u8().await.unwrap());
            assert!(headers.len() < 8192);
        }
        (stream, String::from_utf8(headers).unwrap())
    }

    async fn first_message(stream: &mut tokio::net::TcpStream, value: Value) -> Value {
        let bytes = value.to_string().into_bytes();
        let mask = [1u8, 2, 3, 4];
        let mut frame = vec![0x81];
        if bytes.len() < 126 { frame.push(0x80 | bytes.len() as u8); }
        else { frame.push(0x80 | 126); frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes()); }
        frame.extend_from_slice(&mask);
        frame.extend(bytes.iter().enumerate().map(|(index, byte)| byte ^ mask[index % 4]));
        stream.write_all(&frame).await.unwrap();
        let opcode = stream.read_u8().await.unwrap();
        assert_eq!(opcode & 0xf, 1);
        let length = stream.read_u8().await.unwrap();
        assert_eq!(length & 0x80, 0);
        let length = if length == 126 { stream.read_u16().await.unwrap() as usize } else { length as usize };
        assert!(length < 4096);
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload).await.unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    #[tokio::test]
    async fn upgrade_rejects_bad_origins_and_first_frame_auth_precedes_core_access() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(AppState {
            engine: Arc::new(devflow_core::DevFlowEngine::new(vec![], None).unwrap()),
            api_token: "socket-owner-secret".into(),
            share_network: crate::share::Network { address: None, suggested_url: None, error: None },
            share_slots: Arc::new(Semaphore::new(8)),
        });
        let app = axum::Router::new().route("/api/workspace/terminal/socket", axum::routing::get(terminal_socket))
            .route_layer(axum::middleware::from_fn_with_state(state.clone(), security::guard)).with_state(state);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        tokio::time::timeout(Duration::from_secs(5), async {
            for origin in ["", "Origin: http://attacker.test\r\n"] {
                let (_, headers) = handshake(address, origin).await;
                assert!(headers.starts_with("HTTP/1.1 403"));
            }
            for (token, revision, expected) in [("wrong", 99, "unauthorized"), ("socket-owner-secret", 99, "conflict"), ("socket-owner-secret", 0, "not_found")] {
                let (mut stream, headers) = handshake(address, "Origin: http://localhost:5173\r\n").await;
                assert!(headers.starts_with("HTTP/1.1 101"));
                let message = first_message(&mut stream, json!({ "token": token, "expectedRevision": revision, "id": "missing", "after": 0 })).await;
                assert_eq!(message["type"], "error");
                assert_eq!(message["code"], expected);
            }
        }).await.unwrap();
        server.abort();
    }

    #[test]
    fn window_bounds_pending_bytes_and_accepts_only_sent_frame_cursors() {
        let mut window = OutputWindow::default();
        for index in 1..=4 { assert!(window.writable()); window.sent(index * PAGE, PAGE); }
        assert!(!window.writable());
        assert!(window.acknowledge(PAGE + 1).is_err());
        assert_eq!(window.bytes, WINDOW);
        window.acknowledge(2 * PAGE).unwrap();
        assert_eq!(window.bytes, 2 * PAGE);
        assert!(window.writable());
        assert!(window.acknowledge(PAGE).is_err());
        // A ring-buffer truncation can jump the byte cursor; credit counts the
        // bytes actually sent, not the cursor distance across dropped output.
        window.sent(10 * PAGE, 7);
        window.acknowledge(10 * PAGE).unwrap();
        assert_eq!(window.bytes, 0);
    }
}
