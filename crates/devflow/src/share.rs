//! Separate LAN listener: only this read-only router is reachable remotely.
use crate::{AppState, call_core, http, security};
use axum::{Json, Router, extract::{ConnectInfo, Request, State}, http::{HeaderMap, StatusCode, header}, middleware::{self, Next}, response::{IntoResponse, Response}, routing::{get, post}};
use devflow_core::workspace::{ShareCreateRequest, ShareReadRequest, ShareRevokeRequest};
use serde::Serialize;
use serde_json::{Value, json};
use std::{net::{IpAddr, SocketAddr, UdpSocket}, sync::Arc};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Network { pub address: Option<SocketAddr>, pub suggested_url: Option<String>, pub error: Option<String> }
pub async fn bind() -> (Option<tokio::net::TcpListener>, Network) {
    let configured = std::env::var("DEVFLOW_SHARE_BIND").unwrap_or_else(|_| "0.0.0.0:31082".into());
    let failed = |message| (None, Network { address: None, suggested_url: None, error: Some(message) });
    if configured == "off" { return failed("局域网分享已禁用（DEVFLOW_SHARE_BIND=off）".into()); }
    let address = match configured.parse::<SocketAddr>() {
        Ok(address) if address.is_ipv4() && (address.ip().is_unspecified() || address.ip().is_loopback() || matches!(address.ip(), IpAddr::V4(ip) if ip.is_private())) => address,
        _ => return failed("DEVFLOW_SHARE_BIND 应为局域网 IPv4 地址和端口，例如 0.0.0.0:31082".into()),
    };
    let listener = match tokio::net::TcpListener::bind(address).await { Ok(listener) => listener, Err(error) => return failed(format!("分享端口不可用：{error}")) };
    let actual = match listener.local_addr() { Ok(address) => address, Err(error) => return failed(error.to_string()) };
    let ip = if !actual.ip().is_unspecified() { Some(actual.ip()) } else {
        // UDP connect selects the route/interface without sending a datagram.
        UdpSocket::bind("0.0.0.0:0").ok().and_then(|socket| socket.connect("192.0.2.1:9").ok().and_then(|_| socket.local_addr().ok())).map(|address| address.ip())
    };
    let suggested_url = ip.filter(|ip| ip.is_loopback() || matches!(ip, IpAddr::V4(ip) if ip.is_private())).map(|ip| format!("http://{ip}:{}", actual.port()));
    (Some(listener), Network { address: Some(actual), suggested_url, error: None })
}
pub async fn list(State(state): State<Arc<AppState>>, headers: HeaderMap) -> http::ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    let network = state.share_network.clone();
    http::response(call_core(state, move |engine| Ok(json!({"network":network,"invitations":engine.workspace.share_list(authenticated)?}))).await)
}
pub async fn create(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(request): Json<ShareCreateRequest>) -> http::ApiResult {
    if state.share_network.address.is_none() { return http::response::<Value>(Err(devflow_core::workspace::WorkspaceError { code: "unavailable", message: state.share_network.error.clone().unwrap_or_else(|| "分享端口未启动".into()) })); }
    let authenticated = security::authenticated(&headers, &state);
    http::response(call_core(state, move |engine| engine.workspace.share_create(request, authenticated)).await)
}
pub async fn revoke(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(request): Json<ShareRevokeRequest>) -> http::ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    http::response(call_core(state, move |engine| engine.workspace.share_revoke(request, authenticated)).await)
}
fn allowed_host(value: &str, network: &Network) -> bool {
    let Some(address) = network.address else { return false; };
    if value == format!("localhost:{}", address.port()) || (value == "localhost" && address.port() == 80) { return address.ip().is_unspecified() || address.ip().is_loopback(); }
    let Ok(host) = value.parse::<SocketAddr>().or_else(|_| value.parse::<std::net::Ipv4Addr>().map(|ip| SocketAddr::new(IpAddr::V4(ip), 80))) else { return false; };
    host.port() == address.port() && host.ip().is_ipv4() && !host.ip().is_unspecified()
        && (host.ip().is_loopback() || matches!(host.ip(), IpAddr::V4(ip) if ip.is_private()))
        && (address.ip().is_unspecified() || host.ip() == address.ip())
}
async fn guard(State(state): State<Arc<AppState>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host = headers.get(header::HOST).and_then(|value| value.to_str().ok()).unwrap_or("");
    let origin_ok = headers.get(header::ORIGIN).is_none_or(|value| value.to_str().ok().and_then(|value| value.strip_prefix("http://")).is_some_and(|value| value == host));
    let peer_ok = peer.ip().is_loopback() || matches!(peer.ip(), IpAddr::V4(ip) if ip.is_private());
    if !peer_ok || !allowed_host(host, &state.share_network) || !origin_ok || headers.get("sec-fetch-site").is_some_and(|value| value == "cross-site") {
        return (StatusCode::FORBIDDEN, Json(json!({"code":"forbidden_origin","message":"请求来源不被允许"}))).into_response();
    }
    let mut response = next.run(request).await;
    for (name, value) in [("cache-control", "no-store"), ("referrer-policy", "no-referrer"), ("x-content-type-options", "nosniff"), ("x-frame-options", "DENY")] {
        response.headers_mut().insert(name, value.parse().unwrap());
    }
    response
}
async fn read(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(request): Json<ShareReadRequest>) -> http::ApiResult {
    let token = headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer ")).unwrap_or("").to_owned();
    let permit = match state.share_slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return Err((StatusCode::TOO_MANY_REQUESTS, Json(json!({"code":"busy","message":"分享服务繁忙，请稍后刷新"})))),
    };
    http::response(call_core(state, move |engine| { let _permit = permit; engine.share_read(&token, request) }).await)
}
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/shared/read", post(read))
        .route("/share", get(|| async { axum::response::Redirect::temporary("/share/") }))
        .route_service("/share/", ServeFile::new("apps/web/dist/index.html"))
        .nest_service("/assets", ServeDir::new("apps/web/dist/assets"))
        .nest_service("/vendor", ServeDir::new("apps/web/dist/vendor"))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lan_host_validation_rejects_external_hosts_and_wrong_ports() {
        let network = Network { address: Some("0.0.0.0:31082".parse().unwrap()), suggested_url: None, error: None };
        assert!(allowed_host("192.168.1.10:31082", &network));
        assert!(allowed_host("127.0.0.1:31082", &network));
        for value in ["192.168.1.10:31080", "example.com:31082", "8.8.8.8:31082", "192.168.1.10.attacker.test:31082", "0.0.0.0:31082"] { assert!(!allowed_host(value, &network)); }
    }
    #[tokio::test]
    async fn guest_listener_has_no_owner_or_mutation_routes_and_rejects_owner_token() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(AppState {
            engine: Arc::new(devflow_core::DevFlowEngine::new(vec![], None).unwrap()),
            api_token: "owner-secret-is-not-a-guest-token".into(),
            share_network: Network { address: Some(address), suggested_url: None, error: None },
            share_slots: Arc::new(tokio::sync::Semaphore::new(8)),
        });
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
        let server = tokio::spawn(async move { axum::serve(listener, app).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
        for (path, body, expected) in [
            ("/api/session", "{}", "404"),
            ("/api/workspace", "{}", "404"),
            ("/api/workspace/file", "{}", "404"),
            ("/api/workspace/git/commit/apply", "{}", "404"),
            ("/api/workspace/shares", "{}", "404"),
            ("/mcp", "{}", "404"),
            ("/api/shared/read", "{\"type\":\"workspace\"}", "401"),
            ("/api/shared/read", "{\"type\":\"write\",\"path\":\"a\"}", "422"),
        ] {
            let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
            let request = format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer owner-secret-is-not-a-guest-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            tokio::time::timeout(std::time::Duration::from_secs(5), stream.read_to_end(&mut response)).await.unwrap().unwrap();
            let response = String::from_utf8(response).unwrap();
            assert!(response.starts_with(&format!("HTTP/1.1 {expected}")), "{path}: {response}");
            assert!(!response.contains("\"token\":"));
        }
        stop.send(()).unwrap(); server.await.unwrap();
    }
}
