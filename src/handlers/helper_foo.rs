use crate::handlers::sockets_handler::{AppState, ws_handler};
use axum::{Router, routing::get};
use std::sync::Arc;
use tower_http::services::ServeFile;
use tracing::{info};


// Extracted server startup so we don't write it twice
pub async fn start_server(shared_state: Arc<AppState>, bind_address: &str) {
    let app = Router::new()
        .nest_service("/", ServeFile::new("html/index.html"))
        .route("/ws", get(ws_handler))
        .with_state(shared_state);

    let listener = tokio::net::TcpListener::bind(&bind_address).await.unwrap();
    info!("Ground Station UI available at: http://{}", &bind_address);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal()) //
        .await
        .unwrap();
}

async fn shutdown_signal() { //
    tokio::signal::ctrl_c().await.unwrap();
    info!("Shutdown signal received. Powering down.");
}