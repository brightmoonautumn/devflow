mod http;
mod mcp;
mod security;
mod share;

use axum::{
    Router,
    routing::{get, post},
};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub engine: Arc<devflow_core::DevFlowEngine>,
    pub api_token: String,
    pub share_network: share::Network,
    pub share_slots: Arc<tokio::sync::Semaphore>,
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args_os().nth(1).as_deref()
        == Some(std::ffi::OsStr::new("--devflow-runtime-worker"))
    {
        let directory = std::env::args_os()
            .nth(2)
            .ok_or("Missing runtime worker directory")?;
        devflow_core::runtime::managed::run_worker(std::path::Path::new(&directory))?;
        return Ok(());
    }
    serve()
}

#[tokio::main]
async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "devflow=info".into()),
        )
        .init();
    let root = std::env::var_os("DEVFLOW_WORKSPACE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| path.canonicalize())
        .transpose()?;
    let mut roots: Vec<PathBuf> = std::env::var_os("DEVFLOW_ALLOWED_ROOTS")
        .map(|value| {
            std::env::split_paths(&value)
                .filter(|path| !path.as_os_str().is_empty())
                .collect()
        })
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
    let restore_recent =
        root.is_none() || std::env::var("DEVFLOW_RESTORE_RECENT").is_ok_and(|value| value == "1");
    let engine = devflow_core::DevFlowEngine::new(roots, None)?;
    devflow_core::runtime::managed::start_maintenance()?;
    engine.workspace.configure_recent(
        std::env::current_dir()?.join(".devflow/recent-workspace.json"),
        root,
        restore_recent,
    )?;
    let (share_listener, share_network) = share::bind().await;
    if let Some(error) = &share_network.error {
        tracing::warn!("{error}");
    }
    let state = Arc::new(AppState {
        engine: Arc::new(engine),
        api_token,
        share_network,
        share_slots: Arc::new(tokio::sync::Semaphore::new(8)),
    });
    let guest_router = share::router(state.clone());
    let terminal_workspace = state.engine.workspace.clone();
    let app = Router::new()
        .route("/api/health", get(http::health))
        .route("/api/workspace/terminal", post(http::terminal_execute))
        .route("/api/workspace/terminal/socket", get(http::terminal_socket))
        .route(
            "/api/workspace/shares",
            get(share::list).post(share::create),
        )
        .route("/api/workspace/shares/revoke", post(share::revoke))
        .route("/api/session", get(http::session))
        .route("/api/workspace/board", get(http::board_read).post(http::board_execute))
        .route("/api/projects", get(http::projects))
        .route(
            "/api/workspace",
            get(http::projects).post(http::workspace_execute),
        )
        .route("/api/workspace/catalog", get(http::workspace_catalog))
        .route("/api/workspace/browse", get(http::workspace_browse))
        .route("/api/workspace/pick", post(http::workspace_pick))
        .route("/api/workspace/pick/cancel", post(http::workspace_pick_cancel))
        .route("/api/workspace/children", get(http::workspace_children))
        .route("/api/workspace/search", post(http::workspace_search))
        .route(
            "/api/workspace/replace/preview",
            post(http::workspace_replace_preview),
        )
        .route(
            "/api/workspace/replace/apply",
            post(http::workspace_replace_apply),
        )
        .route(
            "/api/workspace/file",
            get(http::workspace_file_read)
                .put(http::workspace_file_write)
                .delete(http::workspace_file_delete)
                .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        .route("/api/workspace/file/copy", post(http::workspace_file_copy))
        .route("/api/runtime/projects", get(http::runtime_projects))
        .route("/api/runtime/self", get(http::devflow_metrics))
        .route("/api/runtime/diagnostics", get(http::runtime_diagnostics))
        .route("/api/runtime/diagnosis", get(http::diagnosis_get).post(http::diagnosis_execute))
        .route("/api/runtime/java", post(http::java_execute))
        .route("/api/runtime/java/options", get(http::java_options))
        .route("/api/runtime/java/output", get(http::java_output))
        .route("/api/runtime/node", post(http::node_execute))
        .route("/api/runtime/node/options", get(http::node_options))
        .route("/api/runtime/node/output", get(http::node_output))
        .route(
            "/mcp",
            post(mcp::handle).layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        // Authenticate API/MCP routes; the browser must load the static shell
        // before it can obtain a session token. Add the static fallback below.
        .route_layer(axum::middleware::from_fn_with_state(
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

    let port = std::env::var("DEVFLOW_PORT")
        .unwrap_or_else(|_| "31080".into())
        .parse::<u16>()?;
    if port == 0 {
        return Err("DEVFLOW_PORT must be between 1 and 65535".into());
    }
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("DevFlow at http://{addr}");
    if let Some(share_listener) = share_listener {
        tokio::spawn(async move {
            if let Err(error) = axum::serve(
                share_listener,
                guest_router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            {
                tracing::error!("只读分享监听失败：{error}");
            }
        });
    }
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    terminal_workspace.close_terminals()?;
    result?;
    Ok(())
}
