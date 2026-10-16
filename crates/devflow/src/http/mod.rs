use crate::AppState;
use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};
use std::sync::Arc;

pub async fn health() -> Json<Value> {
    Json(json!({"status":"ok","name":"devflow","version":env!("CARGO_PKG_VERSION")}))
}

pub async fn projects(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(root) = &state.workspace_root else {
        return Ok(Json(
            json!({"workspaceRoot":null,"projects":[],"hint":"Set DEVFLOW_WORKSPACE to an existing folder"}),
        ));
    };
    match devflow_core::workspace::detect_projects(root) {
        Ok(projects) => Ok(Json(
            json!({"workspaceRoot":root.display().to_string(),"projects":projects}),
        )),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":err.to_string()})),
        )),
    }
}
