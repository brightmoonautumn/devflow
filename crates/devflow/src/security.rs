use crate::AppState;
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::sync::Arc;

fn local_host(value: &str) -> bool {
    matches!(
        value,
        "127.0.0.1:31080" | "localhost:31080" | "127.0.0.1:5173" | "localhost:5173"
    )
}
pub fn authenticated(headers: &HeaderMap, state: &AppState) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.api_token)
}
pub async fn guard(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host_ok = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .is_some_and(local_host);
    let origin_ok = match headers.get("origin") {
        None => true,
        Some(origin) => origin
            .to_str()
            .ok()
            .and_then(|s| s.strip_prefix("http://"))
            .is_some_and(local_host),
    };
    let cross_site = headers
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site");
    if !host_ok || !origin_ok || cross_site {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"code":"forbidden_origin","message":"请求来源不被允许"})),
        )
            .into_response();
    }
    let path = request.uri().path();
    if path == "/api/session" {
        // A custom header forces cross-origin browser requests through preflight;
        // no CORS permission is granted. Host validation also blocks DNS rebinding.
        if headers.get("x-devflow-client").is_none_or(|v| v != "web") {
            return StatusCode::FORBIDDEN.into_response();
        }
    } else if path != "/api/health" && !authenticated(headers, &state) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"unauthorized","message":"需要有效的访问令牌"})),
        )
            .into_response();
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restricts_hosts_and_rebinding() {
        assert!(local_host("127.0.0.1:31080"));
        assert!(local_host("localhost:5173"));
        assert!(!local_host("127.0.0.1.attacker.test:31080"));
        assert!(!local_host("localhost:31080@attacker.test"));
        assert!(!local_host("attacker.test:31080"));
    }
}
