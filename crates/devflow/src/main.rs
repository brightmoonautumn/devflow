mod http;
mod mcp;
mod security;

use axum::{
    Router,
    routing::{get, post},
};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub engine: Arc<devflow_core::DevFlowEngine>,
    pub api_token: String,
}

pub async fn call_core<T: Send + 'static>(
    state: Arc<AppState>,
    call: impl FnOnce(
        &devflow_core::DevFlowEngine,
    ) -> Result<T, devflow_core::workspace::WorkspaceError>
    + Send
    + 'static,
) -> Result<T, devflow_core::workspace::WorkspaceError> {
    tokio::task::spawn_blocking(move || call(&state.engine))
        .await
        .map_err(|_| devflow_core::workspace::WorkspaceError {
            code: "unavailable",
            message: "后台任务执行失败".into(),
        })?
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "devflow=info".into()),
        )
        .init();
    let root = std::env::var_os("DEVFLOW_WORKSPACE")
        .map(PathBuf::from)
        .map(|path| path.canonicalize())
        .transpose()?;
    let mut roots: Vec<PathBuf> = std::env::var_os("DEVFLOW_ALLOWED_ROOTS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    if let Some(initial) = &root {
        let initial = initial.canonicalize()?;
        roots.push(if initial.is_dir() {
            initial
        } else {
            initial.parent().ok_or("Invalid workspace file")?.to_owned()
        });
    }
    let api_token = std::env::var("DEVFLOW_API_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let state = Arc::new(AppState {
        engine: Arc::new(devflow_core::DevFlowEngine::new(roots, root)?),
        api_token,
    });
    let app = Router::new()
        .route("/api/health", get(http::health))
        .route("/api/session", get(http::session))
        .route("/api/projects", get(http::projects))
        .route(
            "/api/workspace",
            get(http::projects).post(http::workspace_execute),
        )
        .route("/api/workspace/catalog", get(http::workspace_catalog))
        .route("/api/workspace/browse", get(http::workspace_browse))
        .route("/api/workspace/children", get(http::workspace_children))
        .route("/api/runtime/projects", get(http::runtime_projects))
        .route("/api/runtime/self", get(http::devflow_metrics))
        .route("/api/runtime/java", post(http::java_execute))
        .route("/api/runtime/java/options", get(http::java_options))
        .route("/api/runtime/java/output", get(http::java_output))
        .route("/mcp", post(mcp::handle))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            security::guard,
        ))
        .with_state(state);

    // With a built frontend, serve static assets. In dev, Vite proxies API requests.
    let app = if PathBuf::from("apps/web/dist/index.html").exists() {
        app.fallback_service(
            ServeDir::new("apps/web/dist")
                .not_found_service(ServeFile::new("apps/web/dist/index.html")),
        )
    } else {
        app
    };

    let addr: SocketAddr = "127.0.0.1:31080".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("DevFlow at http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
