use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};

use serde_json;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

use crate::telemetry::data::Telemetry;
pub struct AppState {
    // Broadcasts telemetry to all connected WebSocket clients
    // When telemetry comes in, we shout it out to every connected dashboard
    pub telemetry_tx: broadcast::Sender<Telemetry>,
    // Sends commands from Web interface to reocket : any dashboards can send commands,
    // but they all funnel down to one single "Radio Link" task that talks to the rocke
    pub command_tx: mpsc::Sender<String>,
}

// 2. WebSocket Upgrade
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

pub async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut telemetry_rx = state.telemetry_tx.subscribe();

    loop {
        tokio::select! {
            // Forward Telemetry -> Browser
            res = telemetry_rx.recv() => {
                match res {
                    Ok(data) => {
                        if let Ok(msg) = serde_json::to_string(&data) {
                            if socket.send(Message::Text(msg)).await.is_err() {
                                break; // Client disconnected, exit loop
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // We are receiving data faster than we can send it.
                        // We just continue to the next loop to get the newest data.
                        continue;
                    }
                    Err(_) => break, // Channel closed
                }
            }

            // Receive Command <- Browser
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let _ = state.command_tx.send(text).await;
                    }
                    _ => break, // Socket closed or error
                }
            }
        }
    }
}
