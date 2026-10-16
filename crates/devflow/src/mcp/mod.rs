//! Minimal stateless Streamable HTTP JSON-RPC MCP seed implementation.
//! Before production: add authenticated sessions, protocol compliance tests, and tools lifecycle.
use crate::AppState;
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub async fn handle(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    // Protect loopback service from cross-site browser calls.
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if origin != "http://127.0.0.1:31080" && origin != "http://localhost:31080" {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    let Some(id) = request.get("id") else {
        // Notifications have no JSON-RPC response.
        return Err(StatusCode::ACCEPTED);
    };
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(json!({}));
    let result: Result<Value, Value> = match method {
        "initialize" => Ok(json!({
            "protocolVersion":"2025-03-26",
            "capabilities":{"tools":{"listChanged":false}},
            "serverInfo":{"name":"devflow","version":env!("CARGO_PKG_VERSION")}
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools":[
            {"name":"workspace_list","description":"List Maven and Node projects under the configured local workspace root", "inputSchema":{"type":"object","properties":{},"additionalProperties":false}}
        ]})),
        "tools/call" => match params.get("name").and_then(Value::as_str) {
            Some("workspace_list") => {
                let projects = state
                    .workspace_root
                    .as_ref()
                    .and_then(|root| devflow_core::workspace::detect_projects(root).ok())
                    .unwrap_or_default();
                Ok(
                    json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&projects).unwrap_or_default()}]}),
                )
            }
            _ => Err(json!({"code":-32602,"message":"Unknown tool"})),
        },
        _ => Err(json!({"code":-32601,"message":"Method not found"})),
    };
    Ok(Json(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => json!({"jsonrpc":"2.0","id":id,"error":error}),
    }))
}
