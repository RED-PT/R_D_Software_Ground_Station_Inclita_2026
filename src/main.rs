// main.rs - INTEGRATED WITH YOUR RFM95 INITIALIZATION CODE

//! user imports
use axum::{
    Json,
    Router,
    routing::{get, post},
};

use std::sync::Arc;
use tokio::{sync::{broadcast, mpsc, Mutex}, Duration};
use tower_http::services::ServeFile;
use tokio::fs::OpenOptions;



pub use crate::hardware_cfg::{Hardware, LoRaHandle, HardwareError};
pub use crate::telemetry::data::Telemetry;

mod handlers;
mod telemetry;
mod hardware_cfg; // Import hardware module
use handlers::sockets_handler::{AppState, ws_handler};

#[tokio::main]
async fn main() {
    println!("Starting Cyberdeck Groundstation...");

    let (telemetry_tx, _) = broadcast::channel::<Telemetry>(100);
    let (command_tx, mut command_rx) = mpsc::channel::<String>(32);

    // ------------------------------------------------------------------
    // 5. HARDWARE INITIALIZATION WITH ERROR HANDLING
    // ------------------------------------------------------------------
    
    // Define pin configuration at startup (CONFIGURABLE!)
    const CS_GPIO: u8 = 10;   // Adjust based on your hardware
    const RST_GPIO: u8 = 5;   // Adjust based on your hardware
    
    match Hardware::new(CS_GPIO, RST_GPIO) {
        Ok(hw) => {
            println!("LoRa GPIO (CS/RST/DIO0) initialized successfully");
            
            let shared_state = Arc::new(AppState {
                telemetry_tx: telemetry_tx.clone(),
                command_tx,
                hardware: Arc::new(Mutex::new(hw)), // Wrapped for thread-safe access
            });

            // Initialize LoRa radio (will panic on failure now!)
            #[cfg(feature = "lora")]
            {
                match shared_state.hardware.lock().await.init_radio() {
                    Ok(_radio) => {
                        println!("RFM95 LoRa Radio initialized and configured!");
                    }
                    Err(e) => {
                        eprintln!("RFM95 initialization failed: {}", e);
                        // Exit gracefully if radio initialization fails
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "lora"))]
            {
                println!("LoRa feature disabled, skipping radio init");
            }

            // ------------------------------------------------------------------
            // 1. THE INGEST TASK (Blocking SPI Reader)
            // ------------------------------------------------------------------
            let ingest_tx = telemetry_tx.clone();
            tokio::task::spawn_blocking(move || {
                let mut log_file = match OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("lora_telemetry.log") {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("Failed to open telemetry log file: {:?}", e);
                            return; // Exit task gracefully
                        }
                    };

                writeln!(log_file, "\n--- NEW LAUNCH SESSION ---").unwrap();

                let hw = Arc::clone(&shared_state.hardware);
                
                loop {
                    if let Ok(Some(_)) = hw.lock().map(|h| h.dio0_pin.poll_interrupt(true, Some(Duration::from_secs(1)))) {
                        match hw.lock() {
                            Ok(hw) => {
                                // Handle packet here (when radio is initialized)
                                if hw.radio.is_some() {
                                    log::info!("Received LoRa packet interrupt");
                                    // TODO: Read and deserialize packet data
                                } else {
                                    log::warn!("Radio not initialized yet - skipping packet processing");
                                }
                            }
                            Err(_) => {
                                eprintln!("Hardware lock failed");
                            }
                        }
                    } else {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            });

            // ------------------------------------------------------------------
            // 2. THE TRANSMIT TASK
            // ------------------------------------------------------------------
            tokio::spawn(async move {
                while let Some(cmd) = command_rx.recv().await {
                    println!("RADIO LINK: Sending command '{}' to rocket", cmd);

                    match shared_state.hardware.lock().await.transmit_command(&cmd) {
                        Ok(_) => {
                            println!("Command transmitted successfully");
                        }
                        Err(e) => {
                            eprintln!("Transmission error: {:?}", e);
                        }
                    }
                }
            });

            // ------------------------------------------------------------------
            // 3. THE WEB SERVER
            // ------------------------------------------------------------------
            let app = Router::new()
                .nest_service("/", ServeFile::new("html/index.html"))
                .route("/ws", get(ws_handler))
                .route("/mock-ingest", post(mock_ingest))
                .with_state(shared_state);

            match tokio::net::TcpListener::bind("0.0.0.0:3000").await {
                Ok(listener) => {
                    println!("Ground Station Server live on port 3000");
                    
                    axum::serve(listener, app)
                        .with_graceful_shutdown(shutdown_signal())
                        .await
                        .unwrap_or_else(|e| {
                            eprintln!("Server shutdown error: {:?}", e);
                        });
                }
                Err(e) => {
                    eprintln!("Failed to bind port 3000: {:?}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Hardware initialization failed: {}", e);
            std::process::exit(1);
        }
    }
}


// ------------------------------------------------------------------
// SHUTDOWN SIGNAL HANDLER
// ------------------------------------------------------------------
pub async fn shutdown_signal() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.unwrap() };
    let _term = async { tokio::signal::keyboard_interrupt().await };
    
    tokio::select! {
        _ = ctrl_c => println!("Received shutdown signal, cleaning up..."),
        _ = _term => println!("Received keyboard interrupt, cleaning up..."),
    }
    
    println!("Goodbye!");
}

// ------------------------------------------------------------------
// MOCK INGEST HANDLER
// ------------------------------------------------------------------
async fn mock_ingest(_request: Json<String>) -> String {
    "Mock response".to_string()
}

