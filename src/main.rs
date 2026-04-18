use axum::{Router, routing::get};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tower_http::services::ServeFile;

pub use crate::telemetry::data::Telemetry;

mod mock;
mod handlers;
mod telemetry;

// Only include the hardware module if we are NOT mocking
#[cfg(not(feature = "mock"))]
mod hardware_cfg;
#[cfg(not(feature = "mock"))]
pub use crate::hardware_cfg::{Hardware, HardwareError};
#[cfg(not(feature = "mock"))]
use embedded_hal::digital::v2::InputPin;
#[cfg(not(feature = "mock"))]
use tokio::time::Duration;

use handlers::sockets_handler::{AppState, ws_handler};

#[tokio::main]
async fn main() {
    let (telemetry_tx, _) = broadcast::channel::<Telemetry>(100);
    let (command_tx, mut command_rx) = mpsc::channel::<String>(32);

    // ==========================================
    // TIMELINE 1: MOCK MODE (Compiled on Laptop)
    // ==========================================
    #[cfg(feature = "mock")]
    {
        println!("🚀 Starting Groundstation in COMPILE-TIME MOCK MODE...");

        let shared_state = Arc::new(AppState {
            telemetry_tx: telemetry_tx.clone(),
            command_tx,
        });

        // 1. Start Mock Ingest
        mock::spawn_mock_telemetry_task(shared_state.telemetry_tx.clone());

        // 2. Start Mock Transmit Task
        tokio::spawn(async move {
            while let Some(cmd) = command_rx.recv().await {
                println!("[MOCK TX] Pretending to send command: {}", cmd);
            }
        });

        start_server(shared_state).await;
    }

    // ==========================================
    // TIMELINE 2: HARDWARE MODE (Compiled on Pi)
    // ==========================================
    #[cfg(not(feature = "mock"))]
    {
        println!("📡 Starting Groundstation in REAL HARDWARE MODE...");

        const CS_GPIO: u64 = 10;
        const RST_GPIO: u64 = 5;

        match Hardware::new(CS_GPIO.into(), RST_GPIO.into()) {
            Ok(mut hw) => {
                if let Err(e) = hw.init_radio() {
                    eprintln!("RFM95 initialization failed: {}", e);
                    std::process::exit(1);
                }
                println!("RFM95 LoRa Radio initialized!");

                let shared_state = Arc::new(AppState {
                    telemetry_tx: telemetry_tx.clone(),
                    command_tx,
                    hardware: Arc::new(Mutex::new(hw)),
                });

                // 1. Start Hardware Ingest
                let ingest_state = shared_state.clone();
                tokio::task::spawn_blocking(move || {
                    loop {
                        {
                            let mut hw_lock = ingest_state.hardware.blocking_lock();
                            if hw_lock.dio0_pin.is_high().unwrap_or(false) {
                                if let Ok(ref buffer) = hw_lock.read_packet() {
                                    if let Ok(data) = bincode::deserialize::<Telemetry>(buffer) {
                                        println!("Received Telemetry! Pressure: {}", data.pressure);
                                        let _ = ingest_state.telemetry_tx.send(data);
                                    }
                                }
                            }
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                });

                // 2. Start Hardware Transmit
                let tx_state = shared_state.clone();
                tokio::spawn(async move {
                    while let Some(cmd) = command_rx.recv().await {
                        let mut hw_lock = tx_state.hardware.lock().await;
                        if let Err(e) = hw_lock.transmit_command(&cmd) {
                            eprintln!("TX Error: {}", e);
                        } else {
                            println!("Command '{}' transmitted successfully.", cmd);
                        }
                    }
                });

                start_server(shared_state).await;
            }
            Err(e) => {
                eprintln!("Hardware init failed: {}", e);
                std::process::exit(1);
            }
        }
    }
}

// Extracted server startup so we don't write it twice
async fn start_server(shared_state: Arc<AppState>) {
    let app = Router::new()
        .nest_service("/", ServeFile::new("html/index.html"))
        .route("/ws", get(ws_handler))
        .with_state(shared_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Ground Station UI available at: http://0.0.0.0:3000");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

pub async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.unwrap();
    println!("Shutdown signal received. Powering down.");
}
