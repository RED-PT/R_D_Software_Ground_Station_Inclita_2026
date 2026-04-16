use axum::{
    Router,
    routing::get,
};
use std::sync::Arc;
use tokio::{sync::{broadcast, mpsc, Mutex}, time::Duration};
use tower_http::services::ServeFile;

// Import our custom modules
pub use crate::hardware_cfg::{Hardware, LoRaHandle, HardwareError};
pub use crate::data::Telemetry; // Using data.rs based on your file structure

mod handlers;
mod data; 
mod hardware_cfg;
use handlers::sockets_handler::{AppState, ws_handler};

#[tokio::main]
async fn main() {
    println!("Starting Cyberdeck Groundstation...");

    let (telemetry_tx, _) = broadcast::channel::<Telemetry>(100);
    let (command_tx, mut command_rx) = mpsc::channel::<String>(32);

    const CS_GPIO: u8 = 10;
    const RST_GPIO: u8 = 5;
    
    match Hardware::new(CS_GPIO, RST_GPIO) {
        Ok(hw) => {
            let shared_state = Arc::new(AppState {
                telemetry_tx: telemetry_tx.clone(),
                command_tx,
                hardware: Arc::new(Mutex::new(hw)),
            });

            #[cfg(feature = "lora")]
            {
                if let Err(e) = shared_state.hardware.lock().await.init_radio() {
                    eprintln!("RFM95 initialization failed: {}", e);
                    std::process::exit(1);
                }
                println!("RFM95 LoRa Radio initialized!");
            }

            // 1. THE INGEST TASK
            let ingest_state = shared_state.clone();
            tokio::task::spawn_blocking(move || {
                loop {
                    // Lock hardware to poll and read
                    if let Ok(mut hw) = ingest_state.hardware.blocking_lock() {
                        
                        // FIX: Use .is_high() instead of poll_interrupt
                        if hw.dio0_pin.is_high() {
                            if let Some(ref mut handle) = hw.radio {
                                // sx127x_lora specific read
                                if let Ok(buffer) = handle.radio.read_packet() {
                                    
                                    // FIX: Requires bincode = "1.3.3" in Cargo.toml
                                    if let Ok(data) = bincode::deserialize::<Telemetry>(&buffer) {
                                        println!("Received Telemetry! Pressure: {}", data.pressure);
                                        let _ = ingest_state.telemetry_tx.send(data);
                                    }
                                }
                            }
                        }
                    }
                    // Prevent CPU hogging
                    std::thread::sleep(Duration::from_millis(10));
                }
            });

            // 2. THE TRANSMIT TASK
            let tx_state = shared_state.clone();
            tokio::spawn(async move {
                while let Some(cmd) = command_rx.recv().await {
                    let mut hw = tx_state.hardware.lock().await;
                    if let Err(e) = hw.transmit_command(&cmd) {
                        eprintln!("TX Error: {}", e);
                    } else {
                        println!("Command '{}' transmitted successfully.", cmd);
                    }
                }
            });

            // 3. THE WEB SERVER
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
        Err(e) => {
            eprintln!("Hardware init failed: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.unwrap();
    println!("Shutdown signal received.");
}
async fn mock_ingest(_request: Json<String>) -> String {
    "Mock response".to_string()
}

