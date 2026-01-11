//! Web server setup and routing

use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use super::handlers;
use super::state::AppState;

/// Start the web UI server
pub async fn start_server(port: u16, results_dir: PathBuf) -> anyhow::Result<()> {
    let state = Arc::new(AppState::new(results_dir));

    // Load existing results
    if let Err(e) = state.load_results().await {
        tracing::warn!("Failed to load initial results: {}", e);
    }

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        // HTML pages
        .route("/", get(handlers::index))
        .route("/live", get(handlers::live_page))
        .route("/results", get(handlers::results_page))
        .route("/eval/:eval_id", get(handlers::eval_detail_page))
        .route("/session/:session_id", get(handlers::session_detail_page))
        // API endpoints
        .route("/api/health", get(handlers::health))
        .route("/api/sessions", get(handlers::api_list_sessions))
        .route("/api/sessions/:session_id", get(handlers::api_get_session))
        .route(
            "/api/sessions/:session_id/logs",
            get(handlers::api_get_session_logs),
        )
        .route("/api/results", get(handlers::api_list_results))
        .route("/api/results/:eval_id", get(handlers::api_get_result))
        .route("/api/results/refresh", post(handlers::api_refresh_results))
        .layer(cors)
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting web UI server on http://localhost:{}", port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
