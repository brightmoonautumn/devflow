mod http;
mod mcp;

use axum::{
    Router,
    routing::{get, post},
};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
pub struct AppState {
    pub workspace_root: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "devflow=info".into()),
        )
        .init();
    let root = std::env::var_os("DEVFLOW_WORKSPACE").map(PathBuf::from);
    let root = match root {
        Some(root) => Some(
            root.canonicalize()
                .map_err(|err| format!("DEVFLOW_WORKSPACE is invalid: {err}"))?,
        ),
        None => None,
    };
    let state = Arc::new(AppState {
        workspace_root: root,
    });
    let app = Router::new()
        .route("/api/health", get(http::health))
        .route("/api/projects", get(http::projects))
        .route("/mcp", post(mcp::handle))
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
    axum::serve(listener, app).await?;
    Ok(())
}
