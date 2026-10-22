use crate::{AppState, call_core, security};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use devflow_core::workspace::{WorkspaceChildrenRequest, WorkspaceError, WorkspaceRequest};
use serde_json::{Value, json};
use std::sync::Arc;

mod terminal_socket;
pub use terminal_socket::terminal_socket;

pub(crate) type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
pub(crate) fn response<T: serde::Serialize>(result: Result<T, WorkspaceError>) -> ApiResult {
    match result {
        Ok(value) => Ok(Json(json!(value))),
        Err(err) => {
            let status = match err.code {
                "unauthorized" => StatusCode::UNAUTHORIZED,
                "outside_roots" | "outside_workspace" | "excluded_directory"
                | "permission_denied" => StatusCode::FORBIDDEN,
                "not_found" => StatusCode::NOT_FOUND,
                "conflict" | "file_conflict" | "already_exists" | "unsaved_changes"
                | "duplicate_folder" => StatusCode::CONFLICT,
                "unavailable" | "io_error" => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::BAD_REQUEST,
            };
            Err((status, Json(json!(err))))
        }
    }
}
pub async fn health() -> Json<Value> {
    Json(json!({"status":"ok","name":"devflow","version":env!("CARGO_PKG_VERSION")}))
}
pub async fn session(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({"token": state.api_token}))
}

pub async fn board_read(
    State(state): State<Arc<AppState>>, headers: HeaderMap,
    Query(request): Query<devflow_core::workspace::BoardRead>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(call_core(state, move |e| e.workspace.board_read(request, authenticated)).await)
}
pub async fn board_execute(
    State(state): State<Arc<AppState>>, headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::BoardRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(call_core(state, move |e| e.workspace.board_execute(request, authenticated, "你", true)).await)
}
#[derive(serde::Deserialize)]
pub struct WorkspaceQuery {
    #[serde(default)]
    refresh: bool,
}
pub async fn projects(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceQuery>,
) -> ApiResult {
    response(
        call_core(state, move |engine| {
            if query.refresh {
                engine.workspace.refresh()
            } else {
                engine.workspace.snapshot()
            }
        })
        .await,
    )
}
pub async fn workspace_catalog(State(state): State<Arc<AppState>>) -> ApiResult {
    response(call_core(state, |engine| engine.workspace.catalog()).await)
}
pub async fn workspace_browse(
    State(state): State<Arc<AppState>>,
    Query(request): Query<devflow_core::workspace::WorkspaceBrowseRequest>,
) -> ApiResult {
    response(call_core(state, move |engine| engine.workspace.browse(request)).await)
}
pub async fn workspace_pick(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::WorkspacePickRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.pick_local(request, authenticated)
        })
        .await,
    )
}
pub async fn workspace_pick_cancel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::WorkspacePickCancelRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(call_core(state, move |engine| engine.workspace.cancel_local_pick(request, authenticated)).await)
}
pub async fn workspace_file_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(request): Query<devflow_core::workspace::WorkspaceFileReadRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.read_file(request, authenticated)
        })
        .await,
    )
}
pub async fn workspace_search(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::WorkspaceSearchRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.search(request, authenticated)
        })
        .await,
    )
}
pub async fn workspace_file_write(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::WorkspaceFileWriteRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.write_file(request, authenticated)
        })
        .await,
    )
}
pub async fn workspace_file_copy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::WorkspaceFileReadRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.copy_file(request, authenticated)
        })
        .await,
    )
}
pub async fn workspace_file_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::WorkspaceFileReadRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.delete_file(request, authenticated)
        })
        .await,
    )
}
pub async fn workspace_replace_preview(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::ReplacePreviewRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.replace_preview(request, authenticated)
        })
        .await,
    )
}
pub async fn workspace_replace_apply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::ReplaceApplyRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.replace_apply(request, authenticated)
        })
        .await,
    )
}
pub async fn workspace_children(
    State(state): State<Arc<AppState>>,
    Query(request): Query<WorkspaceChildrenRequest>,
) -> ApiResult {
    response(call_core(state, move |engine| engine.workspace.children(request)).await)
}
pub async fn workspace_execute(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<WorkspaceRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.execute(request, authenticated)
        })
        .await,
    )
}
pub async fn runtime_projects(State(state): State<Arc<AppState>>) -> ApiResult {
    response(call_core(state, |engine| engine.runtime_snapshot()).await)
}
pub async fn devflow_metrics(State(state): State<Arc<AppState>>) -> ApiResult {
    response(
        call_core(state, |engine| {
            engine.runtime.self_snapshot().map_err(Into::into)
        })
        .await,
    )
}
pub async fn java_execute(
    State(state): State<Arc<AppState>>,
    Json(request): Json<devflow_core::runtime::java::JavaRequest>,
) -> ApiResult {
    response(call_core(state, move |engine| engine.java_execute(request)).await)
}
pub async fn java_options(
    State(state): State<Arc<AppState>>,
    Query(query): Query<devflow_core::runtime::java::JavaQuery>,
) -> ApiResult {
    response(
        call_core(state, move |engine| {
            devflow_core::runtime::java::options(
                &engine
                    .workspace
                    .runtime_project(&query.project_path, None)?,
            )
        })
        .await,
    )
}
pub async fn java_output(
    State(state): State<Arc<AppState>>,
    Query(query): Query<devflow_core::runtime::java::JavaQuery>,
) -> ApiResult {
    response(
        call_core(state, move |engine| {
            engine.java.output(
                &engine
                    .workspace
                    .runtime_project(&query.project_path, None)?,
                query.after,
                query.run_id.as_deref(),
            )
        })
        .await,
    )
}

pub async fn node_execute(
    State(state): State<Arc<AppState>>,
    Json(request): Json<devflow_core::runtime::node::NodeRequest>,
) -> ApiResult {
    response(call_core(state, move |engine| engine.node_execute(request)).await)
}
pub async fn node_options(
    State(state): State<Arc<AppState>>,
    Query(query): Query<devflow_core::runtime::node::NodeQuery>,
) -> ApiResult {
    response(
        call_core(state, move |engine| {
            let project = engine.workspace.node_runtime_project(
                &query.project_path,
                query.script.as_deref(),
                None,
            )?;
            devflow_core::runtime::node::options(&project)
        })
        .await,
    )
}
pub async fn node_output(
    State(state): State<Arc<AppState>>,
    Query(query): Query<devflow_core::runtime::node::NodeQuery>,
) -> ApiResult {
    response(
        call_core(state, move |engine| {
            let project = engine.workspace.node_runtime_project(
                &query.project_path,
                query.script.as_deref(),
                None,
            )?;
            engine
                .node
                .output(&project, query.after, query.run_id.as_deref())
        })
        .await,
    )
}

/// Owner-only PTY operations; never mounted on the read-only sharing listener.
pub async fn terminal_execute(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<devflow_core::workspace::TerminalRequest>,
) -> ApiResult {
    let authenticated = security::authenticated(&headers, &state);
    response(
        call_core(state, move |engine| {
            engine.workspace.terminal_execute(request, authenticated)
        })
        .await,
    )
}
