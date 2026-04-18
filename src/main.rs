use axum::{
    Json,
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use tokio::{sync::{broadcast, mpsc, Mutex}, time::Duration};
use tower_http::services::ServeFile;
// Required to use `.is_high()` on the linux-embedded-hal pins
use embedded_hal::digital::v2::InputPin; 

pub use crate::hardware_cfg::{Hardware, HardwareError};
pub use crate::telemetry::data::Telemetry;

mod mock;
mod handlers;
mod telemetry;
mod hardware_cfg;
use handlers::sockets_handler::{AppState, ws_handler};

#[tokio::main]
async fn main() {
    println!("Starting Cyberdeck Groundstation...");

    let (telemetry_tx, _) = broadcast::channel::<Telemetry>(100);
    let (command_tx, mut command_rx) = mpsc::channel::<String>(32);

    // linux-embedded-hal strictly expects u64 for pin numbers
    const CS_GPIO: u64 = 10;
    const RST_GPIO: u64 = 5;
    
    match Hardware::new(CS_GPIO.into(), RST_GPIO.into()) {
        Ok(hw) => {
            let shared_state = Arc::new(AppState {
                telemetry_tx: telemetry_tx.clone(),
                command_tx,
                hardware: Arc::new(Mutex::new(hw)),
            });

            // Initialize the radio
            {
                let mut hw_lock = shared_state.hardware.lock().await;
                if let Err(e) = hw_lock.init_radio() {
                    eprintln!("RFM95 initialization failed: {}", e);
                    std::process::exit(1);
                }
                println!("RFM95 LoRa Radio initialized via linux-embedded-hal!");
            }

            // 1. THE INGEST TASK
            let ingest_state = shared_state.clone();
            tokio::task::spawn_blocking(move || {
                loop {
                    { 
                        // Safely lock the hardware for this brief check
                        let mut hw = ingest_state.hardware.blocking_lock();
                        
                        // embedded-hal's v2 traits return a Result for is_high(), so we safely unwrap it
                        if hw.dio0_pin.is_high().unwrap_or(false) {
                            if let Ok(ref buffer) = hw.read_packet() {
                                if let Ok(data) = bincode::deserialize::<Telemetry>(buffer) {
                                    println!("Received Telemetry! Altitude/Pressure: {}", data.pressure);
                                    let _ = ingest_state.telemetry_tx.send(data);
                                }
                            }
                        }
                    } 
                    // Briefly sleep so the transmit task can grab the lock if needed
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
mock::spawn_mock_telemetry_task(shared_state.telemetry_tx.clone());
            // 3. THE WEB SERVER
            let app = Router::new()
                .nest_service("/", ServeFile::new("html/index.html"))
                .route("/ws", get(ws_handler))
                .route("/mock", post(mock_ingest))
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
    println!("Shutdown signal received. Ground station powering down.");
}

